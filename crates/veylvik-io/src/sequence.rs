//! Bounded sequence export. Only a completed manifest denotes a complete sequence.
use crate::Result;
use serde_json::{Value, json};
use std::path::Path;
use veylvik_document::{
    Composition, NodeId, Time, atomic_create, atomic_write, sequence::FrameRange,
};
use veylvik_render::{ImageInputs, Plan, Region, RenderRequest, Renderer};
#[derive(Clone, Copy)]
pub struct SequenceRequest {
    pub frames: FrameRange,
    pub format: crate::FrameFormat,
    pub output: NodeId,
    pub region: Option<Region>,
}
/// Compile and load sources once, then render one frame at a time into a new directory.
/// `progress(completed, total)` may return false to cancel between frames. Completed
/// frames remain usable on cancellation/failure. Existing directories are rejected.
pub fn render_sequence(
    renderer: &Renderer,
    document: &Composition,
    base: &Path,
    directory: &Path,
    request: SequenceRequest,
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
    request: SequenceRequest,
    progress: impl FnMut(u32, u32) -> bool,
) -> Result<Value> {
    export(
        renderer,
        document,
        directory,
        request,
        |plan| {
            for asset in plan.required_assets() {
                if !inputs.contains_key(asset) {
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
    request: SequenceRequest,
    load: impl FnOnce(&Plan) -> Result<ImageInputs>,
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
        let frame = renderer.render_gpu_cached(
            &plan,
            RenderRequest {
                time,
                region: Some(region),
            },
            &inputs,
        )?;
        let frame = renderer.read_frame(&frame)?;
        let path = directory.join(format!(
            "frame-{completed:06}.{}",
            request.format.extension()
        ));
        let pixels = crate::export::sdr_pixels(&path, &frame.pixels, document.display);
        crate::export::save_frame_new(&path, region.width, region.height, &pixels)?;
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
