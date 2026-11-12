//! Translation-only GPU feature matching. Pixel data never leaves the GPU.
use crate::{GpuFrame, Renderer, Result, error};
use std::sync::Arc;
#[derive(Debug, Clone, Copy)]
pub struct Match {
    pub position: [f32; 2],
    pub confidence: f32,
    pub separation: f32,
}
pub struct PointTracker {
    pipeline: wgpu::ComputePipeline,
    owner: Arc<crate::frame::DeviceIdentity>,
}
impl PointTracker {
    pub fn new(renderer: &Renderer) -> Self {
        let module = renderer
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("point tracking"),
                source: wgpu::ShaderSource::Wgsl(include_str!("tracking.wgsl").into()),
            });
        let pipeline = renderer
            .device
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
            owner: renderer.owner.clone(),
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
        if !Arc::ptr_eq(&self.owner, &renderer.owner)
            || !Arc::ptr_eq(&reference.owner, &renderer.owner)
            || !Arc::ptr_eq(&current.owner, &renderer.owner)
        {
            return Err(error("Tracking frames belong to another renderer"));
        }
        if reference.region.x != 0
            || reference.region.y != 0
            || reference.region.width != reference.width
            || reference.region.height != reference.height
            || current.region != reference.region
            || current.width != reference.width
            || current.height != reference.height
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
            || origin[0] + patch as i32 >= reference.width as i32
            || origin[1] + patch as i32 >= reference.height as i32
        {
            return Err(error("Feature box must fit inside the source image"));
        }
        let predicted = predicted.map(|v| v.round() as i32);
        let side = search * 2 + 1;
        let bytes = u64::from(side * side) * 4;
        let mut pool = renderer
            .pool
            .lock()
            .map_err(|_| error("Tracking resource pool unavailable"))?;
        if pool.failed {
            return Err(error("Renderer unavailable after GPU failure"));
        }
        let mut resources = pool.frame(renderer.budget_bytes);
        let params = resources.take(
            &renderer.device,
            32,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            "tracking region",
        )?;
        renderer.queue.write_buffer(
            &params,
            0,
            bytemuck::cast_slice(&[
                reference.width,
                reference.height,
                origin[0] as u32,
                origin[1] as u32,
                predicted[0] as u32,
                predicted[1] as u32,
                patch,
                search,
            ]),
        );
        let output = resources.take(
            &renderer.device,
            bytes,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            "tracking scores",
        )?;
        let readback = resources.take(
            &renderer.device,
            bytes,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            "tracking score readback",
        )?;
        let bind = renderer
            .device
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
                        resource: reference.storage.buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: current.storage.buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: output.as_entire_binding(),
                    },
                ],
            });
        let mut encoder = renderer.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups((side * side).div_ceil(64), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
        resources.submitted();
        let submission = renderer.queue.submit([encoder.finish()]);
        let scores: Vec<f32> = renderer.map_values(&readback, submission)?;
        resources.completed();
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
