//! Nuke scripts into Veylvik compositions: the nodes that have a Veylvik counterpart are rebuilt with their settings and keys, wired as the script wires them; every other node is passed through and named in the report, so nothing is lost silently.
mod script;
pub use script::{Read, Script, Value, read};
use serde_json::{Value as Json, json};
use veylvik_document::{Composition, Node, NodeId, NodePosition, Operation, Time, registry};

/// A script rebuilt: the composition, and what was and was not taken over.
#[derive(Debug)]
pub struct Imported {
    pub composition: Composition,
    /// Nodes rebuilt.
    pub rebuilt: usize,
    /// Each node not rebuilt (passed through, or left out), as `"name (Class)"`.
    pub not_rebuilt: Vec<String>,
    /// What was rebuilt only roughly, and why.
    pub notes: Vec<String>,
}
type Result<T> = std::result::Result<T, String>;

/// Numbers in a knob's value: a word, or a braced list.
fn numbers(value: &Value) -> Vec<f32> {
    value
        .text()
        .split_whitespace()
        .filter_map(|w| w.parse().ok())
        .collect()
}
/// The script's settings: size, rate and frame range.
struct Frame {
    width: u32,
    height: u32,
    rate: Time,
    first: i64,
}
impl Frame {
    fn of(script: &Script) -> Result<(Self, i64)> {
        let knob = |name: &str| script.root.iter().find(|(n, _)| n == name).map(|(_, v)| v);
        // format "W H x y r t aspect NAME"
        let (width, height) = knob("format")
            .map(|v| {
                let n: Vec<u32> = v
                    .text()
                    .split_whitespace()
                    .take(2)
                    .filter_map(|w| w.parse().ok())
                    .collect();
                (
                    n.first().copied().unwrap_or(1920),
                    n.get(1).copied().unwrap_or(1080),
                )
            })
            .unwrap_or((1920, 1080));
        let fps: f64 = knob("fps")
            .and_then(|v| v.text().parse().ok())
            .unwrap_or(24.0);
        let rate = match (fps * 1000.0).round() as i64 {
            23976 => Time::new(24000, 1001),
            29970 => Time::new(30000, 1001),
            59940 => Time::new(60000, 1001),
            n if n % 1000 == 0 => Time::new(n / 1000, 1),
            n => Time::new(n, 1000),
        }
        .map_err(|e| e.to_string())?;
        let first: i64 = knob("first_frame")
            .and_then(|v| v.text().parse().ok())
            .unwrap_or(1);
        let last: i64 = knob("last_frame")
            .and_then(|v| v.text().parse().ok())
            .unwrap_or(first + 99);
        Ok((
            Self {
                width: width.clamp(16, 16384),
                height: height.clamp(16, 16384),
                rate,
                first,
            },
            (last - first + 1).clamp(1, 1_000_000),
        ))
    }
    /// A knob's value as a Veylvik setting: a constant, or keys where it is a curve
    /// (`{{curve x1 0 x30 100}}`: x sets a frame, later values go on frame by frame).
    fn scalar(&self, value: &Value, index: usize, scale: f32, offset: f32) -> Json {
        let text = value.text();
        // Several channels, each maybe a curve: `{{curve ..} {curve ..}}` or `{1 2}`.
        let parts = channels(text);
        let part = parts
            .get(index)
            .or(parts.first())
            .cloned()
            .unwrap_or_default();
        if let Some(curve) = part
            .trim()
            .strip_prefix("{curve")
            .or(part.trim().strip_prefix("curve"))
        {
            let mut frame = self.first;
            let mut keys = Vec::new();
            for word in curve.trim_end_matches('}').split_whitespace() {
                if let Some(at) = word.strip_prefix('x') {
                    if let Ok(f) = at.parse::<f64>() {
                        frame = f.round() as i64;
                    }
                } else if let Ok(v) = word.parse::<f32>() {
                    let time = Time::at_frame(frame - self.first, self.rate)
                        .unwrap_or(Time { ticks: 0, rate: 1 });
                    keys.push(json!({"time": {"ticks": time.ticks, "rate": time.rate}, "value": v * scale + offset}));
                    frame += 1;
                }
            }
            if keys.len() > 1 {
                return json!({"kind": "smooth", "keys": keys});
            }
            if let Some(k) = keys.first() {
                return json!({"kind": "constant", "value": k["value"]});
            }
        }
        let value = part.trim().parse::<f32>().unwrap_or(0.0);
        json!({"kind": "constant", "value": value * scale + offset})
    }
}
/// A knob's channels: the words of a list, or the braced parts of one.
fn channels(text: &str) -> Vec<String> {
    // The reader has taken off the knob's own braces: what is left is its channels.
    let inner = text.trim();
    let mut out = Vec::new();
    let mut depth = 0;
    let mut current = String::new();
    for c in inner.chars() {
        match c {
            '{' => {
                depth += 1;
                current.push(c);
            }
            '}' => {
                depth -= 1;
                current.push(c);
                if depth == 0 {
                    out.push(std::mem::take(&mut current));
                }
            }
            c if c.is_whitespace() && depth == 0 => {
                if !current.trim().is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        out.push(inner.to_string());
    }
    out
}
/// A sequence's file name for `frame`: `%04d` (or `%d`) and `####` filled in.
fn first_frame(pattern: &str, frame: i64) -> String {
    if let Some(at) = pattern.find('%') {
        let rest = &pattern[at + 1..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if rest[digits.len()..].starts_with('d') {
            let width: usize = digits.trim_start_matches('0').parse().unwrap_or(0);
            return format!(
                "{}{:0width$}{}",
                &pattern[..at],
                frame,
                &rest[digits.len() + 1..],
                width = width
            );
        }
    }
    if let Some(at) = pattern.find('#') {
        let width = pattern[at..].chars().take_while(|c| *c == '#').count();
        return format!(
            "{}{:0width$}{}",
            &pattern[..at],
            frame,
            &pattern[at + width..],
            width = width
        );
    }
    pattern.to_string()
}
/// A node of `kind` with its default settings, then `settings` over them.
fn made(
    kind: &registry::NodeType,
    label: &str,
    settings: Json,
    inputs: &[(&str, NodeId)],
) -> Result<Node> {
    let mut value = kind.default_operation();
    if let (Some(base), Some(over)) = (value.as_object_mut(), settings.as_object()) {
        for (k, v) in over {
            base.insert(k.clone(), v.clone());
        }
    }
    let operation: Operation =
        serde_json::from_value(value).map_err(|e| format!("{label}: {e}"))?;
    Ok(Node::new(label, operation, inputs))
}
/// Rebuilds `text`, a Nuke script.
pub fn import(text: &str) -> Result<Imported> {
    let script = read(text)?;
    let (frame, frames) = Frame::of(&script)?;
    let mut composition = veylvik_document::blank(frame.width, frame.height, frame.rate)
        .map_err(|e| e.to_string())?;
    composition.frame_count = frames as u32;
    let background = composition.output;
    let mut nodes: Vec<Node> = Vec::new();
    // What each script node became: the Veylvik node its output is (None: nothing).
    let mut became: Vec<Option<NodeId>> = Vec::with_capacity(script.nodes.len());
    let (mut rebuilt, mut not_rebuilt, mut notes) = (0, Vec::new(), Vec::new());
    let mut output = None;
    for node in &script.nodes {
        let input = |k: usize| {
            node.inputs
                .get(k)
                .copied()
                .flatten()
                .and_then(|i| became[i])
        };
        let label = node.name();
        // A knob set by an expression keeps the node's default (expressions are not
        // evaluated) and is named; layout knobs never matter.
        let expression = |value: &Value| {
            channels(value.text())
                .iter()
                .any(|c| c.trim_start().starts_with('{') && !c.contains("curve"))
        };
        for (name, value) in &node.knobs {
            if !matches!(
                name.as_str(),
                "xpos" | "ypos" | "name" | "label" | "note_font"
            ) && expression(value)
            {
                notes.push(format!(
                    "{label}: {name} is an expression; not evaluated, the default is kept"
                ));
            }
        }
        let knob = |name: &str| node.knob(name).filter(|v| !expression(v));
        let scalar = |name: &str, index: usize, default: f32| -> Json {
            knob(name).map_or_else(
                || json!({"kind": "constant", "value": default}),
                |v| frame.scalar(v, index, 1.0, 0.0),
            )
        };
        let mut make = |kind: &registry::NodeType,
                        settings: Json,
                        inputs: &[(&str, NodeId)]|
         -> Result<NodeId> {
            let mut made = made(kind, &label, settings, inputs)?;
            let at = |name: &str| knob(name).and_then(|v| v.text().parse::<f32>().ok());
            if let (Some(x), Some(y)) = (at("xpos"), at("ypos")) {
                made.position = Some(NodePosition {
                    x: x * 1.4,
                    y: y * 1.4,
                });
            }
            let id = made.id;
            nodes.push(made);
            Ok(id)
        };
        let result: Option<NodeId> = match node.class.as_str() {
            "Read" => {
                let mut file = knob("file")
                    .map(|v| v.text().to_string())
                    .unwrap_or_default();
                if file.contains('%') || file.contains('#') {
                    let first: i64 = knob("first")
                        .and_then(|v| v.text().parse().ok())
                        .unwrap_or(frame.first);
                    let pattern = file.clone();
                    file = first_frame(&pattern, first);
                    notes.push(format!("{label}: an image sequence ({pattern}); its first frame ({file}) stands in: import the sequence with Import sequence"));
                }
                rebuilt += 1;
                Some(make(&registry::FOOTAGE, json!({"asset": file}), &[])?)
            }
            "Constant" => {
                let c = knob("color").map(numbers).unwrap_or_default();
                let c = match c.len() {
                    1 => [c[0], c[0], c[0], 1.0],
                    3 => [c[0], c[1], c[2], 1.0],
                    n if n >= 4 => [c[0], c[1], c[2], c[3]],
                    _ => [0.0, 0.0, 0.0, 1.0],
                };
                rebuilt += 1;
                Some(make(&registry::SOLID, json!({"color": c}), &[])?)
            }
            "Merge2" | "Merge" => {
                let (b, a) = (input(0), input(1));
                match (b, a) {
                    (Some(b), Some(a)) => {
                        let operation = knob("operation").map_or("over", |v| v.text());
                        let mix = knob("mix");
                        let mode = match operation {
                            "over" => None,
                            "plus" => Some("add"),
                            "multiply" => Some("multiply"),
                            "screen" => Some("screen"),
                            "difference" => Some("difference"),
                            "max" => Some("lighten"),
                            "min" => Some("darken"),
                            "overlay" => Some("overlay"),
                            "soft-light" => Some("soft_light"),
                            other => {
                                notes.push(format!("{label}: Merge {other} has no Veylvik counterpart; laid over instead"));
                                None
                            }
                        };
                        rebuilt += 1;
                        Some(match (mode, mix) {
                            (None, None) => make(
                                &registry::MERGE,
                                json!({}),
                                &[("background", b), ("foreground", a)],
                            )?,
                            (mode, _) => make(
                                &registry::BLEND,
                                json!({"mode": mode.unwrap_or("normal"), "opacity": scalar("mix", 0, 1.0)}),
                                &[("background", b), ("foreground", a)],
                            )?,
                        })
                    }
                    (one, other) => {
                        notes.push(format!("{label}: a Merge with one input passes it on"));
                        one.or(other)
                    }
                }
            }
            "Grade" | "ColorCorrect" => match input(0) {
                Some(image) => {
                    if knob("blackpoint").is_some()
                        || knob("whitepoint").is_some()
                        || knob("offset").is_some()
                    {
                        notes.push(format!(
                            "{label}: black point, white point and offset are not carried over"
                        ));
                    }
                    if [knob("gain"), knob("lift"), knob("gamma"), knob("multiply")]
                        .iter()
                        .flatten()
                        .any(|v| channels(v.text()).len() > 1)
                    {
                        notes.push(format!(
                            "{label}: per-channel values are taken as their first (red) channel"
                        ));
                    }
                    let gain = match knob("multiply") {
                        Some(m) => {
                            let m = numbers(m).first().copied().unwrap_or(1.0);
                            knob("gain").map_or_else(
                                || json!({"kind": "constant", "value": m}),
                                |v| frame.scalar(v, 0, m, 0.0),
                            )
                        }
                        None => scalar("gain", 0, 1.0),
                    };
                    let mut settings = json!({"gain": gain, "lift": scalar("lift", 0, 0.0), "gamma": scalar("gamma", 0, 1.0)});
                    if node.class == "ColorCorrect" {
                        settings["saturation"] = scalar("saturation", 0, 1.0);
                        settings["contrast"] = scalar("contrast", 0, 1.0);
                    }
                    rebuilt += 1;
                    Some(make(
                        &registry::COLOR_CORRECT,
                        settings,
                        &[("image", image)],
                    )?)
                }
                None => None,
            },
            "Transform" | "TransformMasked" => match input(0) {
                Some(image) => {
                    let h = frame.height as f32;
                    let translate = knob("translate");
                    let center = knob("center");
                    let scales = knob("scale");
                    let both = scales
                        .map(|v| channels(v.text()).len() > 1)
                        .unwrap_or(false);
                    let settings = json!({
                        "x": translate.map_or(json!({"kind": "constant", "value": 0.0}), |v| frame.scalar(v, 0, 1.0, 0.0)),
                        // Nuke measures y upwards from the bottom.
                        "y": translate.map_or(json!({"kind": "constant", "value": 0.0}), |v| frame.scalar(v, 1, -1.0, 0.0)),
                        // Nuke turns anticlockwise, Veylvik clockwise.
                        "rotation": knob("rotate").map_or(json!({"kind": "constant", "value": 0.0}), |v| frame.scalar(v, 0, -1.0, 0.0)),
                        "scale_x": scales.map_or(json!({"kind": "constant", "value": 1.0}), |v| frame.scalar(v, 0, 1.0, 0.0)),
                        "scale_y": scales.map_or(json!({"kind": "constant", "value": 1.0}), |v| frame.scalar(v, if both { 1 } else { 0 }, 1.0, 0.0)),
                        "pivot_x": center.map_or(json!({"kind": "constant", "value": frame.width as f32 * 0.5}), |v| frame.scalar(v, 0, 1.0, 0.0)),
                        "pivot_y": center.map_or(json!({"kind": "constant", "value": h * 0.5}), |v| frame.scalar(v, 1, -1.0, h)),
                    });
                    if knob("skewX").is_some() || knob("skewY").is_some() {
                        notes.push(format!("{label}: skew is not carried over"));
                    }
                    rebuilt += 1;
                    Some(make(&registry::AFFINE, settings, &[("image", image)])?)
                }
                None => None,
            },
            "Blur" => match input(0) {
                Some(image) => {
                    let radius = knob("size")
                        .map_or(json!({"kind": "constant", "value": 0.0}), |v| {
                            frame.scalar(v, 0, 0.5, 0.0)
                        });
                    rebuilt += 1;
                    Some(make(
                        &registry::BLUR,
                        json!({"radius": radius, "filter": "gaussian"}),
                        &[("image", image)],
                    )?)
                }
                None => None,
            },
            "Text2" | "Text" => {
                let message = knob("message")
                    .map(|v| v.text().to_string())
                    .unwrap_or_default();
                let text = make(
                    &registry::TEXT,
                    json!({"content": message, "size": scalar("global_font_scale", 0, 1.0)["value"].as_f64().map_or_else(|| json!({"kind": "constant", "value": 100.0}), |s| json!({"kind": "constant", "value": 100.0 * s}))}),
                    &[],
                )?;
                notes.push(format!(
                    "{label}: text is placed in the middle at its default font; place it by hand"
                ));
                rebuilt += 1;
                Some(match input(0) {
                    Some(under) => make(
                        &registry::MERGE,
                        json!({}),
                        &[("background", under), ("foreground", text)],
                    )?,
                    None => text,
                })
            }
            "Write" => {
                output = input(0);
                None
            }
            "Dot" | "NoOp" | "Viewer" => input(0),
            "StickyNote" | "BackdropNode" => None,
            other => {
                not_rebuilt.push(format!("{label} ({other})"));
                input(0)
            }
        };
        became.push(result);
    }
    // The picture: what the Write takes, else the last node rebuilt.
    let output = output.or_else(|| became.iter().rev().find_map(|b| *b));
    for node in nodes {
        composition.nodes.insert(node.id, node);
    }
    if let Some(output) = output {
        composition.output = output;
        composition.nodes.remove(&background);
    }
    composition.validate().map_err(|e| e.to_string())?;
    if script.closed_groups > 0 {
        notes.push(format!(
            "{} group(s) were not opened: each passes its input on",
            script.closed_groups
        ));
    }
    Ok(Imported {
        composition,
        rebuilt,
        not_rebuilt,
        notes,
    })
}
#[cfg(test)]
mod tests;
