//! A marker: an empty point in a 3D scene that draws nothing, for cameras and lights to aim at, cameras to focus on and objects to face, keyed and moved like any part of the scene.
#[allow(unused_imports)]
use crate::registry::{NodeType, ParameterKind, PortKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

/// A point in the scene and nothing else.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker3d {
    #[serde(default)]
    pub placement: crate::nodes::Placement,
}

pub static MARKER_3D: NodeType = NodeType {
    type_id: "marker_3d",
    version: 1,
    label: "Marker",
    description: "An empty point in a 3D scene: it draws nothing and casts nothing. Cameras and lights with Aim at set to it keep pointing at it, a camera with Focus on set to it keeps it sharp, and objects with Face set to it turn towards it. Key its position to steer where a camera looks along a path; merge it under a 3D transform and it moves with what that transform moves. Placed like any part of the scene: position 0 is the frame's middle at depth 0.",
    inputs: &[],
    optional: &[],
    output: PortKind::Scene,
    parameters: &[other("placement", ParameterKind::Placement, "transform")],
};

node!(Marker3d, MARKER_3D, []);
