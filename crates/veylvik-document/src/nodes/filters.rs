//! Spatial filters and transforms of one image.
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

node!(Blur, BLUR, [radius]);
node!(Morphology, MORPHOLOGY, [radius]);
node!(Displace, DISPLACE, [x, y]);
node!(Transform, TRANSFORM, [x, y]);
node!(
    Affine,
    AFFINE,
    [x, y, scale_x, scale_y, rotation, pivot_x, pivot_y]
);
