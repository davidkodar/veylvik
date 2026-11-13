//! Explicit color/alpha boundary for textures produced on a shared device.
use crate::{GpuFrame, Region, Renderer, Result, error, frame};

/// RGB transfer function and alpha representation of a non-sRGB GPU texture.
/// Primaries must be Rec.709/sRGB. No gamut conversion is performed.
#[derive(Clone, Copy, Debug)]
#[repr(u32)]
pub enum TextureEncoding {
    LinearPremultiplied = 0,
    LinearStraight = 1,
    SrgbStraight = 2,
    SrgbPremultiplied = 3,
}

/// A texture allocated by this renderer for an external GPU producer.
/// The caller owns its lifetime and memory; it is outside the buffer pool budget.
/// Write through `texture()` on the shared device/queue, then import a snapshot.
/// Do not destroy the texture while it is in use.
pub struct TextureTarget {
    texture: wgpu::Texture,
    owner: std::sync::Arc<frame::DeviceIdentity>,
}
impl TextureTarget {
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
}
fn supported(format: wgpu::TextureFormat) -> bool {
    matches!(
        format,
        wgpu::TextureFormat::Rgba8Unorm
            | wgpu::TextureFormat::Bgra8Unorm
            | wgpu::TextureFormat::Rgba16Float
            | wgpu::TextureFormat::Rgba32Float
    )
}

