//! Serializable compositions and transactional editing, independent of GPU and UI.
pub mod clipboard;
pub mod editing;
pub mod groups;
mod history;
pub mod occlusion;
pub mod presets;
mod procedural;
pub mod recipes;
pub use procedural::procedural_demo;
pub mod inspection;
pub mod limits;
pub mod registry;
pub use history::{Document, HistoryStats};
pub mod blending;
pub mod channels;
pub use channels::MaskChannel;
mod masks;
pub use masks::MaskMode;
mod appearance;
pub mod optical;
pub use appearance::{AtmosphereKind, MaterialKind, PatinaKind, ReflectionKind};
mod gradient;
pub use gradient::GradientMode;
pub mod schema;
pub mod text;
pub use text::{TextAlignment, TextFont};
pub mod color_management;
pub mod media;
pub mod planar;
pub mod sequence;
pub mod surface;
pub mod tracking;
pub mod vector;
pub use blending::BlendMode;
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

pub mod animation;
pub mod expression;
pub mod key_edit;
pub mod nodes;
pub mod track;
pub use animation::{CompiledScalar, Handle, Interpolation, Key, Scalar, Segment, Time};
use nodes::NodeOp;
pub use track::Track3;
/// Evaluates `$body` with `$node` bound to the node struct inside any `Operation`.
#[macro_export]
macro_rules! each_node {
    ($op:expr, $node:ident => $body:expr) => {
        match $op {
            $crate::Operation::SurfaceBlend($node) => $body,
            $crate::Operation::PlanarTrack($node) => $body,
            $crate::Operation::PlanarWarp($node) => $body,
            $crate::Operation::Text($node) => $body,
            $crate::Operation::Solid($node) => $body,
            $crate::Operation::Colorize($node) => $body,
            $crate::Operation::Gradient($node) => $body,
            $crate::Operation::Noise($node) => $body,
            $crate::Operation::SurfaceReflection($node) => $body,
            $crate::Operation::Glass($node) => $body,
            $crate::Operation::SunGlints($node) => $body,
            $crate::Operation::Patina($node) => $body,
            $crate::Operation::MaterialLayers($node) => $body,
            $crate::Operation::Material($node) => $body,
            $crate::Operation::Condensation($node) => $body,
            $crate::Operation::Atmosphere($node) => $body,
            $crate::Operation::MetallicShine($node) => $body,
            $crate::Operation::Fire($node) => $body,
            $crate::Operation::Dust($node) => $body,
            $crate::Operation::Drip($node) => $body,
            $crate::Operation::FractalNoise($node) => $body,
            $crate::Operation::Exposure($node) => $body,
            $crate::Operation::ColorCorrect($node) => $body,
            $crate::Operation::Lut($node) => $body,
            $crate::Operation::Transform($node) => $body,
            $crate::Operation::Displace($node) => $body,
            $crate::Operation::Morphology($node) => $body,
            $crate::Operation::LensBlur($node) => $body,
            $crate::Operation::LightRig($node) => $body,
            $crate::Operation::Blur($node) => $body,
            $crate::Operation::Shadow($node) => $body,
            $crate::Operation::ContactShadow($node) => $body,
            $crate::Operation::Shape3d($node) => $body,
            $crate::Operation::Text3d($node) => $body,
            $crate::Operation::ImageCard($node) => $body,
            $crate::Operation::Transform3d($node) => $body,
            $crate::Operation::MergeScenes($node) => $body,
            $crate::Operation::Camera3d($node) => $body,
            $crate::Operation::Light3d($node) => $body,
            $crate::Operation::RenderScene($node) => $body,
            $crate::Operation::Environment3d($node) => $body,
            $crate::Operation::PointCloud($node) => $body,
            $crate::Operation::MatchGrain($node) => $body,
            $crate::Operation::LensDistortion($node) => $body,
            $crate::Operation::Bloom($node) => $body,
            $crate::Operation::Shader($node) => $body,
            $crate::Operation::GridWarp($node) => $body,
            $crate::Operation::ShapeWarp($node) => $body,
            $crate::Operation::Particles($node) => $body,
            $crate::Operation::Model3d($node) => $body,
            $crate::Operation::Merge($node) => $body,
            $crate::Operation::Blend($node) => $body,
            $crate::Operation::Fade($node) => $body,
            $crate::Operation::ChromaKey($node) => $body,
            $crate::Operation::LumaKey($node) => $body,
            $crate::Operation::RefineMatte($node) => $body,
            $crate::Operation::Image($node) => $body,
            $crate::Operation::Footage($node) => $body,
            $crate::Operation::MotionBlur($node) => $body,
            $crate::Operation::TimeRemap($node) => $body,
            $crate::Operation::Ocio($node) => $body,
            $crate::Operation::OpticalFlow($node) => $body,
            $crate::Operation::Affine($node) => $body,
            $crate::Operation::Ellipse($node) => $body,
            $crate::Operation::Mask($node) => $body,
            $crate::Operation::CombineMask($node) => $body,
            $crate::Operation::ChannelMask($node) => $body,
            $crate::Operation::Path($node) => $body,
        }
    };
}
/// All image ports carry scene-linear Rec.709, premultiplied RGBA float pixels.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    SurfaceBlend(nodes::SurfaceBlend),
    PlanarTrack(nodes::PlanarTrack),
    PlanarWarp(nodes::PlanarWarp),
    Text(nodes::Text),
    Solid(nodes::Solid),
    Colorize(nodes::Colorize),
    Gradient(nodes::Gradient),
    Noise(nodes::Noise),
    SurfaceReflection(nodes::SurfaceReflection),
    Glass(nodes::Glass),
    SunGlints(nodes::SunGlints),
    Patina(nodes::Patina),
    MaterialLayers(nodes::MaterialLayers),
    Material(nodes::Material),
    Condensation(nodes::Condensation),
    Atmosphere(nodes::Atmosphere),
    MetallicShine(nodes::MetallicShine),
    Fire(nodes::Fire),
    Dust(nodes::Dust),
    Drip(nodes::Drip),
    FractalNoise(nodes::FractalNoise),
    Exposure(nodes::Exposure),
    ColorCorrect(nodes::ColorCorrect),
    Lut(nodes::Lut),
    Transform(nodes::Transform),
    Displace(nodes::Displace),
    Morphology(nodes::Morphology),
    LensBlur(nodes::LensBlur),
    LightRig(nodes::LightRig),
    Blur(nodes::Blur),
    Shadow(nodes::Shadow),
    ContactShadow(nodes::ContactShadow),
    #[serde(rename = "shape_3d")]
    Shape3d(nodes::Shape3d),
    #[serde(rename = "text_3d")]
    Text3d(nodes::Text3d),
    ImageCard(nodes::ImageCard),
    #[serde(rename = "transform_3d")]
    Transform3d(nodes::Transform3d),
    MergeScenes(nodes::MergeScenes),
    #[serde(rename = "camera_3d")]
    Camera3d(nodes::Camera3d),
    #[serde(rename = "light_3d")]
    Light3d(nodes::Light3d),
    RenderScene(nodes::RenderScene),
    #[serde(rename = "environment_3d")]
    Environment3d(nodes::Environment3d),
    PointCloud(nodes::PointCloud),
    MatchGrain(nodes::MatchGrain),
    LensDistortion(nodes::LensDistortion),
    Bloom(nodes::Bloom),
    Shader(nodes::Shader),
    GridWarp(nodes::GridWarp),
    ShapeWarp(nodes::ShapeWarp),
    Particles(nodes::Particles),
    #[serde(rename = "model_3d")]
    Model3d(nodes::Model3d),
    Merge(nodes::Merge),
    Blend(nodes::Blend),
    Fade(nodes::Fade),
    ChromaKey(nodes::ChromaKey),
    LumaKey(nodes::LumaKey),
    RefineMatte(nodes::RefineMatte),
    Image(nodes::Image),
    Footage(nodes::Footage),
    MotionBlur(nodes::MotionBlur),
    TimeRemap(nodes::TimeRemap),
    Ocio(nodes::Ocio),
    OpticalFlow(nodes::OpticalFlow),
    Affine(nodes::Affine),
    Ellipse(nodes::Ellipse),
    Mask(nodes::Mask),
    CombineMask(nodes::CombineMask),
    ChannelMask(nodes::ChannelMask),
    Path(nodes::Path),
}
impl Operation {
    pub fn asset(&self) -> Option<&str> {
        each_node!(self, node => node.asset())
    }
    pub fn asset_mut(&mut self) -> Option<&mut String> {
        each_node!(self, node => node.asset_mut())
    }
    /// The key a renderer looks this node's media up by: its asset, or for footage
    /// reading one layer of a multi-layer OpenEXR, the asset, a NUL and the layer
    /// (asset keys never contain NUL). `split_source_key` takes it apart.
    pub fn source_key(&self) -> Option<std::borrow::Cow<'_, str>> {
        match self {
            Operation::Footage(nodes::Footage {
                asset,
                layer: Some(layer),
                ..
            }) => Some(format!("{asset}\0{layer}").into()),
            _ => self.asset().map(Into::into),
        }
    }
    pub fn descriptor(&self) -> &'static registry::NodeType {
        each_node!(self, node => node.descriptor())
    }
    pub fn inputs(&self) -> &'static [&'static str] {
        self.descriptor().inputs
    }
    pub fn scalar(&self, name: &str) -> Option<&Scalar> {
        each_node!(self, node => node.scalar(name))
    }
    pub fn scalar_mut(&mut self, name: &str) -> Option<&mut Scalar> {
        each_node!(self, node => node.scalar_mut(name))
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
        each_node!(self, node => node.validate())
    }
}
/// Authoring coordinates; never used by the render compiler's pixel semantics.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodePosition {
    pub x: f32,
    pub y: f32,
}
impl NodePosition {
    pub fn validate(self) -> Result<()> {
        if [self.x, self.y]
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
        {
            return fail("node position must be finite and within ±1000000");
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: NodeId,
    pub version: u32,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<NodePosition>,
    /// Authored scene backdrop, replaceable without flattening the subject.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub scene_background: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bypass: bool,
    pub operation: Operation,
    pub inputs: BTreeMap<String, NodeId>,
}
impl Node {
    pub fn new(label: &str, operation: Operation, inputs: &[(&str, NodeId)]) -> Self {
        Self {
            id: Uuid::new_v4(),
            version: 1,
            label: label.into(),
            position: None,
            scene_background: false,
            bypass: false,
            operation,
            inputs: inputs.iter().map(|(k, v)| ((*k).into(), *v)).collect(),
        }
    }
}
pub const DEFAULT_FRAME_COUNT: u32 = 240;
pub const MAX_FRAME_COUNT: u32 = 1_000_000;
fn default_frame_count() -> u32 {
    DEFAULT_FRAME_COUNT
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    #[serde(
        default,
        skip_serializing_if = "color_management::OcioViews::is_default"
    )]
    pub ocio: color_management::OcioViews,
    #[serde(
        default,
        skip_serializing_if = "color_management::WorkingSpace::is_default"
    )]
    pub working_space: color_management::WorkingSpace,
    pub schema: u32,
    pub width: u32,
    pub height: u32,
    pub frame_rate: Time,
    #[serde(default = "default_frame_count")]
    pub frame_count: u32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub groups: BTreeMap<NodeId, groups::Group>,
    pub nodes: BTreeMap<NodeId, Node>,
    pub output: NodeId,
    /// SDR display transform for the viewer, PNG and video; projects without it are Standard.
    #[serde(default, skip_serializing_if = "media::DisplayTransform::is_standard")]
    pub display: media::DisplayTransform,
}
impl Composition {
    pub fn ocio_transforms(&self) -> impl Iterator<Item = &color_management::OcioTransform> {
        self.nodes
            .values()
            .filter_map(|node| {
                if let Operation::Ocio(node) = &node.operation {
                    Some(&node.transform)
                } else {
                    None
                }
            })
            .chain(self.ocio.view.iter())
            .chain(self.ocio.output.iter())
    }
    pub fn ocio_transforms_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut color_management::OcioTransform> {
        self.nodes
            .values_mut()
            .filter_map(|node| {
                if let Operation::Ocio(node) = &mut node.operation {
                    Some(&mut node.transform)
                } else {
                    None
                }
            })
            .chain(self.ocio.view.iter_mut())
            .chain(self.ocio.output.iter_mut())
    }

