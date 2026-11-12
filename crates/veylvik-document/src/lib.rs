//! Serializable compositions and transactional editing, independent of GPU and UI.
pub mod editing;
pub mod groups;
mod history;
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
pub use appearance::{AtmosphereKind, MaterialKind, PatinaKind, ReflectionKind};
mod gradient;
pub use gradient::GradientMode;
pub mod schema;
pub mod text;
pub use text::{TextAlignment, TextFont};
pub mod sequence;
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
pub mod key_edit;
pub use animation::{CompiledScalar, Handle, Interpolation, Key, Scalar, Segment, Time};
/// All image ports carry scene-linear Rec.709, premultiplied RGBA float pixels.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Text {
        content: String,
        font: TextFont,
        alignment: TextAlignment,
        rtl: bool,
        language: String,
        size: Scalar,
        x: Scalar,
        y: Scalar,
        tracking: Scalar,
        line_height: Scalar,
        rotation: Scalar,
        scale_x: Scalar,
        scale_y: Scalar,
        wrap_width: Scalar,
        underline: bool,
        features: String,
        color: [f32; 4],
    },
    Solid {
        color: [f32; 4],
    },
    Colorize {
        low: [f32; 4],
        high: [f32; 4],
    },
    Gradient {
        mode: GradientMode,
        start_x: Scalar,
        start_y: Scalar,
        end_x: Scalar,
        end_y: Scalar,
    },
    Noise {
        seed: u32,
        scale: Scalar,
    },
    SurfaceReflection {
        kind: ReflectionKind,
        bevel: Scalar,
        curvature: Scalar,
        roughness: Scalar,
        strength: Scalar,
        rotation: Scalar,
        scale: Scalar,
        offset_x: Scalar,
        offset_y: Scalar,
    },
    Glass {
        bevel: Scalar,
        refraction: Scalar,
        roughness: Scalar,
        ior: Scalar,
        transmission: Scalar,
        tint: Scalar,
        dispersion: Scalar,
    },
    SunGlints {
        seed: u32,
        size: Scalar,
        intensity: Scalar,
        threshold: Scalar,
        evolution: Scalar,
        density: Scalar,
        angle: Scalar,
    },
    Patina {
        kind: PatinaKind,
        seed: u32,
        amount: Scalar,
        dirt: Scalar,
        scale: Scalar,
        wear: Scalar,
        scratches: Scalar,
    },
    Material {
        kind: MaterialKind,
        scale: Scalar,
        bevel: Scalar,
        roughness: Scalar,
        lighting: Scalar,
        grain: Scalar,
        emission: Scalar,
    },
    Atmosphere {
        kind: AtmosphereKind,
        seed: u32,
        x: Scalar,
        y: Scalar,
        width: Scalar,
        height: Scalar,
        evolution: Scalar,
        density: Scalar,
        detail: Scalar,
        lighting: Scalar,
    },
    MetallicShine {
        x: Scalar,
        y: Scalar,
        width: Scalar,
        angle: Scalar,
        intensity: Scalar,
    },
    Fire {
        seed: u32,
        x: Scalar,
        y: Scalar,
        width: Scalar,
        height: Scalar,
        evolution: Scalar,
        intensity: Scalar,
        turbulence: Scalar,
    },
    FractalNoise {
        seed: u32,
        scale: Scalar,
        octaves: Scalar,
        roughness: Scalar,
        evolution: Scalar,
    },
    Exposure {
        stops: Scalar,
    },
    Transform {
        x: Scalar,
        y: Scalar,
    },
    Displace {
        x: Scalar,
        y: Scalar,
    },
    Morphology {
        radius: Scalar,
    },
    Blur {
        radius: Scalar,
    },
    Merge {},
    Blend {
        mode: BlendMode,
        opacity: Scalar,
    },
    Image {
        asset: String,
    },
    Affine {
        x: Scalar,
        y: Scalar,
        scale_x: Scalar,
        scale_y: Scalar,
        rotation: Scalar,
        pivot_x: Scalar,
        pivot_y: Scalar,
    },
    Ellipse {
        x: Scalar,
        y: Scalar,
        radius_x: Scalar,
        radius_y: Scalar,
        feather: Scalar,
    },
    Mask {},
    CombineMask {
        mode: MaskMode,
    },
    ChannelMask {
        channel: MaskChannel,
    },
    Path {
        geometry: vector::PathMask,
    },
}
impl Operation {
    pub fn descriptor(&self) -> &'static registry::NodeType {
        match self {
            Self::Text { .. } => &registry::TEXT,
            Self::MetallicShine { .. } => &registry::METALLIC_SHINE,
            Self::SurfaceReflection { .. } => &registry::SURFACE_REFLECTION,
            Self::Glass { .. } => &registry::GLASS,
            Self::SunGlints { .. } => &registry::SUN_GLINTS,
            Self::Patina { .. } => &registry::PATINA,
            Self::Material { .. } => &registry::MATERIAL,
            Self::Atmosphere { .. } => &registry::ATMOSPHERE,
            Self::Fire { .. } => &registry::FIRE,
            Self::Solid { .. } => &registry::SOLID,
            Self::Colorize { .. } => &registry::COLORIZE,
            Self::FractalNoise { .. } => &registry::FRACTAL_NOISE,
            Self::Gradient { .. } => &registry::GRADIENT,
            Self::Noise { .. } => &registry::NOISE,
            Self::Exposure { .. } => &registry::EXPOSURE,
            Self::Transform { .. } => &registry::TRANSFORM,
            Self::Displace { .. } => &registry::DISPLACE,
            Self::Morphology { .. } => &registry::MORPHOLOGY,
            Self::Blur { .. } => &registry::BLUR,
            Self::Merge {} => &registry::MERGE,
            Self::Blend { .. } => &registry::BLEND,
            Self::Image { .. } => &registry::IMAGE,
            Self::Affine { .. } => &registry::AFFINE,
            Self::Ellipse { .. } => &registry::ELLIPSE,
            Self::ChannelMask { .. } => &registry::CHANNEL_MASK,
            Self::CombineMask { .. } => &registry::COMBINE_MASK,
            Self::Mask {} => &registry::MASK,
            Self::Path { .. } => &registry::PATH,
        }
    }
    pub fn inputs(&self) -> &'static [&'static str] {
        self.descriptor().inputs
    }
    pub fn scalar(&self, name: &str) -> Option<&Scalar> {
        match (self, name) {
            (Self::Noise { scale, .. } | Self::FractalNoise { scale, .. }, "scale") => Some(scale),
            (Self::MetallicShine { x, .. }, "x") => Some(x),
            (Self::MetallicShine { y, .. }, "y") => Some(y),
            (Self::MetallicShine { width, .. }, "width") => Some(width),
            (Self::MetallicShine { angle, .. }, "angle") => Some(angle),
            (Self::MetallicShine { intensity, .. }, "intensity") => Some(intensity),
            (Self::SurfaceReflection { bevel, .. }, "bevel") => Some(bevel),
            (Self::SurfaceReflection { curvature, .. }, "curvature") => Some(curvature),
            (Self::SurfaceReflection { roughness, .. }, "roughness") => Some(roughness),
            (Self::SurfaceReflection { strength, .. }, "strength") => Some(strength),
            (Self::SurfaceReflection { rotation, .. }, "rotation") => Some(rotation),
            (Self::SurfaceReflection { scale, .. }, "scale") => Some(scale),
            (Self::SurfaceReflection { offset_x, .. }, "offset_x") => Some(offset_x),
            (Self::SurfaceReflection { offset_y, .. }, "offset_y") => Some(offset_y),
            (Self::Glass { bevel, .. }, "bevel") => Some(bevel),
            (Self::Glass { refraction, .. }, "refraction") => Some(refraction),
            (Self::Glass { roughness, .. }, "roughness") => Some(roughness),
            (Self::Glass { ior, .. }, "ior") => Some(ior),
            (Self::Glass { transmission, .. }, "transmission") => Some(transmission),
            (Self::Glass { tint, .. }, "tint") => Some(tint),
            (Self::Glass { dispersion, .. }, "dispersion") => Some(dispersion),
            (Self::SunGlints { size, .. }, "size") => Some(size),
            (Self::SunGlints { intensity, .. }, "intensity") => Some(intensity),
            (Self::SunGlints { threshold, .. }, "threshold") => Some(threshold),
            (Self::SunGlints { evolution, .. }, "evolution") => Some(evolution),
            (Self::SunGlints { density, .. }, "density") => Some(density),
            (Self::SunGlints { angle, .. }, "angle") => Some(angle),
            (Self::Patina { amount, .. }, "amount") => Some(amount),
            (Self::Patina { dirt, .. }, "dirt") => Some(dirt),
            (Self::Patina { scale, .. }, "scale") => Some(scale),
            (Self::Patina { wear, .. }, "wear") => Some(wear),
            (Self::Patina { scratches, .. }, "scratches") => Some(scratches),
            (Self::Text { rotation, .. }, "rotation") => Some(rotation),
            (Self::Text { scale_x, .. }, "scale_x") => Some(scale_x),
            (Self::Text { scale_y, .. }, "scale_y") => Some(scale_y),
            (Self::Text { wrap_width, .. }, "wrap_width") => Some(wrap_width),
            (Self::Text { size, .. }, "size") => Some(size),
            (Self::Text { x, .. }, "x") => Some(x),
            (Self::Text { y, .. }, "y") => Some(y),
            (Self::Text { tracking, .. }, "tracking") => Some(tracking),
            (Self::Text { line_height, .. }, "line_height") => Some(line_height),
            (Self::Material { scale, .. }, "scale") => Some(scale),
            (Self::Material { bevel, .. }, "bevel") => Some(bevel),
            (Self::Material { roughness, .. }, "roughness") => Some(roughness),
            (Self::Material { lighting, .. }, "lighting") => Some(lighting),
            (Self::Material { grain, .. }, "grain") => Some(grain),
            (Self::Material { emission, .. }, "emission") => Some(emission),
            (Self::Atmosphere { x, .. }, "x") => Some(x),
            (Self::Atmosphere { y, .. }, "y") => Some(y),
            (Self::Atmosphere { width, .. }, "width") => Some(width),
            (Self::Atmosphere { height, .. }, "height") => Some(height),
            (Self::Atmosphere { evolution, .. }, "evolution") => Some(evolution),
            (Self::Atmosphere { density, .. }, "density") => Some(density),
            (Self::Atmosphere { detail, .. }, "detail") => Some(detail),
            (Self::Atmosphere { lighting, .. }, "lighting") => Some(lighting),
            (Self::Fire { x, .. }, "x") => Some(x),
            (Self::Fire { y, .. }, "y") => Some(y),
            (Self::Fire { width, .. }, "width") => Some(width),
            (Self::Fire { height, .. }, "height") => Some(height),
            (Self::Fire { evolution, .. }, "evolution") => Some(evolution),
            (Self::Fire { intensity, .. }, "intensity") => Some(intensity),
            (Self::Fire { turbulence, .. }, "turbulence") => Some(turbulence),
            (Self::FractalNoise { octaves, .. }, "octaves") => Some(octaves),
            (Self::FractalNoise { roughness, .. }, "roughness") => Some(roughness),
            (Self::FractalNoise { evolution, .. }, "evolution") => Some(evolution),
            (Self::Gradient { start_x, .. }, "start_x") => Some(start_x),
            (Self::Gradient { start_y, .. }, "start_y") => Some(start_y),
            (Self::Gradient { end_x, .. }, "end_x") => Some(end_x),
            (Self::Gradient { end_y, .. }, "end_y") => Some(end_y),
            (Self::Exposure { stops }, "stops") => Some(stops),
            (Self::Transform { x, .. } | Self::Displace { x, .. }, "x") => Some(x),
            (Self::Transform { y, .. } | Self::Displace { y, .. }, "y") => Some(y),
            (Self::Blur { radius } | Self::Morphology { radius }, "radius") => Some(radius),
            (Self::Blend { opacity, .. }, "opacity") => Some(opacity),
            (Self::Affine { x, .. } | Self::Ellipse { x, .. }, "x") => Some(x),
            (Self::Affine { y, .. } | Self::Ellipse { y, .. }, "y") => Some(y),
            (Self::Affine { scale_x, .. }, "scale_x") => Some(scale_x),
            (Self::Affine { scale_y, .. }, "scale_y") => Some(scale_y),
            (Self::Affine { rotation, .. }, "rotation") => Some(rotation),
            (Self::Affine { pivot_x, .. }, "pivot_x") => Some(pivot_x),
            (Self::Affine { pivot_y, .. }, "pivot_y") => Some(pivot_y),
            (Self::Ellipse { radius_x, .. }, "radius_x") => Some(radius_x),
            (Self::Ellipse { radius_y, .. }, "radius_y") => Some(radius_y),
            (Self::Ellipse { feather, .. }, "feather") => Some(feather),
            _ => None,
        }
    }
    pub fn scalar_mut(&mut self, name: &str) -> Option<&mut Scalar> {
        match (self, name) {
            (Self::Noise { scale, .. } | Self::FractalNoise { scale, .. }, "scale") => Some(scale),
            (Self::MetallicShine { x, .. }, "x") => Some(x),
            (Self::MetallicShine { y, .. }, "y") => Some(y),
            (Self::MetallicShine { width, .. }, "width") => Some(width),
            (Self::MetallicShine { angle, .. }, "angle") => Some(angle),
            (Self::MetallicShine { intensity, .. }, "intensity") => Some(intensity),
            (Self::SurfaceReflection { bevel, .. }, "bevel") => Some(bevel),
            (Self::SurfaceReflection { curvature, .. }, "curvature") => Some(curvature),
            (Self::SurfaceReflection { roughness, .. }, "roughness") => Some(roughness),
            (Self::SurfaceReflection { strength, .. }, "strength") => Some(strength),
            (Self::SurfaceReflection { rotation, .. }, "rotation") => Some(rotation),
            (Self::SurfaceReflection { scale, .. }, "scale") => Some(scale),
            (Self::SurfaceReflection { offset_x, .. }, "offset_x") => Some(offset_x),
            (Self::SurfaceReflection { offset_y, .. }, "offset_y") => Some(offset_y),
            (Self::Glass { bevel, .. }, "bevel") => Some(bevel),
            (Self::Glass { refraction, .. }, "refraction") => Some(refraction),
            (Self::Glass { roughness, .. }, "roughness") => Some(roughness),
            (Self::Glass { ior, .. }, "ior") => Some(ior),
            (Self::Glass { transmission, .. }, "transmission") => Some(transmission),
            (Self::Glass { tint, .. }, "tint") => Some(tint),
            (Self::Glass { dispersion, .. }, "dispersion") => Some(dispersion),
            (Self::SunGlints { size, .. }, "size") => Some(size),
            (Self::SunGlints { intensity, .. }, "intensity") => Some(intensity),
            (Self::SunGlints { threshold, .. }, "threshold") => Some(threshold),
            (Self::SunGlints { evolution, .. }, "evolution") => Some(evolution),
            (Self::SunGlints { density, .. }, "density") => Some(density),
            (Self::SunGlints { angle, .. }, "angle") => Some(angle),
            (Self::Patina { amount, .. }, "amount") => Some(amount),
            (Self::Patina { dirt, .. }, "dirt") => Some(dirt),
            (Self::Patina { scale, .. }, "scale") => Some(scale),
            (Self::Patina { wear, .. }, "wear") => Some(wear),
            (Self::Patina { scratches, .. }, "scratches") => Some(scratches),
            (Self::Text { rotation, .. }, "rotation") => Some(rotation),
            (Self::Text { scale_x, .. }, "scale_x") => Some(scale_x),
            (Self::Text { scale_y, .. }, "scale_y") => Some(scale_y),
            (Self::Text { wrap_width, .. }, "wrap_width") => Some(wrap_width),
            (Self::Text { size, .. }, "size") => Some(size),
            (Self::Text { x, .. }, "x") => Some(x),
            (Self::Text { y, .. }, "y") => Some(y),
            (Self::Text { tracking, .. }, "tracking") => Some(tracking),
            (Self::Text { line_height, .. }, "line_height") => Some(line_height),
            (Self::Material { scale, .. }, "scale") => Some(scale),
            (Self::Material { bevel, .. }, "bevel") => Some(bevel),
            (Self::Material { roughness, .. }, "roughness") => Some(roughness),
            (Self::Material { lighting, .. }, "lighting") => Some(lighting),
            (Self::Material { grain, .. }, "grain") => Some(grain),
            (Self::Material { emission, .. }, "emission") => Some(emission),
            (Self::Atmosphere { x, .. }, "x") => Some(x),
            (Self::Atmosphere { y, .. }, "y") => Some(y),
            (Self::Atmosphere { width, .. }, "width") => Some(width),
            (Self::Atmosphere { height, .. }, "height") => Some(height),
            (Self::Atmosphere { evolution, .. }, "evolution") => Some(evolution),
            (Self::Atmosphere { density, .. }, "density") => Some(density),
            (Self::Atmosphere { detail, .. }, "detail") => Some(detail),
            (Self::Atmosphere { lighting, .. }, "lighting") => Some(lighting),
            (Self::Fire { x, .. }, "x") => Some(x),
            (Self::Fire { y, .. }, "y") => Some(y),
            (Self::Fire { width, .. }, "width") => Some(width),
            (Self::Fire { height, .. }, "height") => Some(height),
            (Self::Fire { evolution, .. }, "evolution") => Some(evolution),
            (Self::Fire { intensity, .. }, "intensity") => Some(intensity),
            (Self::Fire { turbulence, .. }, "turbulence") => Some(turbulence),
            (Self::FractalNoise { octaves, .. }, "octaves") => Some(octaves),
            (Self::FractalNoise { roughness, .. }, "roughness") => Some(roughness),
            (Self::FractalNoise { evolution, .. }, "evolution") => Some(evolution),
            (Self::Gradient { start_x, .. }, "start_x") => Some(start_x),
            (Self::Gradient { start_y, .. }, "start_y") => Some(start_y),
            (Self::Gradient { end_x, .. }, "end_x") => Some(end_x),
            (Self::Gradient { end_y, .. }, "end_y") => Some(end_y),
            (Self::Exposure { stops }, "stops") => Some(stops),
            (Self::Transform { x, .. } | Self::Displace { x, .. }, "x") => Some(x),
            (Self::Transform { y, .. } | Self::Displace { y, .. }, "y") => Some(y),
            (Self::Blur { radius } | Self::Morphology { radius }, "radius") => Some(radius),
            (Self::Blend { opacity, .. }, "opacity") => Some(opacity),
            (Self::Affine { x, .. } | Self::Ellipse { x, .. }, "x") => Some(x),
            (Self::Affine { y, .. } | Self::Ellipse { y, .. }, "y") => Some(y),
            (Self::Affine { scale_x, .. }, "scale_x") => Some(scale_x),
            (Self::Affine { scale_y, .. }, "scale_y") => Some(scale_y),
            (Self::Affine { rotation, .. }, "rotation") => Some(rotation),
            (Self::Affine { pivot_x, .. }, "pivot_x") => Some(pivot_x),
            (Self::Affine { pivot_y, .. }, "pivot_y") => Some(pivot_y),
            (Self::Ellipse { radius_x, .. }, "radius_x") => Some(radius_x),
            (Self::Ellipse { radius_y, .. }, "radius_y") => Some(radius_y),
            (Self::Ellipse { feather, .. }, "feather") => Some(feather),
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
            Self::Text {
                content,
                font,
                language,
                features,
                color,
                ..
            } => {
                text::validate(content, font, language)?;
                if features.len() > 256 || !features.is_ascii() {
                    return fail("OpenType feature list must be ASCII, at most 256 bytes");
                }
                if color.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&color[3]) {
                    return fail("Text colour needs finite RGB and alpha in [0,1]");
                }
                Ok(())
            }
            Self::Path { geometry } => geometry.validate(),
            Self::Solid { color }
                if color.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&color[3]) =>
            {
                fail("solid needs finite RGB and alpha in [0,1]")
            }
            Self::Colorize { low, high }
                if [low, high].iter().any(|color| {
                    color.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&color[3])
                }) =>
            {
                fail("colorize needs finite RGB and alpha in [0,1] for both endpoints")
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
}
impl Composition {
    /// Canonical pretty-printed JSON size, bounded without allocating the JSON.
    pub fn encoded_size(&self) -> Result<usize> {
        limits::encoded_size(self)
    }

