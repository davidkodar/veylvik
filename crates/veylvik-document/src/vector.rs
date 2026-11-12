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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<ShapeKey>,
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
        validate_keys(self)?;
        Ok(())
    }
}

impl Default for PathMask {
    fn default() -> Self {
        Self {
            keys: vec![],
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
    let command_list = json!({"type":"array","minItems":1,"maxItems":MAX_PATH_COMMANDS,"items":{"oneOf":commands}});
    json!({"type":"object","properties":{"fill_rule":{"enum":["non_zero","even_odd"]},"commands":command_list,"keys":{"type":"array","maxItems":MAX_SHAPE_KEYS,"items":{"type":"object","properties":{"time":{"type":"object","properties":{"ticks":{"type":"integer","minimum":0},"rate":{"type":"integer","minimum":1}},"required":["ticks","rate"],"additionalProperties":false},"commands":command_list},"required":["time","commands"],"additionalProperties":false}}},"required":["fill_rule","commands"],"additionalProperties":false})
}

pub const MAX_SHAPE_KEYS: usize = 256;
const MAX_ANIMATED_COMMANDS: usize = 65_536;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShapeKey {
    pub time: crate::Time,
    pub commands: Vec<PathCommand>,
}
impl PartialEq for ShapeKey {
    fn eq(&self, other: &Self) -> bool {
        self.time.compare(other.time).is_ok_and(|o| o.is_eq()) && self.commands == other.commands
    }
}
fn same_topology(a: &[PathCommand], b: &[PathCommand]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(a, b)| std::mem::discriminant(a) == std::mem::discriminant(b))
}
fn validate_keys(path: &PathMask) -> Result<()> {
    if path.keys.len() > MAX_SHAPE_KEYS
        || path.keys.iter().map(|k| k.commands.len()).sum::<usize>() > MAX_ANIMATED_COMMANDS
    {
        return fail("shape animation exceeds its key or geometry limit");
    }
    let mut previous = None;
    for key in &path.keys {
        key.time.seconds()?;
        if key.time.ticks < 0
            || previous.is_some_and(|p: crate::Time| !p.compare(key.time).is_ok_and(|o| o.is_lt()))
        {
            return fail("shape keys must have unique increasing nonnegative times");
        }
        if !same_topology(&path.commands, &key.commands) {
            return fail("animated masks require the same points and segment types at every key");
        }
        PathMask {
            keys: vec![],
            fill_rule: path.fill_rule,
            commands: key.commands.clone(),
        }
        .validate()?;
        previous = Some(key.time);
    }
    Ok(())
}
fn coordinates(command: &mut PathCommand) -> Vec<&mut [f32; 2]> {
    match command {
        PathCommand::Move { to } | PathCommand::Line { to } => vec![to],
        PathCommand::Quad { control, to } => vec![control, to],
        PathCommand::Cubic {
            control1,
            control2,
            to,
        } => vec![control1, control2, to],
        PathCommand::Close {} => vec![],
    }
}
impl PathMask {
    /// Sample linearly between matching shapes; hold the first/last key outside the track.
    pub fn at(&self, time: crate::Time) -> Result<Self> {
        time.seconds()?;
        let mut result = Self {
            keys: vec![],
            fill_rule: self.fill_rule,
            commands: self.commands.clone(),
        };
        if self.keys.is_empty() {
            return Ok(result);
        }
        let right = self
            .keys
            .partition_point(|k| k.time.compare(time).is_ok_and(|o| o.is_lt()));
        if right == 0 {
            result.commands = self.keys[0].commands.clone();
            return Ok(result);
        }
        if right == self.keys.len() {
            result.commands = self.keys[right - 1].commands.clone();
            return Ok(result);
        }
        let a = &self.keys[right - 1];
        let b = &self.keys[right];
        let difference = |x: crate::Time, y: crate::Time| {
            (i128::from(x.ticks) * i128::from(y.rate) - i128::from(y.ticks) * i128::from(x.rate))
                as f64
                / (f64::from(x.rate) * f64::from(y.rate))
        };
        let t = (difference(time, a.time) / difference(b.time, a.time)).clamp(0.0, 1.0);
        result.commands = a.commands.clone();
        for (command, other) in result.commands.iter_mut().zip(&b.commands) {
            let mut other = *other;
            for (a, b) in coordinates(command)
                .into_iter()
                .zip(coordinates(&mut other))
            {
                for i in 0..2 {
                    a[i] = ((1.0 - t) * f64::from(a[i]) + t * f64::from(b[i])) as f32;
                }
            }
        }
        Ok(result)
    }
    /// Add or replace a key atomically. Exact rational timestamps identify keys.
    pub fn set_key(&mut self, time: crate::Time, shape: &Self) -> Result<()> {
        self.validate()?;
        shape.validate()?;
        time.seconds()?;
        if shape.fill_rule != self.fill_rule || !same_topology(&self.commands, &shape.commands) {
            return fail(
                "Disable shape animation before changing the mask's points or segment types",
            );
        }
        let mut result = self.clone();
        if let Some(key) = result
            .keys
            .iter_mut()
            .find(|k| k.time.compare(time).is_ok_and(|o| o.is_eq()))
        {
            key.commands = shape.commands.clone();
        } else {
            result.keys.push(ShapeKey {
                time,
                commands: shape.commands.clone(),
            });
            result.keys.sort_by(|a, b| a.time.compare(b.time).unwrap());
        }
        result.validate()?;
        *self = result;
        Ok(())
    }
    /// Edits to an animated mask author the current key; static masks stay static.
    pub fn edit_at(&mut self, time: crate::Time, shape: Self) -> Result<()> {
        if self.keys.is_empty() {
            shape.validate()?;
            *self = shape;
            Ok(())
        } else {
            self.set_key(time, &shape)
        }
    }
    pub fn remove_key(&mut self, time: crate::Time) -> Result<()> {
        let sampled = self.at(time)?;
        self.keys
            .retain(|k| !k.time.compare(time).is_ok_and(|o| o.is_eq()));
        if self.keys.is_empty() {
            *self = sampled;
        }
        Ok(())
    }
}
