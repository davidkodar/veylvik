//! The tracer on the GPU: one compute program over the packed scene, run in passes of a few samples each, sent to the GPU one at a time so a long picture never holds the GPU in one piece, averaging into the picture as it goes, or, to be denoised, into a buffer of its own with each pixel's guides (denoise.rs). Mesh geometry stays on the GPU while the same meshes are in the scene; placings, surfaces, the top tree and lights are sent each frame.
use crate::scene::{GLASS, Mat4, Packed};
use std::sync::Mutex;
use wgpu::util::DeviceExt;

/// A light as the tracer reads it, 80 bytes: position and kind (0 point, 1 spot, 2 sun,
/// 3 area); direction and the cosine of a spot's outer half-angle; colour times strength
/// and the inner half-angle's cosine; an area light's half-width axis and feather, and its
/// half-height axis and how strongly it shows in shiny surfaces (0 to 1), plus 2 when it
/// casts shadows (otherwise it lights through objects). The same meanings as the 3D
/// rasteriser's lights.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Light {
    pub position: [f32; 4],
    pub direction: [f32; 4],
    pub colour: [f32; 4],
    pub right: [f32; 4],
    pub up: [f32; 4],
}
/// Where the camera is and how it sees.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// Camera to world (row-major); it looks along its -z with y down.
    pub camera: Mat4,
    /// Focal length in pixels of the frame.
    pub focal: f32,
    /// The lens's radius in scene units (0: everything sharp).
    pub lens_radius: f32,
    /// How far in front of the camera is sharp.
    pub focus: f32,
}
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// Paths through each pixel.
    pub samples: u32,
    /// Bounces after the first surface.
    pub bounces: u32,
    /// The brightest a bounce may add to a pixel (0: no limit): keeps rare bright paths
    /// from showing as speckles.
    pub clamp: f32,
    /// Also draw light from where the environment is bright, not only where reflected
    /// rays happen to go (far less noise; the same picture on average).
    pub sample_sky: bool,
    pub seed: u32,
    /// Gather the picture to be denoised: the paths so far (a shutter's earlier moments
    /// go into the same picture) and whether these are the last.
    pub denoise: Option<Gather>,
}
/// How far a denoised picture has got: paths a pixel gathered before this render, and
/// whether this render's are the last (then it is smoothed into the target).
#[derive(Clone, Copy, Debug)]
pub struct Gather {
    pub gathered: u32,
    pub last: bool,
}
/// A picture the scene's surfaces read: width x height RGBA f32 at the start of `buffer`.
#[derive(Clone, Copy)]
pub struct Texture<'a> {
    pub buffer: &'a wgpu::Buffer,
    pub width: u32,
    pub height: u32,
}
pub struct Environment<'a> {
    /// The prefiltered environment the renderer's shared WGSL reads.
    pub map: &'a wgpu::Buffer,
    pub strength: f32,
    /// Its turn about the vertical, in radians.
    pub turn: f32,
    /// Seen behind everything where nothing is met.
    pub backdrop: bool,
}
/// A library look: the material's two blocks as the 2D kernels read them (its first
/// 48 bytes of parameters and its studio lighting), and pixels per mesh uv unit.
pub struct Look {
    pub params: [[f32; 4]; 3],
    pub lighting: Vec<[f32; 4]>,
    pub uv_scale: [f32; 2],
}
/// The studio lighting block's size in vec4s (`StudioLighting` in the surface code).
pub const LOOK_LIGHTING: usize = 39;
pub struct Job<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub scene: &'a Packed,
    /// The pictures surfaces name by number.
    pub textures: &'a [Texture<'a>],
    /// The looks surfaces name.
    pub looks: &'a [Look],
    pub lights: &'a [Light],
    pub view: View,
    pub environment: Environment<'a>,
    pub width: u32,
    pub height: u32,
    pub settings: Settings,
    /// The picture: width x height premultiplied RGBA f32.
    pub target: &'a wgpu::Buffer,
    /// How much of the picture this render is: 1 replaces it; less mixes this render in
    /// (motion blur's moments), and then a moment's samples are one pass.
    pub blend: f32,
}
/// Samples one pass may take for every pixel together, so a pass stays short.
const PASS_WORK: u64 = 8_294_400;
const UNIFORM_STEP: u64 = 256;
struct Geometry {
    meshes: Vec<usize>,
    nodes: wgpu::Buffer,
    vertices: wgpu::Buffer,
    triangles: wgpu::Buffer,
    bytes: u64,
}
pub struct Tracer {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    geometry: Mutex<Option<Geometry>>,
    /// The environment made into a table to draw bright directions from, each frame.
    sky_rows: wgpu::ComputePipeline,
    sky_down: wgpu::ComputePipeline,
    sky_layout: wgpu::BindGroupLayout,
    sky_table: wgpu::Buffer,
    denoiser: crate::denoise::Denoiser,
}
/// The table: 256 x 128 running shares, 128 row shares and the sum (sky_table.wgsl).
const SKY_TABLE: u64 = (256 * 128 + 128 + 1) * 4;
fn storage(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}
/// A storage buffer holding `data`, or one zeroed element when it is empty (bindings
/// may not be empty).
fn buffer<T: bytemuck::Pod>(device: &wgpu::Device, label: &str, data: &[T]) -> wgpu::Buffer {
    let one = [T::zeroed()];
    let data = if data.is_empty() { &one[..] } else { data };
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(data),
        usage: wgpu::BufferUsages::STORAGE,
    })
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Frame {
    camera: [[f32; 4]; 3],
    lens: [f32; 4],
    look: [f32; 4],
    counts: [u32; 4],
    more: [f32; 4],
    offsets: [u32; 4],
    lights_at: [u32; 4],
}
/// The frame's small arrays in one list of vec4s, and where each starts: placings (8 a
/// placing), surfaces (5) and the pictures cast onto them (5 each), top tree nodes (2), its order (4 to a vec4), lights (5) and
/// the textures' places in the shared texture buffer (1: first texel, width, height).
fn frame_data(
    scene: &Packed,
    lights: &[Light],
    places: &[[u32; 3]],
    looks: &[Look],
) -> (Vec<[f32; 4]>, [u32; 4], [u32; 3]) {
    let f = f32::from_bits;
    let mut data: Vec<[f32; 4]> = Vec::new();
    let instances = data.len() as u32;
    for i in &scene.instances {
        data.extend(i.to_object);
        data.extend(i.to_world);
        data.push([
            f(i.root),
            f(i.triangle_base),
            f(i.vertex_base),
            f(i.surface),
        ]);
        data.push(i.tint);
    }
    let surfaces = data.len() as u32;
    // The pictures cast onto surfaces follow the surfaces, in the order of their surfaces.
    let mut cast_at = surfaces + 5 * scene.surfaces.len() as u32;
    for s in &scene.surfaces {
        data.push([s.colour[0], s.colour[1], s.colour[2], s.roughness]);
        data.push([s.metallic, s.emission, s.opacity, f(s.flags)]);
        data.push([
            f(s.texture.unwrap_or(u32::MAX)),
            s.shadow,
            s.finish[4],
            f(s.look.unwrap_or(u32::MAX)),
        ]);
        data.push([s.finish[0], s.finish[1], s.finish[2], s.finish[3]]);
        let cast = s.projection.map_or(u32::MAX, |_| {
            cast_at += 5;
            cast_at - 5
        });
        data.push([f(cast), 0.0, 0.0, 0.0]);
    }
    for c in scene.surfaces.iter().filter_map(|s| s.projection) {
        data.extend(c.to_camera);
        data.push([c.eye[0], c.eye[1], c.eye[2], c.focal]);
        data.push([
            f(c.picture),
            f(u32::from(c.lit) | u32::from(c.hide_unseen) << 1),
            0.0,
            0.0,
        ]);
    }
    let top = data.len() as u32;
    for n in &scene.top {
        data.push([n.min[0], n.min[1], n.min[2], f(n.first)]);
        data.push([n.max[0], n.max[1], n.max[2], f(n.count)]);
    }
    let order = data.len() as u32;
    for chunk in scene.top_order.chunks(4) {
        let mut v = [0.0f32; 4];
        for (slot, &k) in v.iter_mut().zip(chunk) {
            *slot = f(k);
        }
        data.push(v);
    }
    let lights_at = data.len() as u32;
    for l in lights {
        data.extend([l.position, l.direction, l.colour, l.right, l.up]);
    }
    let textures_at = data.len() as u32;
    for p in places {
        data.push([f(p[0]), f(p[1]), f(p[2]), 0.0]);
    }
    // Each look: its uv scale, three parameter vec4s and the studio lighting.
    let looks_at = data.len() as u32;
    for l in looks {
        data.push([l.uv_scale[0], l.uv_scale[1], 0.0, 0.0]);
        data.extend(l.params);
        data.extend(&l.lighting);
    }
    (
        data,
        [instances, surfaces, top, order],
        [lights_at, textures_at, looks_at],
    )
}
impl Tracer {
    /// The tracer's program, with `surface_wgsl` (what a library material is at a point)
    /// and `environment_wgsl` (the renderer's shared environment lookups) appended.
    pub fn new(device: &wgpu::Device, surface_wgsl: &str, environment_wgsl: &str) -> Self {
        let source = format!(
            "{}\n{}\n{}",
            include_str!("trace.wgsl"),
            surface_wgsl,
            environment_wgsl
        );
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("path tracer"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Frame>() as u64),
            },
            count: None,
        }];
        entries.extend((1..=5).map(|b| storage(b, true)));
        entries.push(storage(6, false));
        entries.push(storage(7, true));
        entries.push(storage(8, true));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("path tracer"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("path tracer"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("path tracer"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("trace"),
            compilation_options: Default::default(),
            cache: None,
        });
        let sky_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky table"),
            source: wgpu::ShaderSource::Wgsl(
                format!("{}\n{}", include_str!("sky_table.wgsl"), environment_wgsl).into(),
            ),
        });
        let sky_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky table"),
            entries: &[storage(0, true), storage(1, false)],
        });
        let sky_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky table"),
            bind_group_layouts: &[Some(&sky_layout)],
            immediate_size: 0,
        });
        let sky = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("sky table"),
                layout: Some(&sky_pipeline_layout),
                module: &sky_module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            pipeline,
            layout,
            geometry: Mutex::new(None),
            sky_rows: sky("sky_rows"),
            sky_down: sky("sky_down"),
            sky_layout,
            sky_table: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("sky table"),
                size: SKY_TABLE,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
            denoiser: crate::denoise::Denoiser::new(device),
        }
    }
    /// GPU bytes kept between frames (the meshes' geometry, the denoiser's buffers).
    pub fn held_bytes(&self) -> u64 {
        self.geometry
            .lock()
            .map_or(0, |g| g.as_ref().map_or(0, |g| g.bytes))
            + self.denoiser.held_bytes()
    }
    pub fn release(&self) {
        if let Ok(mut g) = self.geometry.lock() {
            *g = None;
        }
        self.denoiser.release();
    }
    pub fn render(&self, job: Job<'_>) -> Result<(), String> {
        let Job {
            device,
            queue,
            encoder,
            scene,
            textures,
            looks,
            lights,
            view,
            environment,
            width,
            height,
            settings,
            target,
            blend,
        } = job;
        if let Some(look) = looks.iter().find(|l| l.lighting.len() != LOOK_LIGHTING) {
            return Err(format!(
                "a look's studio lighting is {} vec4s, not {LOOK_LIGHTING}",
                look.lighting.len()
            ));
        }
        let pixels = u64::from(width) * u64::from(height);
        if pixels == 0 {
            return Ok(());
        }
        if target.size() < pixels * 16 {
            return Err("the path tracer's picture is smaller than the frame".into());
        }
        let samples = settings.samples.clamp(1, 65_536);
        let work = match settings.denoise {
            Some(g) => Some(
                self.denoiser
                    .take(device, target, pixels, g.gathered == 0)?,
            ),
            None => None,
        };
        let gathered = settings.denoise.map_or(0, |g| g.gathered);
        let mut geometry = self
            .geometry
            .lock()
            .map_err(|_| "path tracer lock poisoned")?;
        if geometry.as_ref().is_none_or(|g| g.meshes != scene.meshes) {
            let nodes = buffer(device, "path tracer nodes", &scene.nodes);
            let vertices = buffer(device, "path tracer vertices", &scene.vertices);
            let triangles = buffer(device, "path tracer triangles", &scene.triangles);
            let bytes = nodes.size() + vertices.size() + triangles.size();
            *geometry = Some(Geometry {
                meshes: scene.meshes.clone(),
                nodes,
                vertices,
                triangles,
                bytes,
            });
        }
        let g = geometry.as_ref().expect("geometry was just made");
        // The pictures, one after another in one buffer (a stage may bind few buffers).
        let mut places = Vec::with_capacity(textures.len());
        let mut texels = 0u64;
        for t in textures {
            places.push([texels as u32, t.width, t.height]);
            texels += u64::from(t.width) * u64::from(t.height);
        }
        if texels > u64::from(u32::MAX) {
            return Err("the scene's pictures are too large to path trace together".into());
        }
        let pictures = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("path tracer pictures"),
            size: (texels * 16).max(16),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        for (t, place) in textures.iter().zip(&places) {
            let bytes = u64::from(t.width) * u64::from(t.height) * 16;
            if t.buffer.size() < bytes {
                return Err("a picture is smaller than it says".into());
            }
            encoder.copy_buffer_to_buffer(t.buffer, 0, &pictures, u64::from(place[0]) * 16, bytes);
        }
        let (data, offsets, [lights_at, textures_at, looks_at]) =
            frame_data(scene, lights, &places, looks);
        let data = buffer(device, "path tracer frame", &data);
        // Shadow rays look for glass to pass through only when there is some.
        let has_glass = scene.surfaces.iter().any(|s| s.flags & GLASS != 0);
        // The passes: each takes a few samples for every pixel, as many as keep it short.
        let blend = blend.clamp(0.0, 1.0);
        let per_pass = if blend < 1.0 && work.is_none() {
            samples
        } else {
            (PASS_WORK / pixels).clamp(1, 64) as u32
        };
        let passes = samples.div_ceil(per_pass);
        let mut uniforms = vec![0u8; (u64::from(passes) * UNIFORM_STEP) as usize];
        for p in 0..passes {
            let first = p * per_pass;
            let frame = Frame {
                camera: [view.camera[0], view.camera[1], view.camera[2]],
                lens: [
                    view.focal,
                    width as f32,
                    height as f32,
                    view.lens_radius.max(0.0),
                ],
                look: [
                    view.focus.max(1e-3),
                    environment.strength,
                    environment.turn,
                    f32::from(u8::from(environment.backdrop)),
                ],
                counts: [
                    lights.len() as u32,
                    settings.bounces.min(32),
                    per_pass.min(samples - first),
                    gathered + first,
                ],
                more: [
                    f32::from_bits(settings.seed),
                    scene.instances.len() as f32,
                    settings.clamp.max(0.0),
                    blend,
                ],
                offsets,
                lights_at: [
                    lights_at,
                    u32::from(settings.sample_sky)
                        | if work.is_some() { 2 } else { 0 }
                        | if has_glass { 4 } else { 0 },
                    textures_at,
                    looks_at,
                ],
            };
            let at = (u64::from(p) * UNIFORM_STEP) as usize;
            uniforms[at..at + std::mem::size_of::<Frame>()]
                .copy_from_slice(bytemuck::bytes_of(&frame));
        }
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("path tracer passes"),
            contents: &uniforms,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("path tracer"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &uniform,
                        offset: 0,
                        size: wgpu::BufferSize::new(std::mem::size_of::<Frame>() as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: g.nodes.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: g.vertices.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: g.triangles.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: data.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: environment.map.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: match &work {
                        Some(w) => w.gathered.as_entire_binding(),
                        None => wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: target,
                            offset: 0,
                            size: wgpu::BufferSize::new(pixels * 16),
                        }),
                    },
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: self.sky_table.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: pictures.as_entire_binding(),
                },
            ],
        });
        // The environment's table, made again each frame (a few hundred thousand texel
        // reads): the environment may be another picture, or the same one changed.
        if settings.sample_sky {
            let sky = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sky table"),
                layout: &self.sky_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: environment.map.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.sky_table.as_entire_binding(),
                    },
                ],
            });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("sky table"),
                timestamp_writes: None,
            });
            pass.set_bind_group(0, &sky, &[]);
            pass.set_pipeline(&self.sky_rows);
            pass.dispatch_workgroups(2, 1, 1);
            pass.set_pipeline(&self.sky_down);
            pass.dispatch_workgroups(1, 1, 1);
        }
        for p in 0..passes {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("path tracing"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[(u64::from(p) * UNIFORM_STEP) as u32]);
            pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
            drop(pass);
            // Each pass goes to the GPU on its own and is finished before the next is
            // sent, so a long picture does not hold the GPU in one piece: the screen (an
            // editor drawing on the same device) is drawn between passes instead of
            // waiting behind all of them.
            if p + 1 < passes {
                let done = std::mem::replace(
                    &mut *encoder,
                    device.create_command_encoder(&Default::default()),
                );
                let sent = queue.submit([done.finish()]);
                device
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(sent),
                        timeout: None,
                    })
                    .map_err(|e| format!("path tracing stopped: {e}"))?;
            }
        }
        if let (Some(work), Some(g)) = (work, settings.denoise) {
            if g.last {
                self.denoiser.finish(
                    device,
                    encoder,
                    work,
                    target,
                    width,
                    height,
                    gathered + samples,
                );
            } else {
                self.denoiser.hold(target, work);
            }
        }
        Ok(())
    }
}
