//! Reproducible offline environment filtering, executed on the renderer's Vulkan device.
use crate::{Renderer, Result, error};
impl Renderer {
    /// Nine GGX levels (1024x512 down to 4x2), then 64x32 diffuse irradiance/pi.
    pub fn prefilter_environment(
        &self,
        width: u32,
        height: u32,
        pixels: &[[f32; 4]],
        neutral: bool,
    ) -> Result<Vec<[f32; 4]>> {
        use wgpu::util::DeviceExt;
        if width < 2
            || height < 2
            || width > 8192
            || height > 4096
            || pixels.len() != width as usize * height as usize
            || pixels.iter().any(|p| {
                p[..3]
                    .iter()
                    .any(|v| !v.is_finite() || *v < 0.0 || *v > 65504.0)
            })
        {
            return Err(error("Invalid HDR environment dimensions or radiance"));
        }
        let mip_count = width.max(height).ilog2() + 1;
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("HDR studio source"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: mip_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let half: Vec<[u16; 4]> = pixels
            .iter()
            .map(|p| p.map(|v| half::f16::from_f32(v).to_bits()))
            .collect();
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&half),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 8),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let mip_shader=self.device.create_shader_module(wgpu::ShaderModuleDescriptor{label:Some("HDR mip filter"),source:wgpu::ShaderSource::Wgsl(r#"
@group(0) @binding(0) var source:texture_2d<f32>;
@group(0) @binding(1) var result:texture_storage_2d<rgba16float,write>;
@compute @workgroup_size(8,8) fn downsample(@builtin(global_invocation_id) gid:vec3<u32>) {
 let size=textureDimensions(result);if any(gid.xy>=size){return;}
 let hi=vec2<i32>(textureDimensions(source))-vec2<i32>(1);let p=vec2<i32>(gid.xy)*2;
 let c=(textureLoad(source,min(p,hi),0)+textureLoad(source,min(p+vec2<i32>(1,0),hi),0)+textureLoad(source,min(p+vec2<i32>(0,1),hi),0)+textureLoad(source,min(p+vec2<i32>(1,1),hi),0))*0.25;
 textureStore(result,vec2<i32>(gid.xy),c);
}"#.into())});
        let mip_pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("HDR mip filter"),
                layout: None,
                module: &mip_shader,
                entry_point: Some("downsample"),
                compilation_options: Default::default(),
                cache: None,
            });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for level in 1..mip_count {
            let view = |mip| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: mip,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            };
            let src = view(level - 1);
            let dst = view(level);
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("HDR mip"),
                layout: &mip_pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&src),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&dst),
                    },
                ],
            });
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&mip_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(
                (width >> level).max(1).div_ceil(8),
                (height >> level).max(1).div_ceil(8),
                1,
            );
        }
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("GGX environment convolution"),
                source: wgpu::ShaderSource::Wgsl(include_str!("environment_prefilter.wgsl").into()),
            });
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("GGX environment convolution"),
                layout: None,
                module: &module,
                entry_point: Some("prefilter"),
                compilation_options: Default::default(),
                cache: None,
            });
        let bytes = 701096 * 16;
        let output = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Filtered studio environment"),
                contents: &vec![0; bytes],
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Filtered environment readback"),
            size: bytes as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view = texture.create_view(&Default::default());
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let options = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Environment bake options"),
                contents: bytemuck::cast_slice(&[u32::from(neutral), 0, 0, 0]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Environment bake"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: output.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: options.as_entire_binding(),
                },
            ],
        });
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(128, 64, 10);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes as u64);
        self.map_values(&readback, self.queue.submit([encoder.finish()]))
    }
}