pub(crate) fn pipeline(device: &wgpu::Device) -> wgpu::ComputePipeline {
    let source = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("texture import source"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }],
    });
    let buffers = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("texture import buffers"),
        entries: &[(0, 16, false), (1, 4, false), (2, 16, true)].map(|(binding, size, uniform)| {
            wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if uniform {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage { read_only: false }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(size),
                },
                count: None,
            }
        }),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("texture import"),
        bind_group_layouts: &[Some(&source), Some(&buffers)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("texture import"),
        source: wgpu::ShaderSource::Wgsl(include_str!("texture.wgsl").into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("texture import"),
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("import_texture"),
        compilation_options: Default::default(),
        cache: None,
    })
}
impl Renderer {
    /// Allocate a producer-owned 2D texture on the shared device. Individual
    /// allocations are bounded by the renderer budget; aggregate producer memory
    /// is the caller's responsibility. All targets support render attachments,
    /// texture sampling and copy destinations; RGBA targets also support storage.
    pub fn create_texture_target(
        &self,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> Result<TextureTarget> {
        if width == 0 || height == 0 || width > 8192 || height > 8192 || !supported(format) {
            return Err(error("unsupported texture target dimensions or format"));
        }
        let texel_bytes = match format {
            wgpu::TextureFormat::Rgba32Float => 16,
            wgpu::TextureFormat::Rgba16Float => 8,
            _ => 4,
        };
        if u64::from(width) * u64::from(height) * texel_bytes > self.budget_bytes
            || width > self.device.limits().max_texture_dimension_2d
            || height > self.device.limits().max_texture_dimension_2d
        {
            return Err(error("texture target exceeds allocation limit"));
        }
        let _pool = self
            .pool
            .lock()
            .map_err(|_| error("renderer resource pool poisoned"))?;
        if _pool.failed {
            return Err(error("renderer unavailable after a GPU failure"));
        }
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let allocation = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let mut usage = wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::RENDER_ATTACHMENT;
        if format != wgpu::TextureFormat::Bgra8Unorm {
            usage |= wgpu::TextureUsages::STORAGE_BINDING;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("external producer target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        let allocation_error = pollster::block_on(allocation.pop());
        let validation_error = pollster::block_on(validation.pop());
        if let Some(e) = allocation_error.or(validation_error) {
            return Err(error(e));
        }
        Ok(TextureTarget {
            texture,
            owner: self.owner.clone(),
        })
    }
    /// Convert a shared-device texture into an immutable scene-linear frame.
    /// The caller must submit its writes on the shared queue before this call,
    /// and must not modify/destroy the texture until this call returns.
    /// Only a four-byte validation flag is downloaded; pixels remain on GPU.
    /// Source texture memory is caller-owned and excluded from the buffer budget.
    /// Supports single-layer, single-mip, single-sample 2D RGBA8/BGRA8 unorm,
    /// RGBA16Float and RGBA32Float textures with TEXTURE_BINDING usage.
    pub fn import_texture(
        &self,
        target: &TextureTarget,
        encoding: TextureEncoding,
    ) -> Result<GpuFrame> {
        if !std::sync::Arc::ptr_eq(&target.owner, &self.owner) {
            return Err(error("texture target belongs to another renderer"));
        }
        let texture = target.texture();
        let width = texture.width();
        let height = texture.height();
        let size = u64::from(width) * u64::from(height) * 16;
        let limits = self.device.limits();
        if size > limits.max_buffer_size || size > limits.max_storage_buffer_binding_size {
            return Err(error("texture import exceeds adapter buffer limits"));
        }
        let mut pool = self
            .pool
            .lock()
            .map_err(|_| error("renderer resource pool poisoned"))?;
        if pool.failed {
            return Err(error("renderer unavailable after a GPU failure"));
        }
        let available = self.budget_bytes.saturating_sub(pool.stats().held_bytes);
        pool.trim(self.pool_limit_bytes.min(available));
        if size + 24 > available {
            return Err(error("texture import exceeds GPU buffer budget"));
        }
        // Validate the source view before allocating or submitting.
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let source = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture import source"),
            layout: &self.texture_pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(error(e));
        }
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let allocation = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let result = (|| {
            let mut resources = pool.frame(self.budget_bytes);
            let output = resources.take(
                &self.device,
                size,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                "imported frame",
            )?;
            let flag = resources.take(
                &self.device,
                4,
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                "texture validation",
            )?;
            let params = resources.take(
                &self.device,
                16,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                "texture encoding",
            )?;
            let status = resources.take(
                &self.device,
                4,
                wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                "texture validation readback",
            )?;
            self.queue.write_buffer(&flag, 0, &[0; 4]);
            self.queue.write_buffer(
                &params,
                0,
                bytemuck::cast_slice(&[width, height, encoding as u32, 0]),
            );
            let buffers = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("texture import buffers"),
                layout: &self.texture_pipeline.get_bind_group_layout(1),
                entries: &[&output, &flag, &params]
                    .into_iter()
                    .enumerate()
                    .map(|(i, b)| wgpu::BindGroupEntry {
                        binding: i as u32,
                        resource: b.as_entire_binding(),
                    })
                    .collect::<Vec<_>>(),
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
                pass.set_pipeline(&self.texture_pipeline);
                pass.set_bind_group(0, &source, &[]);
                pass.set_bind_group(1, &buffers, &[]);
                pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
            }
            encoder.copy_buffer_to_buffer(&flag, 0, &status, 0, 4);
            resources.submitted();
            let flags: Vec<u32> =
                self.map_values(&status, self.queue.submit([encoder.finish()]))?;
            resources.completed();
            if flags[0] != 0 {
                return Err(error("texture contains invalid alpha or non-finite pixels"));
            }
            Ok(GpuFrame {
                identity: frame::next_identity()?,
                storage: resources.pin(&output),
                owner: self.owner.clone(),
                width,
                height,
                region: Region {
                    x: 0,
                    y: 0,
                    width,
                    height,
                },
            })
        })();
        let allocation_error = pollster::block_on(allocation.pop());
        let validation_error = pollster::block_on(validation.pop());
        if let Some(e) = allocation_error.or(validation_error) {
            pool.trim(0);
            pool.failed = true;
            return Err(error(e));
        }
        let available = self.budget_bytes.saturating_sub(pool.stats().held_bytes);
        pool.trim(self.pool_limit_bytes.min(available));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn texture(
        renderer: &Renderer,
        format: wgpu::TextureFormat,
        data: &[u8],
        stride: u32,
    ) -> TextureTarget {
        let target = renderer.create_texture_target(2, 1, format).unwrap();
        let texture = target.texture();
        renderer.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(1),
            },
            texture.size(),
        );
        target
    }
    fn near(a: f32, b: f32) {
        assert!((a - b).abs() < 0.00002, "{a} != {b}");
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn texture_import_preserves_color_hdr_ownership_and_budget() {
        let mut renderer = Renderer::new().unwrap();
        let source = texture(
            &renderer,
            wgpu::TextureFormat::Rgba8Unorm,
            &[128, 64, 255, 128, 255, 255, 255, 0],
            8,
        );
        let alpha = 128.0 / 255.0;
        let decode = |v: f32| {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        for encoding in [
            TextureEncoding::LinearStraight,
            TextureEncoding::LinearPremultiplied,
            TextureEncoding::SrgbStraight,
            TextureEncoding::SrgbPremultiplied,
        ] {
            let frame = renderer.import_texture(&source, encoding).unwrap();
            let pixels = renderer.read_frame(&frame).unwrap().pixels;
            let mut expected = [128.0 / 255.0, 64.0 / 255.0, 1.0];
            for v in &mut expected {
                if matches!(encoding, TextureEncoding::SrgbPremultiplied) {
                    *v /= alpha;
                }
                if matches!(
                    encoding,
                    TextureEncoding::SrgbStraight | TextureEncoding::SrgbPremultiplied
                ) {
                    *v = decode(*v);
                }
                if !matches!(encoding, TextureEncoding::LinearPremultiplied) {
                    *v *= alpha;
                }
            }
            for (a, b) in pixels[0][..3].iter().zip(expected) {
                near(*a, b);
            }
            near(pixels[0][3], alpha);
            assert_eq!(pixels[1], [0.0; 4]);
        }
        let bgra = texture(
            &renderer,
            wgpu::TextureFormat::Bgra8Unorm,
            &[255, 0, 0, 255, 0, 0, 0, 0],
            8,
        );
        let bgra_frame = renderer
            .import_texture(&bgra, TextureEncoding::LinearStraight)
            .unwrap();
        assert_eq!(
            renderer.read_frame(&bgra_frame).unwrap().pixels[0],
            [0.0, 0.0, 1.0, 1.0]
        );
        drop(bgra_frame);
        let half: [u16; 8] = [0x4800, 0x3400, 0x4000, 0x3800, 0, 0, 0, 0];
        let half_target = texture(
            &renderer,
            wgpu::TextureFormat::Rgba16Float,
            bytemuck::cast_slice(&half),
            16,
        );
        let half_frame = renderer
            .import_texture(&half_target, TextureEncoding::LinearStraight)
            .unwrap();
        assert_eq!(
            renderer.read_frame(&half_frame).unwrap().pixels[0],
            [4.0, 0.125, 1.0, 0.5]
        );
        drop(half_frame);
        let floats = [[8.0f32, -0.25, 2.0, 0.5], [0.0; 4]];
        let hdr = texture(
            &renderer,
            wgpu::TextureFormat::Rgba32Float,
            bytemuck::cast_slice(&floats),
            32,
        );
        let frame = std::sync::Arc::new(
            renderer
                .import_texture(&hdr, TextureEncoding::LinearStraight)
                .unwrap(),
        );
        assert_eq!(
            renderer.read_frame(&frame).unwrap().pixels[0],
            [4.0, -0.125, 1.0, 0.5]
        );
        // A converted frame is directly consumable by the graph, without CPU uploads.
        let node = veylvik_document::Node::new(
            "texture",
            veylvik_document::Operation::Image(veylvik_document::nodes::Image {
                asset: "texture".into(),
            }),
            &[],
        );
        let mut document = veylvik_document::demo();
        document.width = 2;
        document.height = 1;
        document.output = node.id;
        document.nodes = [(node.id, node)].into();
        let plan = crate::Plan::compile(&document).unwrap();
        let mut inputs = crate::ImageInputs::default();
        inputs.insert_gpu("texture", frame.clone());
        let output = renderer
            .render_frame(
                &plan,
                crate::RenderRequest::full(veylvik_document::Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap();
        assert_eq!(output.pixels[0], [4.0, -0.125, 1.0, 0.5]);
        assert_eq!(renderer.resource_stats().unwrap().source_uploads, 0);
        // Later producer writes cannot mutate an imported snapshot.
        renderer.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: hdr.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[0; 32],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(32),
                rows_per_image: Some(1),
            },
            hdr.texture().size(),
        );
        assert_eq!(
            renderer.read_frame(&frame).unwrap().pixels[0],
            [4.0, -0.125, 1.0, 0.5]
        );
        let fresh = renderer
            .import_texture(&hdr, TextureEncoding::LinearStraight)
            .unwrap();
        assert_eq!(renderer.read_frame(&fresh).unwrap().pixels[0], [0.0; 4]);
        drop(fresh);
        drop(inputs);
        drop(frame);
        renderer.trim_resources().unwrap();
        renderer.budget_bytes = 55; // 32-byte output + 24-byte validation/uniform workspace.
        assert!(
            renderer
                .import_texture(&source, TextureEncoding::SrgbStraight)
                .is_err()
        );
        assert_eq!(renderer.resource_stats().unwrap().resident_bytes, 0);
        renderer.budget_bytes = 56;
        let frame = renderer
            .import_texture(&source, TextureEncoding::SrgbStraight)
            .unwrap();
        assert_eq!(renderer.resource_stats().unwrap().held_bytes, 32);
        assert!(
            renderer
                .import_texture(&source, TextureEncoding::SrgbStraight)
                .is_err()
        );
        drop(frame);
        renderer.budget_bytes = 1024;
        let foreign = Renderer::new().unwrap();
        let foreign_texture = texture(&foreign, wgpu::TextureFormat::Rgba8Unorm, &[0; 8], 8);
        assert!(
            renderer
                .import_texture(&foreign_texture, TextureEncoding::LinearStraight)
                .is_err()
        );
        for bad in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            let pixels = [[1.0f32, 0.0, 0.0, bad], [0.0; 4]];
            let invalid = texture(
                &renderer,
                wgpu::TextureFormat::Rgba32Float,
                bytemuck::cast_slice(&pixels),
                32,
            );
            assert!(
                renderer
                    .import_texture(&invalid, TextureEncoding::LinearStraight)
                    .is_err()
            );
            assert_eq!(renderer.resource_stats().unwrap().held_bytes, 0);
        }
        let frame = renderer
            .import_texture(&hdr, TextureEncoding::LinearStraight)
            .unwrap();
        assert_eq!(renderer.read_frame(&frame).unwrap().pixels[0], [0.0; 4]);
        assert!(
            renderer
                .create_texture_target(2, 1, wgpu::TextureFormat::Rgba8UnormSrgb)
                .is_err()
        );
        assert!(
            renderer
                .create_texture_target(0, 1, wgpu::TextureFormat::Rgba8Unorm)
                .is_err()
        );
        assert!(
            renderer
                .create_texture_target(8192, 8192, wgpu::TextureFormat::Rgba32Float)
                .is_err()
        );
    }
}
