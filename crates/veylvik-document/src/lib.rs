//! Serializable compositions and transactional editing, independent of GPU and UI.
pub mod registry;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use uuid::Uuid;
pub type NodeId = Uuid;
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Error(pub String);
fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error(message.into()))
}

/// Exact seconds. Structural equality is deliberately not exposed; compare rational values.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Time {
    pub ticks: i64,
    pub rate: u32,
}
impl Time {
    pub fn new(ticks: i64, rate: u32) -> Result<Self> {
        if rate == 0 {
            return fail("time rate must be positive");
        }
        Ok(Self { ticks, rate })
    }
    pub fn seconds(self) -> Result<f64> {
        Self::new(self.ticks, self.rate)?;
        Ok(self.ticks as f64 / self.rate as f64)
    }
    pub fn compare(self, other: Self) -> Result<std::cmp::Ordering> {
        Self::new(self.ticks, self.rate)?;
        Self::new(other.ticks, other.rate)?;
        Ok((self.ticks as i128 * other.rate as i128)
            .cmp(&(other.ticks as i128 * self.rate as i128)))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub time: Time,
    pub value: f32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scalar {
    Constant { value: f32 },
    Linear { keys: Vec<Key> },
}
impl From<f32> for Scalar {
    fn from(value: f32) -> Self {
        Self::Constant { value }
    }
}
impl Scalar {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Constant { value } if !value.is_finite() => fail("non-finite scalar"),
            Self::Linear { keys } => {
                if keys.is_empty() {
                    return fail("animation needs at least one key");
                }
                for key in keys {
                    key.time.seconds()?;
                    if !key.value.is_finite() {
                        return fail("non-finite key");
                    }
                }
                for pair in keys.windows(2) {
                    if !pair[0].time.compare(pair[1].time)?.is_lt() {
                        return fail("keys must have strictly increasing times");
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    fn bounded(&self, min: f32, max: f32, name: &str) -> Result<()> {
        self.validate()?;
        let check = |value: f32| {
            if (min..=max).contains(&value) {
                Ok(())
            } else {
                fail(format!("{name} must be in {min}..={max}"))
            }
        };
        match self {
            Self::Constant { value } => check(*value),
            Self::Linear { keys } => {
                for key in keys {
                    check(key.value)?;
                }
                Ok(())
            }
        }
    }
    pub fn at(&self, time: Time) -> Result<f32> {
        self.validate()?;
        time.seconds()?;
        match self {
            Self::Constant { value } => Ok(*value),
            Self::Linear { keys } => {
                let first = &keys[0];
                if !time.compare(first.time)?.is_gt() {
                    return Ok(first.value);
                }
                for pair in keys.windows(2) {
                    if !time.compare(pair[1].time)?.is_gt() {
                        let a = pair[0].time;
                        let b = pair[1].time;
                        // Subtract exactly before converting; large adjacent ticks can
                        // collapse to the same f64 seconds value.
                        let elapsed = time.ticks as i128 * a.rate as i128
                            - a.ticks as i128 * time.rate as i128;
                        let span =
                            b.ticks as i128 * a.rate as i128 - a.ticks as i128 * b.rate as i128;
                        let factor =
                            elapsed as f64 / span as f64 * b.rate as f64 / time.rate as f64;
                        return Ok((pair[0].value as f64 * (1.0 - factor)
                            + pair[1].value as f64 * factor)
                            as f32);
                    }
                }
                Ok(keys.last().unwrap().value)
            }
        }
    }
}
/// All image ports carry scene-linear Rec.709, premultiplied RGBA float pixels.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Solid { color: [f32; 4] },
    Noise { seed: u32, scale: Scalar },
    Exposure { stops: Scalar },
    Transform { x: Scalar, y: Scalar },
    Blur { radius: Scalar },
    Merge,
    Image { asset: String },
}
impl Operation {
    pub fn descriptor(&self) -> &'static registry::NodeType {
        match self {
            Self::Solid { .. } => &registry::SOLID,
            Self::Noise { .. } => &registry::NOISE,
            Self::Exposure { .. } => &registry::EXPOSURE,
            Self::Transform { .. } => &registry::TRANSFORM,
            Self::Blur { .. } => &registry::BLUR,
            Self::Merge => &registry::MERGE,
            Self::Image { .. } => &registry::IMAGE,
        }
    }
    pub fn inputs(&self) -> &'static [&'static str] {
        self.descriptor().inputs
    }
    pub fn scalar(&self, name: &str) -> Option<&Scalar> {
        match (self, name) {
            (Self::Noise { scale, .. }, "scale") => Some(scale),
            (Self::Exposure { stops }, "stops") => Some(stops),
            (Self::Transform { x, .. }, "x") => Some(x),
            (Self::Transform { y, .. }, "y") => Some(y),
            (Self::Blur { radius }, "radius") => Some(radius),
            _ => None,
        }
    }
    pub fn validate(&self) -> Result<()> {
        for parameter in self.descriptor().parameters {
            if let registry::ParameterKind::Scalar = parameter.kind {
                let value = self
                    .scalar(parameter.name)
                    .ok_or_else(|| Error(format!("registry mismatch for {}", parameter.name)))?;
                value.bounded(
                    parameter.min.unwrap(),
                    parameter.max.unwrap(),
                    parameter.name,
                )?;
            }
        }
        match self {
            Self::Solid { color }
                if color.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&color[3]) =>
            {
                fail("solid needs finite RGB and alpha in [0,1]")
            }
            Self::Image { asset }
                if asset.trim().is_empty()
                    || asset.chars().count() > registry::MAX_ASSET_NAME
                    || asset.contains('\0') =>
            {
                fail("image needs a nonempty asset key without NUL, at most 4096 characters")
            }
            _ => Ok(()),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: NodeId,
    pub version: u32,
    pub label: String,
    pub operation: Operation,
    pub inputs: BTreeMap<String, NodeId>,
}
impl Node {
    pub fn new(label: &str, operation: Operation, inputs: &[(&str, NodeId)]) -> Self {
        Self {
            id: Uuid::new_v4(),
            version: 1,
            label: label.into(),
            operation,
            inputs: inputs.iter().map(|(k, v)| ((*k).into(), *v)).collect(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    pub schema: u32,
    pub width: u32,
    pub height: u32,
    pub frame_rate: Time,
    pub nodes: BTreeMap<NodeId, Node>,
    pub output: NodeId,
}
impl Composition {
    /// Validates the entire document, including disconnected nodes, without recursion.
    pub fn validate(&self) -> Result<Vec<NodeId>> {
        if self.schema != 1 {
            return fail("unsupported composition schema");
        }
        if self.width == 0 || self.height == 0 || self.width > 8192 || self.height > 8192 {
            return fail("dimensions must be in 1..=8192");
        }
        if self.frame_rate.seconds()? <= 0.0 {
            return fail("frame rate must be positive");
        }
        if self.nodes.len() > 4096 {
            return fail("composition exceeds 4096 nodes");
        }
        if !self.nodes.contains_key(&self.output) {
            return fail("output node is missing");
        }
        let mut pending = BTreeMap::new();
        let mut consumers: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        for (id, node) in &self.nodes {
            if *id != node.id || node.version != node.operation.descriptor().version {
                return fail(format!("invalid identity or version for {id}"));
            }
            node.operation.validate()?;
            let expected: BTreeSet<_> = node.operation.inputs().iter().copied().collect();
            let actual: BTreeSet<_> = node.inputs.keys().map(String::as_str).collect();
            if expected != actual {
                return fail(format!(
                    "{}: expected input ports {expected:?}, got {actual:?}",
                    node.label
                ));
            }
            pending.insert(*id, node.inputs.len());
            for input in node.inputs.values() {
                if !self.nodes.contains_key(input) {
                    return fail(format!("{}: missing source {input}", node.label));
                }
                consumers.entry(*input).or_default().push(*id);
            }
        }
        let mut ready: BTreeSet<_> = pending
            .iter()
            .filter(|(_, count)| **count == 0)
            .map(|(id, _)| *id)
            .collect();
        let mut order = Vec::new();
        while let Some(id) = ready.pop_first() {
            order.push(id);
            for consumer in consumers.get(&id).into_iter().flatten() {
                let count = pending.get_mut(consumer).unwrap();
                *count -= 1;
                if *count == 0 {
                    ready.insert(*consumer);
                }
            }
        }
        if order.len() != self.nodes.len() {
            return fail("graph contains a cycle");
        }
        Ok(order)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let data = serde_json::to_vec_pretty(self).map_err(|e| Error(e.to_string()))?;
        atomic_write(path, &data)
    }
    pub fn load(path: &Path) -> Result<Self> {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|e| Error(e.to_string()))?;
        let mut bytes = Vec::new();
        file.take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| Error(e.to_string()))?;
        if bytes.len() > 16 * 1024 * 1024 {
            return fail("document exceeds 16 MiB");
        }
        let doc: Self = serde_json::from_slice(&bytes).map_err(|e| Error(e.to_string()))?;
        doc.validate()?;
        Ok(doc)
    }
}
/// Same-directory replacement keeps readers from observing partial saves.
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temp = parent.join(format!(".veylvik-{}.tmp", Uuid::new_v4()));
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(data)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)?;
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result.map_err(|e| Error(e.to_string()))
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Put { node: Node },
    Remove { id: NodeId },
    Output { id: NodeId },
    Resize { width: u32, height: u32 },
}
/// Only this owner publishes revisions. Renderers receive immutable snapshots.
pub struct Document {
    composition: Composition,
    revision: u64,
    undo: Vec<Composition>,
    redo: Vec<Composition>,
}
impl Document {
    pub fn new(composition: Composition) -> Result<Self> {
        composition.validate()?;
        Ok(Self {
            composition,
            revision: 0,
            undo: vec![],
            redo: vec![],
        })
    }
    pub fn snapshot(&self) -> &Composition {
        &self.composition
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    fn check_revision(&self, expected: u64) -> Result<()> {
        if self.revision != expected {
            return fail(format!(
                "revision conflict: expected {expected}, current {}",
                self.revision
            ));
        }
        Ok(())
    }
    pub fn prepare(&self, expected: u64, commands: &[Command]) -> Result<Composition> {
        self.check_revision(expected)?;
        if commands.is_empty() || commands.len() > 4096 {
            return fail("transaction needs 1..=4096 commands");
        }
        let mut next = self.composition.clone();
        for command in commands {
            match command {
                Command::Put { node } => {
                    next.nodes.insert(node.id, node.clone());
                }
                Command::Remove { id } => {
                    if next.nodes.remove(id).is_none() {
                        return fail("cannot remove missing node");
                    }
                }
                Command::Output { id } => next.output = *id,
                Command::Resize { width, height } => {
                    next.width = *width;
                    next.height = *height;
                }
            }
        }
        next.validate()?;
        Ok(next)
    }
    pub fn apply(&mut self, expected: u64, commands: &[Command]) -> Result<u64> {
        let next = self.prepare(expected, commands)?;
        self.undo
            .push(std::mem::replace(&mut self.composition, next));
        if self.undo.len() > 64 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.revision += 1;
        Ok(self.revision)
    }
    pub fn undo(&mut self, expected: u64) -> Result<u64> {
        self.check_revision(expected)?;
        let prev = self
            .undo
            .pop()
            .ok_or_else(|| Error("nothing to undo".into()))?;
        self.redo
            .push(std::mem::replace(&mut self.composition, prev));
        self.revision += 1;
        Ok(self.revision)
    }
    pub fn redo(&mut self, expected: u64) -> Result<u64> {
        self.check_revision(expected)?;
        let next = self
            .redo
            .pop()
            .ok_or_else(|| Error("nothing to redo".into()))?;
        self.undo
            .push(std::mem::replace(&mut self.composition, next));
        self.revision += 1;
        Ok(self.revision)
    }
}
/// A small composition exercising animation, alpha, branching and HDR.
pub fn demo() -> Composition {
    let background = Node::new(
        "Background",
        Operation::Solid {
            color: [0.015, 0.025, 0.06, 1.0],
        },
        &[],
    );
    let noise = Node::new(
        "Noise",
        Operation::Noise {
            seed: 42,
            scale: 18.0.into(),
        },
        &[],
    );
    let exposure = Node::new(
        "Animated exposure",
        Operation::Exposure {
            stops: Scalar::Linear {
                keys: vec![
                    Key {
                        time: Time { ticks: 0, rate: 1 },
                        value: -2.0,
                    },
                    Key {
                        time: Time { ticks: 2, rate: 1 },
                        value: 2.0,
                    },
                ],
            },
        },
        &[("image", noise.id)],
    );
    let blur = Node::new(
        "Soft glow",
        Operation::Blur { radius: 4.0.into() },
        &[("image", exposure.id)],
    );
    let transform = Node::new(
        "Offset",
        Operation::Transform {
            x: 80.0.into(),
            y: 40.0.into(),
        },
        &[("image", blur.id)],
    );
    let merge = Node::new(
        "Composite",
        Operation::Merge,
        &[("background", background.id), ("foreground", transform.id)],
    );
    Composition {
        schema: 1,
        width: 640,
        height: 360,
        frame_rate: Time { ticks: 24, rate: 1 },
        output: merge.id,
        nodes: [background, noise, exposure, blur, transform, merge]
            .into_iter()
            .map(|n| (n.id, n))
            .collect(),
    }
}