    pub fn output_display(&self) -> media::DisplayTransform {
        if self.ocio.output.is_some() {
            media::DisplayTransform::Standard
        } else {
            self.display
        }
    }
    /// Canonical pretty-printed JSON size, bounded without allocating the JSON.
    pub fn encoded_size(&self) -> Result<usize> {
        limits::encoded_size(self)
    }

    /// Validates the entire document, including disconnected nodes, without recursion.
    pub fn validate(&self) -> Result<Vec<NodeId>> {
        self.ocio.validate()?;
        if self.schema != 1 {
            return fail("unsupported composition schema");
        }
        if self.width == 0 || self.height == 0 || self.width > 8192 || self.height > 8192 {
            return fail("dimensions must be in 1..=8192");
        }
        if !(1..=MAX_FRAME_COUNT).contains(&self.frame_count) {
            return fail("frame count must be in 1..=1000000");
        }
        if self.frame_rate.seconds()? <= 0.0 {
            return fail("frame rate must be positive");
        }
        if self.nodes.len() > limits::MAX_NODES {
            return fail("composition exceeds 4096 nodes");
        }
        if !self.nodes.contains_key(&self.output) {
            return fail("output node is missing");
        }
        if self.nodes[&self.output].operation.descriptor().output != registry::PortKind::Image {
            return fail("The output must be an image; a Light rig only lights surfaces");
        }
        self.encoded_size()?;
        expression::check(self)?;
        let mut pending = BTreeMap::new();
        let mut consumers: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        for (id, node) in &self.nodes {
            if *id != node.id || node.version != node.operation.descriptor().version {
                return fail(format!("invalid identity or version for {id}"));
            }
            node.operation.validate()?;
            if let Some(position) = node.position {
                position.validate()?;
            }
            let descriptor = node.operation.descriptor();
            let ports: BTreeSet<_> = descriptor.inputs.iter().copied().collect();
            let required: BTreeSet<_> = descriptor.required().collect();
            let actual: BTreeSet<_> = node.inputs.keys().map(String::as_str).collect();
            if !actual.is_subset(&ports) || !required.is_subset(&actual) {
                return fail(format!(
                    "{}: expected input ports {required:?} (optional {:?}), got {actual:?}",
                    node.label, descriptor.optional
                ));
            }
            for (port, source) in &node.inputs {
                let kind = descriptor.port_kind(port);
                if self
                    .nodes
                    .get(source)
                    .is_some_and(|s| s.operation.descriptor().output != kind)
                {
                    return fail(match kind {
                        registry::PortKind::Lights => {
                            format!("{}: the {port} input takes a Light rig", node.label)
                        }
                        registry::PortKind::Scene => format!(
                            "{}: the {port} input takes a 3D scene (an object, camera, light, 3D transform or Merge scenes)",
                            node.label
                        ),
                        registry::PortKind::Image => format!(
                            "{}: the {port} input takes an image, not a light rig or 3D scene",
                            node.label
                        ),
                    });
                }
            }
            if matches!(
                node.operation,
                Operation::PlanarWarp(crate::nodes::PlanarWarp { .. })
            ) && !node
                .inputs
                .get("track")
                .and_then(|id| self.nodes.get(id))
                .is_some_and(|n| {
                    matches!(
                        n.operation,
                        Operation::PlanarTrack(crate::nodes::PlanarTrack { .. })
                    ) && !n.bypass
                })
            {
                return fail("Planar warp needs an enabled Planar track on its track input");
            }
            if let (Operation::Text3d(_), Some(outline)) =
                (&node.operation, node.inputs.get("outline"))
                && !self
                    .nodes
                    .get(outline)
                    .is_some_and(|n| matches!(n.operation, Operation::Text(_) | Operation::Path(_)))
            {
                return fail("3D text extrudes a Text or Path node on its Outline input");
            }
            if let (Operation::LightRig(_), Some(track)) =
                (&node.operation, node.inputs.get("track"))
                && !self
                    .nodes
                    .get(track)
                    .is_some_and(|n| matches!(n.operation, Operation::PlanarTrack(_)) && !n.bypass)
            {
                return fail("A Light rig's track input takes an enabled Planar track");
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
        groups::validate(self)?;
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
        file.take(limits::MAX_DOCUMENT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| Error(e.to_string()))?;
        if bytes.len() > limits::MAX_DOCUMENT_BYTES {
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
    atomic_write_with(path, |file| file.write_all(data))
}
/// Publish a complete new file without replacing an existing file or symlink.
pub fn atomic_create(path: &Path, data: &[u8]) -> Result<()> {
    use std::io::Write;
    atomic_publish_with(path, false, |file| file.write_all(data))
}
/// Stream a complete replacement through a same-directory temporary file.
/// The callback must finish and flush its buffers before returning success.
pub fn atomic_write_with(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> Result<()> {
    atomic_publish_with(path, true, write)
}
/// Stream a new file without replacing an existing file or symlink.
pub fn atomic_create_with(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> Result<()> {
    atomic_publish_with(path, false, write)
}
fn atomic_publish_with(
    path: &Path,
    replace: bool,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> Result<()> {
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
        write(&mut file)?;
        file.sync_all()?;
        if replace {
            std::fs::rename(&temp, path)?;
        } else {
            std::fs::hard_link(&temp, path)?;
            std::fs::remove_file(&temp)?;
        }
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
    PutGroup {
        group: Box<groups::Group>,
    },
    Ungroup {
        id: NodeId,
    },
    SetControl {
        id: NodeId,
        control: String,
        value: serde_json::Value,
    },
    ConnectGroup {
        id: NodeId,
        input: String,
        source: NodeId,
    },
    Put {
        node: Box<Node>,
    },
    SetParameter {
        id: NodeId,
        parameter: String,
        value: serde_json::Value,
    },
    SetKey {
        id: NodeId,
        parameter: String,
        time: Time,
        value: f32,
    },
    RemoveKey {
        id: NodeId,
        parameter: String,
        time: Time,
    },
    /// Key a whole colour or vector (see `track::Property` for paths).
    SetTrackKey {
        id: NodeId,
        track: String,
        time: Time,
        value: [f32; 3],
    },
    Remove {
        id: NodeId,
    },
    Output {
        id: NodeId,
    },
    Resize {
        width: u32,
        height: u32,
    },
    Place {
        id: NodeId,
        position: Option<NodePosition>,
    },
    FrameRate {
        frame_rate: Time,
    },
    FrameCount {
        frame_count: u32,
    },
    OcioViews {
        settings: color_management::OcioViews,
    },
    WorkingSpace {
        space: color_management::WorkingSpace,
    },
    Display {
        display: media::DisplayTransform,
    },
}

/// A transparent composition with one editable background generator.
pub fn blank(width: u32, height: u32, frame_rate: Time) -> Result<Composition> {
    let node = Node::new(
        "Background",
        Operation::Solid(crate::nodes::Solid { color: [0.0; 4] }),
        &[],
    );
    let composition = Composition {
        ocio: Default::default(),
        working_space: Default::default(),
        display: Default::default(),
        groups: Default::default(),
        schema: 1,
        frame_count: DEFAULT_FRAME_COUNT,
        width,
        height,
        frame_rate,
        output: node.id,
        nodes: [(node.id, node)].into(),
    };
    composition.validate()?;
    Ok(composition)
}

/// A media source key's file path and layer (see `Operation::source_key`).
pub fn split_source_key(key: &str) -> (&str, Option<&str>) {
    match key.split_once('\0') {
        Some((path, layer)) => (path, Some(layer)),
        None => (key, None),
    }
}
pub fn demo() -> Composition {
    let background = Node::new(
        "Background",
        Operation::Solid(crate::nodes::Solid {
            color: [0.015, 0.025, 0.06, 1.0],
        }),
        &[],
    );
    let noise = Node::new(
        "Noise",
        Operation::Noise(crate::nodes::Noise {
            seed: 42,
            scale: 18.0.into(),
        }),
        &[],
    );
    let exposure = Node::new(
        "Animated exposure",
        Operation::Exposure(crate::nodes::Exposure {
            stops: Scalar::Linear {
                keys: vec![
                    Key {
                        segment: None,
                        time: Time { ticks: 0, rate: 1 },
                        value: -2.0,
                    },
                    Key {
                        segment: None,
                        time: Time { ticks: 2, rate: 1 },
                        value: 2.0,
                    },
                ],
            },
        }),
        &[("image", noise.id)],
    );
    let blur = Node::new(
        "Soft glow",
        Operation::Blur(crate::nodes::Blur {
            radius: 4.0.into(),
            filter: Default::default(),
        }),
        &[("image", exposure.id)],
    );
    let transform = Node::new(
        "Offset",
        Operation::Transform(crate::nodes::Transform {
            x: 80.0.into(),
            y: 40.0.into(),
        }),
        &[("image", blur.id)],
    );
    let merge = Node::new(
        "Composite",
        Operation::Merge(crate::nodes::Merge {}),
        &[("background", background.id), ("foreground", transform.id)],
    );
    Composition {
        ocio: Default::default(),
        working_space: Default::default(),
        display: Default::default(),
        groups: Default::default(),
        schema: 1,
        frame_count: DEFAULT_FRAME_COUNT,
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

/// Editable motion-graphics composition using masks, HDR color, and affine animation.
pub fn motion_demo() -> Composition {
    let mut nodes = Vec::new();
    let mut add = |label: &str, operation: Operation, inputs: &[(&str, NodeId)]| {
        let node = Node::new(label, operation, inputs);
        let id = node.id;
        nodes.push(node);
        id
    };
    let mut output = add(
        "Midnight",
        Operation::Solid(crate::nodes::Solid {
            color: [0.004, 0.008, 0.022, 1.0],
        }),
        &[],
    );
    for (name, color, x, y, rx, ry, feather, angle) in [
        (
            "Blue atmosphere",
            [0.05, 0.15, 1.2, 0.65],
            530.0,
            280.0,
            200.0,
            130.0,
            140.0,
            12.0,
        ),
        (
            "Rose ribbon",
            [1.8, 0.025, 0.3, 0.8],
            490.0,
            260.0,
            245.0,
            45.0,
            60.0,
            32.0,
        ),
        (
            "Cyan ribbon",
            [0.015, 1.2, 1.8, 0.85],
            460.0,
            270.0,
            280.0,
            38.0,
            45.0,
            -27.0,
        ),
        (
            "Silver core",
            [1.8, 2.4, 2.8, 0.9],
            440.0,
            260.0,
            180.0,
            5.0,
            8.0,
            -27.0,
        ),
    ] {
        let color = add(
            &format!("{name} color"),
            Operation::Solid(crate::nodes::Solid { color }),
            &[],
        );
        let shape = add(
            &format!("{name} shape"),
            Operation::Ellipse(crate::nodes::Ellipse {
                x: x.into(),
                y: y.into(),
                radius_x: rx.into(),
                radius_y: ry.into(),
                feather: feather.into(),
            }),
            &[],
        );
        let masked = add(
            name,
            Operation::Mask(crate::nodes::Mask {}),
            &[("image", color), ("mask", shape)],
        );
        let motion = add(
            &format!("{name} motion"),
            Operation::Affine(crate::nodes::Affine {
                x: 0.0.into(),
                y: 0.0.into(),
                scale_x: 1.0.into(),
                scale_y: 1.0.into(),
                rotation: Scalar::Smooth {
                    keys: vec![
                        Key {
                            segment: None,
                            time: Time { ticks: 0, rate: 1 },
                            value: angle,
                        },
                        Key {
                            segment: None,
                            time: Time { ticks: 2, rate: 1 },
                            value: -angle,
                        },
                        Key {
                            segment: None,
                            time: Time { ticks: 4, rate: 1 },
                            value: angle,
                        },
                    ],
                },
                pivot_x: 480.0.into(),
                pivot_y: 270.0.into(),
            }),
            &[("image", masked)],
        );
        output = add(
            &format!("{name} over"),
            Operation::Merge(crate::nodes::Merge {}),
            &[("background", output), ("foreground", motion)],
        );
    }
    let output = add(
        "Composite",
        Operation::Exposure(crate::nodes::Exposure {
            stops: (-0.5).into(),
        }),
        &[("image", output)],
    );
    Composition {
        ocio: Default::default(),
        working_space: Default::default(),
        display: Default::default(),
        groups: Default::default(),
        schema: 1,
        frame_count: 96,
        width: 960,
        height: 540,
        frame_rate: Time { ticks: 24, rate: 1 },
        nodes: nodes.into_iter().map(|node| (node.id, node)).collect(),
        output,
    }
}

#[cfg(test)]
mod node_struct_tests {
    use super::*;
    #[test]
    fn node_structs_expose_exactly_the_registered_scalars() {
        for entry in registry::catalog()["nodes"].as_array().unwrap() {
            let op: Operation = serde_json::from_value(entry["default_operation"].clone()).unwrap();
            let descriptor = op.descriptor();
            for parameter in descriptor.parameters {
                let registered = matches!(parameter.kind, registry::ParameterKind::Scalar);
                assert_eq!(
                    op.scalar(parameter.name).is_some(),
                    registered,
                    "{} {}",
                    descriptor.type_id,
                    parameter.name
                );
            }
            assert!(op.scalar("bogus").is_none());
            op.validate().unwrap();
        }
    }
}
