//! Forces on particles as nodes of their own: each pushes particles in its own way, at its own place and reach, and they chain into a Particles node's Forces input.
#[allow(unused_imports)]
use crate::registry::{NodeType, ParameterKind, PortKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

/// How a force moves particles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForceKind {
    /// Draws particles to its point (negative strength drives them away).
    #[default]
    Attract,
    /// Swings particles round an axis through its point.
    Vortex,
    /// Blows them one way.
    Wind,
    /// Stirs them in swirls.
    Turbulence,
    /// Slows them down.
    Drag,
}
impl ForceKind {
    pub const ALL: [&'static str; 5] = ["attract", "vortex", "wind", "turbulence", "drag"];
}
/// A force on particles; produces no image.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Force {
    #[serde(default)]
    pub kind: ForceKind,
    pub strength: Scalar,
    pub x: Scalar,
    pub y: Scalar,
    pub z: Scalar,
    pub radius: Scalar,
    pub softness: Scalar,
    pub direction: Scalar,
    pub lean: Scalar,
    pub scale: Scalar,
}
/// Most forces one Particles node follows (a chain of them).
pub const FORCES_MOST: usize = 8;

pub static FORCE: NodeType = NodeType {
    type_id: "force",
    version: 1,
    label: "Force",
    description: "A force on particles, as a node of its own: connect it to the Forces input of a Particles node, and chain more forces into its own Forces input (up to 8 act on one system, and one force may drive several systems). Kind attract draws particles towards the point x, y, z (pixels in the scenes' space: the frame at depth 0, z towards the viewer) at strength pixels per second squared, easing off within 40 pixels of it (negative strength drives them away); vortex swings them round the axis through that point (pointing at the viewer, turned direction degrees about it and leaned lean degrees from it) at strength pixels per second squared; wind blows them along direction degrees (0 right, 90 up), leaned lean degrees towards the viewer, at strength pixels per second squared; turbulence stirs them in swirls about scale pixels across at strength pixels per second, never bunching them; drag slows them by strength per second (1 loses about two thirds of their speed in a second). Radius limits a force to within that many pixels of its point (0: everywhere), fading out over the outer softness share of it. Keyed settings are followed while the particles fly. Produces no image.",
    inputs: &["forces"],
    optional: &["forces"],
    output: PortKind::Forces,
    parameters: &[
        other("kind", ParameterKind::ForceKind, "force"),
        scalar(
            "strength",
            "pixels per second squared",
            -1_000_000.0,
            1_000_000.0,
            200.0,
        ),
        scalar("x", "pixels", -1_000_000.0, 1_000_000.0, 960.0),
        scalar("y", "pixels", -1_000_000.0, 1_000_000.0, 540.0),
        scalar("z", "pixels", -1_000_000.0, 1_000_000.0, 0.0),
        scalar("radius", "pixels", 0.0, 1_000_000.0, 0.0),
        scalar("softness", "share", 0.0, 1.0, 0.5),
        scalar("direction", "degrees", -36_000.0, 36_000.0, 0.0),
        scalar("lean", "degrees", -90.0, 90.0, 0.0),
        scalar("scale", "pixels", 1.0, 100_000.0, 120.0),
    ],
};

node!(
    Force,
    FORCE,
    [strength, x, y, z, radius, softness, direction, lean, scale]
);
