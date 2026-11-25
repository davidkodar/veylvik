//! Flocks, simulated: stepped forward from their start in fixed steps of a 60th of a second with the kernels in particle_flock.wgsl, a copy kept every so often, so any frame is found from the nearest copy before it and is the same whichever way it was reached.
use crate::nodes::{self, RenderNode};
use crate::plan::Plan;
use crate::{Kernel, Params, Renderer, Result, error};
use veylvik_document::{Node, NodeId, Operation, Time};

/// Seconds a step.
const STEP: f64 = 1.0 / 60.0;
/// Most copies kept of one flock; past it, every other one is let go.
const KEPT: usize = 128;
/// Flocks kept at once, the last used first.
const RUNS: usize = 4;
/// Cells of the neighbour grid, eight values each.
/// The grid buffer, in values: the cells (eight values each), then the fine grid
/// (FINE_LIST in particle_flock.wgsl) and its lists, one value a particle.
const GRID_VALUES: u64 = 983_040 + nodes::particles::FLOCK_MOST as u64;

/// A flock baked to a file: where each particle was at each frame of the composition,
/// read instead of simulated while the flock is as it was when baked (`key`).
pub trait BakedFlock: Send + Sync {
    /// What the flock was made of when it was baked (`flock_key`).
    fn key(&self) -> u64;
    /// The frames it holds, as composition frame numbers, and their rate.
    fn frames(&self) -> std::ops::Range<i64>;
    fn frame_rate(&self) -> Time;
    /// Each particle's place at `frame` and its generation (negative: not alive).
    fn states(&self, frame: i64) -> Result<Vec<[f32; 4]>>;
}
/// The flocks simulated so far and the copies of them kept.
#[derive(Default)]
pub(crate) struct Store {
    runs: Vec<Run>,
}
struct Run {
    key: u64,
    /// Steps between copies (doubled as the flock grows long).
    every: u64,
    /// Copies by step, the step's whole state; and the last step reached.
    kept: Vec<(u64, wgpu::Buffer)>,
    last: Option<(u64, wgpu::Buffer)>,
}
/// What a flock's path depends on: the node, everything its scene and forces are made
/// of, and the frame's size.
pub(crate) fn key(plan: &Plan, node: usize) -> u64 {
    let nodes: Vec<Node> = plan.nodes.iter().chain(&plan.light_data).cloned().collect();
    key_of(&plan.nodes[node], &nodes, plan.width, plan.height)
}
/// The same, for a node among `nodes` in a frame of that size.
pub(crate) fn key_of(me: &Node, nodes: &[Node], width: u32, height: u32) -> u64 {
    use std::hash::{Hash, Hasher};
    let by_id: std::collections::BTreeMap<NodeId, &Node> =
        nodes.iter().map(|n| (n.id, n)).collect();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (width, height).hash(&mut hasher);
    // What it is baked to is not part of what the flock is.
    let me = &match &me.operation {
        Operation::Particles(p) if !p.bake.is_empty() => {
            let mut p = p.clone();
            p.bake.clear();
            Node {
                operation: Operation::Particles(p),
                ..me.clone()
            }
        }
        _ => me.clone(),
    };
    format!("{:?}", me.operation).hash(&mut hasher);
    // A flock born in a burst places each bird's birth inside the burst (since the
    // births were spread over the whole life): bakes made before cannot be found again.
    if let Operation::Particles(p) = &me.operation
        && !matches!(p.duration, veylvik_document::Scalar::Constant { value } if value <= 0.0)
    {
        "births in the burst".hash(&mut hasher);
    }
    let mut todo: Vec<NodeId> = ["scene", "forces"]
        .iter()
        .filter_map(|port| me.inputs.get(*port).copied())
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    while let Some(id) = todo.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(n) = by_id.get(&id) {
            format!("{:?}{:?}", n.operation, n.inputs).hash(&mut hasher);
            todo.extend(n.inputs.values().copied());
        }
    }
    hasher.finish()
}
impl Renderer {
    /// A flock's state at `time` (two values a particle: where and its generation, then
    /// its velocity and whether it is alive) and its length in bytes.
    pub(crate) fn simulate(
        &self,
        plan: &Plan,
        node: usize,
        time: Time,
        bake: Option<&dyn BakedFlock>,
    ) -> Result<(wgpu::Buffer, u64)> {
        let key = key(plan, node);
        // Baked, and as it was when baked: read, not simulated.
        if let Some(read) = self.read_bake(bake, key, time, plan.width, plan.height)? {
            return Ok(read);
        }
        let nodes: Vec<Node> = plan.nodes.iter().chain(&plan.light_data).cloned().collect();
        self.simulate_node(
            &plan.nodes[node],
            &nodes,
            key,
            plan.width,
            plan.height,
            time,
        )
    }
    /// A flock's state read from its bake, when it is baked as it now is (`key`) and the
    /// bake holds the frame at `time`.
    pub(crate) fn read_bake(
        &self,
        bake: Option<&dyn BakedFlock>,
        key: u64,
        time: Time,
        width: u32,
        height: u32,
    ) -> Result<Option<(wgpu::Buffer, u64)>> {
        let Some(bake) = bake.filter(|b| b.key() == key) else {
            return Ok(None);
        };
        let fps = bake.frame_rate().seconds().map_err(error)?;
        let frame = (time.seconds().map_err(error)? * fps).round() as i64;
        if !bake.frames().contains(&frame) {
            return Ok(None);
        }
        let most = nodes::particles::FLOCK_MOST.min(width * height / 2).max(1);
        let mut state = vec![[0.0f32; 4]; most as usize * 2];
        for (k, s) in bake
            .states(frame)?
            .into_iter()
            .take(most as usize)
            .enumerate()
        {
            state[2 * k] = s;
            state[2 * k + 1] = [0.0, 0.0, 0.0, f32::from(u8::from(s[3] >= 0.0))];
        }
        use wgpu::util::DeviceExt;
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("baked flock"),
                contents: bytemuck::cast_slice(&state),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });
        Ok(Some((buffer, u64::from(most) * 32)))
    }
    /// For baking: where each particle of flock `id` is at `time`, and its generation
    /// (negative: not alive), simulated.
    pub fn flock_states(&self, plan: &Plan, id: NodeId, time: Time) -> Result<Vec<[f32; 4]>> {
        let node = plan
            .nodes
            .iter()
            .position(|n| n.id == id)
            .ok_or_else(|| error("the flock is not in the plan"))?;
        let (state, bytes) = self.simulate(plan, node, time, None)?;
        let read = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("flock read back"),
            size: bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&state, 0, &read, 0, bytes);
        self.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        read.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = sender.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(error)?;
        receiver.recv().map_err(error)?.map_err(error)?;
        let values: Vec<[f32; 4]> =
            bytemuck::cast_slice(&read.slice(..).get_mapped_range()).to_vec();
        Ok(values
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                let [x, y, z, generation] = pair[0];
                [x, y, z, if pair[1][3] > 0.5 { generation } else { -1.0 }]
            })
            .collect())
    }
    /// What flock `id` in `composition` is made of, as a bake records it: a bake whose
    /// key differs was made from other settings.
    pub fn flock_key(composition: &veylvik_document::Composition, id: NodeId) -> Option<u64> {
        let nodes: Vec<Node> = composition.nodes.values().cloned().collect();
        let me = composition.nodes.get(&id)?;
        Some(key_of(me, &nodes, composition.width, composition.height))
    }
    /// A flock's state for `me` among `nodes` (where its scene and forces are found),
    /// known by `key`, in a frame of `width` x `height`.
    pub(crate) fn simulate_node(
        &self,
        me: &Node,
        nodes: &[Node],
        key: u64,
        width: u32,
        height: u32,
        time: Time,
    ) -> Result<(wgpu::Buffer, u64)> {
        let Operation::Particles(particles) = &me.operation else {
            return Err(error("only particles are simulated"));
        };
        let most = nodes::particles::FLOCK_MOST.min(width * height / 2).max(1);
        let bytes = u64::from(most) * 32;
        let state = |label| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let begins = f64::from(
            particles
                .start
                .at(Time::new(0, 1).map_err(error)?)
                .map_err(error)?,
        );
        let now = time.seconds().map_err(error)?;
        let out = state("flock at the frame");
        if now <= begins {
            return Ok((out, bytes));
        }
        let wanted = ((now - begins) / STEP).floor() as u64;
        let left = now - begins - wanted as f64 * STEP;
        let mut store = self
            .simulations
            .lock()
            .map_err(|_| error("flock store lock poisoned"))?;
        let at = match store.runs.iter().position(|r| r.key == key) {
            Some(at) => at,
            None => {
                if store.runs.len() >= RUNS {
                    store.runs.pop();
                }
                store.runs.insert(
                    0,
                    Run {
                        key,
                        every: 60,
                        kept: Vec::new(),
                        last: None,
                    },
                );
                0
            }
        };
        let run = store.runs.remove(at);
        store.runs.insert(0, run);
        let run = &mut store.runs[0];
        // From the nearest state at or before the step wanted (none: the start, empty).
        let mut before = state("flock before a step");
        let mut after = state("flock after a step");
        let mut first = 0u64;
        {
            let mut from = None::<&wgpu::Buffer>;
            for (step, buffer) in run.kept.iter().chain(run.last.iter()) {
                if *step <= wanted && *step >= first {
                    (first, from) = (*step, Some(buffer));
                }
            }
            let mut encoder = self.device.create_command_encoder(&Default::default());
            match from {
                Some(buffer) => encoder.copy_buffer_to_buffer(buffer, 0, &before, 0, bytes),
                None => encoder.clear_buffer(&before, 0, None),
            }
            self.queue.submit([encoder.finish()]);
        }
        let grid = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("flock grid"),
            size: GRID_VALUES * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let at_step = |k: u64| begins + k as f64 * STEP;
        for k in first..wanted {
            self.flock_step(
                me,
                nodes,
                width,
                height,
                at_step(k + 1),
                STEP,
                &before,
                &after,
                &grid,
            )?;
            std::mem::swap(&mut before, &mut after);
            let step = k + 1;
            if step % run.every == 0 && !run.kept.iter().any(|(s, _)| *s == step) {
                let copy = state("kept flock");
                let mut encoder = self.device.create_command_encoder(&Default::default());
                encoder.copy_buffer_to_buffer(&before, 0, &copy, 0, bytes);
                self.queue.submit([encoder.finish()]);
                run.kept.push((step, copy));
                if run.kept.len() > KEPT {
                    run.every *= 2;
                    let every = run.every;
                    run.kept.retain(|(s, _)| s % every == 0);
                }
            }
        }
        // Playing on continues from here.
        if wanted > first {
            let copy = state("last flock");
            let mut encoder = self.device.create_command_encoder(&Default::default());
            encoder.copy_buffer_to_buffer(&before, 0, &copy, 0, bytes);
            self.queue.submit([encoder.finish()]);
            run.last = Some((wanted, copy));
        }
        drop(store);
        if left > 1e-7 {
            self.flock_step(me, nodes, width, height, now, left, &before, &out, &grid)?;
        } else {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            encoder.copy_buffer_to_buffer(&before, 0, &out, 0, bytes);
            self.queue.submit([encoder.finish()]);
        }
        Ok((out, bytes))
    }
    /// One step of a flock to `seconds`, `span` seconds on from `before` into `after`.
    #[allow(clippy::too_many_arguments)]
    fn flock_step(
        &self,
        me: &Node,
        nodes: &[Node],
        width: u32,
        height: u32,
        seconds: f64,
        span: f64,
        before: &wgpu::Buffer,
        after: &wgpu::Buffer,
        grid: &wgpu::Buffer,
    ) -> Result<()> {
        let Operation::Particles(particles) = &me.operation else {
            return Err(error("only particles are simulated"));
        };
        let time = Time::new((seconds * 24000.0).round() as i64, 24000).map_err(error)?;
        let cx = nodes::Cx {
            node: me,
            nodes,
            width,
            height,
            time,
        };
        let data = particles
            .data(&cx)?
            .ok_or_else(|| error("particles have settings data"))?;
        let mut p: Params = bytemuck::Zeroable::zeroed();
        // The frame's size, as every pass is given it before the node's own values.
        p.size_op[0] = width;
        p.size_op[1] = height;
        particles.params(&cx, &|_| Ok(0.0), &mut p)?;
        p.values[2] = span as f32;
        let settings = self.settings_buffer(width, height, &data);
        let uniform = self.uniform_of(&p);
        let none = self.nothing();
        let gather = self.kernel_group(&uniform, &none, before, grid, &settings, &none);
        let step = self.kernel_group(&uniform, grid, before, after, &settings, &none);
        let (threads, row) = (p.values[0] as u32, (p.values[1] as u32).max(1));
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.clear_buffer(grid, 0, None);
        for (kernel, bind) in [
            (Kernel::ParticleFlockGrid, &gather),
            (Kernel::ParticleFlockScan, &gather),
            (Kernel::ParticleFlockList, &gather),
            (Kernel::ParticleStep, &step),
        ] {
            let pipeline = self.pipeline_for(kernel, nodes::particles::features_of(me))?;
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, bind, &[]);
            if kernel == Kernel::ParticleFlockScan {
                pass.dispatch_workgroups(1, 1, 1);
            } else {
                pass.dispatch_workgroups(row.div_ceil(64), threads.div_ceil(row).max(1), 1);
            }
        }
        self.queue.submit([encoder.finish()]);
        Ok(())
    }
    /// A particle system's settings as the fields pass lays them out: after the target's
    /// tile table, one value a texel in w.
    pub(crate) fn settings_buffer(
        &self,
        width: u32,
        height: u32,
        data: &crate::Image,
    ) -> wgpu::Buffer {
        use wgpu::util::DeviceExt;
        let tiles = nodes::particles::tiles_of(width, height) as usize;
        let mut settings = vec![[0.0f32; 4]; tiles];
        settings.extend(
            data.pixels()
                .iter()
                .flat_map(|t| [t[0], t[1], t[2]])
                .map(|v| [0.0, 0.0, 0.0, v]),
        );
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("particle settings"),
                contents: bytemuck::cast_slice(&settings),
                usage: wgpu::BufferUsages::STORAGE,
            })
    }
    pub(crate) fn uniform_of(&self, p: &Params) -> wgpu::Buffer {
        use wgpu::util::DeviceExt;
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("particle parameters"),
                contents: &bytemuck::bytes_of(p)[..48],
                usage: wgpu::BufferUsages::UNIFORM,
            })
    }
    pub(crate) fn nothing(&self) -> wgpu::Buffer {
        use wgpu::util::DeviceExt;
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("nothing"),
                contents: &[0u8; 16],
                usage: wgpu::BufferUsages::STORAGE,
            })
    }
    /// The kernels' bind group for a pass of particle work outside a plan's passes.
    pub(crate) fn kernel_group(
        &self,
        uniform: &wgpu::Buffer,
        first: &wgpu::Buffer,
        second: &wgpu::Buffer,
        result: &wgpu::Buffer,
        third: &wgpu::Buffer,
        fourth: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        let entries = [
            (crate::origin::BINDING, &self.whole_frame),
            (0, uniform),
            (1, first),
            (2, second),
            (3, result),
            (4, third),
            (5, &self.ltc),
            (6, &self.default_lighting),
            (7, &self.studio_environment),
            (8, fourth),
        ];
        let entries: Vec<wgpu::BindGroupEntry<'_>> = entries
            .iter()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: *binding,
                resource: buffer.as_entire_binding(),
            })
            .collect();
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particle work"),
            layout: &self.layout,
            entries: &entries,
        })
    }
}
