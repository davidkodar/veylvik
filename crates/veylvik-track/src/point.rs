//! Translation-only GPU feature matching. Pixel data never leaves the GPU.
use crate::{Result, error};
use veylvik_render::{GpuFrame, Renderer, compute::DeviceKey};
#[derive(Debug, Clone, Copy)]
pub struct Match {
    pub position: [f32; 2],
    pub confidence: f32,
    pub separation: f32,
}
pub struct PointTracker {
    pipeline: wgpu::ComputePipeline,
    owner: DeviceKey,
}
impl PointTracker {
    pub fn new(renderer: &Renderer) -> Self {
        let module = renderer
            .device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("point tracking"),
                source: wgpu::ShaderSource::Wgsl(include_str!("point.wgsl").into()),
            });
        let pipeline =
            renderer
                .device()
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("point tracking"),
                    layout: None,
                    module: &module,
                    entry_point: Some("correlate"),
                    compilation_options: Default::default(),
                    cache: None,
                });
        Self {
            pipeline,
            owner: renderer.device_key(),
        }
    }
    /// Fixed reference patch prevents cumulative template drift. Search is centred
    /// on the preceding accepted point. Callers decide whether confidence is adequate.
    pub fn track(
        &self,
        renderer: &Renderer,
        reference: &GpuFrame,
        current: &GpuFrame,
        origin: [f32; 2],
        predicted: [f32; 2],
        radii: [u32; 2],
    ) -> Result<Match> {
        let [patch, search] = radii;
        if self.owner != renderer.device_key()
            || !renderer.owns(reference)
            || !renderer.owns(current)
        {
            return Err(error("Tracking frames belong to another renderer"));
        }
        if reference.region().x != 0
            || reference.region().y != 0
            || reference.region().width != reference.width()
            || reference.region().height != reference.height()
            || current.region() != reference.region()
            || current.width() != reference.width()
            || current.height() != reference.height()
        {
            return Err(error("Tracking requires matching uncropped frames"));
        }
        if !(3..=32).contains(&patch)
            || !(1..=64).contains(&search)
            || origin
                .into_iter()
                .chain(predicted)
                .any(|v| !v.is_finite() || v.abs() > 8192.0)
        {
            return Err(error("Invalid tracking region"));
        }
        let origin = origin.map(|v| v.round() as i32);
        if origin[0] < patch as i32
            || origin[1] < patch as i32
            || origin[0] + patch as i32 >= reference.width() as i32
            || origin[1] + patch as i32 >= reference.height() as i32
        {
            return Err(error("Feature box must fit inside the source image"));
        }
        let predicted = predicted.map(|v| v.round() as i32);
        let side = search * 2 + 1;
        let bytes = u64::from(side * side) * 4;
        let scores: Vec<f32> = renderer.compute(|scratch| {
            let params = scratch.buffer(
                32,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                "tracking region",
            )?;
            scratch.queue().write_buffer(
                &params,
                0,
                bytemuck::cast_slice(&[
                    reference.width(),
                    reference.height(),
                    origin[0] as u32,
                    origin[1] as u32,
                    predicted[0] as u32,
                    predicted[1] as u32,
                    patch,
                    search,
                ]),
            );
            let output = scratch.buffer(
                bytes,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                "tracking scores",
            )?;
            let readback = scratch.buffer(
                bytes,
                wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                "tracking score readback",
            )?;
            let bind = scratch
                .device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("tracking frames"),
                    layout: &self.pipeline.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: params.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: reference.buffer().as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: current.buffer().as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: output.as_entire_binding(),
                        },
                    ],
                });
            let mut encoder = scratch.device().create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups((side * side).div_ceil(64), 1, 1);
            }
            encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
            scratch.read::<f32>(encoder, &readback)
        })?;
        let (best, &confidence) = scores
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .ok_or_else(|| error("No tracking candidates"))?;
        let bx = best as u32 % side;
        let by = best as u32 / side;
        let second = scores
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                ((*i as u32 % side).abs_diff(bx) > 2) || ((*i as u32 / side).abs_diff(by) > 2)
            })
            .map(|(_, &s)| s)
            .fold(-2.0_f32, f32::max);
        Ok(Match {
            position: [
                (predicted[0] + bx as i32 - search as i32) as f32,
                (predicted[1] + by as i32 - search as i32) as f32,
            ],
            confidence,
            separation: confidence - second,
        })
    }
}
