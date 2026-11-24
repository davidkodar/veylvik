//! MCP arguments for planar tracking, parsed identically by the headless and live servers.
use crate::planar::Settings;
use serde_json::Value;
use veylvik_document::{NodeId, planar::Quad};

pub struct Request {
    pub source: NodeId,
    pub surface: Quad,
    /// Matte whose alpha marks foreground objects to ignore, rendered per frame.
    pub occlusion: Option<NodeId>,
    /// Optional area to analyze instead of the corners: any textured part of the same plane.
    pub region: Option<Quad>,
    /// Composition frames; `start` is the reference frame, `end` may precede it.
    pub frames: [i64; 2],
    /// Also track backward from `start` to this earlier frame, joined at the reference.
    pub back_to: Option<i64>,
    pub settings: Settings,
}
impl Request {
    /// Parse MCP arguments; headless and live Studio servers share this exactly.
    pub fn from_json(args: &Value) -> Result<Self, String> {
        let frame = |name: &str| -> Result<i64, String> {
            let value = args[name]
                .as_i64()
                .ok_or(format!("{name} must be a frame number"))?;
            if !(0..=veylvik_document::MAX_FRAME_COUNT as i64).contains(&value) {
                return Err(format!("{name} is outside the composition frame range"));
            }
            Ok(value)
        };
        let mut settings = Settings::default();
        if let Some(value) = args.get("feature_radius") {
            settings.patch_radius =
                u32::try_from(value.as_u64().ok_or("feature_radius must be an integer")?)
                    .map_err(|e| e.to_string())?;
        }
        if let Some(value) = args.get("search_radius") {
            settings.search_radius =
                u32::try_from(value.as_u64().ok_or("search_radius must be an integer")?)
                    .map_err(|e| e.to_string())?;
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
        if let Some(value) = args.get("motion") {
            settings.motion = serde_json::from_value(value.clone()).map_err(
                |_| "motion must be auto, perspective, affine, similarity or translation",
            )?;
        }
        if let Some(value) = args.get("refine") {
            settings.refine = value.as_bool().ok_or("refine must be boolean")?;
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
                .parse()
                .map_err(|e| format!("source: {e}"))?,
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
                .map(|value| -> Result<NodeId, String> {
                    value
                        .as_str()
                        .ok_or("occlusion must be a node UUID")?
                        .parse()
                        .map_err(|e| format!("occlusion: {e}"))
                })
                .transpose()?,
            frames: [frame("start")?, frame("end")?],
            back_to: args.get("back_to").map(|_| frame("back_to")).transpose()?,
            settings,
        })
    }
}

impl Request {
    /// The tool's arguments as a JSON schema's `properties` (a server adds its own
    /// `revision`, and `session` when live).
    pub fn schema() -> serde_json::Value {
        serde_json::json!({"source":{"type":"string","format":"uuid"},"surface":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number"}}},"region":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number"}}},"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0},"feature_radius":{"type":"integer","minimum":3,"maximum":16},"search_radius":{"type":"integer","minimum":1,"maximum":48},"tolerance":{"type":"number","minimum":0.1,"maximum":32},"reanchor":{"type":"boolean"},"area_follows":{"enum":["surface","screen"],"default":"surface"},"occlusion":{"type":"string","format":"uuid"},"motion":{"enum":["auto","perspective","affine","similarity","translation"],"default":"auto"},"refine":{"type":"boolean","default":true},"back_to":{"type":"integer","minimum":0}})
    }
    /// The arguments a call must give, besides the server's own.
    pub const REQUIRED: &'static [&'static str] = &["source", "surface", "start", "end"];
    /// Whether the request fits `doc`: the source exists, the frames are inside it and
    /// differ, tracking both ways runs back then forward, and the range is at most 4096
    /// frames.
    pub fn check(&self, doc: &veylvik_document::Composition) -> Result<(), String> {
        veylvik_document::planar::validate_quad(self.surface).map_err(|e| e.to_string())?;
        if !doc.nodes.contains_key(&self.source) {
            return Err("tracking source node does not exist".into());
        }
        let last = i64::from(doc.frame_count.saturating_sub(1));
        let [start, end] = self.frames;
        if start > last || end > last {
            return Err(format!("start and end must be within frames 0..={last}"));
        }
        if start == end {
            return Err("start and end must differ".into());
        }
        if let Some(back) = self.back_to
            && (back >= start || end < start || back > last)
        {
            return Err(
                "back_to must precede start, and end must follow start, when tracking both ways"
                    .into(),
            );
        }
        let first = self.back_to.unwrap_or(start.min(end));
        if start.max(end) - first >= 4096 {
            return Err("a tracking range is limited to 4096 frames".into());
        }
        Ok(())
    }
}
