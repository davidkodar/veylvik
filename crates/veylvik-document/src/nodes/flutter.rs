//! Flutter: ripples everything in a 3D scene with travelling waves out of its surface, as a banner or a cloth moves in wind, pinned along one edge if wanted; the shapes are bent where they are built, so 3D renders and particles born on them both follow.
#[allow(unused_imports)]
use crate::registry::{NodeType, ParameterKind, PortKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

/// Waves through the shapes of the scene on its input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flutter3d {
    /// How far the surface swings out of its plane, in pixels.
    pub amplitude: Scalar,
    /// The distance from one crest to the next, in pixels.
    pub wavelength: Scalar,
    /// How fast the waves travel along the shape, in pixels a second.
    pub speed: Scalar,
    /// 1 holds the shape's left edge still and lets the waves grow to its free end, as a
    /// flag on its pole; 0 moves it all alike.
    pub pinned: Scalar,
}

pub static FLUTTER_3D: NodeType = NodeType {
    type_id: "flutter_3d",
    version: 1,
    label: "Flutter",
    description: "Ripples every shape, 3D text, card, figure and model on its Scene input with waves travelling along it (along its own x, left to right) that swing its surface out of its plane by Amplitude pixels, a crest every Wavelength pixels, moving at Speed pixels a second, with a smaller ripple crossing them so it never looks mechanical: a banner, a flag, a sheet of cloth in wind. Pinned 1 holds the shape's left edge still and lets the swing grow to its free end; 0 moves all of it alike. A flat shape needs enough Detail to bend smoothly. The shapes are bent where they are built, so the 3D render, shadows, and particles born on them or gathering onto them all follow the waves. Cameras and lights on the input pass through unchanged.",
    inputs: &["scene"],
    optional: &[],
    output: PortKind::Scene,
    parameters: &[
        scalar("amplitude", "pixels", 0.0, 10000.0, 40.0),
        scalar("wavelength", "pixels", 1.0, 100000.0, 400.0),
        scalar("speed", "pixels per second", -100000.0, 100000.0, 300.0),
        scalar("pinned", "factor", 0.0, 1.0, 1.0),
    ],
};

node!(
    Flutter3d,
    FLUTTER_3D,
    [amplitude, wavelength, speed, pinned]
);
