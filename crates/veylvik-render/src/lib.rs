//! Whole-frame GPU execution. No window, document mutation, or timeline dependency.
mod frame;
pub use frame::{Image, ImageInputs, Region, RenderRequest, RenderedFrame};
mod plan;
mod pool;
mod scheduler;
pub use plan::{Kernel, PassInfo, Plan};
pub use pool::ResourceStats;
pub use scheduler::{PreviewResult, PreviewScheduler, PreviewTicket};
use std::collections::BTreeMap;
use std::sync::Mutex;
use veylvik_document::{Node, Operation, Time};
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
fn params(node: &Node, width: u32, height: u32, time: Time) -> Result<Params> {
    let mut p = Params {
        size_op: [width, height, 0, 0],
        values: [0.0; 4],
        extra: [0.0; 4],
    };
    match &node.operation {
        Operation::Solid { color } => {
            p.values = *color;
        }
        Operation::Noise { seed, scale } => {
            p.size_op[3] = *seed;
            p.values[0] = scale.at(time)?;
        }
        Operation::Exposure { stops } => {
            let stops = stops.at(time)?;
            p.values[0] = stops.exp2();
        }
        Operation::Transform { x, y } => {
            p.values[0] = x.at(time)?;
            p.values[1] = y.at(time)?;
        }
        Operation::Blur { radius } => {
            p.values[0] = radius.at(time)?;
        }
        Operation::Merge | Operation::Image { .. } => {}
    }
    Ok(p)
}
/// One device and queue own every intermediate. Rendering does not silently fall back to CPU.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: BTreeMap<Kernel, wgpu::ComputePipeline>,
    layout: wgpu::BindGroupLayout,
    pool: Mutex<pool::Pool>,
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
        if let Some(e) = scope.pop().await {
            return Err(error(e));
        }
        Ok(Self {
            device,
            queue,
            pipelines,
            layout,
            pool: Mutex::new(pool::Pool::default()),
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
    pub fn render_frame(
        &self,
        plan: &Plan,
        request: RenderRequest,
        inputs: &ImageInputs,
    ) -> Result<RenderedFrame> {
        let mut pool = self
            .pool
            .lock()
            .map_err(|_| error("renderer resource pool poisoned"))?;
        pool.trim(self.pool_limit_bytes.min(self.budget_bytes));
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
        let mut source_bytes = 0;
        for asset in plan.required_assets() {
            let source = inputs
                .get(asset)
                .ok_or_else(|| error(format!("missing image input: {asset}")))?;
            if source.byte_size() > limits.max_storage_buffer_binding_size
                || source.byte_size() > limits.max_buffer_size
            {
                return Err(error(format!(
                    "image input {asset} exceeds adapter buffer limits"
                )));
            }
            source_bytes += source.byte_size();
        }
        let working_bytes = plan.working_bytes() + source_bytes;
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
            .map(|n| params(n, plan.width, plan.height, time))
            .collect::<Result<_>>()?;
        for (node, p) in plan.nodes.iter().zip(&mut evaluated) {
            if let Operation::Image { asset } = &node.operation {
                let image = inputs.get(asset).unwrap();
                p.extra[0] = image.width() as f32;
                p.extra[1] = image.height() as f32;
            }
        }
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let allocation = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let result = self.encode_and_read(plan, &evaluated, byte_size, inputs, &mut pool);
        let allocation_error = pollster::block_on(allocation.pop());
        let validation_error = pollster::block_on(validation.pop());
        if let Some(e) = allocation_error.or(validation_error) {
            pool.trim(0);
            pool.failed = true;
            return Err(error(e));
        }
        pool.trim(self.pool_limit_bytes.min(self.budget_bytes));
        let full = result?;
        if region.x == 0
            && region.y == 0
            && region.width == plan.width
            && region.height == plan.height
        {
            return Ok(RenderedFrame {
                region,
                pixels: full,
            });
        }
        let mut pixels = Vec::with_capacity(region.width as usize * region.height as usize);
        for y in region.y..region.y + region.height {
            let start = (y * plan.width + region.x) as usize;
            pixels.extend_from_slice(&full[start..start + region.width as usize]);
        }
        Ok(RenderedFrame { region, pixels })
    }
    fn encode_and_read(
        &self,
        plan: &Plan,
        evaluated: &[Params],
        byte_size: u64,
        inputs: &ImageInputs,
        pool: &mut pool::Pool,
    ) -> Result<Vec<[f32; 4]>> {
        let mut resources = pool.frame(self.budget_bytes);
        let dummy = resources.take(
            &self.device,
            16,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            "empty source",
        )?;
        self.queue.write_buffer(&dummy, 0, &[0; 16]);
        let mut sources = BTreeMap::new();
        for key in plan.required_assets() {
            let image = inputs.get(key).unwrap();
            let buffer = resources.take(
                &self.device,
                image.byte_size(),
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                key,
            )?;
            self.queue
                .write_buffer(&buffer, 0, bytemuck::cast_slice(image.pixels()));
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
                pass.dispatch_workgroups(plan.width.div_ceil(8), plan.height.div_ceil(8), 1);
            }
        }
        let readback = resources.take(
            &self.device,
            byte_size,
            wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            "readback",
        )?;
        encoder.copy_buffer_to_buffer(&buffers[plan.output_slot], 0, &readback, 0, byte_size);
        resources.submitted();
        let submission = self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .map_err(error)?;
        rx.recv_timeout(std::time::Duration::from_secs(1))
            .map_err(error)?
            .map_err(error)?;
        let mapped = readback.slice(..).get_mapped_range();
        let pixels: Vec<[f32; 4]> = bytemuck::cast_slice(&mapped).to_vec();
        drop(mapped);
        readback.unmap();
        resources.completed();
        if pixels.iter().flatten().any(|v| !v.is_finite()) {
            return Err(error("render produced non-finite pixels"));
        }
        Ok(pixels)
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
