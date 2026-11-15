//! Saved planar tracks and the warps that consume them.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, other, scalar};
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

pub static PLANAR_TRACK: NodeType = NodeType {
    type_id: "planar_track",
    version: 1,
    label: "Planar track",
    description: "Reusable normalized surface motion and feature diagnostics. Image passes through unchanged. Connect its output to any Planar warp track input.",
    inputs: &["image"],
    optional: &[],
    parameters: &[other(
        "data",
        ParameterKind::PlanarTrack,
        "normalized image coordinates",
    )],
};
pub static PLANAR_WARP: NodeType = NodeType {
    type_id: "planar_warp",
    version: 1,
    label: "Planar warp",
    description: "GPU perspective sampling driven by a Planar track. Follow reference placement, fit the full image to the surface, or stabilize the tracked plane.",
    inputs: &["image", "track"],
    optional: &[],
    parameters: &[other("mode", ParameterKind::PlanarMode, "")],
};

node!(PlanarTrack, PLANAR_TRACK, [], {
    fn validate(&self) -> Result<()> {
        self.data.validate()
    }
});
node!(PlanarWarp, PLANAR_WARP, []);
