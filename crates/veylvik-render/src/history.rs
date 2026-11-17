//! An image's recent past as one atlas: the frame at several earlier moments, each averaged down into a tile, for nodes whose output depends on what an input looked like before (particles born on a moving emitter). Bounded to 16 moments.
use crate::{GpuFrame, Plan, Renderer, Result, error};
use veylvik_document::Time;

/// Tiles along one side of the atlas for a node asking for `moments` (0 is none).
pub(crate) fn grid(moments: u32) -> u32 {
    match moments {
        0 => 0,
        1..=4 => 2,
        5..=9 => 3,
        _ => 4,
    }
}
/// The moments tile 0, 1, ... show: the frame, then evenly back to `life` seconds
/// before it, each on a whole frame (so earlier renders are found in the cache).
pub(crate) fn moments(time: Time, life: f32, grid: u32, frame_rate: Time) -> Result<Vec<Time>> {
    let fps = frame_rate.seconds()?;
    let now = time.seconds()?;
    let count = grid * grid;
    (0..count)
        .map(|j| {
            let earlier = now - f64::from(life) * f64::from(j) / f64::from(count - 1);
            let frame = (earlier * fps).round().max(0.0) as i64;
            Time::at_frame(frame, frame_rate).map_err(error)
        })
        .collect()
}
pub(crate) fn pipeline(device: &wgpu::Device) -> wgpu::ComputePipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("History atlas"),
        source: wgpu::ShaderSource::Wgsl(include_str!("history.wgsl").into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("History atlas"),
        layout: None,
        module: &shader,
        entry_point: Some("pack_tile"),
        compilation_options: Default::default(),
        cache: None,
    })
}
impl Renderer {
    /// The atlas of `plan`'s output at `times` (grid x grid of them). `sample`
    /// renders one moment; None from it cancels.
    pub(crate) fn pack_history(
        &self,
        plan: &Plan,
        grid: u32,
        times: Vec<Time>,
        mut sample: impl FnMut(Time) -> Result<Option<GpuFrame>>,
    ) -> Result<Option<GpuFrame>> {
        let output = self.compute(|scratch| {
            let buffer = scratch.buffer(
                u64::from(plan.width) * u64::from(plan.height) * 16,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                "History atlas",
            )?;
            scratch.frame(&buffer, plan.width, plan.height)
        })?;
        for (tile, time) in times.into_iter().enumerate() {
            let Some(frame) = sample(time)? else {
                return Ok(None);
            };
            self.compute(|scratch| {
                let uniform = scratch.buffer(
                    16,
                    wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    "History tile",
                )?;
                scratch.queue().write_buffer(
                    &uniform,
                    0,
                    bytemuck::cast_slice(&[plan.width, plan.height, grid, tile as u32]),
                );
                let bindings = scratch
                    .device()
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("History tile"),
                        layout: &self.history_pipeline.get_bind_group_layout(0),
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: frame.buffer().as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: output.buffer().as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: uniform.as_entire_binding(),
                            },
                        ],
                    });
                let mut encoder =
                    scratch
                        .device()
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("History tile"),
                        });
                {
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("History atlas"),
                        timestamp_writes: None,
                    });
                    pass.set_pipeline(&self.history_pipeline);
                    pass.set_bind_group(0, &bindings, &[]);
                    pass.dispatch_workgroups(
                        (plan.width / grid).div_ceil(16),
                        (plan.height / grid).div_ceil(16),
                        1,
                    );
                }
                scratch.run(encoder)
            })?;
        }
        Ok(Some(output))
    }
}
