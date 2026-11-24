//! MCP arguments for subject segmentation, parsed identically by the headless and live servers.
use crate::{Point, analysis};
use serde_json::Value;
use veylvik_document::NodeId;

pub struct Request {
    pub source: NodeId,
    pub analysis: analysis::Request,
    /// Edge matting: `None` uses the matting model when it is installed and switched on.
    pub matting: Option<bool>,
    /// Which matting model: `Some(true)` the fine one, `Some(false)` the standard one,
    /// `None` the fine one when it is installed and switched on.
    pub fine: Option<bool>,
    pub model: String,
}
impl Request {
    /// `end` defaults to the composition's last frame, so a prompt tracks through the shot.
    /// Parse MCP arguments; headless and live Studio servers share this exactly.
    pub fn from_json(args: &Value, frame_count: u32) -> Result<Self, String> {
        let frame = |name: &str| -> Result<i64, String> {
            args[name]
                .as_i64()
                .ok_or_else(|| format!("{name} must be a frame number"))
        };
        let points: Vec<Point> = serde_json::from_value(args["points"].clone()).map_err(|e| {
            format!("points must be [{{\"x\":0..1,\"y\":0..1,\"positive\":true}}, ...]: {e}")
        })?;
        Ok(Self {
            source: args["source"]
                .as_str()
                .ok_or("source must be a node UUID")?
                .parse()
                .map_err(|e| format!("source: {e}"))?,
            analysis: analysis::Request {
                points,
                frames: [
                    frame("frame")?,
                    match args.get("end") {
                        Some(_) => frame("end")?,
                        None => i64::from(frame_count.max(1) - 1),
                    },
                ],
                back_to: args.get("back_to").map(|_| frame("back_to")).transpose()?,
                detail: match args.get("detail") {
                    None => true,
                    Some(v) => v.as_bool().ok_or("detail must be boolean")?,
                },
                matting: None,
                steady: match args.get("steady") {
                    None => true,
                    Some(v) => v.as_bool().ok_or("steady must be boolean")?,
                },
            },
            matting: args
                .get("matting")
                .map(|v| v.as_bool().ok_or("matting must be boolean"))
                .transpose()?,
            fine: match args.get("edges").map(Value::as_str) {
                None => None,
                Some(Some("fine")) => Some(true),
                Some(Some("standard")) => Some(false),
                Some(_) => return Err("edges must be standard or fine".into()),
            },
            model: args
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("edgetam")
                .to_owned(),
        })
    }
}

impl Request {
    /// The tool's arguments as a JSON schema's `properties` (a server adds its own
    /// `revision`, and `session` when live).
    pub fn schema() -> serde_json::Value {
        serde_json::json!({"source":{"type":"string","format":"uuid"},"points":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"object","additionalProperties":false,"properties":{"x":{"type":"number","minimum":0,"maximum":1},"y":{"type":"number","minimum":0,"maximum":1},"positive":{"type":"boolean"}},"required":["x","y","positive"]}},"frame":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0},"back_to":{"type":"integer","minimum":0},"model":{"enum":["edgetam","sam2.1-base-plus","sam2.1-large"],"default":"edgetam"},"detail":{"type":"boolean","default":true},"matting":{"type":"boolean"},"steady":{"type":"boolean","default":true,"description":"steady the mattes over time afterwards (each frame against its neighbours, lined up along the footage's motion), so edges do not flicker"},"edges":{"type":"string","enum":["standard","fine"],"description":"the matting model for hair and soft edges: fine is the larger model at full size in tiles (curls and single strands, about three times the matting time); default is fine when it is installed"}})
    }
    /// The arguments a call must give, besides the server's own.
    pub const REQUIRED: &'static [&'static str] = &["source", "points", "frame"];
    /// Whether the request fits `doc`: the source exists, the frames are inside it, and
    /// tracking back runs to an earlier frame.
    pub fn check(&self, doc: &veylvik_document::Composition) -> Result<(), String> {
        if !doc.nodes.contains_key(&self.source) {
            return Err("segmentation source node does not exist".into());
        }
        let last = i64::from(doc.frame_count.saturating_sub(1));
        let frames = self.analysis.frames;
        if frames
            .iter()
            .chain(&self.analysis.back_to)
            .any(|f| !(0..=last).contains(f))
        {
            return Err(format!("frame, end and back_to must lie within 0..={last}"));
        }
        if self.analysis.back_to.is_some_and(|back| back >= frames[0]) {
            return Err("back_to must precede the prompted frame".into());
        }
        Ok(())
    }
}