    /// Validates the entire document, including disconnected nodes, without recursion.
    pub fn validate(&self) -> Result<Vec<NodeId>> {
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
        self.encoded_size()?;
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
}

/// A transparent composition with one editable background generator.
pub fn blank(width: u32, height: u32, frame_rate: Time) -> Result<Composition> {
    let node = Node::new("Background", Operation::Solid { color: [0.0; 4] }, &[]);
    let composition = Composition {
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
        Operation::Merge {},
        &[("background", background.id), ("foreground", transform.id)],
    );
    Composition {
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
        Operation::Solid {
            color: [0.004, 0.008, 0.022, 1.0],
        },
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
        let color = add(&format!("{name} color"), Operation::Solid { color }, &[]);
        let shape = add(
            &format!("{name} shape"),
            Operation::Ellipse {
                x: x.into(),
                y: y.into(),
                radius_x: rx.into(),
                radius_y: ry.into(),
                feather: feather.into(),
            },
            &[],
        );
        let masked = add(
            name,
            Operation::Mask {},
            &[("image", color), ("mask", shape)],
        );
        let motion = add(
            &format!("{name} motion"),
            Operation::Affine {
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
            },
            &[("image", masked)],
        );
        output = add(
            &format!("{name} over"),
            Operation::Merge {},
            &[("background", output), ("foreground", motion)],
        );
    }
    let output = add(
        "Composite",
        Operation::Exposure {
            stops: (-0.5).into(),
        },
        &[("image", output)],
    );
    Composition {
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
