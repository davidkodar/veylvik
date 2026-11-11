//! Whole-frame GPU execution. No window, document mutation, or timeline dependency.
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
        let order = composition.validate()?;
        let mut reachable = BTreeSet::new();
        let mut todo = vec![composition.output];
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
        *uses.entry(composition.output).or_default() += 1;
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
            output: composition.output,
            nodes,
            slots,
            slot_count,
        })
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
            if !(1.0..=8192.0).contains(&p.values[0]) {
                return Err(error("noise scale must be in 1..=8192 pixels"));
            }
        }
        Operation::Exposure { stops } => {
            p.size_op[2] = 2;
            let stops = stops.at(time)?;
            if !(-32.0..=32.0).contains(&stops) {
                return Err(error("exposure must be in -32..=32 stops"));
            }
            p.values[0] = stops.exp2();
        }
        Operation::Transform { x, y } => {
            p.size_op[2] = 3;
            p.values[0] = x.at(time)?;
            p.values[1] = y.at(time)?;
            if p.values[..2].iter().any(|v| v.abs() > 1_000_000.0) {
                return Err(error("translation exceeds one million pixels"));
            }
        }
        Operation::Blur { radius } => {
            p.size_op[2] = 4;
            p.values[0] = radius.at(time)?;
            if !(0.0..=32.0).contains(&p.values[0]) {
                return Err(error("blur radius must be in 0..=32 pixels"));
            }
        }
        Operation::Merge => p.size_op[2] = 5,
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
        time.seconds()?;
        let byte_size = plan.width as u64 * plan.height as u64 * 16;
        let limits = self.device.limits();
        if byte_size > limits.max_storage_buffer_binding_size || byte_size > limits.max_buffer_size
        {
            return Err(error("frame exceeds adapter buffer limits"));
        }
        if plan.working_bytes() > self.budget_bytes {
            return Err(error(format!(
                "render requires {} bytes, budget is {}",
                plan.working_bytes(),
                self.budget_bytes
            )));
        }
        // Validate evaluated values before allocating or submitting any GPU work.
        let evaluated: Vec<_> = plan
            .nodes
            .iter()
            .map(|n| params(n, plan.width, plan.height, time))
            .collect::<Result<_>>()?;
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let allocation = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let result = self.encode_and_read(plan, &evaluated, byte_size);
        let allocation_error = pollster::block_on(allocation.pop());
        let validation_error = pollster::block_on(validation.pop());
        if let Some(e) = allocation_error.or(validation_error) {
            return Err(error(e));
        }
        result
    }
    fn encode_and_read(
        &self,
        plan: &Plan,
        evaluated: &[Params],
        byte_size: u64,
    ) -> Result<Vec<[f32; 4]>> {
        let dummy = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("empty source"),
                contents: &[0; 16],
                usage: wgpu::BufferUsages::STORAGE,
            });
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
