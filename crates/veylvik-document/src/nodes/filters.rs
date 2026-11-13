//! Spatial filters and transforms of one image.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Blur {
    pub radius: Scalar,
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
    description: "Box blur, radius rounded to an integer; transparent borders.",
    inputs: &["image"],
    parameters: &[scalar("radius", "pixels", 0.0, 256.0, 4.0)],
};
pub static MORPHOLOGY: NodeType = NodeType {
    type_id: "morphology",
    version: 1,
    label: "Grow/shrink mask",
    description: "Expand alpha with a positive radius (dilation), contract with a negative radius (erosion). Uses a square neighborhood and transparent borders. Radius rounds to the nearest integer, half away from zero. Outputs a white premultiplied mask, including at radius zero; source RGB is ignored.",
    inputs: &["image"],
    parameters: &[scalar("radius", "pixels", -256.0, 256.0, 2.0)],
};
pub static DISPLACE: NodeType = NodeType {
    type_id: "displace",
    version: 1,
    label: "Displace",
    description: "Map red/green displace image horizontally/vertically. Straight map 0.5 is neutral, 0/1 give negative/positive strength; map alpha attenuates displacement. Map channels clamp to [0,1] before alpha weighting. Bilinear premultiplied sampling with transparent borders.",
    inputs: &["image", "map"],
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
node!(Morphology, MORPHOLOGY, [radius]);
node!(Displace, DISPLACE, [x, y]);
node!(Transform, TRANSFORM, [x, y]);
node!(
    Affine,
    AFFINE,
    [x, y, scale_x, scale_y, rotation, pivot_x, pivot_y]
);
