//! Rasterises evaluated 3D scenes for Render scene: depth-tested, 4x antialiased PBR lit by the studio environment and scene lights.
//!
//! Registered with `Renderer::set_scene_renderer`; the render crate plans the pass
//! and hands over the scene, camera, card images and target buffer.
use std::collections::HashMap;
use std::sync::Mutex;
use veylvik_render::{Result, SceneJob, SceneRenderer};
use veylvik_scene::{Geometry, Light, math};
use wgpu::util::DeviceExt;

const SAMPLES: u32 = 4;
const COLOUR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Near plane in pixels; depth is reversed with an infinite far plane.
const NEAR: f32 = 1.0;
/// Meshes kept on the GPU between frames.
const MESH_CACHE: usize = 128;
/// Strength of the studio environment, as a material's default rig.
const ENVIRONMENT: f32 = 0.65;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ObjectUniform {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 4],
    view_proj: [[f32; 4]; 4],
    camera: [f32; 4],
    surface: [f32; 4],
    params: [f32; 4],
    counts: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LightUniform {
    position: [f32; 4],
    direction: [f32; 4],
    colour: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LookUniform {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 4],
    view_proj: [[f32; 4]; 4],
    view: [[f32; 4]; 4],
    camera: [f32; 4],
    uv_scale: [f32; 4],
}
struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    used: u64,
}
pub struct Engine {
    layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    look_layout: wgpu::BindGroupLayout,
    look: wgpu::RenderPipeline,
    lit: wgpu::RenderPipeline,
    card: wgpu::RenderPipeline,
    copy: wgpu::ComputePipeline,
    meshes: Mutex<(HashMap<String, GpuMesh>, u64)>,
    /// Panorama to studio-layout conversion.
    resample: wgpu::ComputePipeline,
    reduce: wgpu::ComputePipeline,
    irradiance: wgpu::ComputePipeline,
    /// The converted panorama, reused between frames.
    panorama: Mutex<Option<wgpu::Buffer>>,
    sky: wgpu::RenderPipeline,
}
/// Texels in the studio environment layout (nine levels and the diffuse layer).
const ENVIRONMENT_TEXELS: u64 = 701_096;
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SkyUniform {
    camera: [[f32; 4]; 4],
    view: [f32; 4],
    more: [f32; 4],
}
fn error(message: impl ToString) -> veylvik_render::Error {
    veylvik_render::Error::Message(message.to_string())
}
/// Row-major to WGSL's column-major layout.
fn columns(m: &math::Mat4) -> [[f32; 4]; 4] {
    std::array::from_fn(|c| std::array::from_fn(|r| m[r][c]))
}
/// Inverse transpose of the upper 3x3, for normals under non-uniform scale.
fn normal_matrix(m: &math::Mat4) -> math::Mat4 {
    let a = |r: usize, c: usize| m[r][c];
    let cof = |r0, r1, c0, c1| a(r0, c0) * a(r1, c1) - a(r0, c1) * a(r1, c0);
    let c = [
        [cof(1, 2, 1, 2), -cof(1, 2, 0, 2), cof(1, 2, 0, 1)],
        [-cof(0, 2, 1, 2), cof(0, 2, 0, 2), -cof(0, 2, 0, 1)],
        [cof(0, 1, 1, 2), -cof(0, 1, 0, 2), cof(0, 1, 0, 1)],
    ];
    // The cofactor matrix is the inverse transpose up to a positive scale for
    // proper transforms; its sign follows the determinant.
    let det = a(0, 0) * c[0][0] + a(0, 1) * c[0][1] + a(0, 2) * c[0][2];
    let s = if det < 0.0 { -1.0 } else { 1.0 };
    let mut out = math::IDENTITY;
    for (r, row) in c.iter().enumerate() {
        for (k, v) in row.iter().enumerate() {
            out[r][k] = v * s;
        }
    }
    out
}
impl Engine {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("3D scene"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}{}",
                    include_str!("scene.wgsl"),
                    veylvik_render::ENVIRONMENT_WGSL
                )
                .into(),
            ),
        });
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("3D object"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage(1),
                storage(2),
                storage(3),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("3D object"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
        let pipeline = |entry: &str, blend: Option<wgpu::BlendState>, write: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: 32,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attributes,
                    }],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(write),
                    depth_compare: Some(wgpu::CompareFunction::Greater),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: SAMPLES,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOUR,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let lit = pipeline("lit", None, true);
        let card = pipeline(
            "card",
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            false,
        );
        // Library looks: the material kernels' bindings in group 0, the object in 1.
        let entry = |binding, ty, writable: bool| wgpu::BindGroupLayoutEntry {
            binding,
            // Only the object uniform (group 1) feeds the vertex stage.
            visibility: if ty == 0 && binding == 0 && !writable {
                wgpu::ShaderStages::VERTEX_FRAGMENT
            } else {
                wgpu::ShaderStages::FRAGMENT
            },
            ty: match ty {
                0 => wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                _ => wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage {
                        read_only: !writable,
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            },
            count: None,
        };
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("3D look material"),
            entries: &[
                entry(0, 0, false),
                entry(1, 1, false),
                entry(2, 1, false),
                entry(3, 1, true),
                entry(4, 1, false),
                entry(5, 1, false),
                entry(6, 0, false),
                entry(7, 1, false),
                entry(8, 1, false),
            ],
        });
        let look_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("3D look object"),
            entries: &[entry(0, 0, false), entry(1, 1, false)],
        });
        let look_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("3D looks"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}{}",
                    veylvik_render::MATERIAL_WGSL,
                    include_str!("look.wgsl")
                )
                .into(),
            ),
        });
        let look_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("3D looks"),
            bind_group_layouts: &[Some(&material_layout), Some(&look_layout)],
            immediate_size: 0,
        });
        let look = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("3D looks"),
            layout: Some(&look_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &look_module,
                entry_point: Some("look_vertex"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 32,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                }],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Greater),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: SAMPLES,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &look_module,
                entry_point: Some("look_surface"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOUR,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let copy_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("3D resolve copy"),
            source: wgpu::ShaderSource::Wgsl(include_str!("copy.wgsl").into()),
        });
        let copy = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("3D resolve copy"),
            layout: None,
            module: &copy_module,
            entry_point: Some("copy"),
            compilation_options: Default::default(),
            cache: None,
        });
        let env_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("3D environment"),
            source: wgpu::ShaderSource::Wgsl(include_str!("env.wgsl").into()),
        });
        let compute = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: None,
                module: &env_module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let (resample, reduce, irradiance) = (
            compute("resample"),
            compute("reduce"),
            compute("irradiance"),
        );
        let sky_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("3D sky"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}{}",
                    include_str!("sky.wgsl"),
                    veylvik_render::ENVIRONMENT_WGSL
                )
                .into(),
            ),
        });
        let sky = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("3D sky"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &sky_module,
                entry_point: Some("sky_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: SAMPLES,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &sky_module,
                entry_point: Some("sky_fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOUR,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            resample,
            reduce,
            irradiance,
            panorama: Mutex::new(None),
            sky,
            layout,
            material_layout,
            look_layout,
            look,
            lit,
            card,
            copy,
            meshes: Mutex::new((HashMap::new(), 0)),
        }
    }
    /// Convert a premultiplied panorama of `width` x `height` into the studio
    /// environment layout, on `encoder`; returns the buffer to light with.
    fn panorama(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        image: &wgpu::Buffer,
        width: u32,
        height: u32,
    ) -> Result<wgpu::Buffer> {
        let target = {
            let mut slot = self
                .panorama
                .lock()
                .map_err(|_| error("panorama lock poisoned"))?;
            slot.get_or_insert_with(|| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("3D panorama"),
                    size: ENVIRONMENT_TEXELS * 16,
                    usage: wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                })
            })
            .clone()
        };
        let params = |level: u32| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("panorama level"),
                contents: bytemuck::cast_slice(&[width, height, level, 0u32]),
                usage: wgpu::BufferUsages::UNIFORM,
            })
        };
        let mut run = |pipeline: &wgpu::ComputePipeline,
                       bindings: &[(u32, &wgpu::Buffer)],
                       size: (u32, u32)| {
            let entries: Vec<wgpu::BindGroupEntry> = bindings
                .iter()
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding: *binding,
                    resource: buffer.as_entire_binding(),
                })
                .collect();
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("panorama"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &entries,
            });
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(size.0.div_ceil(8), size.1.div_ceil(8), 1);
        };
        let first = params(0);
        run(
            &self.resample,
            &[(0, &first), (1, image), (2, &target)],
            (1024, 512),
        );
        for level in 1..=8u32 {
            let p = params(level);
            run(
                &self.reduce,
                &[(0, &p), (2, &target)],
                ((1024 >> level).max(1), (512 >> level).max(1)),
            );
        }
        run(&self.irradiance, &[(2, &target)], (64, 32));
        Ok(target)
    }
    /// Upload (or reuse) the mesh of an object; meshes are keyed by their geometry.
    fn mesh(
        &self,
        device: &wgpu::Device,
        object: &veylvik_scene::Object,
    ) -> Result<(wgpu::Buffer, wgpu::Buffer, u32)> {
        let key = format!("{:?}", object.geometry);
        let mut guard = self
            .meshes
            .lock()
            .map_err(|_| error("mesh cache lock poisoned"))?;
        let (meshes, clock) = &mut *guard;
        *clock += 1;
        if let Some(m) = meshes.get_mut(&key) {
            m.used = *clock;
            return Ok((m.vertices.clone(), m.indices.clone(), m.count));
        }
        let mesh = veylvik_scene::mesh(object).map_err(error)?;
        let vertices: Vec<f32> = mesh
            .vertices
            .iter()
            .flat_map(|v| v.position.into_iter().chain(v.normal).chain(v.uv))
            .collect();
        let mut indices = mesh.indices;
        if indices.is_empty() {
            indices = vec![0, 0, 0];
        }
        let vertex_bytes: &[u8] = if vertices.is_empty() {
            &[0; 32]
        } else {
            bytemuck::cast_slice(&vertices)
        };
        let entry = GpuMesh {
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("3D vertices"),
                contents: vertex_bytes,
                usage: wgpu::BufferUsages::VERTEX,
            }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("3D indices"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            count: if mesh.vertices.is_empty() {
                0
            } else {
                indices.len() as u32
            },
            used: *clock,
        };
        if meshes.len() >= MESH_CACHE
            && let Some(oldest) = meshes
                .iter()
                .min_by_key(|(_, m)| m.used)
                .map(|(k, _)| k.clone())
        {
            meshes.remove(&oldest);
        }
        let out = (entry.vertices.clone(), entry.indices.clone(), entry.count);
        meshes.insert(key, entry);
        Ok(out)
    }
}
/// Pixels per mesh uv unit: titles' uv is already in pixels; primitives wrap 0..1.
fn uv_scale(geometry: &Geometry) -> [f32; 4] {
    use veylvik_document::nodes::ShapeKind;
    match geometry {
        Geometry::Shape { kind, size, .. } => {
            let around = std::f32::consts::PI * size[0].max(size[2]);
            match kind {
                ShapeKind::Cube | ShapeKind::Plane => {
                    let side = size[0].max(size[1]).max(size[2]);
                    [side, side, 0.0, 0.0]
                }
                _ => [around, size[1], 0.0, 0.0],
            }
        }
        _ => [1.0, 1.0, 0.0, 0.0],
    }
}
fn light_uniform(light: &Light) -> LightUniform {
    use veylvik_document::nodes::LightKind;
    let p = light.position();
    let d = light.direction();
    let outer = (light.cone * 0.5).to_radians();
    let inner = outer * (1.0 - light.softness).max(0.0);
    LightUniform {
        position: [
            p[0],
            p[1],
            p[2],
            match light.kind {
                LightKind::Point => 0.0,
                LightKind::Spot => 1.0,
                LightKind::Sun => 2.0,
            },
        ],
        direction: [d[0], d[1], d[2], outer.cos()],
        colour: [
            light.colour[0] * light.intensity,
            light.colour[1] * light.intensity,
            light.colour[2] * light.intensity,
            inner.cos().max(outer.cos() + 1e-4),
        ],
    }
}
/// Perspective with y down (the scene frame) into wgpu clip space, reversed depth
/// with an infinite far plane.
fn projection(camera: &veylvik_scene::Camera, width: u32, height: u32) -> math::Mat4 {
    let f = veylvik_scene::home_distance(width, camera.focal_length, camera.sensor_width);
    let (fx, fy) = (f / (width as f32 * 0.5), f / (height as f32 * 0.5));
    [
        [fx, 0.0, 0.0, 0.0],
        [0.0, -fy, 0.0, 0.0],
        [0.0, 0.0, 0.0, NEAR],
        [0.0, 0.0, -1.0, 0.0],
    ]
}
impl SceneRenderer for Engine {
    fn render(&self, job: SceneJob<'_>) -> Result<()> {
        let SceneJob {
            device,
            encoder,
            scene,
            camera,
            width,
            height,
            images,
            environment,
            ltc,
            time,
            target,
            ..
        } = job;
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = |label, format, samples, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let colour = texture(
            "3D colour",
            COLOUR,
            SAMPLES,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let resolved = texture(
            "3D resolved",
            COLOUR,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let depth = texture(
            "3D depth",
            DEPTH,
            SAMPLES,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let view_proj = math::mul(
            &projection(&camera, width, height),
            &math::rigid_inverse(&camera.world),
        );
        let eye = math::point(&camera.world, [0.0; 3]);
        let lights: Vec<LightUniform> = scene.lights.iter().map(light_uniform).collect();
        let light_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("3D lights"),
            contents: if lights.is_empty() {
                &[0; std::mem::size_of::<LightUniform>()]
            } else {
                bytemuck::cast_slice(&lights)
            },
            usage: wgpu::BufferUsages::STORAGE,
        });
        let empty = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("no image"),
            contents: &[0; 16],
            usage: wgpu::BufferUsages::STORAGE,
        });
        // Solid objects first; cards after them, far to near, blended over.
        let mut order: Vec<&veylvik_scene::Object> = scene.objects.iter().collect();
        let distance = |o: &veylvik_scene::Object| {
            let p = math::point(&o.world, [0.0; 3]);
            (p[0] - eye[0]).powi(2) + (p[1] - eye[1]).powi(2) + (p[2] - eye[2]).powi(2)
        };
        order.sort_by(|a, b| {
            let card = |o: &veylvik_scene::Object| matches!(o.geometry, Geometry::Card { .. });
            card(a).cmp(&card(b)).then(if card(a) {
                distance(b).total_cmp(&distance(a))
            } else {
                std::cmp::Ordering::Equal
            })
        });
        // The scene's environment: a converted panorama, or the studio, with its
        // strength and turn for plain surfaces, looks and the backdrop.
        let surround = scene.environment;
        let panorama = match surround.and_then(|e| e.image) {
            Some(source) => match images.iter().find(|(id, _)| *id == source) {
                Some((_, image)) => Some(self.panorama(device, encoder, image, width, height)?),
                None => None,
            },
            None => None,
        };
        let environment = panorama.as_ref().unwrap_or(environment);
        let strength = match surround {
            Some(e) if panorama.is_some() => e.intensity,
            Some(e) => ENVIRONMENT * e.intensity,
            None => ENVIRONMENT,
        };
        let turn = surround.map_or(0.0, |e| e.rotation.to_radians());
        let look_environment = surround.map(|e| veylvik_render::EnvironmentOverride {
            strength: e.intensity,
            absolute: panorama.is_some(),
            rotation: e.rotation.to_radians(),
        });
        let view = math::rigid_inverse(&camera.world);
        let mut draws = Vec::new();
        let mut looks = Vec::new();
        for object in order {
            let (vertices, indices, count) = self.mesh(device, object)?;
            if count == 0 {
                continue;
            }
            if let Some(look) = &object.surface.look {
                let (params, lighting) =
                    veylvik_render::material_uniforms(look, width, height, time, look_environment)?;
                let buffer = |label, bytes: &[u8], usage| {
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(label),
                        contents: bytes,
                        usage,
                    })
                };
                let p = buffer("look parameters", &params, wgpu::BufferUsages::UNIFORM);
                let studio = buffer("look lighting", &lighting, wgpu::BufferUsages::UNIFORM);
                let scratch = buffer("look scratch", &[0; 16], wgpu::BufferUsages::STORAGE);
                let material = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("3D look material"),
                    layout: &self.material_layout,
                    entries: &[
                        (0, &p),
                        (1, &empty),
                        (2, &empty),
                        (3, &scratch),
                        (4, &empty),
                        (5, ltc),
                        (6, &studio),
                        (7, environment),
                        (8, &empty),
                    ]
                    .map(|(binding, buffer)| wgpu::BindGroupEntry {
                        binding,
                        resource: buffer.as_entire_binding(),
                    }),
                });
                let uniform = LookUniform {
                    model: columns(&object.world),
                    normal: columns(&normal_matrix(&object.world)),
                    view_proj: columns(&view_proj),
                    view: columns(&view),
                    camera: [eye[0], eye[1], eye[2], 1.0],
                    uv_scale: {
                        let [x, y, ..] = uv_scale(&object.geometry);
                        [x, y, lights.len() as f32, 0.0]
                    },
                };
                let uniform = buffer(
                    "3D look object",
                    bytemuck::bytes_of(&uniform),
                    wgpu::BufferUsages::UNIFORM,
                );
                let object_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("3D look object"),
                    layout: &self.look_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: uniform.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: light_buffer.as_entire_binding(),
                        },
                    ],
                });
                looks.push((material, object_bind, vertices, indices, count));
                continue;
            }
            let (pipeline, image) = match &object.geometry {
                Geometry::Card { image, .. } => (
                    &self.card,
                    images.iter().find(|(id, _)| id == image).map(|(_, b)| *b),
                ),
                _ => (&self.lit, None),
            };
            let uniform = ObjectUniform {
                model: columns(&object.world),
                normal: columns(&normal_matrix(&object.world)),
                view_proj: columns(&view_proj),
                camera: [eye[0], eye[1], eye[2], 1.0],
                surface: [
                    object.surface.colour[0],
                    object.surface.colour[1],
                    object.surface.colour[2],
                    object.surface.roughness,
                ],
                params: [
                    object.surface.metallic,
                    f32::from(object.surface.unlit),
                    width as f32,
                    height as f32,
                ],
                counts: [lights.len() as f32, strength, turn, 0.0],
            };
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("3D object"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("3D object"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: light_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: environment.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: image.unwrap_or(&empty).as_entire_binding(),
                    },
                ],
            });
            draws.push((pipeline, bind, vertices, indices, count));
        }
        let sky = surround.filter(|e| e.background).map(|e| {
            let uniform = SkyUniform {
                camera: columns(&camera.world),
                view: [
                    width as f32,
                    height as f32,
                    veylvik_scene::home_distance(width, camera.focal_length, camera.sensor_width),
                    strength,
                ],
                more: [turn, e.blur * 8.0, 0.0, 0.0],
            };
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("3D sky"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("3D sky"),
                layout: &self.sky.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: environment.as_entire_binding(),
                    },
                ],
            })
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3D scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &colour,
                    depth_slice: None,
                    resolve_target: Some(&resolved),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(sky) = &sky {
                pass.set_pipeline(&self.sky);
                pass.set_bind_group(0, sky, &[]);
                pass.draw(0..3, 0..1);
            }
            pass.set_pipeline(&self.look);
            for (material, object, vertices, indices, count) in &looks {
                pass.set_bind_group(0, material, &[]);
                pass.set_bind_group(1, object, &[]);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..1);
            }
            for (pipeline, bind, vertices, indices, count) in &draws {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, bind, &[]);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..1);
            }
        }
        let copy = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("3D resolve copy"),
            layout: &self.copy.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&resolved),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: target.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&self.copy);
        pass.set_bind_group(0, &copy, &[]);
        pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        Ok(())
    }
}
