//! Saved planar tracks and the warps that consume them.
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanarTrack {
    pub data: Box<planar::Track>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanarWarp {
    pub mode: planar::WarpMode,
}
