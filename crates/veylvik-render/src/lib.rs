//! Whole-frame GPU execution. No window, document mutation, or timeline dependency.
mod cache;
mod diagnostics;
mod environment_bake;
mod material_preview;
pub use material_preview::material_preview_revision;
pub mod custom;
pub mod flow;
/// How many letters of the user's text a material swatch shows (the first ones, large).
pub const SWATCH_LETTERS: usize = 3;
mod history;
mod host_frame;
mod host_source;
pub use host_frame::HostPixels;
pub use host_source::{HostFrame, HostMedia, HostSource};
pub mod ocio;
mod shadow_lighting;
mod shutter;
mod temporal;
pub use shutter::Shutter;
mod studio_lighting;
pub use diagnostics::Diagnostics;
mod frame;
pub use cache::CacheStats;
pub use frame::{
    FrameSource, GpuFrame, Image, ImageInputs, Region, RenderRequest, RenderedFrame, SourceInfo,
};
pub mod compute;
pub mod display;
mod encode;
pub use encode::Look;
mod nodes;
mod origin;
mod pipeline_cache;
mod plan;
mod pool;
mod results;
pub use pipeline_cache::default_pipeline_folder;
mod preview_scale;
mod probe;
mod scene_hook;
/// WGSL for the prefiltered studio environment (`environment_specular`), for
/// renderers that light with the same studio as materials.
pub const ENVIRONMENT_WGSL: &str = include_str!("environment.wgsl");
/// The material kernels with their studio environment, for renderers that shade
/// surfaces with `material_colour(uv, normal)` exactly as titles are shaded. Their
/// bindings are group 0: 0 `p` (48-byte parameters), 1, 2, 4, 8 images, 3 the
/// result, 5 the LTC table, 6 `studio` lighting, 7 the environment.
pub const MATERIAL_WGSL: &str = concat!(
    include_str!("kernels.wgsl"),
    include_str!("environment.wgsl")
);
/// Size of the `studio` lighting uniform in `MATERIAL_WGSL`.
pub const MATERIAL_LIGHTING_BYTES: u64 = studio_lighting::SIZE;
/// How a scene's environment changes a material's studio: its strength (set
/// outright, or scaling the material's own) and an extra turn in radians.
#[derive(Debug, Clone, Copy)]
pub struct EnvironmentOverride {
    pub strength: f32,
    pub absolute: bool,
    pub rotation: f32,
}
/// The kernel parameters (`p`) and studio lighting (`studio`) a material reads at
/// `time` on a `width` x `height` canvas, as bytes for `MATERIAL_WGSL` uniforms.
pub fn material_uniforms(
    material: &veylvik_document::nodes::Material,
    width: u32,
    height: u32,
    time: Time,
    environment: Option<EnvironmentOverride>,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let node = Node::new("look", Operation::Material(material.clone()), &[]);
    let sample = |name: &str| -> Result<f32> {
        node.operation
            .scalar(name)
            .ok_or_else(|| error("material parameter missing"))?
            .at(time)
            .map_err(Into::into)
    };
    let mut p = Params {
        size_op: [width, height, 0, 0],
        values: [0.0; 4],
        extra: [0.0; 4],
        more: [0.0; 8],
        anchor: [0.0; 4],
    };
    let cx = nodes::Cx {
        node: &node,
        nodes: &[],
        width,
        height,
        time,
    };
    nodes::RenderNode::params(material, &cx, &sample, &mut p)?;
    let colours = material
        .colours
        .as_ref()
        .map(|c| veylvik_document::optical::palette_at(c, time))
        .transpose()?;
    let mut lighting = studio_lighting::Lighting::new(
        material.kind,
        material.optics,
        colours,
        &material.studio.at(time)?,
    );
    lighting.set_finish(material, time)?;
    if let Some(e) = environment {
        lighting.environment[2] = if e.absolute {
            e.strength
        } else {
            lighting.environment[2] * e.strength
        };
        lighting.environment[3] += e.rotation;
    }
    Ok((
        bytemuck::bytes_of(&p).to_vec(),
        bytemuck::bytes_of(&lighting).to_vec(),
    ))
}
pub use scene_hook::{SceneJob, SceneRenderer};
mod texture;
mod vector;
mod video;
pub use nodes::{Extent, Reach};
/// How long one pass of a render took on the card ([`Renderer::time_passes`]).
#[derive(Debug, Clone)]
pub struct PassTime {
    pub node: String,
    pub kind: &'static str,
    pub pass: String,
    pub milliseconds: f64,
}
/// A hash that is the same in every build and session (FNV-1a, 64 bits), from `seed`.
pub fn fnv(bytes: &[u8], seed: u64) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325 ^ seed;
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
pub use plan::{Kernel, PassInfo, Plan};
pub use pool::ResourceStats;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
pub use texture::{TextureEncoding, TextureTarget};
pub use vector::MAX_VECTOR_DIMENSION;
use veylvik_document::{CompiledScalar, Node, Operation, Time};
pub use video::{VideoEncoding, VideoFormat};
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Document(#[from] veylvik_document::Error),
}
pub type Result<T> = std::result::Result<T, Error>;
fn error(message: impl ToString) -> Error {
    Error::Message(message.to_string())
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    size_op: [u32; 4],
    values: [f32; 4],
    extra: [f32; 4],
    /// Eight more values, read only by custom shaders (other kernels declare the
    /// first three fields; a longer buffer is allowed).
    more: [f32; 8],
    /// For a custom shader that follows a scene object: where it is in the frame
    /// (pixels) and whether it is seen (1), not there or behind the camera (-1), or
    /// nothing is followed (0).
    anchor: [f32; 4],
}
fn params(
    node: &Node,
    nodes: &[Node],
    curves: &BTreeMap<&'static str, CompiledScalar>,
    width: u32,
    height: u32,
    time: Time,
) -> Result<Params> {
    let sample = |name: &str| -> Result<f32> {
        curves
            .get(name)
            .ok_or_else(|| error("compiled scalar parameter missing"))?
            .at(time)
            .map_err(Into::into)
    };
    let mut p = Params {
        size_op: [width, height, 0, 0],
        values: [0.0; 4],
        extra: [0.0; 4],
        more: [0.0; 8],
        anchor: [0.0; 4],
    };
    let cx = nodes::Cx {
        node,
        nodes,
        width,
        height,
        time,
    };
    veylvik_document::each_node!(&node.operation, n => nodes::RenderNode::params(n, &cx, &sample, &mut p))?;
    Ok(p)
}
/// One device and queue own every intermediate. Rendering does not silently fall back to CPU.
/// GPU programs compiled for rendering in this process (see `Renderer::compiled_programs`).
pub(crate) static COMPILED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// The kernels' module; each kernel's pipeline is compiled the first time it runs.
    shader: wgpu::ShaderModule,
    /// The particle passes' module: the same layout, with the output as atomics.
    particle_shader: wgpu::ShaderModule,
    pipelines: std::sync::Mutex<BTreeMap<Kernel, wgpu::ComputePipeline>>,
    /// Compiled programs kept between runs, once the host asks for it.
    pipeline_cache: std::sync::Mutex<Option<Arc<pipeline_cache::Kept>>>,
    /// The kernels' pipeline layout, for custom shaders compiled later.
    pipeline_layout: wgpu::PipelineLayout,
    custom: custom::Cache,
    layout: wgpu::BindGroupLayout,
    ltc: wgpu::Buffer,
    studio_environment: wgpu::Buffer,
    default_lighting: wgpu::Buffer,
    validation_pipeline: wgpu::ComputePipeline,
    texture_pipeline: wgpu::ComputePipeline,
    video_pipeline: wgpu::ComputePipeline,
    encode_pipeline: wgpu::ComputePipeline,
    shutter_pipeline: wgpu::ComputePipeline,
    history_pipeline: wgpu::ComputePipeline,
    /// Converts frames exchanged with a host.
    host_pipeline: wgpu::ComputePipeline,
    preview_pipeline: wgpu::ComputePipeline,
    vector_backend: Mutex<Option<vector::Backend>>,
    vector_cache: Mutex<cache::FrameCache<vector::Key>>,
    pub vector_cache_limit_bytes: u64,
    /// What the last frame's shapes and text take when all are kept.
    vector_wanted: std::sync::atomic::AtomicU64,
    pool: Mutex<pool::Pool>,
    cache: Mutex<cache::FrameCache>,
    /// Nodes' results kept between frames while they do not change (see results.rs).
    results: Mutex<results::Results>,
    flow_engine: Mutex<Option<Arc<flow::Engine>>>,
    scene_renderer: std::sync::RwLock<Option<Arc<dyn SceneRenderer>>>,
    /// One 3D render gives both the picture and the depth picture when two Render scene
    /// nodes film the same scene the same way. On unless switched off to compare.
    pub share_scene_renders: bool,
    /// A one-pass node that is clear outside a box (a shape, a mask, a merge of small
    /// layers) is run over that box only, its output cleared elsewhere. On unless
    /// switched off to compare.
    pub area_dispatch: bool,
    /// A node's result unchanged since the frame before is kept and used again (see
    /// results.rs). On unless switched off to compare.
    pub keep_results: bool,
    /// Passes run over a box only, since the start.
    areas_used: std::sync::atomic::AtomicU64,
    /// The origin of a pass over the whole frame.
    whole_frame: wgpu::Buffer,
    /// While set, each pass of a render is finished on the card before the next begins
    /// and its time is put here: slower, and only for finding where a frame's time goes.
    pass_times: Mutex<Option<Vec<PassTime>>>,
    /// When the last thing was noted while timing, for the work between the passes.
    noted: Mutex<std::time::Instant>,
    ocio_cache: Mutex<std::collections::VecDeque<ocio::CachedProcessor>>,
    pub cache_limit_bytes: u64,
    owner: Arc<frame::DeviceIdentity>,
    pub pool_limit_bytes: u64,
    pub adapter: wgpu::AdapterInfo,
    pub budget_bytes: u64,
}
/// Request native frame buffers beyond WebGPU's conservative 128 MiB default.
/// UHD fits that default narrowly; DCI 4K requires larger bindings, and a whole-sky
/// picture of its own size (12288 by 6144 is 1.2 GiB) larger still. Cap requests to
/// the adapter's advertised support and 2 GiB, retaining explicit render-time checks;
/// the render budget still bounds what is held at once.
/// The largest single picture the renderer asks the adapter for.
const PICTURE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub fn device_limits(adapter: &wgpu::Adapter) -> wgpu::Limits {
    let available = adapter.limits();
    wgpu::Limits {
        max_storage_buffer_binding_size: available
            .max_storage_buffer_binding_size
            .min(PICTURE_BYTES as _),
        max_buffer_size: available.max_buffer_size.min(PICTURE_BYTES),
        ..Default::default()
    }
}
impl Renderer {
    /// Register what draws Render scene nodes (the host's 3D renderer).
    pub fn set_scene_renderer(&self, renderer: Arc<dyn SceneRenderer>) {
        if let Ok(mut slot) = self.scene_renderer.write() {
            *slot = Some(renderer);
        }
    }
    /// Register a 3D renderer made by `make` unless one is registered already: two
    /// callers at once make it once.
    pub fn ensure_scene_renderer(&self, make: impl FnOnce() -> Arc<dyn SceneRenderer>) {
        if let Ok(mut slot) = self.scene_renderer.write()
            && slot.is_none()
        {
            *slot = Some(make());
        }
    }
    pub fn new() -> Result<Self> {
        pollster::block_on(Self::create())
    }
    async fn create() -> Result<Self> {
        let (_, adapter, device, queue) = create_gpu().await?;
        let info = adapter.get_info();
        Self::from_device(device, queue, info)
    }
    /// Share a Vulkan device with a window host. The host supplies the matching
    /// queue and adapter info; all frame consumers must use this same device.
    pub fn from_device(
        device: wgpu::Device,
        queue: wgpu::Queue,
        info: wgpu::AdapterInfo,
    ) -> Result<Self> {
        if info.backend != wgpu::Backend::Vulkan {
            return Err(error("Veylvik requires a Vulkan device"));
        }
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composition kernels"),
            source: wgpu::ShaderSource::Wgsl(
                origin::with_origin(concat!(
                    include_str!("kernels.wgsl"),
                    include_str!("environment.wgsl"),
                    include_str!("working_space.wgsl"),
                    include_str!("matte.wgsl"),
                    include_str!("keyer.wgsl"),
                    include_str!("grain.wgsl"),
                    include_str!("lens.wgsl"),
                    include_str!("lens_depth.wgsl"),
                    include_str!("bloom.wgsl"),
                    include_str!("warp.wgsl"),
                    include_str!("lut.wgsl"),
                    include_str!("shadow.wgsl"),
                    include_str!("particles.wgsl"),
                    include_str!("material_layers.wgsl")
                ))
                .into(),
            ),
        });
        use wgpu::util::DeviceExt;
        let ltc = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Fitted GGX LTC tables"),
            contents: include_bytes!("../assets/ltc/ggx.bin"),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let studio_environment = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Prefiltered HDR studio"),
            contents: include_bytes!("../assets/studio/filtered.bin"),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let whole_frame = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whole-frame origin"),
            contents: &[0; 16],
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let default_lighting = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Default studio lighting"),
            contents: bytemuck::bytes_of(&studio_lighting::Lighting::new(
                veylvik_document::MaterialKind::Gold,
                None,
                None,
                &Default::default(),
            )),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composition buffers"),
            entries: &[
                // Where a pass's threads begin (see origin.rs).
                wgpu::BindGroupLayoutEntry {
                    binding: origin::BINDING,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(16),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(701096 * 16),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(studio_lighting::SIZE),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(131072),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        // Kernels read the first 48 bytes; a custom shader all 80.
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(16),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(16),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(16),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(16),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(16),
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composition"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let particle_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("particles"),
            source: wgpu::ShaderSource::Wgsl(include_str!("particle_system.wgsl").into()),
        });
        let validation_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("finite output validation"),
            source: wgpu::ShaderSource::Wgsl(include_str!("validate.wgsl").into()),
        });
        let validation_pipeline =
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("finite output validation"),
                layout: None,
                module: &validation_shader,
                entry_point: Some("validate"),
                compilation_options: Default::default(),
                cache: None,
            });
        let texture_pipeline = texture::pipeline(&device);
        let video_pipeline = video::pipeline(&device);
        let encode_pipeline = encode::pipeline(&device);
        let shutter_pipeline = shutter::pipeline(&device);
        let history_pipeline = history::pipeline(&device);
        let host_pipeline = host_frame::pipeline(&device);
        let preview_pipeline = preview_scale::pipeline(&device);
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(error(e));
        }
        Ok(Self {
            device,
            queue,
            shader,
            particle_shader,
            pipelines: Default::default(),
            pipeline_cache: Default::default(),
            pipeline_layout,
            custom: Default::default(),
            layout,
            ltc,
            studio_environment,
            default_lighting,
            validation_pipeline,
            texture_pipeline,
            video_pipeline,
            encode_pipeline,
            shutter_pipeline,
            history_pipeline,
            host_pipeline,
            preview_pipeline,
            vector_backend: Mutex::new(None),
            vector_cache: Mutex::new(cache::FrameCache::default()),
            vector_cache_limit_bytes: 32 * 1024 * 1024,
            vector_wanted: Default::default(),
            pool: Mutex::new(pool::Pool::default()),
            cache: Mutex::new(cache::FrameCache::default()),
            results: Mutex::new(results::Results::default()),
            ocio_cache: Mutex::new(Default::default()),
            flow_engine: Mutex::new(None),
            scene_renderer: std::sync::RwLock::new(None),
            pass_times: Mutex::new(None),
            noted: Mutex::new(std::time::Instant::now()),
            share_scene_renders: true,
            area_dispatch: true,
            keep_results: true,
            areas_used: Default::default(),
            whole_frame,
            cache_limit_bytes: 64 * 1024 * 1024,
            owner: Arc::new(frame::DeviceIdentity),
            pool_limit_bytes: 128 * 1024 * 1024,
            adapter: info,
            budget_bytes: 2048 * 1024 * 1024,
        })
    }
    /// The device timed out or was lost: it must not receive more submissions.
    pub fn device_failed(&self) -> bool {
        self.pool.lock().map_or(true, |p| p.failed)
    }
    pub fn resource_stats(&self) -> Result<ResourceStats> {
        Ok(self
            .pool
            .lock()
            .map_err(|_| error("renderer resource pool poisoned"))?
            .stats())
    }
    /// Release idle allocations. Rendering and pool inspection are serialized per device.
    pub fn trim_resources(&self) -> Result<()> {
        self.pool
            .lock()
            .map_err(|_| error("renderer resource pool poisoned"))?
            .trim(0);
        self.results
            .lock()
            .map_err(|_| error("kept results lock poisoned"))?
            .clear();
        Ok(())
    }
    /// The returned pixels remain scene-linear and premultiplied, including values above one.
    pub fn render(&self, plan: &Plan, time: Time) -> Result<Vec<[f32; 4]>> {
        Ok(self
            .render_frame(plan, RenderRequest::full(time), &ImageInputs::default())?
            .pixels)
    }
    fn source_bytes(&self, plan: &Plan, inputs: &ImageInputs) -> Result<u64> {
        let mut seen = std::collections::BTreeSet::new();
        let mut bytes = 0;
        for asset in plan.required_inputs() {
            let source = inputs
                .source(asset)
                .ok_or_else(|| error(format!("missing image input: {asset}")))?;
            match source {
                frame::Source::Gpu(frame) if !Arc::ptr_eq(&frame.owner, &self.owner) => {
                    return Err(error("GPU image input belongs to another renderer"));
                }
                frame::Source::Cpu(image) if seen.insert(image.identity()) => {
                    bytes += image.byte_size()
                }
                _ => {} // Same-renderer GPU sources already count as held allocations.
            }
        }
        Ok(bytes)
    }
    fn execute(
        &self,
        plan: &Plan,
        request: RenderRequest,
        inputs: &ImageInputs,
        download: bool,
    ) -> Result<(Executed, Region)> {
        let vectors = self.vector_sources(plan, request, inputs, download)?;
        self.note("drawing shapes and text");
        let mut pool = self
            .pool
            .lock()
            .map_err(|_| error("renderer resource pool poisoned"))?;
        let available = self.budget_bytes.saturating_sub(pool.stats().held_bytes);
        pool.trim(self.pool_limit_bytes.min(available));
        if pool.failed {
            return Err(error(
                "renderer unavailable after a GPU failure; create a new renderer",
            ));
        }
        let region = request.region.unwrap_or(Region {
            x: 0,
            y: 0,
            width: plan.width,
            height: plan.height,
        });
        region.validate(plan.width, plan.height)?;
        let time = request.time;
        time.seconds()?;
        let byte_size = plan.width as u64 * plan.height as u64 * 16;
        let limits = self.device.limits();
        if byte_size > limits.max_storage_buffer_binding_size || byte_size > limits.max_buffer_size
        {
            return Err(error("frame exceeds adapter buffer limits"));
        }
        let source_bytes = self.source_bytes(plan, inputs)?;
        for asset in plan.required_inputs() {
            let source = inputs
                .source(asset)
                .ok_or_else(|| error(format!("missing image input: {asset}")))?;
            if source.byte_size() > limits.max_storage_buffer_binding_size
                || source.byte_size() > limits.max_buffer_size
            {
                return Err(error(format!(
                    "image input {asset} exceeds adapter buffer limits"
                )));
            }
        }
        let working = if download {
            plan.working_bytes()
        } else {
            plan.gpu_working_bytes()
        } + source_bytes
            + pool.stats().held_bytes
            - plan.vector_bytes();
        // Kept node results count too, and give way before a render goes without room.
        let working_bytes = {
            let mut results = self
                .results
                .lock()
                .map_err(|_| error("kept results lock poisoned"))?;
            while working + results.bytes() > self.budget_bytes && results.evict() {}
            working + results.bytes()
        };
        if working_bytes > self.budget_bytes {
            let mib = |bytes: u64| bytes.div_ceil(1024 * 1024);
            return Err(error(format!(
                "this render needs {} MiB of GPU buffers and the budget is {} MiB: lower the resolution, or raise the GPU budget in Preferences or with VEYLVIK_GPU_BUDGET_MIB",
                mib(working_bytes),
                mib(self.budget_bytes)
            )));
        }
        // Validate evaluated values before allocating or submitting any GPU work.
        let mut evaluated: Vec<_> = plan
            .nodes
            .iter()
            .zip(&plan.curves)
            .map(|(n, curves)| params(n, &plan.nodes, curves, plan.width, plan.height, time))
            .collect::<Result<_>>()?;
        // A drawn shape is kept over its own box: its pass is told the box.
        for (index, (frame, corner)) in &vectors {
            if matches!(plan.nodes[*index].operation, Operation::Path(_)) {
                let p = &mut evaluated[*index];
                p.values = [
                    frame.width() as f32,
                    frame.height() as f32,
                    corner[0] as f32,
                    corner[1] as f32,
                ];
                p.extra[0] = frame.width() as f32;
            }
            // Text is drawn over its own box too: its pass reads the box (extra =
            // width, height, left, top) and is clear outside it.
            if matches!(plan.nodes[*index].operation, Operation::Text(_)) {
                evaluated[*index].extra = [
                    frame.width() as f32,
                    frame.height() as f32,
                    corner[0] as f32,
                    corner[1] as f32,
                ];
            }
        }
        for (node, p) in plan.nodes.iter().zip(&mut evaluated) {
            if let Some(asset) = node.operation.asset() {
                let key = if matches!(
                    node.operation,
                    Operation::Footage(veylvik_document::nodes::Footage { .. })
                ) {
                    plan::footage_key(node.id)
                } else {
                    asset.to_owned()
                };
                let source = inputs.source(&key).unwrap();
                let region = source.region();
                if let Operation::Footage(veylvik_document::nodes::Footage { placement, .. }) =
                    node.operation
                {
                    use veylvik_document::media::Placement;
                    p.size_op[3] = match placement {
                        Placement::Legacy => 0,
                        Placement::Fit => 1,
                        Placement::Fill => 2,
                        Placement::Original => 3,
                        Placement::Stretch => 4,
                    };
                }
                p.values[0] = region.width as f32;
                p.values[1] = region.height as f32;
                p.extra = [
                    source.storage_width() as f32,
                    0.0,
                    region.x as f32,
                    region.y as f32,
                ];
            }
        }
        // What each pass makes, to know a node's result again in the next frame.
        let ids = cache::pass_identities(plan, request, inputs)?;
        self.note("working out every node's settings");
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let allocation = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let result = self.encode_and_read(
            plan,
            (
                &evaluated,
                request.time,
                request.scene_view,
                request.scene_reflections,
            ),
            inputs,
            &vectors,
            &mut pool,
            download,
            &ids,
        );
        let allocation_error = pollster::block_on(allocation.pop());
        let validation_error = pollster::block_on(validation.pop());
        if let Some(e) = allocation_error.or(validation_error) {
            pool.trim(0);
            pool.failed = true;
            return Err(error(e));
        }
        let available = self.budget_bytes.saturating_sub(pool.stats().held_bytes);
        pool.trim(self.pool_limit_bytes.min(available));
        Ok((result?, region))
    }
    /// How many GPU programs this process has compiled for rendering (kernels and
    /// custom shaders, as each is first used). A render during which it grew paid for a
    /// compile, often seconds, which says nothing about what the next frame costs.
    pub fn compiled_programs(&self) -> u64 {
        COMPILED.load(std::sync::atomic::Ordering::Relaxed)
    }
    /// A kernel's pipeline, compiled on first use: the driver's compile of all of
    /// them took far longer than starting the renderer, and a graph uses a handful.
    fn pipeline(&self, kernel: Kernel) -> Result<wgpu::ComputePipeline> {
        if let Some(pipeline) = self
            .pipelines
            .lock()
            .map_err(|_| error("kernel pipelines lock poisoned"))?
            .get(&kernel)
        {
            return Ok(pipeline.clone());
        }
        if !Kernel::ALL.contains(&kernel) {
            return Err(error("kernel has no pipeline of its own"));
        }
        // Compiled with the list unlocked: a driver can take many seconds over one
        // kernel the first time, and a render that needs another must not wait for it.
        // Two threads may then compile the same one; the second's is simply used.
        let kept = self.kept_pipelines();
        let compiling = std::time::Instant::now();
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(kernel.entry()),
                layout: Some(&self.pipeline_layout),
                module: if kernel.entry().starts_with("particle_") {
                    &self.particle_shader
                } else {
                    &self.shader
                },
                entry_point: Some(kernel.entry()),
                compilation_options: Default::default(),
                cache: kept.as_ref().map(|kept| &kept.cache),
            });
        COMPILED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.pipelines
            .lock()
            .map_err(|_| error("kernel pipelines lock poisoned"))?
            .insert(kernel, pipeline.clone());
        // A slow compile is worth keeping at once, written off this thread.
        if compiling.elapsed() > std::time::Duration::from_millis(200) {
            self.save_pipelines_later();
        }
        Ok(pipeline)
    }
    /// Compile the kernels `plan` uses now, those of its branches included, so its first
    /// frame does not wait for them: the first compile of one kernel can take a driver
    /// many seconds. Meant for a thread of its own; renders go on meanwhile.
    pub fn compile_for(&self, plan: &Plan) -> Result<usize> {
        let mut kernels = std::collections::BTreeSet::new();
        fn gather(plan: &Plan, kernels: &mut std::collections::BTreeSet<Kernel>) {
            kernels.extend(plan.passes.iter().map(|pass| pass.kernel));
            for branch in &plan.temporal {
                gather(&branch.plan, kernels);
            }
        }
        gather(plan, &mut kernels);
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let built = kernels
            .into_iter()
            .filter(|kernel| Kernel::ALL.contains(kernel))
            .map(|kernel| self.pipeline(kernel))
            .collect::<Result<Vec<_>>>();
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(error(e));
        }
        let built = built?.len();
        // Called on a thread of the host's: write the programs now.
        let _ = self.save_pipeline_cache();
        Ok(built)
    }
    /// Compile every kernel now, so no later frame waits for one. Errors if any fails.
    pub fn compile_kernels(&self) -> Result<usize> {
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let built = Kernel::ALL
            .into_iter()
            .map(|kernel| self.pipeline(kernel))
            .collect::<Result<Vec<_>>>();
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(error(e));
        }
        let built = built?.len();
        // Called on a thread of the host's: write the programs now.
        let _ = self.save_pipeline_cache();
        Ok(built)
    }
    pub fn render_frame(
        &self,
        plan: &Plan,
        request: RenderRequest,
        inputs: &ImageInputs,
    ) -> Result<RenderedFrame> {
        if !plan.temporal.is_empty() {
            return self.read_frame(&self.render_gpu(plan, request, inputs)?);
        }
        let resolved = inputs.resolve_on(plan, request.time, Some(self))?;
        let inputs = &resolved;
        let (Executed::Cpu(full), region) = self.execute(plan, request, inputs, true)? else {
            unreachable!()
        };
        Ok(crop(full, region, plan.width, plan.height))
    }
    /// While timing: what was done since the last note took this long, outside any pass.
    fn note(&self, what: &'static str) {
        let (Ok(mut times), Ok(mut noted)) = (self.pass_times.lock(), self.noted.lock()) else {
            return;
        };
        if let Some(times) = times.as_mut() {
            times.push(PassTime {
                node: what.into(),
                kind: "outside the passes",
                pass: String::new(),
                milliseconds: noted.elapsed().as_secs_f64() * 1000.0,
            });
            *noted = std::time::Instant::now();
        }
    }
    /// Render once with every pass finished on the card before the next begins, and say
    /// how long each took. The sum is more than an ordinary render takes (the card
    /// cannot overlap anything), so read it for the shares, not the total. Plans made
    /// of other moments are timed as their outermost plan only.
    pub fn time_passes(
        &self,
        plan: &Plan,
        request: RenderRequest,
        inputs: &ImageInputs,
    ) -> Result<Vec<PassTime>> {
        *self
            .pass_times
            .lock()
            .map_err(|_| error("pass times poisoned"))? = Some(Vec::new());
        if let Ok(mut noted) = self.noted.lock() {
            *noted = std::time::Instant::now();
        }
        let rendered = self.render_gpu(plan, request, inputs);
        self.note("handing the frame back");
        let times = self
            .pass_times
            .lock()
            .map_err(|_| error("pass times poisoned"))?
            .take()
            .unwrap_or_default();
        rendered.map(|_| times)
    }
    /// Evaluate without any pixel readback. Held outputs count against the budget
    /// of later renders until their handles are dropped.
    pub fn render_gpu(
        &self,
        plan: &Plan,
        request: RenderRequest,
        inputs: &ImageInputs,
    ) -> Result<GpuFrame> {
        if !plan.temporal.is_empty() {
            return self
                .render_gpu_temporal(plan, request, inputs, false, || false)?
                .ok_or_else(|| error("render cancelled"));
        }
        self.render_gpu_resolved(plan, request, inputs)
    }
    fn render_gpu_resolved(
        &self,
        plan: &Plan,
        request: RenderRequest,
        inputs: &ImageInputs,
    ) -> Result<GpuFrame> {
        let resolved = inputs.resolve_on(plan, request.time, Some(self))?;
        self.note("reading the inputs");
        let inputs = &resolved;
        let (Executed::Gpu(storage), region) = self.execute(plan, request, inputs, false)? else {
            unreachable!()
        };
        Ok(GpuFrame {
            flow_report: None,
            identity: frame::next_identity()?,
            storage: Arc::new(storage),
            owner: self.owner.clone(),
            region,
            width: plan.width,
            height: plan.height,
        })
    }
    /// Cache completed output by evaluated pass parameters, topology, crop, and
    /// immutable source identities. Labels, unreachable nodes, and equivalent
    /// animation times do not invalidate it. As many as fit the cache's limit are retained.
    pub fn render_gpu_cached(
        &self,
        plan: &Plan,
        request: RenderRequest,
        inputs: &ImageInputs,
    ) -> Result<Arc<GpuFrame>> {
        if !plan.temporal.is_empty() {
            return self.render_gpu(plan, request, inputs).map(Arc::new);
        }
        let resolved = inputs.resolve_on(plan, request.time, Some(self))?;
        let inputs = &resolved;
        let source_bytes = self.source_bytes(plan, inputs)?;
        let key = cache::Key::new(plan, request, inputs)?;
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| error("frame cache poisoned"))?;
        cache.trim(self.cache_limit_bytes.min(self.budget_bytes));
        {
            let mut pool = self
                .pool
                .lock()
                .map_err(|_| error("renderer resource pool poisoned"))?;
            if pool.failed {
                return Err(error("renderer unavailable after a GPU failure"));
            }
            let available = self.budget_bytes.saturating_sub(pool.stats().held_bytes);
            pool.trim(self.pool_limit_bytes.min(available));
        }
        self.trim_vector_cache_to_budget()?;
        // Budget reductions apply even when a previous request is a cache hit.
        while self.resource_stats()?.held_bytes > self.budget_bytes && cache.evict() {}
        if self.resource_stats()?.held_bytes > self.budget_bytes {
            return Err(error("held GPU frames exceed budget"));
        }
        if let Some(frame) = cache.lookup(&key) {
            return Ok(frame);
        }
        let needed = plan.gpu_working_bytes() + source_bytes;
        while self.resource_stats()?.held_bytes + needed > self.budget_bytes && cache.evict() {}
        let frame = Arc::new(self.render_gpu(plan, request, inputs)?);
        cache.insert(
            key,
            frame.clone(),
            self.cache_limit_bytes.min(self.budget_bytes),
        );
        Ok(frame)
    }
    /// The identity of the picture `plan` gives for `request`: equal identities are the
    /// same picture, so one may stand for the other. It is made of everything a pass is
    /// given (its values at that time, its inputs, a scene's contents, a shader's text),
    /// never of the time itself unless a pass reads it.
    pub fn identity(
        &self,
        plan: &Plan,
        request: RenderRequest,
        inputs: &ImageInputs,
    ) -> Result<u64> {
        Ok(self.identities(plan, request, inputs)?.0)
    }
    /// Both identities of a picture at once: [`Renderer::identity`] and
    /// [`Renderer::lasting_identity`].
    pub fn identities(
        &self,
        plan: &Plan,
        request: RenderRequest,
        inputs: &ImageInputs,
    ) -> Result<(u64, Option<u64>)> {
        use std::hash::{Hash, Hasher};
        // As a preview is made: a motion blur's preview samples, a flow's preview size.
        let key = self.key(plan, request, inputs, true, &mut 256)?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.words().hash(&mut hasher);
        let lasting = key
            .portable()
            .then(|| fnv(bytemuck::cast_slice(key.words()), 0));
        Ok((hasher.finish(), lasting))
    }
    /// As [`Renderer::identity`], but one that means the same picture in another session
    /// of the same build: none when the plan is made of other moments, or when an input
    /// is known only by this session's count and not by a name.
    pub fn lasting_identity(
        &self,
        plan: &Plan,
        request: RenderRequest,
        inputs: &ImageInputs,
    ) -> Result<Option<u64>> {
        Ok(self.identities(plan, request, inputs)?.1)
    }
    pub fn cache_stats(&self) -> Result<CacheStats> {
        Ok(self
            .cache
            .lock()
            .map_err(|_| error("frame cache poisoned"))?
            .stats())
    }
    /// How many passes have run over a box only (see `area_dispatch`), since the start.
    pub fn areas_used(&self) -> Result<u64> {
        Ok(self.areas_used.load(std::sync::atomic::Ordering::Relaxed))
    }
    /// Nodes' results kept between frames: how many, their bytes, and how many passes
    /// they have stood for since the start.
    pub fn kept_results(&self) -> Result<(usize, u64, u64)> {
        let results = self
            .results
            .lock()
            .map_err(|_| error("kept results lock poisoned"))?;
        Ok((results.len(), results.bytes(), results.hits))
    }
    pub fn clear_frame_cache(&self) -> Result<()> {
        self.results
            .lock()
            .map_err(|_| error("kept results lock poisoned"))?
            .clear();
        if let Some(engine) = self
            .flow_engine
            .lock()
            .map_err(|_| error("flow engine poisoned"))?
            .as_ref()
        {
            engine.clear_cache()?;
        }
        self.cache
            .lock()
            .map_err(|_| error("frame cache poisoned"))?
            .trim(0);
        self.vector_cache
            .lock()
            .map_err(|_| error("vector cache poisoned"))?
            .trim(0);
        Ok(())
    }
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
    #[allow(clippy::too_many_arguments)]
    fn encode_and_read(
        &self,
        plan: &Plan,
        (evaluated, time, scene_view, scene_reflections): (
            &[Params],
            veylvik_document::Time,
            Option<veylvik_scene::Camera>,
            bool,
        ),
        inputs: &ImageInputs,
        vectors: &BTreeMap<usize, (Arc<GpuFrame>, [u32; 2])>,
        pool: &mut pool::Pool,
        download: bool,
        ids: &[Option<u64>],
    ) -> Result<Executed> {
        let byte_size = u64::from(plan.width) * u64::from(plan.height) * 16;
        let mut resources = pool.frame(self.budget_bytes);
        let dummy = resources.take(
            &self.device,
            16,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            "empty source",
        )?;
        self.queue.write_buffer(&dummy, 0, &[0; 16]);
        let mut sources = BTreeMap::new();
        let mut source_buffers = BTreeMap::<u64, wgpu::Buffer>::new();
        for key in plan.required_inputs() {
            let image = inputs.source(key).unwrap();
            let buffer = if let Some(buffer) = source_buffers.get(&image.identity()) {
                buffer.clone()
            } else {
                let buffer = match image {
                    frame::Source::Cpu(image) => {
                        let (buffer, upload) = resources.source(
                            &self.device,
                            image.identity(),
                            image.byte_size(),
                            key,
                        )?;
                        if upload {
                            self.queue.write_buffer(
                                &buffer,
                                0,
                                bytemuck::cast_slice(image.pixels()),
                            );
                        }
                        buffer
                    }
                    frame::Source::Gpu(frame) => frame.buffer().clone(),
                };
                source_buffers.insert(image.identity(), buffer.clone());
                buffer
            };
            sources.insert(key, buffer);
        }
        let buffers = (0..plan.slot_count)
            .map(|_| {
                resources.take(
                    &self.device,
                    byte_size,
                    wgpu::BufferUsages::STORAGE
                        | wgpu::BufferUsages::COPY_SRC
                        | wgpu::BufferUsages::COPY_DST,
                    "frame slot",
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        // Nodes that cannot reach the picture at this time (behind a faded-out layer, say)
        // are not run. What reads them ignores their content, but must not meet whatever
        // an earlier pass left in the slot (a stray infinity times zero is not zero), so a
        // skipped slot is cleared when, and only when, something that runs reads it.
        self.note("taking frames and sending the sources to the card");
        let visible = plan.visible_at(time)?;
        let mut stale = vec![false; plan.slot_count];
        // Nodes' results from earlier frames that stand for passes now, and those to keep.
        let room = if self.keep_results {
            self.budget_bytes / 8
        } else {
            0
        };
        let decision = self
            .results
            .lock()
            .map_err(|_| error("kept results lock poisoned"))?
            .decide(plan, ids, &visible, room);
        let mut keeping: Vec<(u64, wgpu::Buffer)> = Vec::new();
        // Each pass that runs over a box only: a one-pass node clear outside it.
        let areas = self.areas(plan, time, vectors)?;
        // A result to keep is copied out before the next pass can reuse its slot.
        let mut to_keep: Option<usize> = None;
        // Only when asked to time the passes: what ran since the last mark is finished
        // on the card and timed, then the next pass begins on a new encoder.
        let timing = self.pass_times.lock().is_ok_and(|times| times.is_some());
        let mut running: Option<(usize, Kernel, std::time::Instant)> = None;
        let mut mark =
            |encoder: &mut wgpu::CommandEncoder, next: Option<(usize, Kernel)>| -> Result<()> {
                if !timing {
                    return Ok(());
                }
                let done = std::mem::replace(
                    encoder,
                    self.device.create_command_encoder(&Default::default()),
                );
                self.wait(self.queue.submit([done.finish()]))?;
                if let Some((node, kernel, begun)) = running.take()
                    && let Ok(mut times) = self.pass_times.lock()
                    && let Some(times) = times.as_mut()
                {
                    times.push(PassTime {
                        node: plan.nodes[node].label.clone(),
                        kind: plan.nodes[node].operation.descriptor().type_id,
                        pass: format!("{kernel:?}"),
                        milliseconds: begun.elapsed().as_secs_f64() * 1000.0,
                    });
                }
                running = next.map(|(node, kernel)| (node, kernel, std::time::Instant::now()));
                if let Ok(mut noted) = self.noted.lock() {
                    *noted = std::time::Instant::now();
                }
                Ok(())
            };
        // A Render scene pass whose picture its twin has already made (see below), kept
        // in a frame of its own until the pass's turn comes.
        let mut made: Vec<Option<wgpu::Buffer>> = vec![None; plan.passes.len()];
        let keep_now = |encoder: &mut wgpu::CommandEncoder,
                        at: usize,
                        keeping: &mut Vec<(u64, wgpu::Buffer)>,
                        buffers: &[wgpu::Buffer]| {
            let Some(id) = ids[at] else { return };
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("kept node result"),
                size: byte_size,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(
                &buffers[plan.passes[at].output],
                0,
                &buffer,
                0,
                byte_size,
            );
            keeping.push((id, buffer));
        };
        for (at_pass, scheduled) in plan.passes.iter().enumerate() {
            if let Some(at) = to_keep.take() {
                keep_now(&mut encoder, at, &mut keeping, &buffers);
            }
            if !visible.contains(&plan.nodes[scheduled.node].id) || !decision.run[at_pass] {
                stale[scheduled.output] = true;
                continue;
            }
            if decision.keep[at_pass] {
                to_keep = Some(at_pass);
            }
            if let Some(ready) = &decision.kept[at_pass] {
                mark(&mut encoder, Some((scheduled.node, scheduled.kernel)))?;
                encoder.copy_buffer_to_buffer(ready, 0, &buffers[scheduled.output], 0, byte_size);
                stale[scheduled.output] = false;
                continue;
            }
            mark(&mut encoder, Some((scheduled.node, scheduled.kernel)))?;
            if let Some(ready) = made[at_pass].take() {
                encoder.copy_buffer_to_buffer(&ready, 0, &buffers[scheduled.output], 0, byte_size);
                stale[scheduled.output] = false;
                continue;
            }
            for input in scheduled
                .inputs
                .iter()
                .chain(scheduled.cards.iter().map(|(_, input)| input))
            {
                if let plan::Input::Value(slot) = input
                    && std::mem::take(&mut stale[*slot])
                {
                    encoder.clear_buffer(&buffers[*slot], 0, None);
                }
            }
            stale[scheduled.output] = false;
            if scheduled.kernel == Kernel::RenderScene {
                let renderer = self
                    .scene_renderer
                    .read()
                    .map_err(|_| error("scene renderer lock poisoned"))?
                    .clone()
                    .ok_or_else(|| {
                        error("Render scene needs the 3D renderer, which this host has not loaded")
                    })?;
                let images: Vec<scene_hook::Picture<'_>> = scheduled
                    .cards
                    .iter()
                    .filter_map(|(source, input)| match input {
                        plan::Input::Value(slot) => {
                            // Named by the pass that made it this frame.
                            let made = (0..at_pass)
                                .rev()
                                .find(|&q| plan.passes[q].output == *slot)
                                .and_then(|q| ids[q]);
                            Some((*source, &buffers[*slot], [plan.width, plan.height], made))
                        }
                        // A picture of its own size (an Own size node).
                        plan::Input::Asset(key) => {
                            let picture = inputs.source(key)?;
                            Some((
                                *source,
                                &sources[key.as_str()],
                                [picture.region().width, picture.region().height],
                                Some(picture.identity()),
                            ))
                        }
                        _ => None,
                    })
                    .collect();
                // Motion blur renders several moments and averages them in place.
                let moments = scene_hook::sample_times(plan, scheduled.node, time)?;
                // A picture and a depth picture of the same scene filmed the same way are
                // one render with two outputs: find the other, if writing it now is safe.
                // The other's picture goes into a frame of its own, if there is room for one.
                let twin = if self.share_scene_renders {
                    scene_hook::twin(plan, at_pass, time, &visible)?
                } else {
                    None
                };
                let spare = match twin {
                    Some(other) => resources
                        .take(
                            &self.device,
                            byte_size,
                            wgpu::BufferUsages::STORAGE
                                | wgpu::BufferUsages::COPY_SRC
                                | wgpu::BufferUsages::COPY_DST,
                            "scene twin",
                        )
                        .ok()
                        .map(|buffer| (other, buffer)),
                    None => None,
                };
                for (k, at) in moments.into_iter().enumerate() {
                    let (scene, camera, occlusion) =
                        scene_hook::evaluate(plan, scheduled.node, at)?;
                    let camera = scene_view.unwrap_or(camera);
                    renderer.render(SceneJob {
                        device: &self.device,
                        queue: &self.queue,
                        encoder: &mut encoder,
                        scene: &scene,
                        camera,
                        occlusion,
                        blend: 1.0 / (k + 1) as f32,
                        time: at,
                        width: plan.width,
                        height: plan.height,
                        depth: scene_hook::shows_depth(plan, scheduled.node),
                        reflections: scene_reflections,
                        images: &images,
                        models: inputs.models(),
                        environment: &self.studio_environment,
                        ltc: &self.ltc,
                        target: &buffers[scheduled.output],
                        also: spare.as_ref().map(|(other, buffer)| {
                            (
                                buffer,
                                scene_hook::shows_depth(plan, plan.passes[*other].node),
                            )
                        }),
                    })?;
                }
                if let Some((other, buffer)) = spare {
                    made[other] = Some(buffer);
                }
                continue;
            }
            let node = &plan.nodes[scheduled.node];
            let p = &evaluated[scheduled.node];
            // Only a custom shader reads the eight values past the first 48 bytes.
            let size = if scheduled.kernel == Kernel::Custom {
                std::mem::size_of::<Params>()
            } else {
                48
            };
            let uniform = resources.take(
                &self.device,
                size as u64,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                "parameters",
            )?;
            self.queue
                .write_buffer(&uniform, 0, &bytemuck::bytes_of(p)[..size]);
            let lighting = if let Some(lighting_data) = shadow_lighting::for_node(node, plan, time)?
            {
                let buffer = resources.take(
                    &self.device,
                    studio_lighting::SIZE,
                    wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    "Studio lighting",
                )?;
                self.queue
                    .write_buffer(&buffer, 0, bytemuck::bytes_of(&lighting_data));
                buffer
            } else {
                self.default_lighting.clone()
            };
            let output = &buffers[scheduled.output];
            let input = |index: usize| -> &wgpu::Buffer {
                match &scheduled.inputs[index] {
                    plan::Input::Empty => &dummy,
                    plan::Input::Value(slot) => &buffers[*slot],
                    plan::Input::Asset(key) => &sources[key.as_str()],
                    plan::Input::Vector(index) => vectors[index].0.buffer(),
                }
            };
            let begins = match areas[at_pass] {
                Some([x, y, _, _]) => {
                    self.areas_used
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let buffer = resources.take(
                        &self.device,
                        16,
                        wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        "origin",
                    )?;
                    self.queue
                        .write_buffer(&buffer, 0, bytemuck::cast_slice(&[x, y, 0, 0]));
                    buffer
                }
                None => self.whole_frame.clone(),
            };
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&node.label),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: origin::BINDING,
                        resource: begins.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: self.studio_environment.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: self.ltc.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: lighting.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: input(0).as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: input(1).as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: output.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: input(2).as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: input(3).as_entire_binding(),
                    },
                ],
            });
            let custom_pipeline = match &node.operation {
                Operation::Shader(shader) if scheduled.kernel == Kernel::Custom => Some(
                    self.custom
                        .pipeline(&self.device, &self.pipeline_layout, &shader.code)?,
                ),
                _ => Some(self.pipeline(scheduled.kernel)?),
            };
            // Particles add into their output, so it starts from zero; so does a pass run
            // over its box only, since it is clear everywhere else.
            if scheduled.kernel == Kernel::ParticleSplat || areas[at_pass].is_some() {
                encoder.clear_buffer(output, 0, None);
            }
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some(&node.label),
                    ..Default::default()
                });
                pass.set_pipeline(custom_pipeline.as_ref().expect("every pass has a pipeline"));
                pass.set_bind_group(0, &bind, &[]);
                match scheduled.kernel {
                    // One thread per particle, in rows of `values.y` threads.
                    Kernel::ParticleSplat => {
                        let (threads, row) = (p.values[0] as u32, (p.values[1] as u32).max(1));
                        pass.dispatch_workgroups(row.div_ceil(64), threads.div_ceil(row).max(1), 1);
                    }
                    Kernel::GlintSeeds => {
                        let cell = (p.values[0] * 2.).max(8.).ceil() as u32;
                        pass.dispatch_workgroups(
                            plan.width.div_ceil(cell),
                            plan.height.div_ceil(cell),
                            1,
                        );
                    }
                    // One 64-pixel row segment per workgroup.
                    Kernel::BlurHorizontal | Kernel::FrostHorizontal => {
                        pass.dispatch_workgroups(plan.width.div_ceil(64), plan.height, 1)
                    }
                    _ => match areas[at_pass] {
                        Some([_, _, width, height]) => {
                            if width > 0 && height > 0 {
                                pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1)
                            }
                        }
                        None => pass.dispatch_workgroups(
                            plan.width.div_ceil(8),
                            plan.height.div_ceil(8),
                            1,
                        ),
                    },
                }
            }
        }
        mark(&mut encoder, None)?;
        // The picture itself was not run (it is clear at this time): give a clear one.
        if let Some(at) = to_keep.take() {
            keep_now(&mut encoder, at, &mut keeping, &buffers);
        }
        if stale[plan.output_slot] {
            encoder.clear_buffer(&buffers[plan.output_slot], 0, None);
        }
        let readback = if download {
            let buffer = resources.take(
                &self.device,
                byte_size,
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                "readback",
            )?;
            encoder.copy_buffer_to_buffer(&buffers[plan.output_slot], 0, &buffer, 0, byte_size);
            Some(buffer)
        } else {
            let flag = resources.take(
                &self.device,
                4,
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                "finite output flag",
            )?;
            self.queue.write_buffer(&flag, 0, &[0; 4]);
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("finite output validation"),
                layout: &self.validation_pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffers[plan.output_slot].as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: flag.as_entire_binding(),
                    },
                ],
            });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
                pass.set_pipeline(&self.validation_pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(plan.width.div_ceil(8), plan.height.div_ceil(8), 1);
            }
            let status = resources.take(
                &self.device,
                4,
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                "validation readback",
            )?;
            encoder.copy_buffer_to_buffer(&flag, 0, &status, 0, 4);
            Some(status)
        };
        resources.submitted();
        let submission = self.queue.submit([encoder.finish()]);
        // Kept only once their frame is on its way to the card: a render that failed
        // before this leaves nothing half made in the store.
        if !keeping.is_empty()
            && let Ok(mut results) = self.results.lock()
        {
            for (id, buffer) in keeping {
                results.insert(id, buffer, room);
            }
        }
        let readback = readback.unwrap();
        if download {
            let pixels = self.map_values(&readback, submission)?;
            resources.completed();
            validate_pixels(&pixels)?;
            Ok(Executed::Cpu(pixels))
        } else {
            let flags: Vec<u32> = self.map_values(&readback, submission)?;
            resources.completed();
            if flags[0] != 0 {
                return Err(error("render produced non-finite pixels"));
            }
            Ok(Executed::Gpu(resources.pin(&buffers[plan.output_slot])))
        }
    }

    /// For each pass of `plan`, the box (`[x, y, width, height]`) it runs over when that
    /// is less than half the frame: the one pass of a node that is clear outside it. The
    /// output is cleared first, so it is the picture a full-frame pass makes.
    fn areas(
        &self,
        plan: &Plan,
        time: veylvik_document::Time,
        vectors: &BTreeMap<usize, (Arc<GpuFrame>, [u32; 2])>,
    ) -> Result<Vec<Option<[u32; 4]>>> {
        let count = plan.passes.len();
        if !self.area_dispatch {
            return Ok(vec![None; count]);
        }
        use crate::nodes::Extent;
        // A shape or a text is drawn over its own box (see vector.rs).
        let known = |i: usize| -> Option<Extent> {
            if !matches!(
                plan.nodes[i].operation,
                Operation::Path(_) | Operation::Text(_)
            ) {
                return None;
            }
            let (frame, [x, y]) = vectors.get(&i)?;
            Some(Extent::Area([
                *x as i32,
                *y as i32,
                (*x + frame.width()) as i32,
                (*y + frame.height()) as i32,
            ]))
        };
        let extents = plan.extents(time, &|_| Extent::Everything, &known)?;
        let (width, height) = (plan.width as i32, plan.height as i32);
        let frame = Extent::Area([0, 0, width, height]);
        let alone = |at: usize| {
            let node = plan.passes[at].node;
            (at == 0 || plan.passes[at - 1].node != node)
                && plan.passes.get(at + 1).is_none_or(|next| next.node != node)
        };
        // Kernels whose threads are pixels of the frame, dispatched eight by eight.
        let pixels = |kernel: Kernel| {
            !matches!(
                kernel,
                Kernel::ParticleSplat
                    | Kernel::GlintSeeds
                    | Kernel::BlurHorizontal
                    | Kernel::FrostHorizontal
                    | Kernel::Custom
                    | Kernel::RenderScene
            ) && !kernel.entry().starts_with("particle_")
        };
        Ok((0..count)
            .map(|at| {
                let pass = &plan.passes[at];
                if !alone(at) || !pixels(pass.kernel) {
                    return None;
                }
                let area = match extents[pass.node].intersect(frame) {
                    Extent::Nothing => [0, 0, 0, 0],
                    Extent::Area([l, t, r, b]) => {
                        [l as u32, t as u32, (r - l) as u32, (b - t) as u32]
                    }
                    Extent::Everything => return None,
                };
                // Under half the frame: worth a clear and a smaller dispatch.
                (u64::from(area[2]) * u64::from(area[3]) * 2
                    < u64::from(plan.width) * u64::from(plan.height))
                .then_some(area)
            })
            .collect())
    }
    fn wait(&self, submission: wgpu::SubmissionIndex) -> Result<()> {
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .map_err(error)?;
        Ok(())
    }
    fn map_values<T: bytemuck::Pod>(
        &self,
        buffer: &wgpu::Buffer,
        submission: wgpu::SubmissionIndex,
    ) -> Result<Vec<T>> {
        let (tx, rx) = std::sync::mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.wait(submission)?;
        rx.recv_timeout(std::time::Duration::from_secs(1))
            .map_err(error)?
            .map_err(error)?;
        let mapped = buffer.slice(..).get_mapped_range();
        let pixels = bytemuck::cast_slice(&mapped).to_vec();
        drop(mapped);
        buffer.unmap();
        Ok(pixels)
    }
    /// Explicit readback for export, picking, or reference checks. The frame must
    /// belong to this renderer. Its crop is applied after the full buffer is read.
    pub fn read_frame(&self, frame: &GpuFrame) -> Result<RenderedFrame> {
        if !Arc::ptr_eq(&self.owner, &frame.owner) {
            return Err(error("GPU frame belongs to another renderer"));
        }
        let mut pool = self
            .pool
            .lock()
            .map_err(|_| error("renderer resource pool poisoned"))?;
        let available = self.budget_bytes.saturating_sub(pool.stats().held_bytes);
        pool.trim(self.pool_limit_bytes.min(available));
        if pool.failed {
            return Err(error("renderer unavailable after a GPU failure"));
        }
        if pool.stats().held_bytes + frame.byte_size() > self.budget_bytes {
            return Err(error("GPU readback exceeds budget"));
        }
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let allocation = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let result = (|| {
            let mut resources = pool.frame(self.budget_bytes);
            let buffer = resources.take(
                &self.device,
                frame.byte_size(),
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                "readback",
            )?;
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            encoder.copy_buffer_to_buffer(frame.buffer(), 0, &buffer, 0, frame.byte_size());
            resources.submitted();
            let pixels = self.map_values(&buffer, self.queue.submit([encoder.finish()]))?;
            resources.completed();
            validate_pixels(&pixels)?;
            Ok(crop(pixels, frame.region, frame.width, frame.height))
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
/// Display/output boundary: unpremultiply, clip to SDR and apply the sRGB transfer curve.
/// The engine itself never clips HDR values to this range.
pub fn to_srgba8(pixels: &[[f32; 4]]) -> Vec<u8> {
    pixels
        .iter()
        .flat_map(|p| {
            let a = p[3].clamp(0.0, 1.0);
            let encode = |v: f32| -> u8 {
                let x = if a > 0.0 {
                    (v / a).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let s = if x <= 0.0031308 {
                    x * 12.92
                } else {
                    1.055 * x.powf(1.0 / 2.4) - 0.055
                };
                (s * 255.0).round() as u8
            };
            [
                encode(p[0]),
                encode(p[1]),
                encode(p[2]),
                (a * 255.0).round() as u8,
            ]
        })
        .collect()
}

enum Executed {
    Cpu(Vec<[f32; 4]>),
    Gpu(pool::PinnedBuffer),
}
fn validate_pixels(pixels: &[[f32; 4]]) -> Result<()> {
    if pixels.iter().flatten().any(|v| !v.is_finite()) {
        return Err(error("render produced non-finite pixels"));
    }
    Ok(())
}
fn crop(full: Vec<[f32; 4]>, region: Region, width: u32, height: u32) -> RenderedFrame {
    if region.x == 0 && region.y == 0 && region.width == width && region.height == height {
        return RenderedFrame {
            region,
            pixels: full,
        };
    }
    let mut pixels = Vec::with_capacity(region.width as usize * region.height as usize);
    for y in region.y..region.y + region.height {
        let start = (y * width + region.x) as usize;
        pixels.extend_from_slice(&full[start..start + region.width as usize]);
    }
    RenderedFrame { region, pixels }
}

/// Shared headless/window Vulkan setup with optional DMA-BUF ownership support.
pub async fn create_gpu() -> Result<(wgpu::Instance, wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        })
        .await
        .map_err(error)?;
    let (device, queue) = veylvik_interop::request_device(
        &adapter,
        &wgpu::DeviceDescriptor {
            label: Some("Veylvik"),
            required_limits: device_limits(&adapter),
            // Compiled programs kept between runs, where the driver allows it.
            required_features: adapter.features() & wgpu::Features::PIPELINE_CACHE,
            ..Default::default()
        },
    )
    .await
    .map_err(error)?;

    Ok((instance, adapter, device, queue))
}

pub use veylvik_interop::{DmaFrame, DmaLayout};
