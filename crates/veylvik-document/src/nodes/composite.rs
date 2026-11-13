//! Layering, blending and mask combination of two or more images.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Merge {}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Blend {
    pub mode: BlendMode,
    pub opacity: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mask {}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CombineMask {
    pub mode: MaskMode,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelMask {
    pub channel: MaskChannel,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceBlend {
    pub opacity: Scalar,
    pub lighting: Scalar,
    pub detail: Scalar,
    pub wear: Scalar,
    pub threshold: Scalar,
    pub distortion: Scalar,
    pub softness: Scalar,
    pub grain: Scalar,
}

pub static MERGE: NodeType = NodeType {
    type_id: "merge",
    version: 1,
    label: "Merge",
    description: "Premultiplied foreground over background.",
    inputs: &["background", "foreground"],
    parameters: &[],
};
pub static BLEND: NodeType = NodeType {
    type_id: "blend",
    version: 1,
    label: "Blend",
    description: "Scene-linear blending with source-over alpha and animated foreground opacity. RGB is not clamped; Add sums premultiplied RGB with union alpha.",
    inputs: &["background", "foreground"],
    parameters: &[
        other("mode", ParameterKind::BlendMode, "blend mode"),
        scalar("opacity", "factor", 0.0, 1.0, 1.0),
    ],
};
pub static MASK: NodeType = NodeType {
    type_id: "mask",
    version: 1,
    label: "Apply mask",
    description: "Multiply all premultiplied image channels by mask alpha; mask RGB is ignored.",
    inputs: &["image", "mask"],
    parameters: &[],
};
pub static COMBINE_MASK: NodeType = NodeType {
    type_id: "combine_mask",
    version: 1,
    label: "Combine masks",
    description: "Combine input alpha into a white premultiplied mask. For alpha a from mask and b from other: union a+b-a*b, intersect a*b, subtract a*(1-b), exclude a*(1-b)+b*(1-a). Input RGB is ignored; soft edges remain fractional.",
    inputs: &["mask", "other"],
    parameters: &[other("mode", ParameterKind::MaskMode, "coverage operation")],
};
pub static CHANNEL_MASK: NodeType = NodeType {
    type_id: "channel_mask",
    version: 1,
    label: "Channel mask",
    description: "Convert premultiplied red, green, blue, alpha or linear Rec.709 luminance to white mask coverage. Coverage clamps to [0, source alpha], preserving transparent boundaries without division by alpha.",
    inputs: &["image"],
    parameters: &[other("channel", ParameterKind::MaskChannel, "channel")],
};
pub static SURFACE_BLEND: NodeType = NodeType {
    type_id: "surface_blend",
    version: 1,
    label: "Surface blend",
    description: "Composite a premultiplied layer onto a surface using its local illumination, fine luminance detail, and dark recesses. Optional gradient displacement, edge softness and temporal grain run on GPU. This is image-based blending, not automatic object segmentation.",
    inputs: &["background", "foreground"],
    parameters: &[
        scalar("opacity", "factor", 0.0, 1.0, 1.0),
        scalar("lighting", "amount", 0.0, 1.0, 0.35),
        scalar("detail", "amount", 0.0, 1.0, 0.5),
        scalar("wear", "amount", 0.0, 1.0, 0.0),
        scalar("threshold", "contrast", 0.0, 1.0, 0.3),
        scalar("distortion", "pixels", 0.0, 16.0, 0.0),
        scalar("softness", "pixels", 0.0, 8.0, 0.0),
        scalar("grain", "amount", 0.0, 0.25, 0.0),
    ],
};

node!(Merge, MERGE, []);
node!(Blend, BLEND, [opacity]);
node!(Mask, MASK, []);
node!(CombineMask, COMBINE_MASK, []);
node!(ChannelMask, CHANNEL_MASK, []);
node!(
    SurfaceBlend,
    SURFACE_BLEND,
    [
        opacity, lighting, detail, wear, threshold, distortion, softness, grain
    ]
);
