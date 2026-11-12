use eframe::{egui, egui_wgpu};
use std::sync::Arc;
use veylvik_render::{GpuFrame, Renderer};
use wgpu::util::DeviceExt;

pub struct Display {
    pipeline: wgpu::RenderPipeline,
    format: wgpu::TextureFormat,
}
impl Display {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewer display transform"),
            source: wgpu::ShaderSource::Wgsl(include_str!("display.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewer"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline, format }
    }
    pub fn frame(&self, renderer: &Renderer, frame: Arc<GpuFrame>) -> Arc<DisplayFrame> {
        let uniform = renderer
            .device()
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("viewer size"),
                contents: bytemuck::cast_slice(&[
                    frame.width(),
                    frame.height(),
                    u32::from(self.format.is_srgb()),
                    0,
                ]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind = renderer
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("viewer frame"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: frame.buffer().as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: uniform.as_entire_binding(),
                    },
                ],
            });
        Arc::new(DisplayFrame {
            pipeline: self.pipeline.clone(),
            bind,
            frame,
            queue: renderer.queue().clone(),
        })
    }
}
pub struct DisplayFrame {
    pipeline: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    pub frame: Arc<GpuFrame>,
    queue: wgpu::Queue,
}
impl Drop for DisplayFrame {
    fn drop(&mut self) {
        // The app retains this object across painting and releases it on a later
        // UI update, after eframe has submitted the previous frame's commands.
        let frame = self.frame.clone();
        self.queue.on_submitted_work_done(move || drop(frame));
    }
}
pub struct Paint(pub Arc<DisplayFrame>);
impl egui_wgpu::CallbackTrait for Paint {
    fn paint(
        &self,
        _: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        _: &egui_wgpu::CallbackResources,
    ) {
        pass.set_pipeline(&self.0.pipeline);
        pass.set_bind_group(0, &self.0.bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::{Composition, Node, Operation, Time};
    use veylvik_render::{ImageInputs, Plan, RenderRequest};
    #[test]
    #[ignore = "requires Vulkan"]
    fn viewer_shader_applies_display_transform_on_shared_device() {
        let renderer = Renderer::new().unwrap();
        let node = Node::new(
            "red",
            Operation::Solid {
                color: [0.25, 0.0, 0.0, 1.0],
            },
            &[],
        );
        let doc = Composition {
            schema: 1,
            width: 2,
            height: 2,
            frame_rate: Time::new(24, 1).unwrap(),
            output: node.id,
            nodes: [(node.id, node)].into_iter().collect(),
        };
        let frame = renderer
            .render_gpu(
                &Plan::compile(&doc).unwrap(),
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &ImageInputs::default(),
            )
            .unwrap();
        let device = renderer.device();
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let display = Display::new(device, wgpu::TextureFormat::Rgba8Unorm);
        let frame = display.frame(&renderer, Arc::new(frame));
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 512,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let view = target.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&frame.pipeline);
            pass.set_bind_group(0, &frame.bind, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(2),
                },
            },
            target.size(),
        );
        let submission = renderer.queue().submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                tx.send(result).unwrap();
            });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .unwrap();
        rx.recv().unwrap().unwrap();
        let bytes = readback.slice(..).get_mapped_range();
        assert_eq!(&bytes[..4], &[137, 0, 0, 255]);
        assert_eq!(&bytes[256..260], &[137, 0, 0, 255]);
        drop(bytes);
        readback.unmap();
        assert!(pollster::block_on(scope.pop()).is_none());
    }
}
