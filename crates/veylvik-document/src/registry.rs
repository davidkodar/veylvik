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
    KeyView,
    OcioTransform,
    Brushing,
    StudioRig,
    SurfaceOptics,
    Palette,
    PlanarTrack,
    PlanarMode,
    Scalar,
    Typography,
    Letters,
    MediaTiming,
    FadeTiming,
    KeyColor,
    MediaPlacement,
    MediaColour,
    ExrLayer,
    ShaderCode,
    GridPoints,
    TextFeatures,
    TextContent,
    TextFont,
    TextAlignment,
    TextLanguage,
    Boolean,
    Seed,
    /// A whole number within the parameter's own min and max (a count or a size).
    Integer,
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
    CameraOrbit,
    Colour3,
    ShapeKind,
    Fit,
    /// Preview or path traced (Render scene).
    SceneQuality,
    LightKind,
    /// What a Volume fills: box, sphere or ground mist.
    VolumeShape,
    ForceKind,
    SpritePlay,
    Look,
    /// Another node by id, or null.
    NodeRef,
    /// A list of [x, y, z] points.
    Points3,
    /// A model's animations as read from its file: names and lengths.
    ModelClips,
    /// Eight values over a particle's life (birth to end, evenly), or none.
    LifeCurve,
    /// Eight linear RGBA colours over a particle's life, or none.
    LifeColours,
    /// A file path (string).
    FilePath,
    /// A plain [x, y, z].
    Vector3,
    /// An OpenFX plugin's identifier.
    OfxPlugin,
    /// filter, general or generator.
    OfxContext,
    /// The plugin clip each port feeds.
    OfxClips,
    /// An OpenFX plugin's settings by name.
    OfxSettings,
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
            ParameterKind::KeyView => json!(crate::nodes::KeyView::Result),
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
            ParameterKind::ExrLayer => json!(null),
            ParameterKind::ShaderCode => json!(veylvik_shader::EXAMPLE),
            ParameterKind::GridPoints => json!(vec![[0.0f32; 2]; 16]),
            ParameterKind::MediaTiming => json!(crate::media::Timing::default()),
            ParameterKind::FadeTiming => json!(crate::nodes::FadeTiming::default()),
            ParameterKind::Typography => json!(crate::text::Typography::default()),
            ParameterKind::Letters => json!(crate::nodes::Letters::default()),
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
            ParameterKind::Seed | ParameterKind::Integer => json!(self.default as u32),
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
            ParameterKind::CameraOrbit => json!(null),
            ParameterKind::Placement => json!(crate::nodes::Placement::default()),
            ParameterKind::Colour3 => json!([0.6_f32, 0.6_f32, 0.6_f32]),
            ParameterKind::ShapeKind => json!(crate::nodes::ShapeKind::default()),
            ParameterKind::Fit => json!(crate::nodes::Fit::default()),
            ParameterKind::SceneQuality => json!(crate::nodes::SceneQuality::default()),
            ParameterKind::LightKind => json!(crate::nodes::LightKind::default()),
            ParameterKind::VolumeShape => json!(crate::nodes::VolumeShape::default()),
            ParameterKind::ForceKind => json!(crate::nodes::ForceKind::default()),
            ParameterKind::SpritePlay => json!(crate::nodes::SpritePlay::default()),
            ParameterKind::Look | ParameterKind::NodeRef => json!(null),
            ParameterKind::Points3 | ParameterKind::ModelClips => json!([]),
            ParameterKind::LifeCurve | ParameterKind::LifeColours => json!(null),
            ParameterKind::FilePath => json!(""),
            ParameterKind::OfxPlugin => json!(""),
            ParameterKind::OfxContext => json!(crate::nodes::OfxContext::Filter),
            ParameterKind::OfxClips => json!(["Source"]),
            ParameterKind::OfxSettings => json!({}),
            ParameterKind::Vector3 => json!([1.0, 1.0, 1.0]),
        }
    }
    fn schema(&self) -> Value {
        match self.kind {
            ParameterKind::CameraOrbit => crate::nodes::CameraOrbit::schema(),
            ParameterKind::FrameRate => {
                json!({"type":"object","properties":{"ticks":{"type":"integer","minimum":1},"rate":{"type":"integer","minimum":1}},"required":["ticks","rate"],"additionalProperties":false})
            }
            ParameterKind::FlowQuality => {
                json!({"type":"string","enum":crate::nodes::FlowQuality::ALL})
            }
            ParameterKind::BlurFilter => {
                json!({"type":"string","enum":crate::nodes::BlurFilter::ALL})
            }
            ParameterKind::KeyView => {
                json!({"type":"string","enum":crate::nodes::KeyView::ALL})
            }
            ParameterKind::OcioTransform => crate::color_management::OcioTransform::schema(),
            ParameterKind::Brushing => {
                json!({"type":"string","enum":crate::optical::Brushing::ALL})
            }
            ParameterKind::StudioRig => crate::optical::StudioRig::schema(),
            ParameterKind::SurfaceOptics => {
                json!({"anyOf":[{"type":"null"},crate::optical::SurfaceOptics::schema()]})
            }
            ParameterKind::Palette => crate::optical::palette_schema(),
            ParameterKind::Placement => crate::nodes::Placement::schema(),
            ParameterKind::Colour3 => crate::track::schema(0.0, 64.0),
            ParameterKind::ShapeKind => {
                json!({"type":"string","enum":crate::nodes::ShapeKind::ALL})
            }
            ParameterKind::Fit => json!({"type":"string","enum":crate::nodes::Fit::ALL}),
            ParameterKind::SceneQuality => {
                json!({"type":"string","enum":crate::nodes::SceneQuality::ALL})
            }
            ParameterKind::LightKind => {
                json!({"type":"string","enum":crate::nodes::LightKind::ALL})
            }
            ParameterKind::VolumeShape => {
                json!({"type":"string","enum":crate::nodes::VolumeShape::ALL})
            }
            ParameterKind::ForceKind => {
                json!({"type":"string","enum":crate::nodes::ForceKind::ALL})
            }
            ParameterKind::SpritePlay => {
                json!({"type":"string","enum":crate::nodes::SpritePlay::ALL})
            }
            ParameterKind::Look => {
                json!({"anyOf":[{"type":"null"},{"type":"object","description":"A Text surface (material) definition: the parameters of a material node without its type."}]})
            }
            ParameterKind::FilePath => json!({"type":"string","maxLength":4096}),
            ParameterKind::OfxPlugin => json!({"type":"string","maxLength":1024}),
            ParameterKind::OfxContext => {
                json!({"type":"string","enum":crate::nodes::OfxContext::ALL})
            }
            ParameterKind::OfxClips => {
                json!({"type":"array","maxItems":4,"items":{"type":"string","maxLength":256}})
            }
            ParameterKind::OfxSettings => json!({
                "type":"object","maxProperties":crate::nodes::OFX_MAX_SETTINGS,
                "additionalProperties":{"oneOf":[
                    {"type":"array","minItems":1,"maxItems":4,"items":scalar_schema(None, None)},
                    {"type":"string","maxLength":65536},
                    {"type":"array","maxItems":16,"items":{"type":"array","maxItems":256,"items":{
                        "type":"array","minItems":2,"maxItems":2,"items":{"type":"number"}}}}
                ]}
            }),
            ParameterKind::Vector3 => {
                json!({"type":"array","minItems":3,"maxItems":3,"items":{"type":"number"}})
            }
            ParameterKind::LifeCurve => {
                json!({"anyOf":[{"type":"null"},{"type":"array","minItems":8,"maxItems":8,"items":{"type":"number","minimum":0,"maximum":8}}]})
            }
            ParameterKind::LifeColours => {
                json!({"anyOf":[{"type":"null"},{"type":"array","minItems":8,"maxItems":8,"items":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"number","minimum":0,"maximum":64}}}]})
            }
            ParameterKind::ModelClips => {
                json!({"type":"array","maxItems":crate::nodes::MAX_MODEL_CLIPS,"items":{"type":"object","properties":{"name":{"type":"string","maxLength":256},"seconds":{"type":"number","minimum":0}},"required":["name","seconds"],"additionalProperties":false}})
            }
            ParameterKind::Points3 => {
                json!({"type":"array","maxItems":crate::nodes::MAX_CLOUD_POINTS,"items":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"number"}}})
            }
            ParameterKind::NodeRef => {
                json!({"anyOf":[{"type":"null"},{"type":"string","format":"uuid"}]})
            }
            ParameterKind::PlanarTrack => crate::planar::schema(),
            ParameterKind::PlanarMode => {
                json!({"type":"string","enum":crate::planar::WarpMode::ALL})
            }
            ParameterKind::Typography => crate::text::typography_schema(),
            ParameterKind::Letters => crate::nodes::Letters::schema(),
            ParameterKind::TextFeatures => json!({"type":"string","maxLength":256}),
            ParameterKind::TextContent => json!({"type":"string","maxLength":4096}),
            ParameterKind::TextFont => crate::text::font_schema(),
            ParameterKind::TextAlignment => {
                json!({"type":"string","enum":crate::TextAlignment::ALL})
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
            ParameterKind::Integer => {
                json!({"type":"integer","minimum":self.min.map(|m| m as u32),"maximum":self.max.map(|m| m as u32)})
            }
            ParameterKind::KeyColor => {
                json!({"type":"array","minItems":4,"maxItems":4,"items":{"type":"number","minimum":0}})
            }
            ParameterKind::Color => {
                json!({"type":"array","minItems":4,"maxItems":4,"prefixItems":[{"type":"number"},{"type":"number"},{"type":"number"},{"type":"number","minimum":0,"maximum":1}]})
            }
            ParameterKind::Path => crate::vector::schema(),
            ParameterKind::MediaTiming => crate::media::schema(),
            ParameterKind::MediaColour => crate::media::InputColour::schema(),
            ParameterKind::ExrLayer => {
                json!({"type":["string","null"],"minLength":1,"maxLength":512})
            }
            ParameterKind::ShaderCode => {
                json!({"type":"string","minLength":1,"maxLength":veylvik_shader::MAX_SOURCE})
            }
            ParameterKind::GridPoints => {
                json!({"type":"array","minItems":4,"maxItems":289,"items":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number","minimum":-8192,"maximum":8192}}})
            }
            ParameterKind::FadeTiming => crate::nodes::fade_schema(),
            ParameterKind::MediaPlacement => {
                json!({"type":"string","enum":crate::media::Placement::ALL})
            }
            ParameterKind::Asset => {
                json!({"type":"string","minLength":1,"maxLength":MAX_ASSET_NAME})
            }
        }
    }
}
/// A whole number from `min` to `max`: its default and limits, declared once, give the
/// schema and are what validation enforces.
pub(crate) const fn integer(
    name: &'static str,
    unit: &'static str,
    min: u32,
    max: u32,
    default: u32,
) -> Parameter {
    Parameter {
        name,
        kind: ParameterKind::Integer,
        unit,
        min: Some(min as f32),
        max: Some(max as f32),
        default: default as f32,
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
    /// Forces on particles: no picture; Particles and other forces take them.
    Forces,
    /// Sound on the timeline: no picture, and no input takes it.
    Sound,
    /// An image of a size of its own (an Own size node): used as it is by inputs
    /// that wrap or place a picture of any size, fitted into the frame elsewhere.
    Picture,
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
        } else if port == "forces" {
            PortKind::Forces
        } else if port == "scene" || port.starts_with("scene_") || port == "camera" {
            PortKind::Scene
        } else {
            PortKind::Image
        }
    }
    /// Whether `port` uses a picture that is not frame-sized as it is.
    pub fn takes_picture(&self, port: &str) -> bool {
        matches!(
            (self.type_id, port),
            ("image_card", "image") | ("shape_3d", "texture") | ("environment_3d", "image")
        )
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
        if self.type_id == "rectangle" {
            value["color"] = json!([1.0_f32, 1.0_f32, 1.0_f32, 1.0_f32]);
        }
        // A card's tint starts white: the picture as it is; a volume scatters white light.
        if self.type_id == "image_card" || self.type_id == "volume_3d" {
            value["colour"] = json!([1.0_f32, 1.0_f32, 1.0_f32]);
        }
        // A new Material is gold, lit and finished to look like it.
        if self.type_id == "material"
            && let Ok(crate::Operation::Material(mut material)) =
                serde_json::from_value(value.clone())
        {
            material.shine();
            material.brushing = crate::optical::Brushing::Directional;
            material.roughness = 0.15.into();
            material.ridges = 0.7.into();
            if let Ok(shining) = serde_json::to_value(crate::Operation::Material(material)) {
                value = shining;
            }
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
    AFFINE, ATMOSPHERE, BLEND, BLOOM, BLUR, CAMERA_3D, CAMERA_PROJECTION, CHANNEL_MASK, CHROMA_KEY,
    COLOR_CORRECT, COLORIZE, COMBINE_MASK, CONDENSATION, DISPLACE, DRIP, DUST, ELLIPSE,
    ENVIRONMENT_3D, EXPOSURE, FADE, FIGURE_3D, FIRE, FOOTAGE, FORCE, FRACTAL_NOISE,
    GAUSSIAN_SPLATS, GLASS, GRADIENT, GRID_WARP, IMAGE, IMAGE_CARD, LENS_BLUR, LENS_DISTORTION,
    LIGHT_3D, LIGHT_RIG, LUMA_KEY, LUT, MARKER_3D, MASK, MATCH_GRAIN, MATERIAL, MATERIAL_LAYERS,
    MERGE, MERGE_SCENES, METALLIC_SHINE, MODEL_3D, MORPHOLOGY, MOTION_BLUR, NOISE, OCIO,
    OPTICAL_FLOW, PARTICLE_INSTANCES, PARTICLES, PATH, PATINA, PLANAR_TRACK, PLANAR_WARP,
    POINT_CLOUD, RECTANGLE, REFINE_MATTE, RENDER_SCENE, REPEAT, SHADER, SHAPE_3D, SHAPE_WARP,
    SOLID, SOUND, SUN_GLINTS, SURFACE_BLEND, SURFACE_REFLECTION, TEXT, TEXT_3D, TIME_REMAP,
    TRANSFORM, TRANSFORM_3D,
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
    &crate::nodes::FORCE,
    &crate::nodes::PARTICLE_INSTANCES,
    &crate::nodes::CAMERA_PROJECTION,
    &crate::nodes::GAUSSIAN_SPLATS,
    &crate::nodes::FIGURE_3D,
    &crate::nodes::MARKER_3D,
    &crate::nodes::VOLUME_3D,
    &crate::nodes::FLUTTER_3D,
    &crate::nodes::OFX_EFFECT,
    &SOUND,
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
    &crate::nodes::BLOOM,
    &crate::nodes::LIGHT_WRAP,
    &crate::nodes::ZOOM_BLUR,
    &crate::nodes::SHADER,
    &crate::nodes::OWN_SIZE,
    &crate::nodes::GRID_WARP,
    &crate::nodes::SHAPE_WARP,
    &crate::nodes::PARTICLES,
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
    &RECTANGLE,
    &REPEAT,
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
/// The short form of [`operation_schema`] for tool lists: the type names and where
/// the full schemas are. Every operation is still validated in full when applied.
pub fn operation_reference() -> Value {
    json!({"type":"object","properties":{"type":{"enum":TYPES.iter().map(|node|node.type_id).collect::<Vec<_>>()}},"required":["type"],"description":"A complete operation: its type and every setting. The nodes tool lists each type's default_operation (a valid starting point) and operation_schema; add_node fills the defaults in for you."})
}
pub fn catalog() -> Value {
    json!({"recipes":crate::recipes::catalog(),"nodes":TYPES.iter().map(|node|json!({"descriptor":node,"default_operation":node.default_operation(),"operation_schema":node.schema()})).collect::<Vec<_>>(),"scene_contract":"Scene coordinates are composition pixels: x right, y down, z toward the viewer. Objects use offsets from their natural placement; camera home is [width/2,height/2,width*focal_length/sensor_width]. Camera placement.position is an offset from home. placement position/rotation/scale support vector tracks {interpolation,keys:[{time:{ticks,rate},value:[x,y,z]}]}; set_track_key accepts these placement paths. aim_at is an object UUID in the same scene. Optional camera orbit overrides placement position/rotation while enabled, follows aim_at, and evaluates continuously including motion-blur subframes. 2D image branches composited after Render scene remain screen-fixed; use Image card or Environment with background for camera-relative content.","image_contract":"scene-linear Rec.709, premultiplied RGBA32F","time_contract":"Two forms, by what the value is. A point on the timeline that must be exact (keyframe times, a Fade's or Footage's timing, markers, render times) is {ticks, rate}: ticks / rate seconds, so frame 45 at 30 fps is {\"ticks\":45,\"rate\":30}. A duration or start used as an ordinary number (a Text's typography and a 3D text's letters: start, duration, stagger, exit; Particles: start, duration, life) is plain seconds, as its unit says. Frame numbers appear only as tool arguments (preview, render_sequence, markers shown in Studio).","scalar_contract":"constant, linear, step, smooth or monotone keyframes; smooth uses cubic smoothstep per interval; monotone uses bounded cubic Hermite with shared harmonic interior slopes and secant endpoint slopes; time is ticks/rate seconds; 1..4096 strictly increasing keys, as a list or, evenly spaced, as {from, every, values}"})
}

/// An exact rational time.
pub fn time_schema() -> Value {
    json!({"type":"object","properties":{"ticks":{"type":"integer","minimum":i64::MIN,"maximum":i64::MAX},"rate":{"type":"integer","minimum":1,"maximum":u32::MAX}},"required":["ticks","rate"],"additionalProperties":false})
}
/// Evenly spaced keys in their compact form: the first key's time, the ticks between
/// keys and the values (see `crate::animation::even`).
pub fn even_schema(value: Value) -> Value {
    json!({"type":"object","description":"Keys `every` ticks apart from `from`, one per value: the compact form of evenly spaced keys","properties":{"from":time_schema(),"every":{"type":"integer","minimum":1},"values":{"type":"array","minItems":1,"maxItems":crate::animation::MAX_KEYS,"items":value}},"required":["from","every","values"],"additionalProperties":false})
}
/// A constant or keyed number within bounds.
pub fn scalar_schema(min: Option<f32>, max: Option<f32>) -> Value {
    let number = json!({"type":"number","minimum":min,"maximum":max});
    json!({"oneOf":[
        {"type":"object","properties":{"kind":{"const":"constant"},"value":number},"required":["kind","value"],"additionalProperties":false},
        {"type":"object","description":"A formula of time (seconds), frame, numbers, + - * / % ^ and functions (sin, cos, noise, random, clamp, mix, smoothstep, ...), with links [Node].parameter to other nodes' animatable parameters by node id or unique label; clamped to the parameter's range","properties":{"kind":{"const":"expression"},"expression":{"type":"string","minLength":1,"maxLength":veylvik_expr::MAX_SOURCE}},"required":["kind","expression"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"enum":["linear","step","smooth","monotone"]},"keys":{"anyOf":[
            {"type":"array","minItems":1,"maxItems":crate::animation::MAX_KEYS,"items":{"type":"object","properties":{"time":time_schema(),"value":number,"segment":crate::schema::segment()},"required":["time","value"],"additionalProperties":false}},
            even_schema(number)]}},"required":["kind","keys"],"additionalProperties":false}
    ]})
}
#[cfg(test)]
mod bounds_tests {
    use super::*;
    /// The limits a number setting's schema states, as (min, max).
    fn stated(schema: &Value) -> (Option<f64>, Option<f64>) {
        let number = schema
            .get("oneOf")
            .map(|forms| forms[0]["properties"]["value"].clone())
            .unwrap_or_else(|| schema.clone());
        (number["minimum"].as_f64(), number["maximum"].as_f64())
    }
    fn valid_with(kind: &NodeType, name: &str, value: Value) -> bool {
        let mut op = kind.default_operation();
        op[name] = value;
        // A grid warp holds one offset for each of its points.
        if kind.type_id == "grid_warp" {
            let count = op["columns"].as_u64().unwrap_or(0) * op["rows"].as_u64().unwrap_or(0);
            op["points"] = json!(vec![[0.0f32; 2]; count.min(1024) as usize]);
        }
        serde_json::from_value::<crate::Operation>(op).is_ok_and(|op| op.validate().is_ok())
    }
    #[test]
    fn every_choice_a_schema_lists_is_accepted_and_every_default_is_listed() {
        let mut wrong = Vec::new();
        for kind in TYPES {
            for parameter in kind.parameters {
                let schema = parameter.schema();
                let Some(choices) = schema["enum"].as_array() else {
                    continue;
                };
                let default = kind.default_operation()[parameter.name].clone();
                if !choices.contains(&default) {
                    wrong.push(format!(
                        "{} {}: default {default} is not listed",
                        kind.type_id, parameter.name
                    ));
                }
                for choice in choices {
                    let mut op = kind.default_operation();
                    op[parameter.name] = choice.clone();
                    if serde_json::from_value::<crate::Operation>(op).is_err() {
                        wrong.push(format!(
                            "{} {}: lists {choice}, which is refused",
                            kind.type_id, parameter.name
                        ));
                    }
                }
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }
    #[test]
    fn every_number_setting_is_refused_exactly_beyond_the_limits_its_schema_states() {
        let mut wrong = Vec::new();
        for kind in TYPES {
            for parameter in kind.parameters {
                let wrap = |v: f64| match parameter.kind {
                    ParameterKind::Scalar => json!({"kind":"constant","value":v}),
                    _ => json!(v as u64),
                };
                if !matches!(
                    parameter.kind,
                    ParameterKind::Scalar | ParameterKind::Seed | ParameterKind::Integer
                ) {
                    continue;
                }
                let (min, max) = stated(&parameter.schema());
                // A step a stored f32 still tells apart from the limit.
                let step = |m: f64| match parameter.kind {
                    ParameterKind::Seed | ParameterKind::Integer => 1.0,
                    _ => (m.abs() * 1e-5).max(1e-3),
                };
                for (edge, inside, outside) in [
                    ("min", min, min.map(|m| m - step(m))),
                    ("max", max, max.map(|m| m + step(m))),
                ] {
                    let Some(at) = inside else { continue };
                    if !valid_with(kind, parameter.name, wrap(at)) {
                        wrong.push(format!(
                            "{} {}: refuses its stated {edge} {at}",
                            kind.type_id, parameter.name
                        ));
                    }
                    // Out of range for a u32 cannot even be written.
                    if let Some(beyond) = outside.filter(|v| *v >= 0.0 && *v <= f64::from(u32::MAX))
                        && valid_with(kind, parameter.name, wrap(beyond))
                    {
                        wrong.push(format!(
                            "{} {}: accepts {beyond}, beyond its stated {edge}",
                            kind.type_id, parameter.name
                        ));
                    }
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "{} disagreements:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }
    #[test]
    fn a_new_render_scene_is_denoised_as_an_older_file_without_the_setting_is() {
        let kind = TYPES.iter().find(|t| t.type_id == "render_scene").unwrap();
        assert_eq!(kind.default_operation()["denoise"], json!(true));
        let mut older = kind.default_operation();
        older.as_object_mut().unwrap().remove("denoise");
        let crate::Operation::RenderScene(read) = serde_json::from_value(older).unwrap() else {
            panic!("not a Render scene");
        };
        assert!(read.denoise);
    }
}
