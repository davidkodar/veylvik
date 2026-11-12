//! Whole-frame GPU execution. No window, document mutation, or timeline dependency.
mod cache;
mod diagnostics;
pub use diagnostics::Diagnostics;
mod frame;
pub use cache::CacheStats;
pub use frame::{GpuFrame, Image, ImageInputs, Region, RenderRequest, RenderedFrame};
mod plan;
mod pool;
mod scheduler;
mod texture;
mod vector;
pub use plan::{Kernel, PassInfo, Plan};
pub use pool::ResourceStats;
pub use scheduler::{GpuPreviewScheduler, PreviewResult, PreviewScheduler, PreviewTicket};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
pub use texture::{TextureEncoding, TextureTarget};
pub use vector::MAX_VECTOR_DIMENSION;
use veylvik_document::{CompiledScalar, Node, Operation, Time};
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
    match &node.operation {
        Operation::Solid { color } => {
            p.values = *color;
        }
        Operation::Colorize { low, high } => {
            p.values = *low;
            p.extra = *high;
        }
        Operation::Noise { seed, .. } => {
            p.size_op[3] = *seed;
            p.values[0] = sample("scale")?;
        }
        Operation::FractalNoise { seed, .. } => {
            p.size_op[3] = *seed;
            p.values = [
                sample("scale")?,
                sample("octaves")?.round(),
                sample("roughness")?,
                sample("evolution")?,
            ];
        }
        Operation::Exposure { .. } => {
            let stops = sample("stops")?;
            p.values[0] = stops.exp2();
        }
        Operation::Transform { .. } | Operation::Displace { .. } => {
            p.values[0] = sample("x")?;
            p.values[1] = sample("y")?;
        }
        Operation::Blur { .. } => {
            p.values[0] = sample("radius")?;
        }
        Operation::Affine { .. } => {
            let (sx, sy) = (sample("scale_x")? as f64, sample("scale_y")? as f64);
            if sx.abs() >= 1e-6_f32 as f64 && sy.abs() >= 1e-6_f32 as f64 {
                let radians = (sample("rotation")? as f64).rem_euclid(360.0).to_radians();
                let (sin, cos) = radians.sin_cos();
                let (px, py) = (sample("pivot_x")? as f64, sample("pivot_y")? as f64);
                let (tx, ty) = (px + sample("x")? as f64, py + sample("y")? as f64);
                let m = [cos / sx, sin / sx, -sin / sy, cos / sy];
                p.values = m.map(|v| v as f32);
                p.extra = [
                    (px - m[0] * tx - m[1] * ty) as f32,
                    (py - m[2] * tx - m[3] * ty) as f32,
                    1.0,
                    0.0,
                ];
            }
        }
        Operation::Ellipse { .. } => {
            p.values = [
                sample("x")?,
                sample("y")?,
                sample("radius_x")?,
                sample("radius_y")?,
            ];
            p.extra[0] = sample("feather")?.max(1.0);
        }
        Operation::Path { .. } => {
            p.values = [width as f32, height as f32, 0.0, 0.0];
            p.extra[0] = width as f32;
        }
        Operation::ChannelMask { channel } => {
            p.size_op[3] = *channel as u32;
        }
        Operation::Blend { mode, .. } => {
            p.size_op[3] = *mode as u32;
            p.values[0] = sample("opacity")?;
        }
        Operation::Merge {} | Operation::Mask {} | Operation::Image { .. } => {}
    }
    Ok(p)
}
/// One device and queue own every intermediate. Rendering does not silently fall back to CPU.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: BTreeMap<Kernel, wgpu::ComputePipeline>,
    layout: wgpu::BindGroupLayout,
    validation_pipeline: wgpu::ComputePipeline,
    texture_pipeline: wgpu::ComputePipeline,
    vector_backend: Mutex<Option<vector::Backend>>,
    vector_cache: Mutex<cache::FrameCache<vector::Key>>,
    pub vector_cache_limit_bytes: u64,
    pool: Mutex<pool::Pool>,
    cache: Mutex<cache::FrameCache>,
    pub cache_limit_bytes: u64,
    owner: Arc<frame::DeviceIdentity>,
    pub pool_limit_bytes: u64,
    pub adapter: wgpu::AdapterInfo,
    pub budget_bytes: u64,
}
impl Renderer {
    pub fn new() -> Result<Self> {
        pollster::block_on(Self::create())
    }
    async fn create() -> Result<Self> {
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
        let info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Veylvik"),
                ..Default::default()
            })
            .await
            .map_err(error)?;
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
            source: wgpu::ShaderSource::Wgsl(include_str!("kernels.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composition buffers"),
            entries: &[
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
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(error(e));
        }
        Ok(Self {
            device,
            queue,
            pipelines,
            layout,
            validation_pipeline,
            texture_pipeline,
            vector_backend: Mutex::new(None),
            vector_cache: Mutex::new(cache::FrameCache::default()),
            vector_cache_limit_bytes: 32 * 1024 * 1024,
            pool: Mutex::new(pool::Pool::default()),
            cache: Mutex::new(cache::FrameCache::default()),
            cache_limit_bytes: 64 * 1024 * 1024,
            owner: Arc::new(frame::DeviceIdentity),
            pool_limit_bytes: 128 * 1024 * 1024,
            adapter: info,
            budget_bytes: 512 * 1024 * 1024,
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
        for asset in plan.required_assets() {
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
        for asset in plan.required_assets() {
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
            .map(|(n, curves)| params(n, curves, plan.width, plan.height, time))
            .collect::<Result<_>>()?;
        for (node, p) in plan.nodes.iter().zip(&mut evaluated) {
            if let Operation::Image { asset } = &node.operation {
                let source = inputs.source(asset).unwrap();
                let region = source.region();
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
        let result = self.encode_and_read(plan, &evaluated, inputs, &vectors, &mut pool, download);
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
        let (Executed::Gpu(storage), region) = self.execute(plan, request, inputs, false)? else {
            unreachable!()
        };
        Ok(GpuFrame {
            identity: frame::next_identity()?,
            storage,
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
        evaluated: &[Params],
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
        for key in plan.required_assets() {
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
            let node = &plan.nodes[scheduled.node];
            let p = &evaluated[scheduled.node];
            let uniform = resources.take(
                &self.device,
                48,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                "parameters",
            )?;
            self.queue.write_buffer(&uniform, 0, bytemuck::bytes_of(p));
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
                    Kernel::BlurHorizontal => {
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
