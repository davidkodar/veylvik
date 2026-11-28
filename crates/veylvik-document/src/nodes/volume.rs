//! A volume: smoke, mist, haze or dust filling a shape in a 3D scene, a see-through medium that light scatters in and is dimmed by, lit and shadowed by the scene's lights and seen in front of and behind its objects.
#[allow(unused_imports)]
use crate::registry::{NodeType, ParameterKind, PortKind, integer, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

/// What a volume fills.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolumeShape {
    /// A box size x by y by z.
    #[default]
    Box,
    /// An ellipsoid within that box.
    Sphere,
    /// Mist lying on the bottom of the box, thinning upwards over its height.
    Ground,
}
choices!(VolumeShape {
    Box,
    Sphere,
    Ground
});

/// A medium filling a shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Volume3d {
    #[serde(default)]
    pub placement: crate::nodes::Placement,
    #[serde(default)]
    pub shape: VolumeShape,
    pub size_x: Scalar,
    pub size_y: Scalar,
    pub size_z: Scalar,
    /// How thick it is: light going 100 pixels through it is dimmed to e^-density.
    pub density: Scalar,
    /// The colour of the light it scatters (linear RGB).
    pub colour: Track3,
    /// The share of the light it stops that it swallows rather than scatters (0 white
    /// smoke, 1 black soot).
    pub absorption: Scalar,
    /// Which way it scatters light: towards where the light was going (up to 1: haze in
    /// a sunbeam glows looking towards the sun), evenly (0) or back (down to -1).
    pub forward: Scalar,
    /// How much noise breaks it into wisps and clumps (0: even).
    pub noise: Scalar,
    /// The size of the noise's largest clumps in pixels.
    pub noise_size: Scalar,
    /// Layers of finer noise over the largest (1 to 6).
    pub detail: Scalar,
    /// How fast the noise drifts, in pixels a second along x, y and z.
    pub drift_x: Scalar,
    pub drift_y: Scalar,
    pub drift_z: Scalar,
    /// How far in from its edges it fades, as a share of its half size.
    pub softness: Scalar,
    /// Which noise; the same seed always gives the same clumps.
    #[serde(default)]
    pub seed: u32,
}

pub static VOLUME_3D: NodeType = NodeType {
    type_id: "volume_3d",
    version: 1,
    label: "Volume",
    description: "Smoke, mist, haze or dust filling a shape in a 3D scene: a see-through medium that the scene's lights shine into and through, so it glows where it is lit, is dark where objects shade it (light shafts through haze) and dims what is behind it. Shape box fills a box size_x by size_y by size_z pixels, sphere the ellipsoid within it, and ground lies on the box's bottom and thins upwards over its height, as mist on a field. Density is how thick it is (light going 100 pixels through it is dimmed to e to the minus density); Colour is the colour of the light it scatters, and Absorption the share it swallows instead (0 white smoke, 1 black soot). Forward sets which way it scatters: up to 1 most light goes on the way it was going, so haze glows looking towards the light, 0 evenly, below 0 back. Noise breaks it into wisps and clumps, Noise size pixels across at the largest, with Detail layers of finer noise, drifting at drift_x, drift_y and drift_z pixels a second; Softness fades it in from its edges; Seed picks the noise. It is placed like any part of the scene (position 0 is the frame's middle at depth 0) and casts no shadows on objects. Outputs a scene.",
    inputs: &[],
    optional: &[],
    output: PortKind::Scene,
    parameters: &[
        other("placement", ParameterKind::Placement, "transform"),
        other("shape", ParameterKind::VolumeShape, "what it fills"),
        scalar("size_x", "pixels", 1.0, 1_000_000.0, 600.0),
        scalar("size_y", "pixels", 1.0, 1_000_000.0, 400.0),
        scalar("size_z", "pixels", 1.0, 1_000_000.0, 600.0),
        scalar("density", "per 100 pixels", 0.0, 100.0, 0.5),
        other("colour", ParameterKind::Colour3, "linear RGB"),
        scalar("absorption", "factor", 0.0, 1.0, 0.0),
        scalar("forward", "factor", -0.95, 0.95, 0.3),
        scalar("noise", "factor", 0.0, 1.0, 0.6),
        scalar("noise_size", "pixels", 1.0, 1_000_000.0, 150.0),
        scalar("detail", "layers", 1.0, 6.0, 4.0),
        scalar("drift_x", "pixels a second", -100_000.0, 100_000.0, 0.0),
        scalar("drift_y", "pixels a second", -100_000.0, 100_000.0, 0.0),
        scalar("drift_z", "pixels a second", -100_000.0, 100_000.0, 0.0),
        scalar("softness", "factor", 0.0, 1.0, 0.3),
        other("seed", ParameterKind::Seed, "integer"),
    ],
};

node!(
    Volume3d,
    VOLUME_3D,
    [
        size_x, size_y, size_z, density, absorption, forward, noise, noise_size, detail, drift_x,
        drift_y, drift_z, softness
    ]
);
