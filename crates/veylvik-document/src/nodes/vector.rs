//! Vector path masks.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Path {
    pub geometry: vector::PathMask,
}

pub static PATH: NodeType = NodeType {
    type_id: "path",
    version: 1,
    label: "Path mask",
    description: "Closed vector contours rasterized into an antialiased white alpha mask. Optional geometry.keys animate matching points with linear interpolation; point counts and segment types must match at every key.",
    inputs: &[],
    optional: &[],
    parameters: &[other("geometry", ParameterKind::Path, "pixels")],
};

node!(Path, PATH, [], {
    fn validate(&self) -> Result<()> {
        self.geometry.validate()
    }
});
