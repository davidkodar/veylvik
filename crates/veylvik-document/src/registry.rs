//! Authoritative ports, parameter bounds, defaults and JSON schemas for built-in nodes.
use serde::Serialize;
use serde_json::{Value, json};

pub const MAX_ASSET_NAME: usize = 4096;
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterKind {
    FrameRate,
    FlowQuality,
    BlurFilter,
    OcioTransform,
    Brushing,
    StudioRig,
    SurfaceOptics,
    Palette,
    PlanarTrack,
    PlanarMode,
    Scalar,
    Typography,
    MediaTiming,
    FadeTiming,
    KeyColor,
    MediaPlacement,
    MediaColour,
    TextFeatures,
    TextContent,
    TextFont,
    TextAlignment,
    TextLanguage,
    Boolean,
    Seed,
    Color,
    Asset,
    Path,
    BlendMode,
    MaskChannel,
    GradientMode,
    MaskMode,
    AtmosphereKind,
    MaterialKind,
    PatinaKind,
    ReflectionKind,
    Placement,
    Colour3,
    ShapeKind,
    LightKind,
    Look,
    /// Another node by id, or null.
    NodeRef,
    /// A list of [x, y, z] points.
    Points3,
    /// A file path (string).
    FilePath,
    /// A plain [x, y, z].
    Vector3,
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Parameter {
    pub name: &'static str,
    pub kind: ParameterKind,
    pub unit: &'static str,
    pub min: Option<f32>,
    pub max: Option<f32>,
    #[serde(skip_serializing)]
    pub default: f32,
}
impl Parameter {
    fn default_value(&self) -> Value {
        match self.kind {
            ParameterKind::FrameRate => json!(crate::Time { ticks: 24, rate: 1 }),
            ParameterKind::FlowQuality => json!(crate::nodes::FlowQuality::Balanced),
            ParameterKind::BlurFilter => json!(crate::nodes::BlurFilter::Box),
            ParameterKind::OcioTransform => {
                json!(crate::color_management::OcioTransform::default())
            }
            ParameterKind::Brushing => json!(crate::optical::Brushing::default()),
            ParameterKind::StudioRig => json!(crate::optical::StudioRig::default()),
            ParameterKind::SurfaceOptics => json!(null),
            ParameterKind::Palette => json!(null),
            ParameterKind::PlanarTrack => json!(crate::planar::Track::default()),
            ParameterKind::PlanarMode => json!(crate::planar::WarpMode::Follow),
            ParameterKind::MediaPlacement => json!(crate::media::Placement::default()),
            ParameterKind::MediaColour => json!(crate::media::InputColour::default()),
            ParameterKind::MediaTiming => json!(crate::media::Timing::default()),
            ParameterKind::FadeTiming => json!(crate::nodes::FadeTiming::default()),
            ParameterKind::Typography => json!(crate::text::Typography::default()),
            ParameterKind::TextFeatures => json!(""),
            ParameterKind::TextContent => json!("Your text"),
            ParameterKind::TextFont => json!(crate::TextFont::default()),
            ParameterKind::TextAlignment => json!(crate::TextAlignment::Center),
            ParameterKind::TextLanguage => json!(""),
            ParameterKind::Boolean => json!(self.default != 0.0),
            ParameterKind::Scalar => json!({"kind":"constant","value":self.default}),
            ParameterKind::MaterialKind => json!(crate::MaterialKind::Gold),
            ParameterKind::AtmosphereKind => json!(crate::AtmosphereKind::Smoke),
            ParameterKind::ReflectionKind => json!(crate::ReflectionKind::Metal),
            ParameterKind::PatinaKind => json!(crate::PatinaKind::Tarnish),
            ParameterKind::Seed => json!(self.default as u32),
            ParameterKind::BlendMode => json!(crate::BlendMode::Normal),
            ParameterKind::MaskChannel => json!(crate::MaskChannel::Luminance),
            ParameterKind::GradientMode => json!(crate::GradientMode::Linear),
            ParameterKind::MaskMode => json!(crate::MaskMode::Union),
            ParameterKind::Color => json!([self.default, self.default, self.default, 1.0]),
            ParameterKind::KeyColor => json!([0.0, 1.0, 0.0, 1.0]),
            ParameterKind::Asset => json!("image.png"),
            ParameterKind::Path => {
                serde_json::to_value(crate::vector::PathMask::default()).unwrap()
            }
            ParameterKind::Placement => json!(crate::nodes::Placement::default()),
            ParameterKind::Colour3 => json!([0.6_f32, 0.6_f32, 0.6_f32]),
            ParameterKind::ShapeKind => json!(crate::nodes::ShapeKind::default()),
            ParameterKind::LightKind => json!(crate::nodes::LightKind::default()),
            ParameterKind::Look | ParameterKind::NodeRef => json!(null),
            ParameterKind::Points3 => json!([]),
            ParameterKind::FilePath => json!(""),
            ParameterKind::Vector3 => json!([1.0, 1.0, 1.0]),
        }
    }
    fn schema(&self) -> Value {
        match self.kind {
            ParameterKind::FrameRate => {
                json!({"type":"object","properties":{"ticks":{"type":"integer","minimum":1},"rate":{"type":"integer","minimum":1}},"required":["ticks","rate"],"additionalProperties":false})
            }
            ParameterKind::FlowQuality => {
                json!({"type":"string","enum":["fast","balanced","fine"]})
            }
            ParameterKind::BlurFilter => {
                json!({"type":"string","enum":crate::nodes::BlurFilter::ALL})
            }
            ParameterKind::OcioTransform => crate::color_management::OcioTransform::schema(),
            ParameterKind::Brushing => {
                json!({"type":"string","enum":["auto","smooth","directional"]})
            }
            ParameterKind::StudioRig => crate::optical::StudioRig::schema(),
            ParameterKind::SurfaceOptics => {
                json!({"anyOf":[{"type":"null"},crate::optical::SurfaceOptics::schema()]})
            }
            ParameterKind::Palette => crate::optical::palette_schema(),
            ParameterKind::Placement => crate::nodes::Placement::schema(),
            ParameterKind::Colour3 => crate::track::schema(0.0, 1.0),
            ParameterKind::ShapeKind => {
                json!({"type":"string","enum":crate::nodes::ShapeKind::ALL})
            }
            ParameterKind::LightKind => {
                json!({"type":"string","enum":crate::nodes::LightKind::ALL})
            }
            ParameterKind::Look => {
                json!({"anyOf":[{"type":"null"},{"type":"object","description":"A Text surface (material) definition: the parameters of a material node without its type."}]})
            }
            ParameterKind::FilePath => json!({"type":"string","maxLength":4096}),
            ParameterKind::Vector3 => {
                json!({"type":"array","minItems":3,"maxItems":3,"items":{"type":"number"}})
            }
            ParameterKind::Points3 => {
                json!({"type":"array","maxItems":crate::nodes::MAX_CLOUD_POINTS,"items":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"number"}}})
            }
            ParameterKind::NodeRef => {
                json!({"anyOf":[{"type":"null"},{"type":"string","format":"uuid"}]})
            }
            ParameterKind::PlanarTrack => crate::planar::schema(),
            ParameterKind::PlanarMode => {
                json!({"type":"string","enum":["follow","fit","stabilize"]})
            }
            ParameterKind::Typography => crate::text::typography_schema(),
            ParameterKind::TextFeatures => json!({"type":"string","maxLength":256}),
            ParameterKind::TextContent => json!({"type":"string","maxLength":4096}),
            ParameterKind::TextFont => crate::text::font_schema(),
            ParameterKind::TextAlignment => {
                json!({"type":"string","enum":["left","center","right"]})
            }
            ParameterKind::TextLanguage => json!({"type":"string","maxLength":64}),
            ParameterKind::Boolean => json!({"type":"boolean"}),
            ParameterKind::Scalar => scalar_schema(self.min, self.max),
            ParameterKind::BlendMode => json!({"type":"string","enum":crate::BlendMode::ALL}),
            ParameterKind::MaskChannel => json!({"type":"string","enum":crate::MaskChannel::ALL}),
            ParameterKind::GradientMode => json!({"type":"string","enum":crate::GradientMode::ALL}),
            ParameterKind::MaskMode => json!({"type":"string","enum":crate::MaskMode::ALL}),
            ParameterKind::MaterialKind => json!({"type":"string","enum":crate::MaterialKind::ALL}),
            ParameterKind::AtmosphereKind => {
                json!({"type":"string","enum":crate::AtmosphereKind::ALL})
            }
            ParameterKind::ReflectionKind => {
                json!({"type":"string","enum":crate::ReflectionKind::ALL})
            }
            ParameterKind::PatinaKind => json!({"type":"string","enum":crate::PatinaKind::ALL}),
            ParameterKind::Seed => json!({"type":"integer","minimum":0,"maximum":u32::MAX}),
            ParameterKind::KeyColor => {
                json!({"type":"array","minItems":4,"maxItems":4,"items":{"type":"number","minimum":0}})
            }
            ParameterKind::Color => {
                json!({"type":"array","minItems":4,"maxItems":4,"prefixItems":[{"type":"number"},{"type":"number"},{"type":"number"},{"type":"number","minimum":0,"maximum":1}]})
            }
            ParameterKind::Path => crate::vector::schema(),
            ParameterKind::MediaTiming => crate::media::schema(),
            ParameterKind::MediaColour => crate::media::InputColour::schema(),
            ParameterKind::FadeTiming => crate::nodes::fade_schema(),
            ParameterKind::MediaPlacement => {
                json!({"type":"string","enum":["legacy","fit","fill","original","stretch"]})
            }
            ParameterKind::Asset => {
                json!({"type":"string","minLength":1,"maxLength":MAX_ASSET_NAME})
            }
        }
    }
}
pub(crate) const fn scalar(
    name: &'static str,
    unit: &'static str,
    min: f32,
    max: f32,
    default: f32,
) -> Parameter {
    Parameter {
        name,
        kind: ParameterKind::Scalar,
        unit,
        min: Some(min),
        max: Some(max),
        default,
    }
}
pub(crate) const fn other(
    name: &'static str,
    kind: ParameterKind,
    unit: &'static str,
) -> Parameter {
    Parameter {
        name,
        kind,
        unit,
        min: None,
        max: None,
        default: 0.0,
    }
}
/// What a port carries. Images flow between effects; light rigs only reach
/// Lights inputs and never render as images.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PortKind {
    Image,
    Lights,
    /// Objects, cameras and lights in 3D; rendered back to an image by Render scene.
    Scene,
}
#[derive(Debug, Serialize)]
pub struct NodeType {
    pub type_id: &'static str,
    pub version: u32,
    pub label: &'static str,
    pub description: &'static str,
    /// Every input port, in socket order.
    pub inputs: &'static [&'static str],
    /// Ports among `inputs` that may stay unconnected; the node then behaves as
    /// it did before the port existed.
    pub optional: &'static [&'static str],
    /// What the node's output carries.
    pub output: PortKind,
    pub parameters: &'static [Parameter],
}
impl NodeType {
    /// What an input port accepts: the port named `lights` takes a light rig.
    pub fn port_kind(&self, port: &str) -> PortKind {
        if port == "lights" {
            PortKind::Lights
        } else if port == "scene" || port.starts_with("scene_") {
            PortKind::Scene
        } else {
            PortKind::Image
        }
    }
    pub fn is_optional(&self, port: &str) -> bool {
        self.optional.contains(&port)
    }
    /// Ports that must be connected.
    pub fn required(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.inputs.iter().copied().filter(|p| !self.is_optional(p))
    }
    pub fn default_operation(&self) -> Value {
        let mut value = json!({"type":self.type_id});
        for param in self.parameters {
            value[param.name] = param.default_value();
        }
        value
    }
    pub fn schema(&self) -> Value {
        let mut properties = json!({"type":{"const":self.type_id}});
        let mut required = vec!["type"];
        for param in self.parameters {
            properties[param.name] = param.schema();
            required.push(param.name);
        }
        json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
    }
}
// Each descriptor lives with its node struct in `crate::nodes`.
pub use crate::nodes::{
    AFFINE, ATMOSPHERE, BLEND, BLUR, CAMERA_3D, CHANNEL_MASK, CHROMA_KEY, COLOR_CORRECT, COLORIZE,
    COMBINE_MASK, CONDENSATION, DISPLACE, DRIP, DUST, ELLIPSE, ENVIRONMENT_3D, EXPOSURE, FADE,
    FIRE, FOOTAGE, FRACTAL_NOISE, GLASS, GRADIENT, IMAGE, IMAGE_CARD, LENS_BLUR, LENS_DISTORTION,
    LIGHT_3D, LIGHT_RIG, LUMA_KEY, LUT, MASK, MATCH_GRAIN, MATERIAL, MATERIAL_LAYERS, MERGE,
    MERGE_SCENES, METALLIC_SHINE, MODEL_3D, MORPHOLOGY, MOTION_BLUR, NOISE, OCIO, OPTICAL_FLOW,
    PATH, PATINA, PLANAR_TRACK, PLANAR_WARP, POINT_CLOUD, REFINE_MATTE, RENDER_SCENE, SHAPE_3D,
    SOLID, SUN_GLINTS, SURFACE_BLEND, SURFACE_REFLECTION, TEXT, TEXT_3D, TIME_REMAP, TRANSFORM,
    TRANSFORM_3D,
};
pub static TYPES: &[&NodeType] = &[
    &SOLID,
    &NOISE,
    &EXPOSURE,
    &COLOR_CORRECT,
    &LUT,
    &TRANSFORM,
    &BLUR,
    &LENS_BLUR,
    &LIGHT_RIG,
    &crate::nodes::SHADOW,
    &crate::nodes::CONTACT_SHADOW,
    &crate::nodes::SHAPE_3D,
    &crate::nodes::TEXT_3D,
    &crate::nodes::IMAGE_CARD,
    &crate::nodes::TRANSFORM_3D,
    &crate::nodes::MERGE_SCENES,
    &crate::nodes::CAMERA_3D,
    &crate::nodes::LIGHT_3D,
    &crate::nodes::RENDER_SCENE,
    &crate::nodes::ENVIRONMENT_3D,
    &crate::nodes::POINT_CLOUD,
    &crate::nodes::MATCH_GRAIN,
    &crate::nodes::MODEL_3D,
    &crate::nodes::LENS_DISTORTION,
    &MERGE,
    &IMAGE,
    &FOOTAGE,
    &MOTION_BLUR,
    &TIME_REMAP,
    &OPTICAL_FLOW,
    &OCIO,
    &AFFINE,
    &PLANAR_TRACK,
    &PLANAR_WARP,
    &ELLIPSE,
    &MASK,
    &TEXT,
    &PATH,
    &BLEND,
    &FADE,
    &CHROMA_KEY,
    &LUMA_KEY,
    &REFINE_MATTE,
    &SURFACE_BLEND,
    &DISPLACE,
    &FRACTAL_NOISE,
    &FIRE,
    &DUST,
    &DRIP,
    &MATERIAL,
    &CONDENSATION,
    &PATINA,
    &MATERIAL_LAYERS,
    &SUN_GLINTS,
    &GLASS,
    &SURFACE_REFLECTION,
    &ATMOSPHERE,
    &METALLIC_SHINE,
    &CHANNEL_MASK,
    &COLORIZE,
    &GRADIENT,
    &MORPHOLOGY,
    &COMBINE_MASK,
];
pub fn operation_schema() -> Value {
    json!({"oneOf":TYPES.iter().map(|node|node.schema()).collect::<Vec<_>>()})
}
pub fn catalog() -> Value {
    json!({"recipes":crate::recipes::catalog(),"nodes":TYPES.iter().map(|node|json!({"descriptor":node,"default_operation":node.default_operation(),"operation_schema":node.schema()})).collect::<Vec<_>>(),"image_contract":"scene-linear Rec.709, premultiplied RGBA32F","scalar_contract":"constant, linear, step, smooth or monotone keyframes; smooth uses cubic smoothstep per interval; monotone uses bounded cubic Hermite with shared harmonic interior slopes and secant endpoint slopes; time is ticks/rate seconds; 1..4096 strictly increasing keys"})
}

/// An exact rational time.
pub fn time_schema() -> Value {
    json!({"type":"object","properties":{"ticks":{"type":"integer","minimum":i64::MIN,"maximum":i64::MAX},"rate":{"type":"integer","minimum":1,"maximum":u32::MAX}},"required":["ticks","rate"],"additionalProperties":false})
}
/// A constant or keyed number within bounds.
pub fn scalar_schema(min: Option<f32>, max: Option<f32>) -> Value {
    let number = json!({"type":"number","minimum":min,"maximum":max});
    json!({"oneOf":[
        {"type":"object","properties":{"kind":{"const":"constant"},"value":number},"required":["kind","value"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"enum":["linear","step","smooth","monotone"]},"keys":{"type":"array","minItems":1,"maxItems":crate::animation::MAX_KEYS,"items":{"type":"object","properties":{"time":time_schema(),"value":number,"segment":crate::schema::segment()},"required":["time","value"],"additionalProperties":false}}},"required":["kind","keys"],"additionalProperties":false}
    ]})
}
