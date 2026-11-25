//! A human figure as a 3D scene part: a person who walks (more movements later) with a smooth body, filmed by the scene's cameras and lit by its lights, and a surface particles can be born on.
#[allow(unused_imports)]
use crate::registry::{NodeType, ParameterKind, PortKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

fn skin() -> crate::Track3 {
    crate::Track3::Constant([0.62, 0.6, 0.58])
}
fn walking() -> Scalar {
    Scalar::Constant { value: 1.0 }
}
fn fine() -> Scalar {
    Scalar::Constant { value: 110.0 }
}

/// A person in the scene, walking along its own forward direction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Figure3d {
    /// How tall the figure stands, in pixels.
    pub height: Scalar,
    /// How far it has walked along its forward direction, in pixels (key it to walk).
    pub distance: Scalar,
    /// 1 walks, 0 stands still; between, the stride eases to standing.
    #[serde(default = "walking")]
    pub pace: Scalar,
    /// How finely the body is shaped: cells of the mesh to the height (24 to 400).
    #[serde(default = "fine")]
    pub detail: Scalar,
    #[serde(default)]
    pub placement: crate::nodes::Placement,
    #[serde(default = "skin")]
    pub colour: crate::Track3,
    pub roughness: Scalar,
    pub metallic: Scalar,
    /// A library material shading the body; colour, roughness and metallic then do not apply.
    #[serde(default)]
    pub look: Option<Box<crate::nodes::Material>>,
}

pub static FIGURE_3D: NodeType = NodeType {
    type_id: "figure_3d",
    version: 1,
    label: "Figure",
    description: "A person as a part of a 3D scene: a smooth adult body (height pixels tall) that walks as people do. Its legs move on the measured joint angles of normal walking with the standing foot kept where it landed, the body rises and falls a few centimetres and passes its weight from side to side, the hips sag on the swinging side, the arms swing loosely against the legs and the head stays level. It walks along its own forward direction (the frame's +x before placement turns it): key distance (pixels walked) to make it walk, a stride being 0.79 of its height (relaxed walking covers about 0.7 of its height a second); pace 1 walks and 0 eases it to standing. It stands on the ground at the frame's centre line plus half its height (placement moves, turns and scales it; turn it about y to walk another way, towards or away from the camera). Detail sets how finely the body is shaped (cells of the mesh to the height, 24 to 400). Colour, roughness and metallic shade it unless a Look (any library material) is set. Particles with Emit from set to it are born all over its body as it walks, and left behind as it moves on.",
    inputs: &[],
    optional: &[],
    output: PortKind::Scene,
    parameters: &[
        scalar("height", "pixels", 1.0, 100_000.0, 600.0),
        scalar("distance", "pixels", -10_000_000.0, 10_000_000.0, 0.0),
        scalar("pace", "factor", 0.0, 1.0, 1.0),
        scalar("detail", "cells", 24.0, 400.0, 110.0),
        other("placement", ParameterKind::Placement, "transform"),
        other("colour", ParameterKind::Colour3, "linear RGB"),
        scalar("roughness", "factor", 0.0, 1.0, 0.55),
        scalar("metallic", "factor", 0.0, 1.0, 0.0),
        other("look", ParameterKind::Look, "library material"),
    ],
};

node!(
    Figure3d,
    FIGURE_3D,
    [height, distance, pace, detail, roughness, metallic]
);
