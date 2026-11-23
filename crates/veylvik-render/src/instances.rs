//! Copies of 3D objects at particles: for each Particle instances node in a scene, where its particles are at a moment, worked out on the card for the 3D renderer to draw in one go.
use crate::nodes::{self, RenderNode};
use crate::plan::Plan;
use crate::{Kernel, Params, Renderer, Result, error};
use veylvik_document::nodes::INSTANCES_MOST;
use veylvik_document::{Node, NodeId, Operation, Time};

/// The copies of one Particle instances node: three values each (where and size, turn,
/// tint) and how many.
pub struct Instances {
    pub node: NodeId,
    pub buffer: wgpu::Buffer,
    pub count: u32,
}
impl Renderer {
    /// The copies for every Particle instances node `scene` draws, at `time`.
    pub(crate) fn instances_for(
        &self,
        plan: &Plan,
        scene: &veylvik_scene::Scene,
        time: Time,
        inputs: Option<&crate::ImageInputs>,
    ) -> Result<Vec<Instances>> {
        let mut wanted: Vec<NodeId> = scene.objects.iter().filter_map(|o| o.instanced).collect();
        wanted.sort();
        wanted.dedup();
        if wanted.is_empty() {
            return Ok(Vec::new());
        }
        let nodes: Vec<Node> = plan.nodes.iter().chain(&plan.light_data).cloned().collect();
        wanted
            .into_iter()
            .map(|id| self.instances(&nodes, id, plan.width, plan.height, time, inputs))
            .collect()
    }
    fn instances(
        &self,
        nodes: &[Node],
        id: NodeId,
        width: u32,
        height: u32,
        time: Time,
        inputs: Option<&crate::ImageInputs>,
    ) -> Result<Instances> {
        let by_id = |id: NodeId| nodes.iter().find(|n| n.id == id);
        let copier = by_id(id).ok_or_else(|| error("a Particle instances node is missing"))?;
        let Operation::ParticleInstances(copies) = &copier.operation else {
            return Err(error("not a Particle instances node"));
        };
        let me = copier
            .inputs
            .get("particles")
            .and_then(|id| by_id(*id))
            .ok_or_else(|| {
                error(format!(
                    "{}: connect a Particles node to particles",
                    copier.label
                ))
            })?;
        let Operation::Particles(particles) = &me.operation else {
            return Err(error(format!(
                "{}: the particles input takes a Particles node",
                copier.label
            )));
        };
        if let Some(port) = ["emitter", "target", "field", "obstacle"]
            .into_iter()
            .find(|port| me.inputs.contains_key(*port))
        {
            return Err(error(format!(
                "{}: particles copied as 3D objects cannot use the {port} input yet",
                copier.label
            )));
        }
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
        p.size_op[0] = width;
        p.size_op[1] = height;
        particles.params(&cx, &|_| Ok(0.0), &mut p)?;
        // Only the particles themselves are copied, not their bursts.
        let at = nodes::particles::ROWS * nodes::particles::COUNT + 32;
        let parents = data.pixels()[at / 3][at % 3];
        let count = (parents.max(0.0) as u32)
            .min(INSTANCES_MOST)
            .min(p.values[0] as u32);
        p.values[0] = count as f32;
        p.values[2] = copies.scale.at(time).map_err(error)?;
        p.values[3] = f32::from(u8::from(copies.tumble));
        let state = if nodes::particles::simulating(particles) {
            let key = crate::simulation::key_of(me, nodes, width, height);
            {
                let bake = inputs
                    .and_then(|i| i.flock(&particles.bake))
                    .map(|b| b.as_ref());
                match self.read_bake(bake, key, time, width, height)? {
                    Some((read, _)) => Some(read),
                    None => Some(self.simulate_node(me, nodes, key, width, height, time)?.0),
                }
            }
        } else {
            None
        };
        let none = self.nothing();
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particle copies"),
            size: u64::from(count.max(1)) * 48,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let settings = self.settings_buffer(width, height, &data);
        let uniform = self.uniform_of(&p);
        let group = self.kernel_group(
            &uniform,
            state.as_ref().unwrap_or(&none),
            &none,
            &buffer,
            &settings,
            &none,
        );
        let row = (p.values[1] as u32).max(1);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let pipeline =
                self.pipeline_for(Kernel::ParticleInstances, nodes::particles::features_of(me))?;
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(row.div_ceil(64), count.div_ceil(row).max(1), 1);
        }
        self.queue.submit([encoder.finish()]);
        Ok(Instances {
            node: id,
            buffer,
            count,
        })
    }
}
