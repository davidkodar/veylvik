//! The seam where a host-registered renderer draws Render scene nodes, so this crate holds no 3D drawing code.
use crate::{Result, error};
use veylvik_document::{NodeId, Operation};

/// Draws a scene into a frame buffer. Studio and the CLI register one (from
/// `veylvik-render3d`) with `Renderer::set_scene_renderer`.
pub trait SceneRenderer: Send + Sync {
    fn render(&self, job: SceneJob<'_>) -> Result<()>;
}
/// One Render scene pass. `target` and every image are premultiplied scene-linear
/// Rec.709 `vec4<f32>` rows of `width * height`; commands go on `encoder`.
pub struct SceneJob<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub scene: &'a veylvik_scene::Scene,
    pub camera: veylvik_scene::Camera,
    /// The frame time, for animated looks.
    pub time: veylvik_document::Time,
    pub width: u32,
    pub height: u32,
    /// Image card pictures by their source node (`Geometry::Card { image }`).
    pub images: &'a [(NodeId, &'a wgpu::Buffer)],
    /// The prefiltered studio environment, laid out as `environment_layer` reads it.
    pub environment: &'a wgpu::Buffer,
    /// The area-light LTC table material shading reads (binding 5 of `MATERIAL_WGSL`).
    pub ltc: &'a wgpu::Buffer,
    pub target: &'a wgpu::Buffer,
}
/// The scene and camera a Render scene node films at `time`.
pub(crate) fn evaluate(
    plan: &crate::Plan,
    node: usize,
    time: veylvik_document::Time,
) -> Result<(veylvik_scene::Scene, veylvik_scene::Camera)> {
    let render = &plan.nodes[node];
    let Operation::RenderScene(op) = &render.operation else {
        return Err(error("not a Render scene node"));
    };
    let root = *render
        .inputs
        .get("scene")
        .ok_or_else(|| error("Render scene needs a scene"))?;
    let by_id: std::collections::BTreeMap<NodeId, &veylvik_document::Node> =
        plan.nodes.iter().map(|n| (n.id, n)).collect();
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
    Ok((scene, camera))
}
/// A stable fingerprint of what a Render scene shows at a frame, for the frame cache.
pub(crate) fn fingerprint(
    plan: &crate::Plan,
    node: usize,
    time: veylvik_document::Time,
) -> Result<[u32; 2]> {
    use std::hash::{Hash, Hasher};
    let (scene, camera) = evaluate(plan, node, time)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!("{scene:?}{camera:?}").hash(&mut hasher);
    let h = hasher.finish();
    Ok([h as u32, (h >> 32) as u32])
}
