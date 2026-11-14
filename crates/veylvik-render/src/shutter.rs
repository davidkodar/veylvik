//! Bounded temporal sampling, ready for branch-level motion-blur integration.
//! Only completed output leaves this module; intermediate pixels remain on the GPU.
use crate::{GpuFrame, ImageInputs, Plan, Region, RenderRequest, Renderer, Result, error};
use veylvik_document::Time;

#[derive(Debug, Clone, Copy)]
pub struct Shutter {
    /// Exposure fraction of a frame: 180 degrees is half a frame.
    pub angle: f32,
    /// Exposure midpoint offset in frame intervals; zero centres on the request.
    pub phase: f32,
    /// Deterministic stratified samples, bounded to 1–32.
    pub samples: u32,
    pub frame_rate: Time,
}
impl Shutter {
    /// Offsets are rounded to nanoseconds before exact rational addition. Absolute
    /// timestamps are never converted through floating point, avoiding long-shot drift.
    pub fn sample_times(self, time: Time) -> Result<Vec<Time>> {
        time.seconds()?;
        let fps = self.frame_rate.seconds()?;
        if !self.angle.is_finite()
            || !(0.0..=360.0).contains(&self.angle)
            || !self.phase.is_finite()
            || !(-0.5..=0.5).contains(&self.phase)
            || !(1..=32).contains(&self.samples)
            || !(1.0..=1000.0).contains(&fps)
        {
            return Err(error(
                "Shutter requires angle 0–360, phase -0.5–0.5, 1–32 samples and frame rate 1–1000",
            ));
        }
        let count = if self.angle == 0.0 { 1 } else { self.samples };
        (0..count)
            .map(|i| {
                let position = (f64::from(i) + 0.5) / f64::from(count) - 0.5;
                let offset =
                    (f64::from(self.phase) + position * f64::from(self.angle) / 360.0) / fps;
                let delta = Time::new((offset * 1e9).round() as i64, 1_000_000_000)?;
                time.checked_add(delta).map_err(error)
            })
            .collect()
    }
}
pub(crate) fn pipeline(device: &wgpu::Device) -> wgpu::ComputePipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Shutter accumulation"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shutter.wgsl").into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Shutter accumulation"),
        layout: None,
        module: &shader,
        entry_point: Some("accumulate"),
        compilation_options: Default::default(),
        cache: None,
    })
}
impl Renderer {
    /// Evaluate this plan's output over an exposure interval. Select a branch with
    /// `Plan::compile_output` to isolate it. No interpolation or optical flow is implied:
    /// footage is resolved at each sample's actual source time, and can already carry blur.
    ///
    /// Cancellation is checked before and after each sample. `None` means cancelled;
    /// partial accumulations are never published. Submitted GPU work completes normally.
    /// The caller chooses preview/final sample counts. No temporal results enter the
    /// ordinary frame cache; at most one sample and one accumulator are held here.
    pub fn render_gpu_shutter(
        &self,
        plan: &Plan,
        request: RenderRequest,
        inputs: &ImageInputs,
        shutter: Shutter,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Option<GpuFrame>> {
        self.sample_shutter(plan, request, shutter, |time| {
            if cancelled() {
                return Ok(None);
            }
            let frame = self.render_gpu(
                plan,
                RenderRequest {
                    time,
                    region: request.region,
                },
                inputs,
            )?;
            Ok((!cancelled()).then_some(frame))
        })
    }
    pub(crate) fn sample_shutter(
        &self,
        plan: &Plan,
        request: RenderRequest,
        shutter: Shutter,
        mut sample: impl FnMut(Time) -> Result<Option<GpuFrame>>,
    ) -> Result<Option<GpuFrame>> {
        let times = shutter.sample_times(request.time)?;
        let region = request.region.unwrap_or(Region {
            x: 0,
            y: 0,
            width: plan.width,
            height: plan.height,
        });
        region.validate(plan.width, plan.height)?;
        if times.len() == 1 {
            return sample(times[0]);
        }
        let mut output = self.compute(|scratch| {
            let buffer = scratch.buffer(
                u64::from(plan.width) * u64::from(plan.height) * 16,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                "Shutter accumulator",
            )?;
            scratch.frame(&buffer, plan.width, plan.height)
        })?;
        let weight = 1.0 / times.len() as f32;
        for (index, time) in times.into_iter().enumerate() {
            let Some(frame) = sample(time)? else {
                return Ok(None);
            };
            self.compute(|scratch| {
                let uniform = scratch.buffer(
                    16,
                    wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    "Shutter sample",
                )?;
                scratch.queue().write_buffer(
                    &uniform,
                    0,
                    bytemuck::cast_slice(&[
                        plan.width * plan.height,
                        plan.width,
                        u32::from(index == 0),
                        weight.to_bits(),
                    ]),
                );
                let bindings = scratch
                    .device()
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("Shutter sample"),
                        layout: &self.shutter_pipeline.get_bind_group_layout(0),
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
                            label: Some("Shutter sample"),
                        });
                {
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("Shutter accumulation"),
                        timestamp_writes: None,
                    });
                    pass.set_pipeline(&self.shutter_pipeline);
                    pass.set_bind_group(0, &bindings, &[]);
                    pass.dispatch_workgroups(plan.width.div_ceil(16), plan.height.div_ceil(16), 1);
                }
                scratch.run(encoder)
            })?;
        }
        output.region = region;
        Ok(Some(output))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exposure_is_centred_and_keeps_long_rational_timestamps() {
        let shutter = Shutter {
            angle: 180.,
            phase: 0.,
            samples: 8,
            frame_rate: Time::new(24000, 1001).unwrap(),
        };
        let time = Time::at_frame(10_000_000, shutter.frame_rate).unwrap();
        let times = shutter.sample_times(time).unwrap();
        for i in 0..4 {
            let a = times[i].checked_sub(time).unwrap().seconds().unwrap();
            let b = times[7 - i].checked_sub(time).unwrap().seconds().unwrap();
            assert!((a + b).abs() < 1e-12);
        }
        assert!(
            Shutter {
                samples: 33,
                ..shutter
            }
            .sample_times(time)
            .is_err()
        );
        assert!(
            Shutter {
                angle: f32::NAN,
                ..shutter
            }
            .sample_times(time)
            .is_err()
        );
        assert_eq!(
            Shutter {
                angle: 0.,
                ..shutter
            }
            .sample_times(time)
            .unwrap()
            .len(),
            1
        );
    }
}
