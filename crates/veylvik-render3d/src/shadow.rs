//! Shadow maps for scene lights: where each light sees the scene from, the depth maps rendered from there, and how surfaces read them.
use veylvik_document::nodes::LightKind;
use veylvik_scene::{Geometry, Light, Scene, math};

/// Lights that cast shadows, per scene.
pub(crate) const MAX_SHADOWS: u32 = 4;
/// Shadow map side in texels.
pub(crate) const SIZE: u32 = 2048;
pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Directions the scene's sky visibility is measured from (ambient occlusion).
pub(crate) const SKY_VIEWS: u32 = 16;
pub(crate) const SKY_SIZE: u32 = 1024;
/// A depth map array, one layer per view, and its comparison sampler.
pub(crate) struct Maps {
    texture: wgpu::Texture,
    pub array: wgpu::TextureView,
    pub layers: Vec<wgpu::TextureView>,
    pub sampler: wgpu::Sampler,
}
impl Maps {
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
    pub fn new(device: &wgpu::Device) -> Self {
        Self::sized(device, SIZE, MAX_SHADOWS)
    }
    pub fn sized(device: &wgpu::Device, side: u32, count: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("3D shadow maps"),
            size: wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: count,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            // Copied out, too, for particles lit by the scene (ShadowJob).
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let array = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let layers = (0..count)
            .map(|layer| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("3D shadow compare"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        Self {
            texture: texture.clone(),
            array,
            layers,
            sampler,
        }
    }
}
/// Centre and radius of everything that casts shadows (not cards or shadow
/// catchers): shadow and occlusion maps hold only casters, so fitting them to the
/// casters keeps their detail when a catcher is a huge ground.
pub(crate) fn bounds(scene: &Scene) -> Option<([f32; 3], f32)> {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for object in scene.objects.iter().filter(|o| {
        !o.surface.catcher && !matches!(o.geometry, Geometry::Card { .. }) && o.instanced.is_none()
    }) {
        let (a, b) = object.local_bounds();
        for i in 0..8 {
            let p = math::point(
                &object.world,
                [
                    if i & 1 == 0 { a[0] } else { b[0] },
                    if i & 2 == 0 { a[1] } else { b[1] },
                    if i & 4 == 0 { a[2] } else { b[2] },
                ],
            );
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
    }
    if lo[0] > hi[0] {
        return None;
    }
    let centre = [0, 1, 2].map(|k| (lo[k] + hi[k]) * 0.5);
    let radius =
        ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt() * 0.5;
    Some((centre, radius.max(1.0)))
}
fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    v.map(|x| x / l)
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
/// A camera-like frame at `eye` looking along `forward`.
fn looking(eye: [f32; 3], forward: [f32; 3]) -> math::Mat4 {
    let z = norm(forward.map(|v| -v));
    let hint = if z[1].abs() > 0.99 {
        [0.0, 0.0, 1.0]
    } else {
        [0.0, -1.0, 0.0]
    };
    let x = norm(cross(hint, z));
    let y = cross(z, x);
    [
        [x[0], y[0], z[0], eye[0]],
        [x[1], y[1], z[1], eye[1]],
        [x[2], y[2], z[2], eye[2]],
        [0.0, 0.0, 0.0, 1.0],
    ]
}
/// An orthographic view along `forward` over the scene's bounds, and its normal offset.
pub(crate) fn parallel(
    forward: [f32; 3],
    centre: [f32; 3],
    radius: f32,
    side: u32,
) -> (math::Mat4, f32) {
    let eye = [0, 1, 2].map(|k| centre[k] - forward[k] * radius * 2.0);
    let (near, far) = (radius * 0.5, radius * 3.5);
    let projection = [
        [1.0 / radius, 0.0, 0.0, 0.0],
        [0.0, -1.0 / radius, 0.0, 0.0],
        [0.0, 0.0, -1.0 / (far - near), -near / (far - near)],
        [0.0, 0.0, 0.0, 1.0],
    ];
    (
        math::mul(&projection, &math::rigid_inverse(&looking(eye, forward))),
        radius * 2.0 / side as f32 * 1.5,
    )
}
/// Evenly spread unit directions over the whole sphere (a Fibonacci lattice).
pub(crate) fn sky_directions() -> Vec<[f32; 3]> {
    let golden = std::f32::consts::PI * (3.0 - 5f32.sqrt());
    (0..SKY_VIEWS)
        .map(|i| {
            let y = 1.0 - 2.0 * (i as f32 + 0.5) / SKY_VIEWS as f32;
            let r = (1.0 - y * y).max(0.0).sqrt();
            let a = golden * i as f32;
            [r * a.cos(), y, r * a.sin()]
        })
        .collect()
}
/// The light's view-projection (depth 0..1) and the normal offset that keeps lit
/// surfaces from shadowing themselves, or None when it cannot see the scene.
pub(crate) fn view(light: &Light, centre: [f32; 3], radius: f32) -> Option<(math::Mat4, f32)> {
    let texel = |span: f32| span / SIZE as f32 * 1.5;
    match light.kind {
        LightKind::Sun => Some(parallel(light.direction(), centre, radius, SIZE)),
        LightKind::Area => None,
        LightKind::Spot | LightKind::Point => {
            let eye = light.position();
            let to = [0, 1, 2].map(|k| centre[k] - eye[k]);
            let distance = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
            let (forward, half) = if light.kind == LightKind::Spot {
                (
                    light.direction(),
                    (light.cone * 0.5 + 5.0).min(85.0).to_radians(),
                )
            } else if distance > radius {
                (
                    norm(to),
                    ((radius / distance).min(0.999).asin().to_degrees() + 5.0)
                        .min(85.0)
                        .to_radians(),
                )
            } else {
                (
                    norm(if distance > 1e-3 { to } else { [0.0, 1.0, 0.0] }),
                    85f32.to_radians(),
                )
            };
            let near = ((distance - radius) * 0.5).max(1.0);
            let far = distance + radius * 1.5 + 1.0;
            let f = 1.0 / half.tan();
            let projection = [
                [f, 0.0, 0.0, 0.0],
                [0.0, -f, 0.0, 0.0],
                [0.0, 0.0, -far / (far - near), -far * near / (far - near)],
                [0.0, 0.0, -1.0, 0.0],
            ];
            Some((
                math::mul(&projection, &math::rigid_inverse(&looking(eye, forward))),
                texel(2.0 * distance.max(radius) * half.tan()),
            ))
        }
    }
}

impl crate::Engine {
    /// Render the depth of every solid (not cards or shadow catchers) through
    /// `matrix` into `target`.
    pub(crate) fn depth_pass(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        scene: &Scene,
        matrix: &math::Mat4,
        target: &wgpu::TextureView,
    ) -> veylvik_render::Result<()> {
        use wgpu::util::DeviceExt;
        let mut casts = Vec::new();
        for object in &scene.objects {
            // Copies at particles cast no shadows (they are not where the object is).
            if object.surface.catcher
                || matches!(object.geometry, Geometry::Card { .. })
                || object.instanced.is_some()
            {
                continue;
            }
            let (vertices, indices, count) = self.mesh(device, object)?;
            if count == 0 {
                continue;
            }
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("3D depth object"),
                contents: bytemuck::bytes_of(&crate::ShadowUniform {
                    model: crate::columns(&object.world),
                    light: crate::columns(matrix),
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("3D depth object"),
                layout: &self.shadow_pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                }],
            });
            casts.push((bind, vertices, indices, count));
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("3D depth map"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: target,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.shadow_pipeline);
        for (bind, vertices, indices, count) in &casts {
            pass.set_bind_group(0, bind, &[]);
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..*count, 0, 0..1);
        }
        Ok(())
    }
}

/// Depth maps for lights asked for by a [`veylvik_render::ShadowJob`] (particles lit by
/// the scene): one layer each, kept between frames for a side.
pub(crate) struct ParticleMaps {
    side: u32,
    maps: std::sync::Arc<Maps>,
}
impl crate::Engine {
    /// The maps of [`veylvik_render::ShadowJob`]: each asked light that casts shadows
    /// sees the scene as for 3D surfaces (an area light from the middle of its panel, as
    /// a wide spot), and its view-projection and map are written to the job's target.
    pub(crate) fn particle_shadows(
        &self,
        job: veylvik_render::ShadowJob<'_>,
    ) -> veylvik_render::Result<()> {
        use veylvik_render::{SHADOW_HEADER, SHADOW_LIGHTS, SHADOW_VALUES};
        use wgpu::util::DeviceExt;
        let side = job.side;
        let scene = self.expand(job.scene, job.models)?;
        let maps = {
            let mut slot = self.particle_maps.lock().map_err(|_| {
                veylvik_render::Error::Message("particle shadow lock poisoned".into())
            })?;
            match slot.as_ref() {
                Some(kept) if kept.side == side => kept.maps.clone(),
                _ => {
                    let maps =
                        std::sync::Arc::new(Maps::sized(job.device, side, SHADOW_LIGHTS as u32));
                    *slot = Some(ParticleMaps {
                        side,
                        maps: maps.clone(),
                    });
                    maps
                }
            }
        };
        let mut header = vec![0.0f32; SHADOW_VALUES * SHADOW_LIGHTS];
        if let Some((centre, radius)) = bounds(&scene) {
            for (k, &index) in job.lights.iter().take(SHADOW_LIGHTS).enumerate() {
                let Some(light) = job.scene.lights.get(index) else {
                    continue;
                };
                if !light.shadows || light.intensity <= 0.0 {
                    continue;
                }
                let mut seen = *light;
                if seen.kind == LightKind::Area {
                    seen.kind = LightKind::Spot;
                    seen.cone = 170.0;
                }
                let Some((matrix, _)) = view(&seen, centre, radius) else {
                    continue;
                };
                self.depth_pass(job.device, job.encoder, &scene, &matrix, &maps.layers[k])?;
                for (row, values) in matrix.iter().enumerate() {
                    header[SHADOW_VALUES * k + 4 * row..SHADOW_VALUES * k + 4 * row + 4]
                        .copy_from_slice(values);
                }
                header[SHADOW_VALUES * k + 16] = 1.0;
                job.encoder.copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo {
                        texture: maps.texture(),
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: 0,
                            y: 0,
                            z: k as u32,
                        },
                        aspect: wgpu::TextureAspect::DepthOnly,
                    },
                    wgpu::TexelCopyBufferInfo {
                        buffer: job.target,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: SHADOW_HEADER
                                + k as u64 * u64::from(side) * u64::from(side) * 4,
                            bytes_per_row: Some(side * 4),
                            rows_per_image: Some(side),
                        },
                    },
                    wgpu::Extent3d {
                        width: side,
                        height: side,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
        let values = job
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("particle shadow views"),
                contents: bytemuck::cast_slice(&header),
                usage: wgpu::BufferUsages::COPY_SRC,
            });
        job.encoder
            .copy_buffer_to_buffer(&values, 0, job.target, 0, (header.len() * 4) as u64);
        Ok(())
    }
}
