//! 3D objects as particles: a scene node that puts a copy of its objects at every particle of a Particles node.
#[allow(unused_imports)]
use crate::registry::{NodeType, ParameterKind, PortKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

/// Copies of the objects at `scene`, one at each particle of the Particles node at
/// `particles`; a 3D scene part.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticleInstances {
    /// Each copy's size as a factor of the object's own.
    pub scale: Scalar,
    /// Copies start turned every way at random, rather than all as the object is.
    #[serde(default = "yes")]
    pub tumble: bool,
}
fn yes() -> bool {
    true
}
/// Most copies drawn: the first so many particles of the system.
pub const INSTANCES_MOST: u32 = 100_000;

pub static PARTICLE_INSTANCES: NodeType = NodeType {
    type_id: "particle_instances",
    version: 1,
    label: "Particle instances",
    description: "3D objects as particles: a copy of the shapes, 3D texts, image cards and models connected to scene (their own place left out, their turn and size kept) at every particle of the Particles node connected to particles, as a part of a 3D scene; merge it into a scene and film it with Render scene. The copies are lit and shadowed like the scene's other solids and hide what is behind them. Each copy is scale times the object's own size, times the particle's growth over its life (grow or size_over_life) and its variation; with tumble it starts turned every way at random, and it turns at the particle's spin degrees a second about an axis of its own; it takes the particle's colour over its life as a tint. The particles' own camera, size, glow, fade and opacity do not apply to copies, and the copies cast no shadows and fade only by shrinking. The particles must not use an emitter, target, field or obstacle image. At most 100,000 copies (the first particles of the system).",
    inputs: &["scene", "particles"],
    optional: &[],
    output: PortKind::Scene,
    parameters: &[
        scalar("scale", "factor", 0.0, 1000.0, 1.0),
        crate::registry::Parameter {
            default: 1.0,
            ..other("tumble", ParameterKind::Boolean, "turned at random")
        },
    ],
};

node!(ParticleInstances, PARTICLE_INSTANCES, [scale]);
