//! MCP arguments for subject segmentation, parsed identically by the headless and live servers.
use crate::{Point, analysis};
use serde_json::Value;
use veylvik_document::NodeId;

pub struct Request {
    pub source: NodeId,
    pub analysis: analysis::Request,
    /// Edge matting: `None` uses the matting model when it is installed and switched on.
    pub matting: Option<bool>,
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
            },
            matting: args
                .get("matting")
                .map(|v| v.as_bool().ok_or("matting must be boolean"))
                .transpose()?,
            model: args
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("edgetam")
                .to_owned(),
        })
    }
}
