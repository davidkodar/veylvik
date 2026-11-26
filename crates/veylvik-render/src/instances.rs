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
/// Particles worked out from their birth alone (not a simulated flock).
fn state_less(particles: &veylvik_document::nodes::Particles) -> bool {
    !nodes::particles::simulating(particles)
}
impl Renderer {
    /// The copies for every Particle instances node `scene` draws, at `time`.
    pub(crate) fn instances_for(
        &self,
        plan: &Plan,
        scene: &veylvik_scene::Scene,
        time: Time,
        inputs: Option<&crate::ImageInputs>,
        pictures: &[crate::scene_hook::Picture<'_>],
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
            .map(|id| self.instances(&nodes, id, plan.width, plan.height, time, inputs, pictures))
            .collect()
    }
    #[allow(clippy::too_many_arguments)]
    fn instances(
        &self,
        nodes: &[Node],
        id: NodeId,
        width: u32,
        height: u32,
        time: Time,
        inputs: Option<&crate::ImageInputs>,
        pictures: &[crate::scene_hook::Picture<'_>],
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
        // The pictures the particles read (made at the frame's size for the scene render;
        // see crate::plan::INSTANCE_PICTURES), by port.
        let picture = |port: &str| -> Result<Option<&wgpu::Buffer>> {
            let Some(source) = me.inputs.get(port) else {
                return Ok(None);
            };
            match pictures.iter().find(|p| p.0 == *source) {
                Some(p) if p.2 == [width, height] => Ok(Some(p.1)),
                Some(_) => Err(error(format!(
                    "{}: the particles' {port} must be a picture of the frame's size (not an Own size picture) to be copied as 3D objects",
                    copier.label
                ))),
                None => Err(error(format!(
                    "{}: the particles' {port} picture was not made",
                    copier.label
                ))),
            }
        };
        let (emitter, target, field, obstacle) = (
            picture("emitter")?,
            picture("target")?,
            picture("field")?,
            picture("obstacle")?,
        );
        let cx = nodes::Cx {
            node: me,
            nodes,
            width,
            height,
            time,
            models: inputs.map(|i| i.models()),
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
        let mut count = (parents.max(0.0) as u32)
            .min(INSTANCES_MOST)
            .min(p.values[0] as u32);
        // Born on an emitter, threads go cell by cell over the picture: cells large enough
        // that the whole picture fits in the copies there may be (the chance of a birth
        // grows with the cell, so the density holds).
        if p.extra[0] > 0.5 && state_less(particles) {
            let slots = p.values[3].max(1.0);
            let most = INSTANCES_MOST as f32;
            let mut stride = p.values[2].max(1.0);
            let threads = |stride: f32| {
                (width as f32 / stride).ceil() * (height as f32 / stride).ceil() * slots
            };
            while threads(stride) > most {
                stride += 1.0;
            }
            p.values[2] = stride;
            p.values[0] = threads(stride);
            p.values[1] = p.values[0].min(nodes::particles::ROW as f32);
            count = p.values[0] as u32;
        }
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
                    None => Some(
                        self.simulate_node(me, nodes, key, width, height, time, emitter)?
                            .0,
                    ),
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
        // Copies are born on the emitter as it is at the frame (a moving emitter's history is
        // for the particles' own picture); a flock reads it in its simulation instead.
        if p.extra[0] > 1.0 {
            p.extra[0] = 1.0;
        }
        let uniform = self.uniform_of(&p);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        // With a target, a field or an obstacle the settings are read as the particles' own
        // picture reads them: through the prepare (the target's tile sums) and fields (the
        // running table, the field's slope, the obstacle) passes; else as they are.
        let settings = if target.is_some() || field.is_some() || obstacle.is_some() {
            use wgpu::util::DeviceExt;
            let frame = u64::from(width) * u64::from(height) * 16;
            let work = |label| {
                self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size: frame,
                    usage: wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                })
            };
            let (prepared, fields) = (work("copies' target tiles"), work("copies' fields"));
            let texels: Vec<[f32; 4]> = data.pixels().to_vec();
            let data_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("copies' settings"),
                    contents: bytemuck::cast_slice(&texels),
                    usage: wgpu::BufferUsages::STORAGE,
                });
            let features = nodes::particles::features_of(me);
            for (kernel, inputs, out) in [
                (
                    Kernel::ParticlePrepare,
                    [target.unwrap_or(&none), &none, &none, &none],
                    &prepared,
                ),
                (
                    Kernel::ParticleFields,
                    [
                        field.unwrap_or(&none),
                        obstacle.unwrap_or(&none),
                        &prepared,
                        &data_buffer,
                    ],
                    &fields,
                ),
            ] {
                let pipeline = self.pipeline_for(kernel, features)?;
                let group =
                    self.kernel_group(&uniform, inputs[0], inputs[1], out, inputs[2], inputs[3]);
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
            }
            fields
        } else {
            self.settings_buffer(width, height, &data)
        };
        let first = match (&state, emitter) {
            (Some(state), _) => state,
            (None, Some(emitter)) => emitter,
            (None, None) => &none,
        };
        let group = self.kernel_group(
            &uniform,
            first,
            target.unwrap_or(&none),
            &buffer,
            &settings,
            &none,
        );
        let row = (p.values[1] as u32).max(1);
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
