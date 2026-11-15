//! Shared light rigs: lights as graph objects that any number of surfaces can use.
#[allow(unused_imports)]
use crate::registry::{NodeType, ParameterKind, PortKind, other};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

/// Environment, ambient and area lights; produces no image. Materials connect it
/// to their Lights input instead of using their own rig.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightRig {
    #[serde(default)]
    pub studio: crate::optical::StudioRig,
}

pub static LIGHT_RIG: NodeType = NodeType {
    type_id: "light_rig",
    version: 1,
    label: "Light rig",
    description: "Shared lighting: the studio environment, ambient light and up to eight area lights. Connect it to the Lights input of any number of Text surface nodes; moving a light updates every surface and the shadows that follow them. Produces no image; its output only connects to Lights inputs. Connect a Planar track to the optional Track input and lights set to follow ride on the tracked point.",
    inputs: &["track"],
    optional: &["track"],
    output: PortKind::Lights,
    parameters: &[other("studio", ParameterKind::StudioRig, "lighting rig")],
};

node!(LightRig, LIGHT_RIG, [], {
    fn validate(&self) -> Result<()> {
        self.studio.validate()
    }
});
