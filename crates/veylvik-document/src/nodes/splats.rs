//! Gaussian splats as a 3D scene part: a scanned place or object from a splat .ply file, filmed by the scene's cameras with its own captured light.
#[allow(unused_imports)]
use crate::registry::{NodeType, ParameterKind, PortKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

/// A splat file placed in the scene.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GaussianSplats {
    /// The .ply file (relative to the project, or absolute); empty shows nothing.
    pub file: String,
    /// The largest side of the box most of its splats lie in, in pixels.
    pub size: Scalar,
    /// How bright its captured colours are (1: as captured).
    pub brightness: Scalar,
    #[serde(default)]
    pub placement: crate::nodes::Placement,
}

pub static GAUSSIAN_SPLATS: NodeType = NodeType {
    type_id: "gaussian_splats",
    version: 1,
    label: "Gaussian splats",
    description: "A scanned place or object as Gaussian splats: millions of soft, coloured, see-through ellipsoids from a splat file: a .ply as 3D Gaussian splatting tools write it (binary, with view-dependent colour up to degree 3) or a compact .splat (no view-dependent colour); their colours, fitted to photographs, are taken into linear light, as a part of a 3D scene. It is centred on the box most of its splats lie in and scaled so that box's largest side is size pixels; placement moves, turns and scales it about the frame centre (scans often come in turned or upside down: turn them with placement). It shows the light it was captured in, times brightness, and takes no light from the scene; the scene's solids hide what is behind them, its splats are drawn over the solids, back to front. It casts no shadows and is not in a depth picture. At most 4,000,000 splats.",
    inputs: &[],
    optional: &[],
    output: PortKind::Scene,
    parameters: &[
        other("file", ParameterKind::FilePath, "splat .ply file"),
        scalar("size", "pixels", 1.0, 1_000_000.0, 600.0),
        scalar("brightness", "factor", 0.0, 64.0, 1.0),
        other(
            "placement",
            ParameterKind::Placement,
            "position, rotation, scale",
        ),
    ],
};

node!(GaussianSplats, GAUSSIAN_SPLATS, [size, brightness]);
