//! GPU planar feature detection and batched perspective-aware patch matching.
use crate::{GpuFrame, Renderer, Result, error};
use std::sync::Arc;
use veylvik_document::planar::{self, Consensus, Feature, Homography, Point, Quad};

pub struct PlanarTracker {
    detect: wgpu::ComputePipeline,
    correlate: wgpu::ComputePipeline,
    owner: Arc<crate::frame::DeviceIdentity>,
}
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub patch_radius: u32,
    pub search_radius: u32,
    pub tolerance_pixels: f64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            patch_radius: 6,
            search_radius: 16,
            tolerance_pixels: 2.,
        }
    }
}
pub struct TrackedPlane {
    pub consensus: Consensus,
    /// Includes rejected matches, for diagnostic viewer overlays.
    pub features: Vec<Feature>,
}
impl PlanarTracker {
    pub fn new(renderer: &Renderer) -> Self {
        let module = renderer
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("planar tracking"),
                source: wgpu::ShaderSource::Wgsl(include_str!("planar.wgsl").into()),
            });
        let pipeline = |entry| {
            renderer
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: None,
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                })
        };
        Self {
            detect: pipeline("detect"),
            correlate: pipeline("correlate"),
            owner: renderer.owner.clone(),
        }
    }
    /// Return up to 64 distributed corners in normalized image coordinates.
    pub fn detect(
        &self,
        renderer: &Renderer,
        frame: &GpuFrame,
        surface: Quad,
        settings: Settings,
    ) -> Result<Vec<Point>> {
        self.validate(renderer, frame, frame, settings)?;
        planar::validate_quad(surface).map_err(|e| error(e.to_string()))?;
        if surface.iter().flatten().any(|v| !(0. ..=1.).contains(v)) {
            return Err(error("Detection surface must be inside the source image"));
        }
        let params = parameters(frame, Homography::IDENTITY, surface, settings)?;
        let values = self.run(
            renderer,
            frame,
            frame,
            &self.detect,
            &params,
            &[[0.; 4]],
            256,
            [1, 1],
        )?;
        let mut points = Vec::new();
        for v in values.as_chunks::<4>().0 {
            if v[2] > 0.0001
                && points.iter().all(|p: &Point| {
                    ((p[0] * f64::from(frame.width) - f64::from(v[0])).powi(2)
                        + (p[1] * f64::from(frame.height) - f64::from(v[1])).powi(2))
                        >= f64::from(settings.patch_radius * 2).powi(2)
                })
            {
                points.push([
                    (f64::from(v[0]) + 0.5) / f64::from(frame.width),
                    (f64::from(v[1]) + 0.5) / f64::from(frame.height),
                ]);
            }
        }
        if points.len() < 6 {
            return Err(error(
                "Surface has too few distinct features; choose a larger or more textured region",
            ));
        }
        Ok(points)
    }
    /// All features are matched in one GPU dispatch/readback. CPU receives only
    /// correlation scores, then robustly fits their common projective motion.
    pub fn track(
        &self,
        renderer: &Renderer,
        reference: &GpuFrame,
        current: &GpuFrame,
        points: &[Point],
        prediction: Homography,
        settings: Settings,
    ) -> Result<TrackedPlane> {
        self.validate(renderer, reference, current, settings)?;
        if !(6..=256).contains(&points.len()) {
            return Err(error("Planar tracking requires 6..256 features"));
        }
        let size = [reference.width, reference.height];
        let mut gpu_points = Vec::with_capacity(points.len());
        let mut origins = Vec::with_capacity(points.len());
        for p in points {
            let pixel = [0, 1].map(|axis| (p[axis] * f64::from(size[axis]) - 0.5).round());
            if pixel.iter().enumerate().any(|(axis, &v)| {
                !v.is_finite()
                    || v < f64::from(settings.patch_radius)
                    || v + f64::from(settings.patch_radius) >= f64::from(size[axis])
            }) {
                return Err(error(
                    "Planar feature patch must fit inside the reference image",
                ));
            }
            gpu_points.push([pixel[0] as f32, pixel[1] as f32, 0., 0.]);
            origins.push([
                (pixel[0] + 0.5) / f64::from(size[0]),
                (pixel[1] + 0.5) / f64::from(size[1]),
            ]);
        }
        let params = parameters(
            reference,
            prediction,
            [[0., 0.], [1., 0.], [1., 1.], [0., 1.]],
            settings,
        )?;
        let side = settings.search_radius * 2 + 1;
        let count = side * side;
        let values = self.run(
            renderer,
            reference,
            current,
            &self.correlate,
            &params,
            &gpu_points,
            count * points.len() as u32,
            [count.div_ceil(64), points.len() as u32],
        )?;
        let mut features = Vec::with_capacity(points.len());
        for (&origin, scores) in origins.iter().zip(values.chunks_exact(count as usize)) {
            let (best, &score) = scores
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .ok_or_else(|| error("Missing planar scores"))?;
            let x = best as u32 % side;
            let y = best as u32 / side;
            let second = scores
                .iter()
                .enumerate()
                .filter(|(i, _)| {
                    (*i as u32 % side).abs_diff(x) > 2 || (*i as u32 / side).abs_diff(y) > 2
                })
                .map(|(_, &v)| v)
                .fold(-2_f32, f32::max);
            let subpixel = |stride: usize, coord: u32| -> f64 {
                if coord == 0 || coord + 1 == side {
                    return 0.;
                }
                let a = scores[best - stride];
                let b = scores[best];
                let c = scores[best + stride];
                let d = a - 2. * b + c;
                if a < -1. || c < -1. || d >= -1e-6 {
                    0.
                } else {
                    f64::from((0.5 * (a - c) / d).clamp(-0.5, 0.5))
                }
            };
            let predicted = prediction
                .project(origin)
                .ok_or_else(|| error("Invalid planar prediction"))?;
            features.push(Feature {
                reference: origin,
                position: [
                    predicted[0]
                        + (f64::from(x) - f64::from(settings.search_radius) + subpixel(1, x))
                            / f64::from(size[0]),
                    predicted[1]
                        + (f64::from(y) - f64::from(settings.search_radius)
                            + subpixel(side as usize, y))
                            / f64::from(size[1]),
                ],
                correlation: f64::from(score.clamp(-1., 1.)),
                separation: f64::from((score - second).clamp(0., 3.)),
                accepted: false,
            });
        }
        let consensus = planar::estimate(&features, size, settings.tolerance_pixels)
            .map_err(|e| error(e.to_string()))?;
        for (f, &accepted) in features.iter_mut().zip(&consensus.accepted) {
            f.accepted = accepted;
        }
        Ok(TrackedPlane {
            consensus,
            features,
        })
    }
    fn validate(
        &self,
        renderer: &Renderer,
        reference: &GpuFrame,
        current: &GpuFrame,
        settings: Settings,
    ) -> Result<()> {
        if !Arc::ptr_eq(&self.owner, &renderer.owner)
            || !Arc::ptr_eq(&reference.owner, &renderer.owner)
            || !Arc::ptr_eq(&current.owner, &renderer.owner)
        {
            return Err(error("Planar frames belong to another renderer"));
        }
        if reference.region.x != 0
            || reference.region.y != 0
            || reference.region.width != reference.width
            || reference.region.height != reference.height
            || current.region != reference.region
            || current.width != reference.width
            || current.height != reference.height
        {
            return Err(error("Planar tracking requires matching uncropped frames"));
        }
        if !(3..=16).contains(&settings.patch_radius)
            || !(1..=48).contains(&settings.search_radius)
            || !(0.1..=32.).contains(&settings.tolerance_pixels)
        {
            return Err(error("Invalid planar tracking settings"));
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn run(
        &self,
        renderer: &Renderer,
        reference: &GpuFrame,
        current: &GpuFrame,
        pipeline: &wgpu::ComputePipeline,
        params: &[u32; 20],
        features: &[[f32; 4]],
        count: u32,
        dispatch: [u32; 2],
    ) -> Result<Vec<f32>> {
        let mut pool = renderer
            .pool
            .lock()
            .map_err(|_| error("Tracking resource pool unavailable"))?;
        if pool.failed {
            return Err(error("Renderer unavailable after GPU failure"));
        }
        let mut resources = pool.frame(renderer.budget_bytes);
        let uniform = resources.take(
            &renderer.device,
            80,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            "planar settings",
        )?;
        let points = resources.take(
            &renderer.device,
            (features.len() * 16) as u64,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            "planar features",
        )?;
        let bytes = u64::from(count) * 4;
        let output = resources.take(
            &renderer.device,
            bytes,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            "planar scores",
        )?;
        let readback = resources.take(
            &renderer.device,
            bytes,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            "planar score readback",
        )?;
        renderer
            .queue
            .write_buffer(&uniform, 0, bytemuck::cast_slice(params));
        renderer
            .queue
            .write_buffer(&points, 0, bytemuck::cast_slice(features));
        // Auto layouts omit unused shader bindings for the detection entry point.
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: reference.storage.buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: output.as_entire_binding(),
            },
        ];
        if std::ptr::eq(pipeline, &self.correlate) {
            entries.push(wgpu::BindGroupEntry {
                binding: 2,
                resource: current.storage.buffer.as_entire_binding(),
            });
            entries.push(wgpu::BindGroupEntry {
                binding: 4,
                resource: points.as_entire_binding(),
            });
        }
        let bind = renderer
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("planar tracking"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &entries,
            });
        let mut encoder = renderer.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(dispatch[0], dispatch[1], 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
        resources.submitted();
        let submission = renderer.queue.submit([encoder.finish()]);
        let result = renderer.map_values(&readback, submission)?;
        resources.completed();
        Ok(result)
    }
}
fn parameters(
    frame: &GpuFrame,
    h: Homography,
    quad: Quad,
    settings: Settings,
) -> Result<[u32; 20]> {
    h.transform_quad([[0., 0.], [1., 0.], [1., 1.], [0., 1.]])
        .map_err(|e| error(e.to_string()))?;
    let w = f64::from(frame.width);
    let height = f64::from(frame.height);
    let scale = Homography {
        matrix: [w, 0., -0.5, 0., height, -0.5, 0., 0., 1.],
    };
    let pixel = scale.compose(h).compose(
        scale
            .inverse()
            .ok_or_else(|| error("Invalid image dimensions"))?,
    );
    let matrix = pixel.matrix.map(|v| (v / pixel.matrix[8]) as f32);
    if matrix.iter().any(|v| !v.is_finite()) {
        return Err(error("Invalid planar prediction"));
    }
    let mut params = [0; 20];
    params[..4].copy_from_slice(&[
        frame.width,
        frame.height,
        settings.patch_radius,
        settings.search_radius,
    ]);
    for (dst, v) in params[4..12].iter_mut().zip(matrix) {
        *dst = v.to_bits();
    }
    for (dst, p) in params[12..].as_chunks_mut::<2>().0.iter_mut().zip(quad) {
        dst[0] = ((p[0] * w) as f32).to_bits();
        dst[1] = ((p[1] * height) as f32).to_bits();
    }
    Ok(params)
}
