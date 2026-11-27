//! The tracer on the GPU: one compute program over the packed scene, run in passes of a few samples each so no single dispatch runs long, averaging into the picture as it goes (no extra full-frame buffer). Mesh geometry stays on the GPU while the same meshes are in the scene; placings, surfaces, the top tree and lights are sent each frame.
use crate::scene::{Mat4, Packed};
use std::sync::Mutex;
use wgpu::util::DeviceExt;

/// A light as the tracer reads it, 80 bytes: position and kind (0 point, 1 spot, 2 sun,
/// 3 area); direction and the cosine of a spot's outer half-angle; colour times strength
/// and the inner half-angle's cosine; an area light's half-width axis and feather, and its
/// half-height axis. The same meanings as the 3D rasteriser's lights.
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
    pub seed: u32,
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
pub struct Job<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub scene: &'a Packed,
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
}
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
/// The frame's small arrays in one list of vec4s, and where each starts: placings (7 a
/// placing), surfaces (2), top tree nodes (2), its order (4 to a vec4), lights (5).
fn frame_data(scene: &Packed, lights: &[Light]) -> (Vec<[f32; 4]>, [u32; 4], u32) {
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
    }
    let surfaces = data.len() as u32;
    for s in &scene.surfaces {
        data.push([s.colour[0], s.colour[1], s.colour[2], s.roughness]);
        data.push([s.metallic, s.emission, s.opacity, f(s.flags)]);
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
    (data, [instances, surfaces, top, order], lights_at)
}
impl Tracer {
    /// The tracer's program, with `environment_wgsl` (the renderer's shared environment
    /// lookups) appended.
    pub fn new(device: &wgpu::Device, environment_wgsl: &str) -> Self {
        let source = format!("{}\n{}", include_str!("trace.wgsl"), environment_wgsl);
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
        Self {
            pipeline,
            layout,
            geometry: Mutex::new(None),
        }
    }
    /// GPU bytes kept between frames (the meshes' geometry).
    pub fn held_bytes(&self) -> u64 {
        self.geometry
            .lock()
            .map_or(0, |g| g.as_ref().map_or(0, |g| g.bytes))
    }
    pub fn release(&self) {
        if let Ok(mut g) = self.geometry.lock() {
            *g = None;
        }
    }
    pub fn render(&self, job: Job<'_>) -> Result<(), String> {
        let Job {
            device,
            queue: _,
            encoder,
            scene,
            lights,
            view,
            environment,
            width,
            height,
            settings,
            target,
            blend,
        } = job;
        let pixels = u64::from(width) * u64::from(height);
        if pixels == 0 {
            return Ok(());
        }
        if target.size() < pixels * 16 {
            return Err("the path tracer's picture is smaller than the frame".into());
        }
        let samples = settings.samples.clamp(1, 65_536);
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
        let (data, offsets, lights_at) = frame_data(scene, lights);
        let data = buffer(device, "path tracer frame", &data);
        // The passes: each takes a few samples for every pixel, as many as keep it short.
        let blend = blend.clamp(0.0, 1.0);
        let per_pass = if blend < 1.0 {
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
                    first,
                ],
                more: [
                    f32::from_bits(settings.seed),
                    scene.instances.len() as f32,
                    settings.clamp.max(0.0),
                    blend,
                ],
                offsets,
                lights_at: [lights_at, 0, 0, 0],
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
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: target,
                        offset: 0,
                        size: wgpu::BufferSize::new(pixels * 16),
                    }),
                },
            ],
        });
        for p in 0..passes {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("path tracing"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[(u64::from(p) * UNIFORM_STEP) as u32]);
            pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        }
        Ok(())
    }
}
