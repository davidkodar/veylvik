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
    /// Imported models by file path (`Geometry::Model { path }`).
    pub models: &'a std::collections::BTreeMap<String, std::sync::Arc<veylvik_scene::Model>>,
    /// Image card pictures by their source node (`Geometry::Card { image }`).
    pub images: &'a [(NodeId, &'a wgpu::Buffer)],
    /// The prefiltered studio environment, laid out as `environment_layer` reads it.
    pub environment: &'a wgpu::Buffer,
    /// The area-light LTC table material shading reads (binding 5 of `MATERIAL_WGSL`).
    pub ltc: &'a wgpu::Buffer,
    pub target: &'a wgpu::Buffer,
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
    let occlusion = op.occlusion.at(time).map_err(error)?.clamp(0.0, 1.0);
    Ok((scene, camera, occlusion))
}
/// Whether the Render scene outputs depth instead of its picture.
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
        format!("{scene:?}{camera:?}{occlusion}{}", shows_depth(plan, node)).hash(&mut hasher);
    }
    let h = hasher.finish();
    Ok([h as u32, (h >> 32) as u32])
}
