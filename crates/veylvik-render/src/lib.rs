//! Whole-frame GPU execution. No window, document mutation, or timeline dependency.
mod cache;
mod diagnostics;
mod environment_bake;
mod material_preview;
pub use material_preview::material_preview_revision;
pub mod flow;
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
mod plan;
pub mod playback;
mod pool;
mod preview_scale;
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
mod scheduler;
mod texture;
mod vector;
mod video;
pub use plan::{Kernel, PassInfo, Plan};
pub use pool::ResourceStats;
pub use scheduler::{GpuPreviewScheduler, PreviewResult, PreviewScheduler, PreviewTicket};
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
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: BTreeMap<Kernel, wgpu::ComputePipeline>,
    layout: wgpu::BindGroupLayout,
    ltc: wgpu::Buffer,
    studio_environment: wgpu::Buffer,
    default_lighting: wgpu::Buffer,
    validation_pipeline: wgpu::ComputePipeline,
    texture_pipeline: wgpu::ComputePipeline,
    video_pipeline: wgpu::ComputePipeline,
    encode_pipeline: wgpu::ComputePipeline,
    shutter_pipeline: wgpu::ComputePipeline,
    preview_pipeline: wgpu::ComputePipeline,
    vector_backend: Mutex<Option<vector::Backend>>,
    vector_cache: Mutex<cache::FrameCache<vector::Key>>,
    pub vector_cache_limit_bytes: u64,
    pool: Mutex<pool::Pool>,
    cache: Mutex<cache::FrameCache>,
    flow_engine: Mutex<Option<Arc<flow::Engine>>>,
    scene_renderer: std::sync::RwLock<Option<Arc<dyn SceneRenderer>>>,
    ocio_cache: Mutex<std::collections::VecDeque<ocio::CachedProcessor>>,
    pub cache_limit_bytes: u64,
    owner: Arc<frame::DeviceIdentity>,
    pub pool_limit_bytes: u64,
    pub adapter: wgpu::AdapterInfo,
    pub budget_bytes: u64,
}
/// Request native frame buffers beyond WebGPU's conservative 128 MiB default.
/// UHD fits that default narrowly; DCI 4K requires larger bindings. Cap requests
/// to the adapter's advertised support, retaining explicit render-time checks.
pub fn device_limits(adapter: &wgpu::Adapter) -> wgpu::Limits {
    let available = adapter.limits();
    wgpu::Limits {
        max_storage_buffer_binding_size: available
            .max_storage_buffer_binding_size
            .min(512 * 1024 * 1024),
        max_buffer_size: available.max_buffer_size.min(512 * 1024 * 1024),
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
                concat!(
                    include_str!("kernels.wgsl"),
                    include_str!("environment.wgsl"),
                    include_str!("working_space.wgsl"),
                    include_str!("matte.wgsl"),
                    include_str!("lut.wgsl"),
                    include_str!("shadow.wgsl"),
                    include_str!("particles.wgsl"),
                    include_str!("material_layers.wgsl")
                )
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
                        min_binding_size: std::num::NonZeroU64::new(48),
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
        let pipelines = Kernel::ALL
            .into_iter()
            .map(|kernel| {
                (
                    kernel,
                    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                        label: Some(kernel.entry()),
                        layout: Some(&pipeline_layout),
                        module: &shader,
                        entry_point: Some(kernel.entry()),
                        compilation_options: Default::default(),
                        cache: None,
                    }),
                )
            })
            .collect();
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
        let preview_pipeline = preview_scale::pipeline(&device);
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(error(e));
        }
        Ok(Self {
            device,
            queue,
            pipelines,
            layout,
            ltc,
            studio_environment,
            default_lighting,
            validation_pipeline,
            texture_pipeline,
            video_pipeline,
            encode_pipeline,
            shutter_pipeline,
            preview_pipeline,
            vector_backend: Mutex::new(None),
            vector_cache: Mutex::new(cache::FrameCache::default()),
            vector_cache_limit_bytes: 32 * 1024 * 1024,
            pool: Mutex::new(pool::Pool::default()),
            cache: Mutex::new(cache::FrameCache::default()),
            ocio_cache: Mutex::new(Default::default()),
            flow_engine: Mutex::new(None),
            scene_renderer: std::sync::RwLock::new(None),
            cache_limit_bytes: 64 * 1024 * 1024,
            owner: Arc::new(frame::DeviceIdentity),
            pool_limit_bytes: 128 * 1024 * 1024,
            adapter: info,
            budget_bytes: 2048 * 1024 * 1024,
        })
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
        let working_bytes = if download {
            plan.working_bytes()
        } else {
            plan.gpu_working_bytes()
        } + source_bytes
            + pool.stats().held_bytes
            - plan.vector_bytes();
        if working_bytes > self.budget_bytes {
            return Err(error(format!(
                "render requires {} bytes, budget is {}",
                working_bytes, self.budget_bytes
            )));
        }
        // Validate evaluated values before allocating or submitting any GPU work.
        let mut evaluated: Vec<_> = plan
            .nodes
            .iter()
            .zip(&plan.curves)
            .map(|(n, curves)| params(n, &plan.nodes, curves, plan.width, plan.height, time))
            .collect::<Result<_>>()?;
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
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let allocation = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let result = self.encode_and_read(
            plan,
            (&evaluated, request.time, request.scene_view),
            inputs,
            &vectors,
            &mut pool,
            download,
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
    /// animation times do not invalidate it. At most 16 frames are retained.
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
    pub fn cache_stats(&self) -> Result<CacheStats> {
        Ok(self
            .cache
            .lock()
            .map_err(|_| error("frame cache poisoned"))?
            .stats())
    }
    pub fn clear_frame_cache(&self) -> Result<()> {
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
    fn encode_and_read(
        &self,
        plan: &Plan,
        (evaluated, time, scene_view): (
            &[Params],
            veylvik_document::Time,
            Option<veylvik_scene::Camera>,
        ),
        inputs: &ImageInputs,
        vectors: &BTreeMap<usize, Arc<GpuFrame>>,
        pool: &mut pool::Pool,
        download: bool,
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
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    "frame slot",
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        for scheduled in &plan.passes {
            if scheduled.kernel == Kernel::RenderScene {
                let renderer = self
                    .scene_renderer
                    .read()
                    .map_err(|_| error("scene renderer lock poisoned"))?
                    .clone()
                    .ok_or_else(|| {
                        error("Render scene needs the 3D renderer, which this host has not loaded")
                    })?;
                let images: Vec<(veylvik_document::NodeId, &wgpu::Buffer)> = scheduled
                    .cards
                    .iter()
                    .filter_map(|(source, input)| match input {
                        plan::Input::Value(slot) => Some((*source, &buffers[*slot])),
                        _ => None,
                    })
                    .collect();
                // Motion blur renders several moments and averages them in place.
                let moments = scene_hook::sample_times(plan, scheduled.node, time)?;
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
                        images: &images,
                        environment: &self.studio_environment,
                        ltc: &self.ltc,
                        target: &buffers[scheduled.output],
                    })?;
                }
                continue;
            }
            let node = &plan.nodes[scheduled.node];
            let p = &evaluated[scheduled.node];
            let uniform = resources.take(
                &self.device,
                48,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                "parameters",
            )?;
            self.queue.write_buffer(&uniform, 0, bytemuck::bytes_of(p));
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
                    plan::Input::Vector(index) => vectors[index].buffer(),
                }
            };
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&node.label),
                layout: &self.layout,
                entries: &[
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
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some(&node.label),
                    ..Default::default()
                });
                pass.set_pipeline(&self.pipelines[&scheduled.kernel]);
                pass.set_bind_group(0, &bind, &[]);
                match scheduled.kernel {
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
                    _ => {
                        pass.dispatch_workgroups(plan.width.div_ceil(8), plan.height.div_ceil(8), 1)
                    }
                }
            }
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
            ..Default::default()
        },
    )
    .await
    .map_err(error)?;

    Ok((instance, adapter, device, queue))
}

pub use veylvik_interop::{DmaFrame, DmaLayout};
