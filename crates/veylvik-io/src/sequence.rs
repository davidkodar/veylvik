//! Bounded sequence export. Only a completed manifest denotes a complete sequence.
use crate::Result;
use serde_json::{Value, json};
use std::path::Path;
use veylvik_document::{
    Composition, NodeId, Time, atomic_create, atomic_write, sequence::FrameRange,
};
use veylvik_render::{ImageInputs, Plan, Region, RenderRequest, Renderer};
#[derive(Clone, Copy)]
pub struct SequenceRequest<'a> {
    pub frames: FrameRange,
    pub format: crate::FrameFormat,
    pub output: NodeId,
    pub region: Option<Region>,
    /// More layers in each EXR beside the picture: other nodes' pictures (a Render
    /// scene's depth or render passes), each named as it is in the file.
    pub layers: &'a [ExportLayer],
}
/// A layer written beside the picture in a layered EXR: a node's picture under a name.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportLayer {
    pub name: String,
    pub node: NodeId,
}
/// The most layers one EXR takes.
pub const MOST_LAYERS: usize = 16;
/// Compile and load sources once, then render one frame at a time into a new directory.
/// `progress(completed, total)` may return false to cancel between frames. Completed
/// frames remain usable on cancellation/failure. Existing directories are rejected.
pub fn render_sequence(
    renderer: &Renderer,
    document: &Composition,
    base: &Path,
    directory: &Path,
    request: SequenceRequest<'_>,
    progress: impl FnMut(u32, u32) -> bool,
) -> Result<Value> {
    export(
        renderer,
        document,
        directory,
        request,
        |plan| crate::load_inputs(plan, base, renderer.budget_bytes),
        progress,
    )
}
/// Render from the host's immutable source snapshot without reopening image files.
pub fn render_sequence_with_inputs(
    renderer: &Renderer,
    document: &Composition,
    inputs: &ImageInputs,
    directory: &Path,
    request: SequenceRequest<'_>,
    progress: impl FnMut(u32, u32) -> bool,
) -> Result<Value> {
    export(
        renderer,
        document,
        directory,
        request,
        |plan| {
            for asset in plan.required_assets() {
                if !inputs.contains_key(&asset) {
                    return Err(format!("missing image input: {asset}").into());
                }
            }
            Ok(inputs.clone())
        },
        progress,
    )
}
fn export(
    renderer: &Renderer,
    document: &Composition,
    directory: &Path,
    request: SequenceRequest<'_>,
    load: impl Fn(&Plan) -> Result<ImageInputs>,
    mut progress: impl FnMut(u32, u32) -> bool,
) -> Result<Value> {
    let total = request.frames.count()?;
    document.validate()?;
    let region = request.region.unwrap_or(Region {
        x: 0,
        y: 0,
        width: document.width,
        height: document.height,
    });
    region.validate(document.width, document.height)?;
    let plan = Plan::compile_output(document, request.output)?;
    let inputs = load(&plan)?;
    if !request.layers.is_empty() && request.format != crate::FrameFormat::Exr {
        return Err("only EXR files hold layers beside the picture".into());
    }
    if request.layers.len() > MOST_LAYERS {
        return Err(format!("an EXR takes at most {MOST_LAYERS} layers").into());
    }
    // Each layer's own plan and what it reads.
    let mut layers = Vec::with_capacity(request.layers.len());
    for layer in request.layers {
        let label = document
            .nodes
            .get(&layer.node)
            .map_or_else(|| layer.node.to_string(), |n| n.label.clone());
        let plan = Plan::compile_output(document, layer.node)
            .map_err(|e| format!("the {} layer ({label}): {e}", layer.name))?;
        let inputs = load(&plan)?;
        layers.push((layer, plan, inputs));
    }
    // Reserving a new directory ensures neither originals nor earlier renders
    // become export targets. Per-file publication also refuses collisions.
    std::fs::create_dir(directory)?;
    let manifest_path = directory.join("sequence.json");
    let mut manifest = json!({
        "schema":1, "status":"rendering", "format":request.format, "color_space":if request.format == crate::FrameFormat::Png { "srgb" } else { "linear_rec709" },
        "alpha":if request.format == crate::FrameFormat::Png { "straight" } else { "premultiplied" }, "frames":request.frames, "count":total, "completed":0,
        "file_pattern":format!("frame-%06d.{}",request.format.extension()), "file_start":0,
        "output_node":request.output, "region":region
    });
    atomic_create(&manifest_path, &serde_json::to_vec_pretty(&manifest)?)?;
    let mut completed = 0;
    while completed < total && progress(completed, total) {
        let frame_index = i64::try_from(request.frames.start as i128 + completed as i128)?;
        let time = Time::at_frame(frame_index, request.frames.frame_rate)?;
        let frame = renderer.render_gpu_export(
            &plan,
            RenderRequest {
                time,
                region: Some(region),
                scene_view: None,
                scene_reflections: false,
            },
            &inputs,
        )?;
        let frame = renderer.read_frame(&frame)?;
        let path = directory.join(format!(
            "frame-{completed:06}.{}",
            request.format.extension()
        ));
        let pixels = crate::export::sdr_pixels(&path, &frame.pixels, document.output_display());
        if layers.is_empty() {
            crate::export::save_frame_new(&path, region.width, region.height, &pixels)?;
        } else {
            let mut made = Vec::with_capacity(layers.len());
            for (layer, plan, inputs) in &layers {
                let picture = renderer.render_gpu_export(
                    plan,
                    RenderRequest {
                        time,
                        region: Some(region),
                        scene_view: None,
                        scene_reflections: false,
                    },
                    inputs,
                )?;
                made.push((
                    *layer,
                    layer_kind(plan, time)?,
                    renderer.read_frame(&picture)?,
                ));
            }
            let written: Vec<crate::export::ExrLayer<'_>> = made
                .iter()
                .map(|(layer, kind, frame)| crate::export::ExrLayer {
                    name: &layer.name,
                    kind: kind.clone(),
                    pixels: &frame.pixels,
                })
                .collect();
            crate::export::save_layered_exr(&path, region.width, region.height, &pixels, &written)?;
        }
        completed += 1;
    }
    manifest["completed"] = json!(completed);
    manifest["status"] = json!(if completed == total {
        "complete"
    } else {
        "cancelled"
    });
    atomic_write(&manifest_path, &serde_json::to_vec_pretty(&manifest)?)?;
    if completed == total {
        progress(completed, total);
    }
    Ok(json!({"directory":directory,"manifest":manifest}))
}

/// How a layer's picture is named in the file, by what its node outputs.
fn layer_kind(plan: &Plan, time: Time) -> Result<crate::export::LayerKind> {
    use crate::export::LayerKind;
    use veylvik_render::ScenePass;
    Ok(match veylvik_render::output_pass(plan) {
        Some(ScenePass::Depth) => LayerKind::Depth,
        Some(ScenePass::Normals | ScenePass::Position) => LayerKind::Vector,
        Some(ScenePass::Motion) => LayerKind::Motion,
        Some(pass @ (ScenePass::Objects | ScenePass::ObjectsMore)) => LayerKind::Objects {
            rank: u32::from(pass == ScenePass::ObjectsMore),
            names: veylvik_render::filmed_objects(plan, time)?,
        },
        Some(ScenePass::Picture) | None => LayerKind::Picture,
    })
}
