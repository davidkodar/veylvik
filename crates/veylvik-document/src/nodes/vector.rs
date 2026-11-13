//! Vector path masks.
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Path {
    pub geometry: vector::PathMask,
}

node!(Path, PATH, [], {
    fn validate(&self) -> Result<()> {
        self.geometry.validate()
    }
});
