//! GPU-only reduction of completed previews; full-resolution authoring semantics remain intact.
use crate::{GpuFrame, Renderer, Result, error};
use std::sync::Arc;
pub(crate) fn pipeline(device: &wgpu::Device) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Preview reduction"),
        source: wgpu::ShaderSource::Wgsl(include_str!("preview_scale.wgsl").into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Preview reduction"),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    })
}
impl Renderer {
    pub fn reduce_preview(&self, frame: Arc<GpuFrame>, divisor: u32) -> Result<Arc<GpuFrame>> {
        if divisor == 1 {
            return Ok(frame);
        }
        if ![2, 4].contains(&divisor) || !self.owns(&frame) {
            return Err(error("Invalid preview reduction"));
        }
        let width = frame.width().div_ceil(divisor);
        let height = frame.height().div_ceil(divisor);
        self.compute(|scratch| {
            let buffer = scratch.buffer(
                u64::from(width) * u64::from(height) * 16,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                "Reduced preview",
            )?;
            let size = scratch.buffer(
                16,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                "Preview dimensions",
            )?;
            scratch.queue().write_buffer(
                &size,
                0,
                bytemuck::cast_slice(&[frame.width(), frame.height(), width, divisor]),
            );
            let group = scratch
                .device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Preview reduction"),
                    layout: &self.preview_pipeline.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: size.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: frame.buffer().as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: buffer.as_entire_binding(),
                        },
                    ],
                });
            let mut encoder =
                scratch
                    .device()
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("Preview reduction"),
                    });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("Preview reduction"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.preview_pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
            }
            scratch.run(encoder)?;
            let mut result = scratch.frame(&buffer, width, height)?;
            result.flow_report = frame.flow_report();
            Ok(Arc::new(result))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::*;
    #[test]
    #[ignore = "requires Vulkan"]
    fn gpu_reduction_preserves_hdr_alpha_and_odd_edges() {
        let renderer = Renderer::new().unwrap();
        let mut doc = blank(13, 7, Time::new(24, 1).unwrap()).unwrap();
        doc.nodes.get_mut(&doc.output).unwrap().operation = Operation::Image(nodes::Image {
            asset: "image".into(),
        });
        let pixels: Vec<_> = (0..91)
            .map(|i| [(i % 7) as f32 - 3., 5., 0.25, 0.5])
            .collect();
        let mut inputs = crate::ImageInputs::default();
        inputs.insert(
            "image",
            Arc::new(crate::Image::new(13, 7, pixels.clone()).unwrap()),
        );
        let plan = crate::Plan::compile(&doc).unwrap();
        let full = Arc::new(
            renderer
                .render_gpu(
                    &plan,
                    crate::RenderRequest::full(Time::new(0, 1).unwrap()),
                    &inputs,
                )
                .unwrap(),
        );
        for divisor in [2, 4] {
            let small = renderer.reduce_preview(full.clone(), divisor).unwrap();
            assert_eq!(small.width(), 13u32.div_ceil(divisor));
            assert_eq!(small.height(), 7u32.div_ceil(divisor));
            let actual = renderer.read_frame(&small).unwrap().pixels;
            for y in 0..small.height() {
                for x in 0..small.width() {
                    let mut sum = [0.; 4];
                    let mut n = 0.;
                    for sy in y * divisor..((y + 1) * divisor).min(7) {
                        for sx in x * divisor..((x + 1) * divisor).min(13) {
                            for c in 0..4 {
                                sum[c] += pixels[(sy * 13 + sx) as usize][c];
                            }
                            n += 1.;
                        }
                    }
                    for c in 0..4 {
                        assert!(
                            (actual[(y * small.width() + x) as usize][c] - sum[c] / n).abs() < 1e-6
                        );
                    }
                }
            }
        }
        assert_eq!(renderer.read_frame(&full).unwrap().pixels, pixels);
    }
}
