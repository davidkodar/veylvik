//! Editable vector geometry, independent of a rasterizer or font backend.
use crate::{Result, fail};
use serde::{Deserialize, Serialize};

pub const MAX_PATH_COMMANDS: usize = 4096;
pub const MAX_PATH_COORDINATE: f32 = 32768.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FillRule {
    #[default]
    NonZero,
    EvenOdd,
}

/// Coordinates use composition pixels, with x rightwards and y downwards.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum PathCommand {
    Move {
        to: [f32; 2],
    },
    Line {
        to: [f32; 2],
    },
    Quad {
        control: [f32; 2],
        to: [f32; 2],
    },
    Cubic {
        control1: [f32; 2],
        control2: [f32; 2],
        to: [f32; 2],
    },
    Close {},
}

/// Closed contours form one fill, so holes follow the selected winding rule.
/// Open contours are rejected rather than silently closed during rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathMask {
    pub fill_rule: FillRule,
    pub commands: Vec<PathCommand>,
}
impl PathMask {
    pub fn validate(&self) -> Result<()> {
        if self.commands.is_empty() || self.commands.len() > MAX_PATH_COMMANDS {
            return fail("path must contain 1..4096 commands");
        }
        let mut open = false;
        let mut segments = 0;
        for command in &self.commands {
            let points: &[[f32; 2]] = match command {
                PathCommand::Move { to } | PathCommand::Line { to } => std::slice::from_ref(to),
                PathCommand::Quad { control, to } => &[*control, *to],
                PathCommand::Cubic {
                    control1,
                    control2,
                    to,
                } => &[*control1, *control2, *to],
                PathCommand::Close {} => &[],
            };
            if points
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > MAX_PATH_COORDINATE)
            {
                return fail("path coordinates must be finite and within ±32768 pixels");
            }
            match command {
                PathCommand::Move { .. } => {
                    if open {
                        return fail("close the current contour before moving");
                    }
                    open = true;
                    segments = 0;
                }
                PathCommand::Close {} => {
                    if !open || segments == 0 {
                        return fail("close requires a contour with segments");
                    }
                    open = false;
                }
                _ => {
                    if !open {
                        return fail("path segment requires a move command");
                    }
                    segments += 1;
                }
            }
        }
        if open {
            return fail("path contour must end with close");
        }
        Ok(())
    }
}

impl Default for PathMask {
    fn default() -> Self {
        Self {
            fill_rule: FillRule::NonZero,
            commands: vec![
                PathCommand::Move { to: [100.0, 100.0] },
                PathCommand::Line { to: [400.0, 100.0] },
                PathCommand::Line { to: [400.0, 300.0] },
                PathCommand::Line { to: [100.0, 300.0] },
                PathCommand::Close {},
            ],
        }
    }
}
/// Strict schema shared by graph authoring and MCP clients.
pub fn schema() -> serde_json::Value {
    use serde_json::json;
    let point = json!({"type":"array","minItems":2,"maxItems":2,"items":{"type":"number","minimum":-MAX_PATH_COORDINATE,"maximum":MAX_PATH_COORDINATE}});
    let commands: Vec<_> = [("move",vec!["to"]),("line",vec!["to"]),("quad",vec!["control","to"]),("cubic",vec!["control1","control2","to"]),("close",vec![])].into_iter().map(|(kind,fields)| {
        let mut properties = json!({"command":{"const":kind}});
        let mut required = vec!["command"];
        for field in fields { properties[field] = point.clone(); required.push(field); }
        json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
    }).collect();
    json!({"type":"object","properties":{"fill_rule":{"enum":["non_zero","even_odd"]},"commands":{"type":"array","minItems":1,"maxItems":MAX_PATH_COMMANDS,"items":{"oneOf":commands}}},"required":["fill_rule","commands"],"additionalProperties":false})
}
