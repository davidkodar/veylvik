//! Headless planar tracking: the Studio analysis on the GPU, published as one Planar track node.
use serde_json::{Value, json};
use std::path::Path;
use veylvik_document::{Command, Composition, NodeId, Time};
use veylvik_render::{Plan, Renderer};
use veylvik_track::planar::PlanarTracker;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub use veylvik_track::request::Request;
/// Tracks the request on `doc` and returns the commands that add the track node, plus a report.
pub fn run(
    renderer: &Renderer,
    doc: &Composition,
    base: &Path,
    request: &Request,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<(NodeId, Vec<Command>, Value)> {
    request.check(doc)?;
    let plan = Plan::compile_output(doc, request.source)?;
    let inputs = crate::assets::load_inputs(&plan, base, renderer.budget_bytes)?;
    let occlusion = request
        .occlusion
        .map(|id| -> Result<_> {
            if !doc.nodes.contains_key(&id) {
                return Err("occlusion node does not exist".into());
            }
            let plan = Plan::compile_output(doc, id)?;
            let inputs = crate::assets::load_inputs(&plan, base, renderer.budget_bytes)?;
            Ok((plan, inputs))
        })
        .transpose()?;
    let budget = veylvik_document::limits::MAX_DOCUMENT_BYTES
        .saturating_sub(doc.encoded_size()?)
        .saturating_sub(64 * 1024);
    let tracker = PlanarTracker::new(renderer);
    let [start, end] = request.frames;
    let ends = match request.back_to {
        Some(back) => [back, start, end],
        None if end > start => [start, start, end],
        None => [end, start, start],
    };
    // The tracker reports each frame as it is done; the span says how many there are.
    let span = (ends[2] - ends[0]).unsigned_abs() as usize + 1;
    let mut seen = 0_usize;
    let range = tracker.track_both(
        renderer,
        &plan,
        occlusion.as_ref().map(|(plan, inputs)| (plan, inputs)),
        &inputs,
        request.surface,
        request.region,
        ends,
        doc.frame_rate,
        request.settings,
        budget,
        &mut |_, _, _| {
            seen += 1;
            progress(seen.min(span), span);
            true
        },
    )?;
    let frame_of = |time: Time| -> Result<i64> {
        Ok((time.seconds()? * doc.frame_rate.seconds()?).round() as i64)
    };
    let samples = &range.data.samples;
    let first = frame_of(samples.first().ok_or("no frames tracked")?.time)?;
    let last = frame_of(samples.last().ok_or("no frames tracked")?.time)?;
    let worst = samples.iter().map(|s| s.rms_pixels).fold(0.0, f64::max);
    let report = json!({
        "frames": samples.len(), "first_frame": first, "last_frame": last,
        "complete": range.stop == veylvik_track::planar::Stop::Complete
            && range.backward.as_ref().is_none_or(|b| *b == veylvik_track::planar::Stop::Complete),
        "message": match &range.backward {
            Some(back) if end > start => format!("forward: {}; backward: {}", range.stop.message(), back.message()),
            Some(back) => back.message(),
            None => range.stop.message(),
        },
        "reanchored": range.anchors - 1,
        "worst_rms_pixels": worst,
        "motion": request.settings.motion,
    });
    let (node, commands) = veylvik_document::planar::save(doc, request.source, range.data)?;
    Ok((node, commands, report))
}
