//! The seam where a host-registered renderer draws Render scene nodes, so this crate holds no 3D drawing code.
use crate::{Result, error};
use veylvik_document::{NodeId, Operation};

/// Draws a scene into a frame buffer. Studio and the CLI register one (from
/// `veylvik-render3d`) with `Renderer::set_scene_renderer`.
pub trait SceneRenderer: Send + Sync {
    fn render(&self, job: SceneJob<'_>) -> Result<()>;
    /// Depth maps of the scene's solids as some of its lights see it, for what is lit by
    /// those lights without being a surface of the scene (particles). See [`ShadowJob`].
    fn shadows(&self, _job: ShadowJob<'_>) -> Result<()> {
        Err(error("this 3D renderer makes no shadow maps"))
    }
    /// GPU bytes it keeps between renders (targets, meshes, textures...), which count
    /// against the renderer's budget.
    fn held_bytes(&self) -> u64 {
        0
    }
    /// Let go of everything kept between renders; it is made again when next needed.
    fn release(&self) {}
}
/// Bytes before the first depth map in a [`ShadowJob`]'s target.
pub const SHADOW_HEADER: u64 = 1024;
/// Values for each light in that header: its view-projection's four rows, then 1 when it
/// has a map (0: nothing of the scene is in its way), then three spare.
pub const SHADOW_VALUES: usize = 20;
/// Most lights a [`ShadowJob`] asks for.
pub const SHADOW_LIGHTS: usize = 6;
/// Depth maps for `lights` (indices into the scene's lights, at most [`SHADOW_LIGHTS`]),
/// each `side` x `side` texels of light-space depth 0..1 (1 nearest the far end), written
/// to `target`: from byte 0 [`SHADOW_VALUES`] floats a light, from [`SHADOW_HEADER`] the
/// maps one after another, rows top first. A light that casts nothing has no map.
pub struct ShadowJob<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub scene: &'a veylvik_scene::Scene,
    pub lights: &'a [usize],
    pub side: u32,
    pub target: &'a wgpu::Buffer,
    pub models: &'a std::collections::BTreeMap<String, std::sync::Arc<veylvik_scene::Model>>,
}
/// One Render scene pass. `target` and every image are premultiplied scene-linear
/// Rec.709 `vec4<f32>` rows of `width * height`; commands go on `encoder`.
/// A picture a scene reads: the node it comes from, its pixels and its size.
/// A picture a scene shows: the node it comes from, its pixels, its size, and what it
/// is made of when known (the same number means the same pixels), so the 3D renderer
/// may keep what it made of it.
pub type Picture<'a> = (NodeId, &'a wgpu::Buffer, [u32; 2], Option<u64>);
pub struct SceneJob<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub scene: &'a veylvik_scene::Scene,
    pub camera: veylvik_scene::Camera,
    /// How much nearby surfaces shade the environment (Render scene's Occlusion).
    pub occlusion: f32,
    /// How much of this render goes into `target`: 1 replaces it; motion blur
    /// averages its samples by blending sample k at 1 / (k + 1).
    pub blend: f32,
    /// The frame time, for animated looks.
    pub time: veylvik_document::Time,
    pub width: u32,
    pub height: u32,
    /// Write each pixel's distance from the camera instead of the picture.
    pub depth: bool,
    /// Paint each light's reflection in a colour of its own instead of the picture.
    pub reflections: bool,
    /// Imported models by file path (`Geometry::Model { path }`).
    pub models: &'a std::collections::BTreeMap<String, std::sync::Arc<veylvik_scene::Model>>,
    /// Gaussian splats by file path (`Geometry::Splats { path }`).
    pub splats: &'a std::collections::BTreeMap<String, std::sync::Arc<veylvik_splat::Splats>>,
    /// Image card pictures by their source node (`Geometry::Card { image }`).
    pub images: &'a [Picture<'a>],
    /// The prefiltered studio environment, laid out as `environment_layer` reads it.
    pub environment: &'a wgpu::Buffer,
    /// The area-light LTC table material shading reads (binding 5 of `MATERIAL_WGSL`).
    pub ltc: &'a wgpu::Buffer,
    /// Copies of objects at particles: the objects whose `instanced` is a node here are
    /// drawn `count` times from its buffer (three values a copy: where and size, turn as
    /// a quaternion, tint), and not where they are.
    pub instances: &'a [crate::Instances],
    pub target: &'a wgpu::Buffer,
    /// A second output of the same render: the other of picture and depth, when two
    /// Render scene nodes film the same scene the same way. The bool is its `depth`.
    pub also: Option<(&'a wgpu::Buffer, bool)>,
    /// Path trace the picture instead of drawing it (never for depth or the reflections view).
    pub trace: Option<Trace>,
}
/// A path traced render: paths through each pixel for this moment, bounces after the
/// first surface, and a seed that makes the noise this moment's own and repeatable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trace {
    pub samples: u32,
    pub bounces: u32,
    pub seed: u32,
    /// Denoised: the moments' paths are gathered into one picture, `gathered` of them
    /// already, and smoothed once the `last` moment's are in.
    pub denoise: Option<Gathered>,
}
/// How far a denoised path traced picture has got across its shutter's moments.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gathered {
    pub gathered: u32,
    pub last: bool,
}
/// The Render scene's path tracing for one of `moments` moments across its shutter at
/// `at` (its samples shared between them), if it path traces.
pub(crate) fn tracing(
    plan: &crate::Plan,
    node: usize,
    at: veylvik_document::Time,
    moment: usize,
    moments: usize,
) -> Option<Trace> {
    let Operation::RenderScene(op) = &plan.nodes[node].operation else {
        return None;
    };
    if op.depth || op.quality != veylvik_document::nodes::SceneQuality::PathTraced {
        return None;
    }
    let seed = crate::fnv(
        &[at.ticks as u64, at.rate as u64, moment as u64]
            .map(|v| v.to_le_bytes())
            .concat(),
        0,
    ) as u32;
    let samples = op.samples.clamp(1, 16384).div_ceil(moments.max(1) as u32);
    Some(Trace {
        samples,
        bounces: op.bounces.min(16),
        seed,
        denoise: op.denoise.then_some(Gathered {
            gathered: samples * moment as u32,
            last: moment + 1 >= moments,
        }),
    })
}
/// The scene and camera a Render scene node films at `time`, and its occlusion.
pub(crate) fn evaluate(
    plan: &crate::Plan,
    node: usize,
    time: veylvik_document::Time,
) -> Result<(veylvik_scene::Scene, veylvik_scene::Camera, f32)> {
    let render = &plan.nodes[node];
    let Operation::RenderScene(op) = &render.operation else {
        return Err(error("not a Render scene node"));
    };
    let root = *render
        .inputs
        .get("scene")
        .ok_or_else(|| error("Render scene needs a scene"))?;
    let by_id: std::collections::BTreeMap<NodeId, &veylvik_document::Node> = plan
        .nodes
        .iter()
        .chain(&plan.light_data)
        .map(|n| (n.id, n))
        .collect();
    let lookup = |id: NodeId| by_id.get(&id).copied();
    let scene = veylvik_scene::evaluate_in(
        &veylvik_scene::Graph {
            width: plan.width,
            height: plan.height,
            node: &lookup,
        },
        root,
        time,
    )
    .map_err(error)?;
    let index = op.camera.at(time).map_err(error)?.round().clamp(0.0, 64.0) as usize;
    let camera = scene.camera(index, plan.width, plan.height);
    let occlusion = op.occlusion.at(time).map_err(error)?.clamp(0.0, 1.0);
    Ok((scene, camera, occlusion))
}
/// The scene at `time` connected to `node`'s scene input (a node that films no camera of
/// its own, such as particles lit by the scene's lights).
pub(crate) fn scene_of(
    plan: &crate::Plan,
    node: usize,
    time: veylvik_document::Time,
) -> Result<veylvik_scene::Scene> {
    let root = *plan.nodes[node]
        .inputs
        .get("scene")
        .ok_or_else(|| error("no scene is connected"))?;
    let by_id: std::collections::BTreeMap<NodeId, &veylvik_document::Node> = plan
        .nodes
        .iter()
        .chain(&plan.light_data)
        .map(|n| (n.id, n))
        .collect();
    let lookup = |id: NodeId| by_id.get(&id).copied();
    veylvik_scene::evaluate_in(
        &veylvik_scene::Graph {
            width: plan.width,
            height: plan.height,
            node: &lookup,
        },
        root,
        time,
    )
    .map_err(error)
}
/// What the scene connected to `node` holds at `time`, for keying what is made of it.
pub(crate) fn scene_fingerprint(
    plan: &crate::Plan,
    node: usize,
    time: veylvik_document::Time,
) -> Result<[u32; 2]> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!("{:?}", scene_of(plan, node, time)?).hash(&mut hasher);
    let h = hasher.finish();
    Ok([h as u32, (h >> 32) as u32])
}
/// Whether the Render scene outputs depth instead of its picture.
/// The later Render scene pass that films the same scene in the same way at every
/// moment as `pass` and differs only in giving depth or the picture: one render can
/// give both. They read the same pictures for their cards, so those are at hand now.
pub(crate) fn twin(
    plan: &crate::Plan,
    pass: usize,
    time: veylvik_document::Time,
    visible: &std::collections::BTreeSet<veylvik_document::NodeId>,
) -> Result<Option<usize>> {
    let first = &plan.passes[pass];
    let filmed = |node: usize| -> Result<Vec<String>> {
        sample_times(plan, node, time)?
            .into_iter()
            .map(|at| {
                let (scene, camera, occlusion) = evaluate(plan, node, at)?;
                Ok(format!(
                    "{:?}{scene:?}{camera:?}{occlusion}",
                    (at.ticks, at.rate)
                ))
            })
            .collect()
    };
    let mut mine: Option<Vec<String>> = None;
    for (other, second) in plan.passes.iter().enumerate().skip(pass + 1) {
        if second.kernel != crate::Kernel::RenderScene
            || !visible.contains(&plan.nodes[second.node].id)
            || shows_depth(plan, second.node) == shows_depth(plan, first.node)
            // A path traced picture is made apart from its depth.
            || tracing(plan, first.node, time, 0, 1).is_some()
            || tracing(plan, second.node, time, 0, 1).is_some()
            || second.cards != first.cards
        {
            continue;
        }
        if mine.is_none() {
            mine = Some(filmed(first.node)?);
        }
        if Some(filmed(second.node)?) == mine {
            return Ok(Some(other));
        }
    }
    Ok(None)
}
pub(crate) fn shows_depth(plan: &crate::Plan, node: usize) -> bool {
    matches!(&plan.nodes[node].operation, Operation::RenderScene(op) if op.depth)
}
/// The moments a Render scene films for the frame at `time`: the frame itself, or
/// with motion blur its samples spread evenly over the shutter, centred on it.
pub(crate) fn sample_times(
    plan: &crate::Plan,
    node: usize,
    time: veylvik_document::Time,
) -> Result<Vec<veylvik_document::Time>> {
    let Operation::RenderScene(op) = &plan.nodes[node].operation else {
        return Err(error("not a Render scene node"));
    };
    let angle = op
        .motion_blur
        .at(time)
        .map_err(error)?
        .clamp(0.0, 360.0)
        .round() as i64;
    let rate = plan.frame_rate;
    if angle == 0 || rate.ticks <= 0 {
        return Ok(vec![time]);
    }
    let n = op
        .blur_samples
        .at(time)
        .map_err(error)?
        .clamp(2.0, 32.0)
        .round() as i64;
    // Sample i sits (2i - (n - 1)) / (2 (n - 1)) of the shutter from the frame;
    // a frame lasts rate.rate / rate.ticks seconds.
    let denominator = u32::try_from(2 * (n - 1) * 360 * rate.ticks)
        .map_err(|_| error("frame rate too fine for motion blur"))?;
    (0..n)
        .map(|i| {
            let offset = veylvik_document::Time::new(
                (2 * i - (n - 1)) * angle * i64::from(rate.rate),
                denominator,
            )
            .map_err(error)?;
            time.checked_add(offset).map_err(error)
        })
        .collect()
}
/// A stable fingerprint of what a Render scene shows at a frame, for the frame cache.
pub(crate) fn fingerprint(
    plan: &crate::Plan,
    node: usize,
    time: veylvik_document::Time,
) -> Result<[u32; 2]> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for at in sample_times(plan, node, time)? {
        let (scene, camera, occlusion) = evaluate(plan, node, at)?;
        format!(
            "{scene:?}{camera:?}{occlusion}{}{:?}",
            shows_depth(plan, node),
            tracing(plan, node, at, 0, 1)
        )
        .hash(&mut hasher);
        // Copies at particles are made of the particles' whole making.
        for id in scene.objects.iter().filter_map(|o| o.instanced) {
            if let Some(source) = plan
                .nodes
                .iter()
                .chain(&plan.light_data)
                .find(|n| n.id == id)
                .and_then(|n| n.inputs.get("particles"))
                && let Some(index) = plan.light_data.iter().position(|n| n.id == *source)
            {
                let nodes: Vec<veylvik_document::Node> =
                    plan.nodes.iter().chain(&plan.light_data).cloned().collect();
                crate::simulation::key_of(&plan.light_data[index], &nodes, plan.width, plan.height)
                    .hash(&mut hasher);
            }
        }
    }
    let h = hasher.finish();
    Ok([h as u32, (h >> 32) as u32])
}
