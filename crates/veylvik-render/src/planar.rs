//! GPU planar feature detection and batched perspective-aware patch matching.
use crate::{GpuFrame, Renderer, Result, error};
use std::sync::Arc;
use veylvik_document::planar::{self, Consensus, Feature, Homography, Point, Quad};

pub struct PlanarTracker {
    detect: wgpu::ComputePipeline,
    correlate: wgpu::ComputePipeline,
    downsample: wgpu::ComputePipeline,
    owner: Arc<crate::frame::DeviceIdentity>,
}
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub patch_radius: u32,
    pub search_radius: u32,
    pub tolerance_pixels: f64,
    /// Re-detect features on the latest accepted frame when fewer than this many matches
    /// are accepted, so scale and viewpoint changes do not exhaust a fixed reference.
    /// Zero keeps the original reference for the whole range.
    pub reanchor_below: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            patch_radius: 6,
            search_radius: 16,
            tolerance_pixels: 2.,
            reanchor_below: 24,
        }
    }
}
/// Why a range analysis ended before its last frame, if it did.
#[derive(Debug, Clone, PartialEq)]
pub enum Stop {
    Complete,
    Cancelled,
    Lost { frame: i64, reason: String },
    SizeLimit { frame: i64 },
}
impl Stop {
    pub fn message(&self) -> String {
        match self {
            Self::Complete => "Tracking complete".into(),
            Self::Cancelled => "Cancelled; accepted frames are available to save".into(),
            Self::Lost { frame, reason } => format!("Stopped at frame {frame}: {reason}"),
            Self::SizeLimit { .. } => {
                "Stopped before the project size limit; accepted frames remain available".into()
            }
        }
    }
}
pub struct Range {
    pub data: planar::Track,
    pub stop: Stop,
    /// References used, including the first; more than one means the tracker re-anchored.
    pub anchors: u32,
}
/// Called after every accepted frame with its index and pixels; return false to cancel.
pub type Observer<'a> = dyn FnMut(i64, &Arc<GpuFrame>, &planar::Sample) -> bool + 'a;
pub struct TrackedPlane {
    pub consensus: Consensus,
    /// Includes rejected matches, for diagnostic viewer overlays.
    pub features: Vec<Feature>,
}
impl PlanarTracker {
    /// Track `surface` from `start` (its reference frame) towards `end`, in either direction,
    /// on the rendered output of `plan`. Accepted frames are kept when the track is lost or
    /// cancelled; `max_bytes` bounds the encoded diagnostics so the result stays savable.
    #[allow(clippy::too_many_arguments)]
    pub fn track_range(
        &self,
        renderer: &Renderer,
        plan: &crate::Plan,
        inputs: &crate::ImageInputs,
        surface: Quad,
        [start, end]: [i64; 2],
        rate: veylvik_document::Time,
        settings: Settings,
        max_bytes: usize,
        observe: &mut Observer<'_>,
    ) -> Result<Range> {
        let at = |frame: i64| {
            veylvik_document::Time::at_frame(frame, rate).map_err(|e| error(e.to_string()))
        };
        let render = |frame: i64| {
            renderer.render_gpu_cached(plan, crate::RenderRequest::full(at(frame)?), inputs)
        };
        let reference_time = at(start)?;
        let mut reference = render(start)?;
        let mut levels = self.pyramid(renderer, &reference)?;
        let mut points = self.detect(renderer, &reference, surface, settings)?;
        let initial = self.track(
            renderer,
            &reference,
            &reference,
            &points,
            Homography::IDENTITY,
            settings,
        )?;
        let first = planar::Sample {
            time: reference_time,
            corners: surface,
            features: initial.features,
            rms_pixels: initial.consensus.rms_pixels,
        };
        let mut data = planar::Track {
            reference_time,
            reference: surface,
            samples: vec![first],
        };
        // Allow for the document's deeper pretty-JSON indentation.
        let size = |value: &dyn erased::Encode| value.encoded_len() * 2;
        let mut bytes = size(&data);
        if bytes > max_bytes {
            return Err(error("Not enough project space for tracking diagnostics"));
        }
        let mut stop = Stop::Complete;
        if !observe(start, &reference, &data.samples[0]) {
            stop = Stop::Cancelled;
        }
        // `anchor` is the surface as seen on the current reference; `motion` maps it onward.
        let (mut anchor, mut motion, mut anchors) = (surface, Homography::IDENTITY, 1);
        // The last accepted frame, kept so a failed frame can retry from a fresh anchor there.
        let mut previous: Option<(Arc<GpuFrame>, Vec<GpuFrame>, Quad)> = None;
        let step = if end > start { 1 } else { -1 };
        let mut frame = start;
        while stop == Stop::Complete && frame != end {
            frame += step;
            let current = render(frame)?;
            let current_levels = self.pyramid(renderer, &current)?;
            let mut attempt = self.track_multiscale(
                renderer,
                &reference,
                &current,
                &levels,
                &current_levels,
                &points,
                motion,
                settings,
            );
            if attempt.is_err()
                && settings.reanchor_below > 0
                && let Some((image, image_levels, corners)) = previous.take()
            {
                let visible = corners.map(|[x, y]| [x.clamp(0.005, 0.995), y.clamp(0.005, 0.995)]);
                if planar::validate_quad(visible).is_ok()
                    && let Ok(fresh) = self.detect(renderer, &image, visible, settings)
                    && fresh.len() >= 12
                {
                    (anchor, motion, points, levels, reference) =
                        (corners, Homography::IDENTITY, fresh, image_levels, image);
                    anchors += 1;
                    attempt = self.track_multiscale(
                        renderer,
                        &reference,
                        &current,
                        &levels,
                        &current_levels,
                        &points,
                        motion,
                        settings,
                    );
                }
            }
            let found = match attempt {
                Ok(found) => found,
                Err(e) => {
                    stop = Stop::Lost {
                        frame,
                        reason: e.to_string(),
                    };
                    break;
                }
            };
            let corners = match found.consensus.motion.transform_quad(anchor) {
                Ok(corners) => corners,
                Err(e) => {
                    stop = Stop::Lost {
                        frame,
                        reason: e.to_string(),
                    };
                    break;
                }
            };
            motion = found.consensus.motion;
            let accepted = found.features.iter().filter(|f| f.accepted).count();
            let sample = planar::Sample {
                time: at(frame)?,
                corners,
                features: found.features,
                rms_pixels: found.consensus.rms_pixels,
            };
            bytes += size(&sample);
            if bytes > max_bytes {
                stop = Stop::SizeLimit { frame };
                break;
            }
            if !observe(frame, &current, &sample) {
                stop = Stop::Cancelled;
            }
            data.samples.push(sample);
            if accepted >= settings.reanchor_below as usize {
                previous = Some((current, current_levels, corners));
                continue;
            }
            {
                // Detect inside the visible part of the surface; the anchor itself may
                // extend past the frame edges. Keep the old reference if too little remains.
                let visible = corners.map(|[x, y]| [x.clamp(0.005, 0.995), y.clamp(0.005, 0.995)]);
                if planar::validate_quad(visible).is_ok()
                    && let Ok(fresh) = self.detect(renderer, &current, visible, settings)
                    && fresh.len() > accepted
                    && fresh.len() >= 12
                {
                    (anchor, motion, points, levels, reference) = (
                        corners,
                        Homography::IDENTITY,
                        fresh,
                        current_levels,
                        current,
                    );
                    anchors += 1;
                } else {
                    previous = Some((current, current_levels, corners));
                }
            }
        }
        data.samples
            .sort_by(|a, b| a.time.compare(b.time).expect("frame times are comparable"));
        data.validate().map_err(|e| error(e.to_string()))?;
        Ok(Range {
            data,
            stop,
            anchors,
        })
    }
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
        let downsample_module =
            renderer
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("planar pyramid"),
                    source: wgpu::ShaderSource::Wgsl(include_str!("planar_pyramid.wgsl").into()),
                });
        let downsample =
            renderer
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("planar pyramid"),
                    layout: None,
                    module: &downsample_module,
                    entry_point: Some("downsample"),
                    compilation_options: Default::default(),
                    cache: None,
                });
        Self {
            downsample,
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
    /// Filtered GPU image pyramid. Pixels remain on the renderer's device;
    /// retained levels count against its shared memory budget.
    pub fn pyramid(&self, renderer: &Renderer, frame: &GpuFrame) -> Result<Vec<GpuFrame>> {
        self.validate(renderer, frame, frame, Settings::default())?;
        let mut levels = Vec::new();
        for _ in 0..3 {
            let input = levels.last().unwrap_or(frame);
            if input.width.min(input.height) < 64 {
                break;
            }
            let width = input.width / 2;
            let height = input.height / 2;
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
                16,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                "pyramid size",
            )?;
            let output = resources.take(
                &renderer.device,
                u64::from(width) * u64::from(height) * 16,
                wgpu::BufferUsages::STORAGE,
                "planar pyramid",
            )?;
            renderer.queue.write_buffer(
                &uniform,
                0,
                bytemuck::cast_slice(&[input.width, input.height, width, height]),
            );
            let bind = renderer
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("planar pyramid"),
                    layout: &self.downsample.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: uniform.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: input.storage.buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: output.as_entire_binding(),
                        },
                    ],
                });
            let mut encoder = renderer.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.downsample);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
            }
            resources.submitted();
            renderer.wait(renderer.queue.submit([encoder.finish()]))?;
            resources.completed();
            levels.push(GpuFrame {
                identity: crate::frame::next_identity()?,
                storage: resources.pin(&output),
                owner: renderer.owner.clone(),
                region: crate::Region {
                    x: 0,
                    y: 0,
                    width,
                    height,
                },
                width,
                height,
            });
        }
        Ok(levels)
    }
    /// Coarse-to-fine projective matching, followed by full-resolution refinement.
    /// A coarse level may lack texture; only a successful full-resolution solve
    /// is returned. Reference levels can be reused throughout an analysis.
    #[allow(clippy::too_many_arguments)]
    pub fn track_multiscale(
        &self,
        renderer: &Renderer,
        reference: &GpuFrame,
        current: &GpuFrame,
        reference_levels: &[GpuFrame],
        current_levels: &[GpuFrame],
        points: &[Point],
        mut prediction: Homography,
        settings: Settings,
    ) -> Result<TrackedPlane> {
        self.validate(renderer, reference, current, settings)?;
        if reference_levels.len() != current_levels.len() {
            return Err(error("Mismatched tracking pyramids"));
        }
        let initial_prediction = prediction;
        for (a, b) in reference_levels.iter().zip(current_levels).rev() {
            let usable: Vec<_> = points
                .iter()
                .copied()
                .filter(|p| {
                    [a.width, a.height].iter().enumerate().all(|(axis, &size)| {
                        let pixel = (p[axis] * f64::from(size) - 0.5).round();
                        pixel >= f64::from(settings.patch_radius)
                            && pixel + f64::from(settings.patch_radius) < f64::from(size)
                    })
                })
                .collect();
            let coarse = Settings {
                tolerance_pixels: settings.tolerance_pixels.max(1.5),
                ..settings
            };
            if let Ok(found) = self.track(renderer, a, b, &usable, prediction, coarse) {
                prediction = found.consensus.motion;
            }
        }
        // Coarse texture can be ambiguous even when fine detail is distinctive.
        // Preserve the local solve as a fallback rather than regressing it.
        let found = self
            .track(renderer, reference, current, points, prediction, settings)
            .or_else(|_| {
                self.track(
                    renderer,
                    reference,
                    current,
                    points,
                    initial_prediction,
                    settings,
                )
            })?;
        Ok(self
            .track(
                renderer,
                reference,
                current,
                points,
                found.consensus.motion,
                settings,
            )
            .unwrap_or(found))
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

mod erased {
    pub trait Encode {
        fn encoded_len(&self) -> usize;
    }
    impl<T: serde::Serialize> Encode for T {
        fn encoded_len(&self) -> usize {
            serde_json::to_vec_pretty(self).map_or(usize::MAX / 4, |v| v.len())
        }
    }
}
