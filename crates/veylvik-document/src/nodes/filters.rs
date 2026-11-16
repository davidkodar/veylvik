//! Spatial filters and transforms of one image.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, PortKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

/// How Blur spreads each pixel over its radius.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlurFilter {
    /// Equal weights: fastest, but bright points become squares.
    #[default]
    Box,
    /// Gaussian falloff (sigma = radius / 3): smooth and round, like a soft lens.
    Gaussian,
}
impl BlurFilter {
    pub const ALL: [Self; 2] = [Self::Box, Self::Gaussian];
    pub fn label(self) -> &'static str {
        match self {
            Self::Box => "Box",
            Self::Gaussian => "Gaussian",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Blur {
    pub radius: Scalar,
    /// Projects saved before the filter existed are box blurs.
    #[serde(default)]
    pub filter: BlurFilter,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LensBlur {
    pub radius: Scalar,
    pub highlights: Scalar,
    pub threshold: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Morphology {
    pub radius: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Displace {
    pub x: Scalar,
    pub y: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    pub x: Scalar,
    pub y: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Affine {
    pub x: Scalar,
    pub y: Scalar,
    pub scale_x: Scalar,
    pub scale_y: Scalar,
    pub rotation: Scalar,
    pub pivot_x: Scalar,
    pub pivot_y: Scalar,
}

pub static BLUR: NodeType = NodeType {
    type_id: "blur",
    version: 1,
    label: "Blur",
    description: "Blur within radius pixels (rounded to an integer); transparent borders. Filter box averages equally (fast; bright points turn square), gaussian falls off smoothly (sigma = radius / 3) for a round, lens-like softness.",
    inputs: &["image"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[
        scalar("radius", "pixels", 0.0, 256.0, 4.0),
        other("filter", ParameterKind::BlurFilter, ""),
    ],
};
pub static LENS_BLUR: NodeType = NodeType {
    type_id: "lens_blur",
    version: 1,
    label: "Lens blur",
    description: "Out-of-focus lens blur: every pixel spreads evenly over a disc of radius pixels, so lights become round bokeh, even single-pixel ones (large discs are prefiltered to the sample spacing). Highlights boosts colour brighter than threshold (scene-linear luminance) by 1 + highlights x (luminance - threshold) before blurring, so lights bloom into bright discs instead of fading. Premultiplied colour and alpha blur together; transparent borders. Animate radius for a rack focus.",
    inputs: &["image"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[
        scalar("radius", "pixels", 0.0, 128.0, 12.0),
        scalar("highlights", "gain", 0.0, 16.0, 2.0),
        scalar("threshold", "luminance", 0.0, 64.0, 0.8),
    ],
};
pub static MORPHOLOGY: NodeType = NodeType {
    type_id: "morphology",
    version: 1,
    label: "Grow/shrink mask",
    description: "Expand alpha with a positive radius (dilation), contract with a negative radius (erosion). Uses a square neighborhood and transparent borders. Radius rounds to the nearest integer, half away from zero. Outputs a white premultiplied mask, including at radius zero; source RGB is ignored.",
    inputs: &["image"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[scalar("radius", "pixels", -256.0, 256.0, 2.0)],
};
pub static DISPLACE: NodeType = NodeType {
    type_id: "displace",
    version: 1,
    label: "Displace",
    description: "Map red/green displace image horizontally/vertically. Straight map 0.5 is neutral, 0/1 give negative/positive strength; map alpha attenuates displacement. Map channels clamp to [0,1] before alpha weighting. Bilinear premultiplied sampling with transparent borders.",
    inputs: &["image", "map"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[
        scalar("x", "pixels", -8192.0, 8192.0, 20.0),
        scalar("y", "pixels", -8192.0, 8192.0, 20.0),
    ],
};
pub static TRANSFORM: NodeType = NodeType {
    type_id: "transform",
    version: 1,
    label: "Transform",
    description: "Bilinear translation with transparent borders.",
    inputs: &["image"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[
        scalar("x", "pixels", -1_000_000.0, 1_000_000.0, 0.0),
        scalar("y", "pixels", -1_000_000.0, 1_000_000.0, 0.0),
    ],
};
pub static AFFINE: NodeType = NodeType {
    type_id: "affine",
    version: 1,
    label: "Affine transform",
    description: "Scale around a pixel-space pivot, rotate clockwise, then translate; bilinear sampling and transparent borders. Scale magnitude below 0.000001 produces transparent output.",
    inputs: &["image"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[
        scalar("x", "pixels", -1_000_000.0, 1_000_000.0, 0.0),
        scalar("y", "pixels", -1_000_000.0, 1_000_000.0, 0.0),
        scalar("scale_x", "factor", -1000.0, 1000.0, 1.0),
        scalar("scale_y", "factor", -1000.0, 1000.0, 1.0),
        scalar(
            "rotation",
            "degrees clockwise",
            -1_000_000.0,
            1_000_000.0,
            0.0,
        ),
        scalar("pivot_x", "pixels", -1_000_000.0, 1_000_000.0, 0.0),
        scalar("pivot_y", "pixels", -1_000_000.0, 1_000_000.0, 0.0),
    ],
};

node!(Blur, BLUR, [radius]);
node!(LensBlur, LENS_BLUR, [radius, highlights, threshold]);
node!(Morphology, MORPHOLOGY, [radius]);
node!(Displace, DISPLACE, [x, y]);
node!(Transform, TRANSFORM, [x, y]);
node!(
    Affine,
    AFFINE,
    [x, y, scale_x, scale_y, rotation, pivot_x, pivot_y]
);

/// Flat receiving-plane shadow. Lighting is resolved from the lighting input branch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shadow {
    #[serde(default)]
    pub follow_lighting: bool,
    pub distance: Scalar,
    pub softness: Scalar,
    pub strength: Scalar,
    pub x: Scalar,
    pub y: Scalar,
    /// Zero selects all enabled area lights; 1–8 selects a particular emitter.
    pub light: Scalar,
}
/// Occlusion where a layer meets the surface behind it: darkest and tightest at the
/// layer's edge, fading softly within its reach, leaning away from the light.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactShadow {
    pub reach: Scalar,
    pub strength: Scalar,
    pub lean: Scalar,
    pub angle: Scalar,
}
pub static CONTACT_SHADOW: NodeType = NodeType {
    type_id: "contact_shadow",
    version: 1,
    label: "Contact shadow",
    description: "Ambient occlusion where a layer meets the surface behind it, so it sits on that surface: dense and sharp right at the layer's edge, fading softly over Reach pixels, and shifted Lean pixels away from the light. With a material on the optional Lighting input it leans away from that material's lights; otherwise towards Angle (degrees, 90 = down). Outputs black premultiplied occlusion only; merge it behind the layer, together with or instead of a cast Shadow.",
    inputs: &["image", "lighting"],
    optional: &["lighting"],
    output: PortKind::Image,
    parameters: &[
        scalar("reach", "pixels", 0., 256., 16.),
        scalar("strength", "factor", 0., 1., 0.7),
        scalar("lean", "pixels", 0., 512., 3.),
        scalar("angle", "degrees", -3600., 3600., 90.),
    ],
};
pub static SHADOW: NodeType = NodeType {
    type_id: "shadow",
    version: 1,
    label: "Shadow",
    description: "Black premultiplied shadow on a flat receiving plane. Manual offset or area-light projection from the nearest material in the lighting input. Environment reflections do not cast directional shadows. Outputs shadow only; merge behind the original image.",
    inputs: &["image", "lighting"],
    optional: &[],
    output: PortKind::Image,
    parameters: &[
        other("follow_lighting", ParameterKind::Boolean, "lighting"),
        scalar("distance", "pixels", 0., 2000., 20.),
        scalar("softness", "pixels", 0., 256., 8.),
        scalar("strength", "factor", 0., 1., 0.65),
        scalar("x", "pixels", -8192., 8192., 8.),
        scalar("y", "pixels", -8192., 8192., 12.),
        scalar("light", "light index (0 = all)", 0., 8., 0.),
    ],
};
node!(Shadow, SHADOW, [distance, softness, strength, x, y, light]);
node!(
    ContactShadow,
    CONTACT_SHADOW,
    [reach, strength, lean, angle]
);

/// Grain like a plate's, added to a clean image such as 3D rendered over footage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchGrain {
    pub amount: Scalar,
    pub size: Scalar,
    pub shadows: Scalar,
    pub midtones: Scalar,
    pub highlights: Scalar,
    pub red: Scalar,
    pub blue: Scalar,
    pub colour: Scalar,
}
pub static MATCH_GRAIN: NodeType = NodeType {
    type_id: "match_grain",
    version: 1,
    label: "Match grain",
    description: "Adds fresh grain every frame to a clean image, such as 3D rendered over footage, so it matches the plate. Shadows, Midtones and Highlights are the grain's strength (standard deviation, linear light) at luminance 0.02, 0.18 and 0.9, blended between; Red and Blue scale it against green; Colour is how much the channels move together (0 independent, 1 grey grain); Size is the grain's blur radius in pixels. Studio's Measure from plate fills them from the optional Plate input, which is otherwise unused. Amount scales the result (1 = matched); grain follows alpha, so edges blend.",
    inputs: &["image", "plate"],
    optional: &["plate"],
    output: PortKind::Image,
    parameters: &[
        scalar("amount", "factor", 0.0, 4.0, 1.0),
        scalar("size", "pixels", 0.0, 9.0, 2.0),
        scalar("shadows", "std", 0.0, 0.5, 0.012),
        scalar("midtones", "std", 0.0, 0.5, 0.008),
        scalar("highlights", "std", 0.0, 0.5, 0.006),
        scalar("red", "factor", 0.0, 4.0, 1.0),
        scalar("blue", "factor", 0.0, 4.0, 1.2),
        scalar("colour", "factor", 0.0, 1.0, 0.35),
    ],
};
node!(
    MatchGrain,
    MATCH_GRAIN,
    [
        amount, size, shadows, midtones, highlights, red, blue, colour
    ]
);
