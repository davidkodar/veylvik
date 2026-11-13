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
pub use appearance::{AtmosphereKind, MaterialKind, PatinaKind, ReflectionKind};
mod gradient;
pub use gradient::GradientMode;
pub mod schema;
pub mod text;
pub use text::{TextAlignment, TextFont};
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
pub mod key_edit;
pub mod nodes;
pub use animation::{CompiledScalar, Handle, Interpolation, Key, Scalar, Segment, Time};
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
    Material(nodes::Material),
    Atmosphere(nodes::Atmosphere),
    MetallicShine(nodes::MetallicShine),
    Fire(nodes::Fire),
    FractalNoise(nodes::FractalNoise),
    Exposure(nodes::Exposure),
    Transform(nodes::Transform),
    Displace(nodes::Displace),
    Morphology(nodes::Morphology),
    Blur(nodes::Blur),
    Merge(nodes::Merge),
    Blend(nodes::Blend),
    Image(nodes::Image),
    Footage(nodes::Footage),
    Affine(nodes::Affine),
    Ellipse(nodes::Ellipse),
    Mask(nodes::Mask),
    CombineMask(nodes::CombineMask),
    ChannelMask(nodes::ChannelMask),
    Path(nodes::Path),
}
impl Operation {
    pub fn asset(&self) -> Option<&str> {
        match self {
            Self::Image(crate::nodes::Image { asset })
            | Self::Footage(crate::nodes::Footage { asset, .. }) => Some(asset),
            _ => None,
        }
    }
    pub fn asset_mut(&mut self) -> Option<&mut String> {
        match self {
            Self::Image(crate::nodes::Image { asset })
            | Self::Footage(crate::nodes::Footage { asset, .. }) => Some(asset),
            _ => None,
        }
    }

