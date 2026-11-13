//! Headless planar tracking: the Studio analysis on the GPU, published as one Planar track node.
use serde_json::{Value, json};
use std::path::Path;
use veylvik_document::{Command, Composition, NodeId, Time, planar::Quad};
use veylvik_render::{Plan, Renderer};
use veylvik_track::planar::{PlanarTracker, Settings};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct Request {
    pub source: NodeId,
    pub surface: Quad,
    /// Matte whose alpha marks foreground objects to ignore, rendered per frame.
    pub occlusion: Option<NodeId>,
    /// Optional area to analyze instead of the corners: any textured part of the same plane.
    pub region: Option<Quad>,
    /// Composition frames; `start` is the reference frame, `end` may precede it.
    pub frames: [i64; 2],
    pub settings: Settings,
}
impl Request {
    pub fn from_json(args: &Value) -> Result<Self> {
        let frame = |name: &str| -> Result<i64> {
            let value = args[name]
                .as_i64()
                .ok_or(format!("{name} must be a frame number"))?;
            if !(0..=veylvik_document::MAX_FRAME_COUNT as i64).contains(&value) {
                return Err(format!("{name} is outside the composition frame range").into());
            }
            Ok(value)
        };
        let mut settings = Settings::default();
        if let Some(value) = args.get("feature_radius") {
            settings.patch_radius =
                u32::try_from(value.as_u64().ok_or("feature_radius must be an integer")?)?;
        }
        if let Some(value) = args.get("search_radius") {
            settings.search_radius =
                u32::try_from(value.as_u64().ok_or("search_radius must be an integer")?)?;
        }
        if let Some(value) = args.get("tolerance") {
            settings.tolerance_pixels = value.as_f64().ok_or("tolerance must be a number")?;
        }
        if let Some(value) = args.get("area_follows") {
            settings.screen_area = match value.as_str() {
                Some("surface") => false,
                Some("screen") => true,
                _ => return Err("area_follows must be \"surface\" or \"screen\"".into()),
            };
        }
        if let Some(value) = args.get("reanchor") {
            settings.reanchor_below = if value.as_bool().ok_or("reanchor must be boolean")? {
                24
            } else {
                0
            };
        }
        Ok(Self {
            source: args["source"]
                .as_str()
                .ok_or("source must be a node UUID")?
                .parse()?,
            surface: serde_json::from_value(args["surface"].clone()).map_err(|e| {
                format!("surface must be four [x, y] corners in 0..1 coordinates: {e}")
            })?,
            region: args
                .get("region")
                .map(|value| serde_json::from_value(value.clone()))
                .transpose()
                .map_err(|e| format!("region must be four [x, y] corners: {e}"))?,
            occlusion: args
                .get("occlusion")
                .map(|value| -> Result<NodeId> {
                    Ok(value
                        .as_str()
                        .ok_or("occlusion must be a node UUID")?
                        .parse()?)
                })
                .transpose()?,
            frames: [frame("start")?, frame("end")?],
            settings,
        })
    }
}
/// Tracks the request on `doc` and returns the commands that add the track node, plus a report.
pub fn run(
    renderer: &Renderer,
    doc: &Composition,
    base: &Path,
    request: &Request,
) -> Result<(NodeId, Vec<Command>, Value)> {
    veylvik_document::planar::validate_quad(request.surface)?;
    if !doc.nodes.contains_key(&request.source) {
        return Err("tracking source node does not exist".into());
    }
    let last = i64::from(doc.frame_count.saturating_sub(1));
    if request.frames.iter().any(|f| *f > last) {
        return Err(format!("start and end must be within frames 0..={last}").into());
    }
    if request.frames[0] == request.frames[1] {
        return Err("start and end must differ".into());
    }
    if request.frames[0].abs_diff(request.frames[1]) >= 4096 {
        return Err("a tracking range is limited to 4096 frames".into());
    }
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
    let range = tracker.track_range(
        renderer,
        &plan,
        occlusion.as_ref().map(|(plan, inputs)| (plan, inputs)),
        &inputs,
        request.surface,
        request.region,
        request.frames,
        doc.frame_rate,
        request.settings,
        budget,
        &mut |_, _, _| true,
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
        "complete": range.stop == veylvik_track::planar::Stop::Complete,
        "message": range.stop.message(), "reanchored": range.anchors - 1,
        "worst_rms_pixels": worst,
    });
    let (node, commands) = veylvik_document::planar::save(doc, request.source, range.data)?;
    Ok((node, commands, report))
}
