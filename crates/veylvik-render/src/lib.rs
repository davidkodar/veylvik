//! Whole-frame GPU execution. No window, document mutation, or timeline dependency.
mod frame;
pub use frame::{Image, ImageInputs, Region, RenderRequest, RenderedFrame};
use std::collections::{BTreeMap, BTreeSet};
use veylvik_document::{Composition, Node, NodeId, Operation, Time};
use wgpu::util::DeviceExt;
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
/// Immutable plan. Only reachable nodes execute; all document nodes are validated.
pub struct Plan {
    width: u32,
    height: u32,
    nodes: Vec<Node>,
    output: NodeId,
    slots: BTreeMap<NodeId, usize>,
    slot_count: usize,
}
impl Plan {
    pub fn compile(composition: &Composition) -> Result<Self> {
        Self::compile_output(composition, composition.output)
    }
    /// Compile an intermediate node for inspection without mutating the saved output.
    pub fn compile_output(composition: &Composition, output: NodeId) -> Result<Self> {
        let order = composition.validate()?;
        if !composition.nodes.contains_key(&output) {
            return Err(error("requested output node is missing"));
        }
        let mut reachable = BTreeSet::new();
        let mut todo = vec![output];
        while let Some(id) = todo.pop() {
            if reachable.insert(id) {
                todo.extend(composition.nodes[&id].inputs.values().copied());
            }
        }
        let nodes: Vec<Node> = order
            .into_iter()
            .filter(|id| reachable.contains(id))
            .map(|id| composition.nodes[&id].clone())
            .collect();
        let mut uses: BTreeMap<NodeId, usize> = BTreeMap::new();
        for node in &nodes {
            for source in node.inputs.values() {
                *uses.entry(*source).or_default() += 1;
            }
        }
        *uses.entry(output).or_default() += 1;
        let mut slots = BTreeMap::new();
        let mut free = Vec::new();
        let mut slot_count = 0;
        for node in &nodes {
            let slot = free.pop().unwrap_or_else(|| {
                let slot = slot_count;
                slot_count += 1;
                slot
            });
            slots.insert(node.id, slot);
            // Release after assigning the output, so a pass never aliases its inputs.
            for source in node.inputs.values() {
                let remaining = uses.get_mut(source).unwrap();
                *remaining -= 1;
                if *remaining == 0 {
                    free.push(slots[source]);
                }
            }
        }
        Ok(Self {
            width: composition.width,
            height: composition.height,
            output,
            nodes,
            slots,
            slot_count,
        })
    }
    pub fn required_assets(&self) -> BTreeSet<&str> {
        self.nodes
            .iter()
            .filter_map(|node| match &node.operation {
                Operation::Image { asset } => Some(asset.as_str()),
                _ => None,
            })
            .collect()
    }
    pub fn image_slots(&self) -> usize {
        self.slot_count
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    /// Conservative upper bound including readback and uniforms, excluding driver overhead.
    pub fn working_bytes(&self) -> u64 {
        self.width as u64 * self.height as u64 * 16 * (self.slot_count as u64 + 1)
            + self.nodes.len() as u64 * 48
            + 16
    }
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
            p.size_op[2] = 1;
            p.size_op[3] = *seed;
            p.values[0] = scale.at(time)?;
        }
        Operation::Exposure { stops } => {
            p.size_op[2] = 2;
            let stops = stops.at(time)?;
            p.values[0] = stops.exp2();
        }
        Operation::Transform { x, y } => {
            p.size_op[2] = 3;
            p.values[0] = x.at(time)?;
            p.values[1] = y.at(time)?;
        }
        Operation::Blur { radius } => {
            p.size_op[2] = 4;
            p.values[0] = radius.at(time)?;
        }
        Operation::Merge => p.size_op[2] = 5,
        Operation::Image { .. } => p.size_op[2] = 6,
    }
    Ok(p)
}
/// One device and queue own every intermediate. Rendering does not silently fall back to CPU.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
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
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("composition"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        if let Some(e) = scope.pop().await {
            return Err(error(e));
        }
        Ok(Self {
            device,
            queue,
            pipeline,
            adapter: info,
            budget_bytes: 512 * 1024 * 1024,
        })
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
        let result = self.encode_and_read(plan, &evaluated, byte_size, inputs);
        let allocation_error = pollster::block_on(allocation.pop());
        let validation_error = pollster::block_on(validation.pop());
        if let Some(e) = allocation_error.or(validation_error) {
            return Err(error(e));
        }
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
    ) -> Result<Vec<[f32; 4]>> {
        let dummy = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("empty source"),
                contents: &[0; 16],
                usage: wgpu::BufferUsages::STORAGE,
            });
        // Upload each asset once, even when several Image nodes reference it.
        let sources: BTreeMap<_, _> = plan
            .required_assets()
            .into_iter()
            .map(|key| {
                let image = inputs.get(key).unwrap();
                (
                    key,
                    self.device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some(key),
                            contents: bytemuck::cast_slice(image.pixels()),
                            usage: wgpu::BufferUsages::STORAGE,
                        }),
                )
            })
            .collect();
        let buffers: Vec<_> = (0..plan.slot_count)
            .map(|_| {
                self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("frame slot"),
                    size: byte_size,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        let layout = self.pipeline.get_bind_group_layout(0);
        for (node, p) in plan.nodes.iter().zip(evaluated) {
            let uniform = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("parameters"),
                    contents: bytemuck::bytes_of(p),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let output = &buffers[plan.slots[&node.id]];
            let ports = node.operation.inputs();
            let input = |index: usize| -> &wgpu::Buffer {
                if index == 0
                    && let Operation::Image { asset } = &node.operation
                {
                    return &sources[asset.as_str()];
                }
                ports
                    .get(index)
                    .and_then(|port| node.inputs.get(*port))
                    .map(|id| &buffers[plan.slots[id]])
                    .unwrap_or(&dummy)
            };
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&node.label),
                layout: &layout,
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
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(plan.width.div_ceil(8), plan.height.div_ceil(8), 1);
            }
        }
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: byte_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(
            &buffers[plan.slots[&plan.output]],
            0,
            &readback,
            0,
            byte_size,
        );
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