    pub fn descriptor(&self) -> &'static registry::NodeType {
        match self {
            Self::SurfaceBlend(crate::nodes::SurfaceBlend { .. }) => &registry::SURFACE_BLEND,
            Self::PlanarTrack(crate::nodes::PlanarTrack { .. }) => &registry::PLANAR_TRACK,
            Self::PlanarWarp(crate::nodes::PlanarWarp { .. }) => &registry::PLANAR_WARP,
            Self::Text(crate::nodes::Text { .. }) => &registry::TEXT,
            Self::MetallicShine(crate::nodes::MetallicShine { .. }) => &registry::METALLIC_SHINE,
            Self::SurfaceReflection(crate::nodes::SurfaceReflection { .. }) => {
                &registry::SURFACE_REFLECTION
            }
            Self::Glass(crate::nodes::Glass { .. }) => &registry::GLASS,
            Self::SunGlints(crate::nodes::SunGlints { .. }) => &registry::SUN_GLINTS,
            Self::Patina(crate::nodes::Patina { .. }) => &registry::PATINA,
            Self::Material(crate::nodes::Material { .. }) => &registry::MATERIAL,
            Self::Atmosphere(crate::nodes::Atmosphere { .. }) => &registry::ATMOSPHERE,
            Self::Fire(crate::nodes::Fire { .. }) => &registry::FIRE,
            Self::Solid(crate::nodes::Solid { .. }) => &registry::SOLID,
            Self::Colorize(crate::nodes::Colorize { .. }) => &registry::COLORIZE,
            Self::FractalNoise(crate::nodes::FractalNoise { .. }) => &registry::FRACTAL_NOISE,
            Self::Gradient(crate::nodes::Gradient { .. }) => &registry::GRADIENT,
            Self::Noise(crate::nodes::Noise { .. }) => &registry::NOISE,
            Self::Exposure(crate::nodes::Exposure { .. }) => &registry::EXPOSURE,
            Self::Transform(crate::nodes::Transform { .. }) => &registry::TRANSFORM,
            Self::Displace(crate::nodes::Displace { .. }) => &registry::DISPLACE,
            Self::Morphology(crate::nodes::Morphology { .. }) => &registry::MORPHOLOGY,
            Self::Blur(crate::nodes::Blur { .. }) => &registry::BLUR,
            Self::Merge(crate::nodes::Merge {}) => &registry::MERGE,
            Self::Blend(crate::nodes::Blend { .. }) => &registry::BLEND,
            Self::Image(crate::nodes::Image { .. }) => &registry::IMAGE,
            Self::Footage(crate::nodes::Footage { .. }) => &registry::FOOTAGE,
            Self::Affine(crate::nodes::Affine { .. }) => &registry::AFFINE,
            Self::Ellipse(crate::nodes::Ellipse { .. }) => &registry::ELLIPSE,
            Self::ChannelMask(crate::nodes::ChannelMask { .. }) => &registry::CHANNEL_MASK,
            Self::CombineMask(crate::nodes::CombineMask { .. }) => &registry::COMBINE_MASK,
            Self::Mask(crate::nodes::Mask {}) => &registry::MASK,
            Self::Path(crate::nodes::Path { .. }) => &registry::PATH,
        }
    }
    pub fn inputs(&self) -> &'static [&'static str] {
        self.descriptor().inputs
    }
    pub fn scalar(&self, name: &str) -> Option<&Scalar> {
        match (self, name) {
            (Self::SurfaceBlend(crate::nodes::SurfaceBlend { grain, .. }), "grain") => Some(grain),
            (Self::SurfaceBlend(crate::nodes::SurfaceBlend { softness, .. }), "softness") => {
                Some(softness)
            }
            (Self::SurfaceBlend(crate::nodes::SurfaceBlend { distortion, .. }), "distortion") => {
                Some(distortion)
            }
            (Self::SurfaceBlend(crate::nodes::SurfaceBlend { threshold, .. }), "threshold") => {
                Some(threshold)
            }
            (Self::SurfaceBlend(crate::nodes::SurfaceBlend { wear, .. }), "wear") => Some(wear),
            (Self::SurfaceBlend(crate::nodes::SurfaceBlend { detail, .. }), "detail") => {
                Some(detail)
            }
            (Self::SurfaceBlend(crate::nodes::SurfaceBlend { lighting, .. }), "lighting") => {
                Some(lighting)
            }
            (Self::SurfaceBlend(crate::nodes::SurfaceBlend { opacity, .. }), "opacity") => {
                Some(opacity)
            }
            (
                Self::Noise(crate::nodes::Noise { scale, .. })
                | Self::FractalNoise(crate::nodes::FractalNoise { scale, .. }),
                "scale",
            ) => Some(scale),
            (Self::MetallicShine(crate::nodes::MetallicShine { x, .. }), "x") => Some(x),
            (Self::MetallicShine(crate::nodes::MetallicShine { y, .. }), "y") => Some(y),
            (Self::MetallicShine(crate::nodes::MetallicShine { width, .. }), "width") => {
                Some(width)
            }
            (Self::MetallicShine(crate::nodes::MetallicShine { angle, .. }), "angle") => {
                Some(angle)
            }
            (Self::MetallicShine(crate::nodes::MetallicShine { intensity, .. }), "intensity") => {
                Some(intensity)
            }
            (Self::SurfaceReflection(crate::nodes::SurfaceReflection { bevel, .. }), "bevel") => {
                Some(bevel)
            }
            (
                Self::SurfaceReflection(crate::nodes::SurfaceReflection { curvature, .. }),
                "curvature",
            ) => Some(curvature),
            (
                Self::SurfaceReflection(crate::nodes::SurfaceReflection { roughness, .. }),
                "roughness",
            ) => Some(roughness),
            (
                Self::SurfaceReflection(crate::nodes::SurfaceReflection { strength, .. }),
                "strength",
            ) => Some(strength),
            (
                Self::SurfaceReflection(crate::nodes::SurfaceReflection { rotation, .. }),
                "rotation",
            ) => Some(rotation),
            (Self::SurfaceReflection(crate::nodes::SurfaceReflection { scale, .. }), "scale") => {
                Some(scale)
            }
            (
                Self::SurfaceReflection(crate::nodes::SurfaceReflection { offset_x, .. }),
                "offset_x",
            ) => Some(offset_x),
            (
                Self::SurfaceReflection(crate::nodes::SurfaceReflection { offset_y, .. }),
                "offset_y",
            ) => Some(offset_y),
            (Self::Glass(crate::nodes::Glass { bevel, .. }), "bevel") => Some(bevel),
            (Self::Glass(crate::nodes::Glass { refraction, .. }), "refraction") => Some(refraction),
            (Self::Glass(crate::nodes::Glass { roughness, .. }), "roughness") => Some(roughness),
            (Self::Glass(crate::nodes::Glass { ior, .. }), "ior") => Some(ior),
            (Self::Glass(crate::nodes::Glass { transmission, .. }), "transmission") => {
                Some(transmission)
            }
            (Self::Glass(crate::nodes::Glass { tint, .. }), "tint") => Some(tint),
            (Self::Glass(crate::nodes::Glass { dispersion, .. }), "dispersion") => Some(dispersion),
            (Self::SunGlints(crate::nodes::SunGlints { size, .. }), "size") => Some(size),
            (Self::SunGlints(crate::nodes::SunGlints { intensity, .. }), "intensity") => {
                Some(intensity)
            }
            (Self::SunGlints(crate::nodes::SunGlints { threshold, .. }), "threshold") => {
                Some(threshold)
            }
            (Self::SunGlints(crate::nodes::SunGlints { evolution, .. }), "evolution") => {
                Some(evolution)
            }
            (Self::SunGlints(crate::nodes::SunGlints { density, .. }), "density") => Some(density),
            (Self::SunGlints(crate::nodes::SunGlints { angle, .. }), "angle") => Some(angle),
            (Self::Patina(crate::nodes::Patina { amount, .. }), "amount") => Some(amount),
            (Self::Patina(crate::nodes::Patina { dirt, .. }), "dirt") => Some(dirt),
            (Self::Patina(crate::nodes::Patina { scale, .. }), "scale") => Some(scale),
            (Self::Patina(crate::nodes::Patina { wear, .. }), "wear") => Some(wear),
            (Self::Patina(crate::nodes::Patina { scratches, .. }), "scratches") => Some(scratches),
            (Self::Text(crate::nodes::Text { rotation, .. }), "rotation") => Some(rotation),
            (Self::Text(crate::nodes::Text { padding, .. }), "padding") => Some(padding),
            (Self::Text(crate::nodes::Text { box_height, .. }), "box_height") => Some(box_height),
            (Self::Text(crate::nodes::Text { scale_x, .. }), "scale_x") => Some(scale_x),
            (Self::Text(crate::nodes::Text { scale_y, .. }), "scale_y") => Some(scale_y),
            (Self::Text(crate::nodes::Text { wrap_width, .. }), "wrap_width") => Some(wrap_width),
            (Self::Text(crate::nodes::Text { size, .. }), "size") => Some(size),
            (Self::Text(crate::nodes::Text { x, .. }), "x") => Some(x),
            (Self::Text(crate::nodes::Text { y, .. }), "y") => Some(y),
            (Self::Text(crate::nodes::Text { tracking, .. }), "tracking") => Some(tracking),
            (Self::Text(crate::nodes::Text { line_height, .. }), "line_height") => {
                Some(line_height)
            }
            (Self::Material(crate::nodes::Material { scale, .. }), "scale") => Some(scale),
            (Self::Material(crate::nodes::Material { bevel, .. }), "bevel") => Some(bevel),
            (Self::Material(crate::nodes::Material { roughness, .. }), "roughness") => {
                Some(roughness)
            }
            (Self::Material(crate::nodes::Material { lighting, .. }), "lighting") => Some(lighting),
            (Self::Material(crate::nodes::Material { grain, .. }), "grain") => Some(grain),
            (Self::Material(crate::nodes::Material { emission, .. }), "emission") => Some(emission),
            (Self::Atmosphere(crate::nodes::Atmosphere { x, .. }), "x") => Some(x),
            (Self::Atmosphere(crate::nodes::Atmosphere { y, .. }), "y") => Some(y),
            (Self::Atmosphere(crate::nodes::Atmosphere { width, .. }), "width") => Some(width),
            (Self::Atmosphere(crate::nodes::Atmosphere { height, .. }), "height") => Some(height),
            (Self::Atmosphere(crate::nodes::Atmosphere { evolution, .. }), "evolution") => {
                Some(evolution)
            }
            (Self::Atmosphere(crate::nodes::Atmosphere { density, .. }), "density") => {
                Some(density)
            }
            (Self::Atmosphere(crate::nodes::Atmosphere { detail, .. }), "detail") => Some(detail),
            (Self::Atmosphere(crate::nodes::Atmosphere { lighting, .. }), "lighting") => {
                Some(lighting)
            }
            (Self::Fire(crate::nodes::Fire { x, .. }), "x") => Some(x),
            (Self::Fire(crate::nodes::Fire { y, .. }), "y") => Some(y),
            (Self::Fire(crate::nodes::Fire { width, .. }), "width") => Some(width),
            (Self::Fire(crate::nodes::Fire { height, .. }), "height") => Some(height),
            (Self::Fire(crate::nodes::Fire { evolution, .. }), "evolution") => Some(evolution),
            (Self::Fire(crate::nodes::Fire { intensity, .. }), "intensity") => Some(intensity),
            (Self::Fire(crate::nodes::Fire { turbulence, .. }), "turbulence") => Some(turbulence),
            (Self::FractalNoise(crate::nodes::FractalNoise { octaves, .. }), "octaves") => {
                Some(octaves)
            }
            (Self::FractalNoise(crate::nodes::FractalNoise { roughness, .. }), "roughness") => {
                Some(roughness)
            }
            (Self::FractalNoise(crate::nodes::FractalNoise { evolution, .. }), "evolution") => {
                Some(evolution)
            }
            (Self::Gradient(crate::nodes::Gradient { start_x, .. }), "start_x") => Some(start_x),
            (Self::Gradient(crate::nodes::Gradient { start_y, .. }), "start_y") => Some(start_y),
            (Self::Gradient(crate::nodes::Gradient { end_x, .. }), "end_x") => Some(end_x),
            (Self::Gradient(crate::nodes::Gradient { end_y, .. }), "end_y") => Some(end_y),
            (Self::Exposure(crate::nodes::Exposure { stops }), "stops") => Some(stops),
            (
                Self::Transform(crate::nodes::Transform { x, .. })
                | Self::Displace(crate::nodes::Displace { x, .. }),
                "x",
            ) => Some(x),
            (
                Self::Transform(crate::nodes::Transform { y, .. })
                | Self::Displace(crate::nodes::Displace { y, .. }),
                "y",
            ) => Some(y),
            (
                Self::Blur(crate::nodes::Blur { radius })
                | Self::Morphology(crate::nodes::Morphology { radius }),
                "radius",
            ) => Some(radius),
            (Self::Blend(crate::nodes::Blend { opacity, .. }), "opacity") => Some(opacity),
            (
                Self::Affine(crate::nodes::Affine { x, .. })
                | Self::Ellipse(crate::nodes::Ellipse { x, .. }),
                "x",
            ) => Some(x),
            (
                Self::Affine(crate::nodes::Affine { y, .. })
                | Self::Ellipse(crate::nodes::Ellipse { y, .. }),
                "y",
            ) => Some(y),
            (Self::Affine(crate::nodes::Affine { scale_x, .. }), "scale_x") => Some(scale_x),
            (Self::Affine(crate::nodes::Affine { scale_y, .. }), "scale_y") => Some(scale_y),
            (Self::Affine(crate::nodes::Affine { rotation, .. }), "rotation") => Some(rotation),
            (Self::Affine(crate::nodes::Affine { pivot_x, .. }), "pivot_x") => Some(pivot_x),
            (Self::Affine(crate::nodes::Affine { pivot_y, .. }), "pivot_y") => Some(pivot_y),
            (Self::Ellipse(crate::nodes::Ellipse { radius_x, .. }), "radius_x") => Some(radius_x),
            (Self::Ellipse(crate::nodes::Ellipse { radius_y, .. }), "radius_y") => Some(radius_y),
            (Self::Ellipse(crate::nodes::Ellipse { feather, .. }), "feather") => Some(feather),
            _ => None,
        }
    }
    pub fn scalar_mut(&mut self, name: &str) -> Option<&mut Scalar> {
        match (self, name) {
            (Self::SurfaceBlend(crate::nodes::SurfaceBlend { grain, .. }), "grain") => Some(grain),
            (Self::SurfaceBlend(crate::nodes::SurfaceBlend { softness, .. }), "softness") => {
                Some(softness)
            }
            (Self::SurfaceBlend(crate::nodes::SurfaceBlend { distortion, .. }), "distortion") => {
                Some(distortion)
            }
            (Self::SurfaceBlend(crate::nodes::SurfaceBlend { threshold, .. }), "threshold") => {
                Some(threshold)
            }
            (Self::SurfaceBlend(crate::nodes::SurfaceBlend { wear, .. }), "wear") => Some(wear),
            (Self::SurfaceBlend(crate::nodes::SurfaceBlend { detail, .. }), "detail") => {
                Some(detail)
            }
            (Self::SurfaceBlend(crate::nodes::SurfaceBlend { lighting, .. }), "lighting") => {
                Some(lighting)
            }
            (Self::SurfaceBlend(crate::nodes::SurfaceBlend { opacity, .. }), "opacity") => {
                Some(opacity)
            }
            (
                Self::Noise(crate::nodes::Noise { scale, .. })
                | Self::FractalNoise(crate::nodes::FractalNoise { scale, .. }),
                "scale",
            ) => Some(scale),
            (Self::MetallicShine(crate::nodes::MetallicShine { x, .. }), "x") => Some(x),
            (Self::MetallicShine(crate::nodes::MetallicShine { y, .. }), "y") => Some(y),
            (Self::MetallicShine(crate::nodes::MetallicShine { width, .. }), "width") => {
                Some(width)
            }
            (Self::MetallicShine(crate::nodes::MetallicShine { angle, .. }), "angle") => {
                Some(angle)
            }
            (Self::MetallicShine(crate::nodes::MetallicShine { intensity, .. }), "intensity") => {
                Some(intensity)
            }
            (Self::SurfaceReflection(crate::nodes::SurfaceReflection { bevel, .. }), "bevel") => {
                Some(bevel)
            }
            (
                Self::SurfaceReflection(crate::nodes::SurfaceReflection { curvature, .. }),
                "curvature",
            ) => Some(curvature),
            (
                Self::SurfaceReflection(crate::nodes::SurfaceReflection { roughness, .. }),
                "roughness",
            ) => Some(roughness),
            (
                Self::SurfaceReflection(crate::nodes::SurfaceReflection { strength, .. }),
                "strength",
            ) => Some(strength),
            (
                Self::SurfaceReflection(crate::nodes::SurfaceReflection { rotation, .. }),
                "rotation",
            ) => Some(rotation),
            (Self::SurfaceReflection(crate::nodes::SurfaceReflection { scale, .. }), "scale") => {
                Some(scale)
            }
            (
                Self::SurfaceReflection(crate::nodes::SurfaceReflection { offset_x, .. }),
                "offset_x",
            ) => Some(offset_x),
            (
                Self::SurfaceReflection(crate::nodes::SurfaceReflection { offset_y, .. }),
                "offset_y",
            ) => Some(offset_y),
            (Self::Glass(crate::nodes::Glass { bevel, .. }), "bevel") => Some(bevel),
            (Self::Glass(crate::nodes::Glass { refraction, .. }), "refraction") => Some(refraction),
            (Self::Glass(crate::nodes::Glass { roughness, .. }), "roughness") => Some(roughness),
            (Self::Glass(crate::nodes::Glass { ior, .. }), "ior") => Some(ior),
            (Self::Glass(crate::nodes::Glass { transmission, .. }), "transmission") => {
                Some(transmission)
            }
            (Self::Glass(crate::nodes::Glass { tint, .. }), "tint") => Some(tint),
            (Self::Glass(crate::nodes::Glass { dispersion, .. }), "dispersion") => Some(dispersion),
            (Self::SunGlints(crate::nodes::SunGlints { size, .. }), "size") => Some(size),
            (Self::SunGlints(crate::nodes::SunGlints { intensity, .. }), "intensity") => {
                Some(intensity)
            }
            (Self::SunGlints(crate::nodes::SunGlints { threshold, .. }), "threshold") => {
                Some(threshold)
            }
            (Self::SunGlints(crate::nodes::SunGlints { evolution, .. }), "evolution") => {
                Some(evolution)
            }
            (Self::SunGlints(crate::nodes::SunGlints { density, .. }), "density") => Some(density),
            (Self::SunGlints(crate::nodes::SunGlints { angle, .. }), "angle") => Some(angle),
            (Self::Patina(crate::nodes::Patina { amount, .. }), "amount") => Some(amount),
            (Self::Patina(crate::nodes::Patina { dirt, .. }), "dirt") => Some(dirt),
            (Self::Patina(crate::nodes::Patina { scale, .. }), "scale") => Some(scale),
            (Self::Patina(crate::nodes::Patina { wear, .. }), "wear") => Some(wear),
            (Self::Patina(crate::nodes::Patina { scratches, .. }), "scratches") => Some(scratches),
            (Self::Text(crate::nodes::Text { rotation, .. }), "rotation") => Some(rotation),
            (Self::Text(crate::nodes::Text { padding, .. }), "padding") => Some(padding),
            (Self::Text(crate::nodes::Text { box_height, .. }), "box_height") => Some(box_height),
            (Self::Text(crate::nodes::Text { scale_x, .. }), "scale_x") => Some(scale_x),
            (Self::Text(crate::nodes::Text { scale_y, .. }), "scale_y") => Some(scale_y),
            (Self::Text(crate::nodes::Text { wrap_width, .. }), "wrap_width") => Some(wrap_width),
            (Self::Text(crate::nodes::Text { size, .. }), "size") => Some(size),
            (Self::Text(crate::nodes::Text { x, .. }), "x") => Some(x),
            (Self::Text(crate::nodes::Text { y, .. }), "y") => Some(y),
            (Self::Text(crate::nodes::Text { tracking, .. }), "tracking") => Some(tracking),
            (Self::Text(crate::nodes::Text { line_height, .. }), "line_height") => {
                Some(line_height)
            }
            (Self::Material(crate::nodes::Material { scale, .. }), "scale") => Some(scale),
            (Self::Material(crate::nodes::Material { bevel, .. }), "bevel") => Some(bevel),
            (Self::Material(crate::nodes::Material { roughness, .. }), "roughness") => {
                Some(roughness)
            }
            (Self::Material(crate::nodes::Material { lighting, .. }), "lighting") => Some(lighting),
            (Self::Material(crate::nodes::Material { grain, .. }), "grain") => Some(grain),
            (Self::Material(crate::nodes::Material { emission, .. }), "emission") => Some(emission),
            (Self::Atmosphere(crate::nodes::Atmosphere { x, .. }), "x") => Some(x),
            (Self::Atmosphere(crate::nodes::Atmosphere { y, .. }), "y") => Some(y),
            (Self::Atmosphere(crate::nodes::Atmosphere { width, .. }), "width") => Some(width),
            (Self::Atmosphere(crate::nodes::Atmosphere { height, .. }), "height") => Some(height),
            (Self::Atmosphere(crate::nodes::Atmosphere { evolution, .. }), "evolution") => {
                Some(evolution)
            }
            (Self::Atmosphere(crate::nodes::Atmosphere { density, .. }), "density") => {
                Some(density)
            }
            (Self::Atmosphere(crate::nodes::Atmosphere { detail, .. }), "detail") => Some(detail),
            (Self::Atmosphere(crate::nodes::Atmosphere { lighting, .. }), "lighting") => {
                Some(lighting)
            }
            (Self::Fire(crate::nodes::Fire { x, .. }), "x") => Some(x),
            (Self::Fire(crate::nodes::Fire { y, .. }), "y") => Some(y),
            (Self::Fire(crate::nodes::Fire { width, .. }), "width") => Some(width),
            (Self::Fire(crate::nodes::Fire { height, .. }), "height") => Some(height),
            (Self::Fire(crate::nodes::Fire { evolution, .. }), "evolution") => Some(evolution),
            (Self::Fire(crate::nodes::Fire { intensity, .. }), "intensity") => Some(intensity),
            (Self::Fire(crate::nodes::Fire { turbulence, .. }), "turbulence") => Some(turbulence),
            (Self::FractalNoise(crate::nodes::FractalNoise { octaves, .. }), "octaves") => {
                Some(octaves)
            }
            (Self::FractalNoise(crate::nodes::FractalNoise { roughness, .. }), "roughness") => {
                Some(roughness)
            }
            (Self::FractalNoise(crate::nodes::FractalNoise { evolution, .. }), "evolution") => {
                Some(evolution)
            }
            (Self::Gradient(crate::nodes::Gradient { start_x, .. }), "start_x") => Some(start_x),
            (Self::Gradient(crate::nodes::Gradient { start_y, .. }), "start_y") => Some(start_y),
            (Self::Gradient(crate::nodes::Gradient { end_x, .. }), "end_x") => Some(end_x),
            (Self::Gradient(crate::nodes::Gradient { end_y, .. }), "end_y") => Some(end_y),
            (Self::Exposure(crate::nodes::Exposure { stops }), "stops") => Some(stops),
            (
                Self::Transform(crate::nodes::Transform { x, .. })
                | Self::Displace(crate::nodes::Displace { x, .. }),
                "x",
            ) => Some(x),
            (
                Self::Transform(crate::nodes::Transform { y, .. })
                | Self::Displace(crate::nodes::Displace { y, .. }),
                "y",
            ) => Some(y),
            (
                Self::Blur(crate::nodes::Blur { radius })
                | Self::Morphology(crate::nodes::Morphology { radius }),
                "radius",
            ) => Some(radius),
            (Self::Blend(crate::nodes::Blend { opacity, .. }), "opacity") => Some(opacity),
            (
                Self::Affine(crate::nodes::Affine { x, .. })
                | Self::Ellipse(crate::nodes::Ellipse { x, .. }),
                "x",
            ) => Some(x),
            (
                Self::Affine(crate::nodes::Affine { y, .. })
                | Self::Ellipse(crate::nodes::Ellipse { y, .. }),
                "y",
            ) => Some(y),
            (Self::Affine(crate::nodes::Affine { scale_x, .. }), "scale_x") => Some(scale_x),
            (Self::Affine(crate::nodes::Affine { scale_y, .. }), "scale_y") => Some(scale_y),
            (Self::Affine(crate::nodes::Affine { rotation, .. }), "rotation") => Some(rotation),
            (Self::Affine(crate::nodes::Affine { pivot_x, .. }), "pivot_x") => Some(pivot_x),
            (Self::Affine(crate::nodes::Affine { pivot_y, .. }), "pivot_y") => Some(pivot_y),
            (Self::Ellipse(crate::nodes::Ellipse { radius_x, .. }), "radius_x") => Some(radius_x),
            (Self::Ellipse(crate::nodes::Ellipse { radius_y, .. }), "radius_y") => Some(radius_y),
            (Self::Ellipse(crate::nodes::Ellipse { feather, .. }), "feather") => Some(feather),
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
            Self::PlanarTrack(crate::nodes::PlanarTrack { data }) => data.validate(),
            Self::Text(crate::nodes::Text {
                content,
                font,
                language,
                features,
                color,
                ..
            }) => {
                text::validate(content, font, language)?;
                if let Self::Text(crate::nodes::Text { typography, .. }) = self {
                    typography.validate()?;
                    for span in &typography.spans {
                        if span.end > content.len()
                            || !content.is_char_boundary(span.start)
                            || !content.is_char_boundary(span.end)
                        {
                            return fail(
                                "Character styles must use UTF-8 boundaries inside the text",
                            );
                        }
                    }
                }
                if features.len() > 256 || !features.is_ascii() {
                    return fail("OpenType feature list must be ASCII, at most 256 bytes");
                }
                if color.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&color[3]) {
                    return fail("Text colour needs finite RGB and alpha in [0,1]");
                }
                Ok(())
            }
            Self::Footage(crate::nodes::Footage { asset, timing, .. }) => {
                if asset.trim().is_empty()
                    || asset.chars().count() > registry::MAX_ASSET_NAME
                    || asset.contains('\0')
                {
                    return fail("Footage needs a valid asset key");
                }
                timing.validate()
            }
            Self::Path(crate::nodes::Path { geometry }) => geometry.validate(),
            Self::Solid(crate::nodes::Solid { color })
                if color.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&color[3]) =>
            {
                fail("solid needs finite RGB and alpha in [0,1]")
            }
            Self::Colorize(crate::nodes::Colorize { low, high })
                if [low, high].iter().any(|color| {
                    color.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&color[3])
                }) =>
            {
                fail("colorize needs finite RGB and alpha in [0,1] for both endpoints")
            }
            Self::Image(crate::nodes::Image { asset })
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
    let node = Node::new(
        "Background",
        Operation::Solid(crate::nodes::Solid { color: [0.0; 4] }),
        &[],
    );
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
        Operation::Blur(crate::nodes::Blur { radius: 4.0.into() }),
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
