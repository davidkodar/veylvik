//! Layering, blending and mask combination of two or more images.
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
