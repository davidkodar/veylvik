//! Volumes (smoke, mist, haze) in the interactive renderer: after the solids, a pass steps each pixel's camera ray through the media to the nearest solid (volume_pass.wgsl), with the same medium the path tracer reads (veylvik-trace's volume.wgsl).
use crate::{Engine, error};
use veylvik_render::Result;
use veylvik_trace::{Medium, VolumeShape};
use wgpu::util::DeviceExt;

/// Steps along each camera ray through the media.
const STEPS: f32 = 64.0;

/// A scene's volume as both renderers read it.
pub(crate) fn medium(v: &veylvik_scene::Volume) -> Medium {
    use veylvik_document::nodes::VolumeShape as S;
    Medium {
        world: v.world,
        size: v.size,
        shape: match v.shape {
            S::Box => VolumeShape::Box,
            S::Sphere => VolumeShape::Sphere,
            S::Ground => VolumeShape::Ground,
        },
        // Density is per 100 pixels.
        sigma: v.density / 100.0,
        absorption: v.absorption,
        forward: v.forward,
        softness: v.softness,
        colour: v.colour,
        noise: v.noise,
        noise_size: v.noise_size,
        detail: v.detail,
        seed: v.seed,
        drift: v.drift,
    }
}
/// The scene's media packed for the shared WGSL, those whose placing can be undone.
pub(crate) fn packed(scene: &veylvik_scene::Scene) -> Vec<[f32; 4]> {
    scene
        .volumes
        .iter()
        .filter_map(|v| medium(v).packed())
        .flatten()
        .collect()
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Fog {
    camera: [[f32; 4]; 3],
    lens: [f32; 4],
    counts: [f32; 4],
}
/// What the volume pass reads: the solids' picture and depth, the scene's lights and
/// shadow maps, and the environment.
pub(crate) struct Behind<'a> {
    pub resolved: &'a wgpu::TextureView,
    pub depth: &'a wgpu::TextureView,
    pub lights: &'a wgpu::Buffer,
    pub light_count: usize,
    pub environment: &'a wgpu::Buffer,
    pub shadow_maps: &'a wgpu::TextureView,
    pub shadow_sampler: &'a wgpu::Sampler,
    pub strength: f32,
}
impl Engine {
    fn volume_pipeline(&self, device: &wgpu::Device) -> &wgpu::ComputePipeline {
        self.volume_pass.get_or_init(|| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("3D volumes"),
                source: wgpu::ShaderSource::Wgsl(
                    format!(
                        "{}{}\n{}\n{}",
                        include_str!("scene.wgsl"),
                        veylvik_render::ENVIRONMENT_WGSL,
                        veylvik_trace::VOLUME_WGSL,
                        include_str!("volume_pass.wgsl")
                    )
                    .into(),
                ),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("3D volumes"),
                layout: None,
                module: &module,
                entry_point: Some("through_volumes"),
                compilation_options: Default::default(),
                cache: None,
            })
        })
    }
    /// The picture with the scene's volumes in front of and among its solids, or None when
    /// it has none.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn through_volumes(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        scene: &veylvik_scene::Scene,
        camera: &veylvik_scene::Camera,
        [width, height]: [u32; 2],
        behind: Behind<'_>,
    ) -> Result<Option<wgpu::TextureView>> {
        let media = packed(scene);
        if media.is_empty() {
            return Ok(None);
        }
        let count = media.len() / 8;
        if count > 64 {
            return Err(error("a scene may hold at most 64 volumes"));
        }
        let pipeline = self.volume_pipeline(device);
        let fogged = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("3D volumes"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: crate::COLOUR,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let fog = Fog {
            camera: [camera.world[0], camera.world[1], camera.world[2]],
            lens: [
                veylvik_scene::home_distance(width, camera.focal_length, camera.sensor_width),
                width as f32,
                height as f32,
                crate::NEAR,
            ],
            counts: [
                count as f32,
                behind.light_count as f32,
                behind.strength,
                STEPS,
            ],
        };
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("3D volumes"),
            contents: bytemuck::bytes_of(&fog),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let media = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("3D media"),
            contents: bytemuck::cast_slice(&media),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let scene_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("3D volumes scene"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: behind.lights.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: behind.environment.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(behind.shadow_maps),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(behind.shadow_sampler),
                },
            ],
        });
        let fog_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("3D volumes"),
            layout: &pipeline.get_bind_group_layout(1),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(behind.resolved),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(behind.depth),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&fogged),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: media.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("3D volumes"),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &scene_group, &[]);
        pass.set_bind_group(1, &fog_group, &[]);
        pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        Ok(Some(fogged))
    }
}
