//! Nodes that synthesize images without inputs.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Solid {
    pub color: [f32; 4],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gradient {
    pub mode: GradientMode,
    pub start_x: Scalar,
    pub start_y: Scalar,
    pub end_x: Scalar,
    pub end_y: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Noise {
    pub seed: u32,
    pub scale: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FractalNoise {
    pub seed: u32,
    pub scale: Scalar,
    pub octaves: Scalar,
    pub roughness: Scalar,
    pub evolution: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fire {
    pub seed: u32,
    pub x: Scalar,
    pub y: Scalar,
    pub width: Scalar,
    pub height: Scalar,
    pub evolution: Scalar,
    pub intensity: Scalar,
    pub turbulence: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Atmosphere {
    pub kind: AtmosphereKind,
    pub seed: u32,
    pub x: Scalar,
    pub y: Scalar,
    pub width: Scalar,
    pub height: Scalar,
    pub evolution: Scalar,
    pub density: Scalar,
    pub detail: Scalar,
    pub lighting: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ellipse {
    pub x: Scalar,
    pub y: Scalar,
    pub radius_x: Scalar,
    pub radius_y: Scalar,
    pub feather: Scalar,
}

pub static SOLID: NodeType = NodeType {
    type_id: "solid",
    version: 1,
    label: "Solid",
    description: "Straight scene-linear Rec.709 colour, converted to premultiplied pixels.",
    inputs: &[],
    parameters: &[other("color", ParameterKind::Color, "linear RGBA")],
};
pub static GRADIENT: NodeType = NodeType {
    type_id: "gradient",
    version: 1,
    label: "Gradient",
    description: "Opaque grayscale ramp clamped to [0,1]. Linear projects from start to end; radial uses start as center and the endpoint distance as radius. Distances below 0.000001 pixels produce black. Connect Colorize for colors or Channel mask for alpha coverage.",
    inputs: &[],
    parameters: &[
        other("mode", ParameterKind::GradientMode, "gradient mode"),
        scalar("start_x", "pixels", -1_000_000.0, 1_000_000.0, 0.0),
        scalar("start_y", "pixels", -1_000_000.0, 1_000_000.0, 0.0),
        scalar("end_x", "pixels", -1_000_000.0, 1_000_000.0, 640.0),
        scalar("end_y", "pixels", -1_000_000.0, 1_000_000.0, 0.0),
    ],
};
pub static NOISE: NodeType = NodeType {
    type_id: "noise",
    version: 1,
    label: "Noise",
    description: "Deterministic cell noise with a seed.",
    inputs: &[],
    parameters: &[
        other("seed", ParameterKind::Seed, "integer"),
        scalar("scale", "pixels", 1.0, 8192.0, 18.0),
    ],
};
pub static FRACTAL_NOISE: NodeType = NodeType {
    type_id: "fractal_noise",
    version: 1,
    label: "Fractal noise",
    description: "Opaque grayscale fractal value noise with quintic interpolation. Scale sets the first octave cell size; octaves round to an integer, roughness controls successive amplitude, evolution moves through a continuous third dimension. Deterministic for a seed and parameter values.",
    inputs: &[],
    parameters: &[
        other("seed", ParameterKind::Seed, "integer"),
        scalar("scale", "pixels", 1.0, 8192.0, 128.0),
        scalar("octaves", "count", 1.0, 8.0, 4.0),
        scalar("roughness", "factor", 0.0, 1.0, 0.5),
        scalar("evolution", "units", -8192.0, 8192.0, 0.0),
    ],
};
pub static FIRE: NodeType = NodeType {
    type_id: "fire",
    version: 1,
    label: "Procedural fire",
    description: "Layered emitting fire volume with rising periodic turbulence and temperature variation. Transparent background; evolution advances upward and repeats every four units. Stateless, not a fluid simulation.",
    inputs: &[],
    parameters: &[
        other("seed", ParameterKind::Seed, "seed"),
        scalar("x", "pixels", -32768.0, 32768.0, 480.0),
        scalar("y", "pixels", -32768.0, 32768.0, 350.0),
        scalar("width", "pixels", 1.0, 8192.0, 600.0),
        scalar("height", "pixels", 1.0, 8192.0, 240.0),
        scalar("evolution", "cycles", -10000.0, 10000.0, 0.0),
        scalar("intensity", "factor", 0.0, 16.0, 1.0),
        scalar("turbulence", "factor", 0.0, 3.0, 1.0),
    ],
};
pub static ATMOSPHERE: NodeType = NodeType {
    type_id: "atmosphere",
    version: 1,
    label: "Smoke, fog and clouds",
    description: "Layered procedural density with turbulent advection, absorption and directional shading. Transparent smoke plumes, soot, fog layers, drifting banks and clouds. Evolution repeats every four units; deterministic approximation, not fluid simulation.",
    inputs: &[],
    parameters: &[
        other("kind", ParameterKind::AtmosphereKind, "atmosphere"),
        other("seed", ParameterKind::Seed, "integer"),
        scalar("x", "pixels", -32768.0, 32768.0, 480.0),
        scalar("y", "pixels", -32768.0, 32768.0, 500.0),
        scalar("width", "pixels", 1.0, 8192.0, 700.0),
        scalar("height", "pixels", 1.0, 8192.0, 450.0),
        scalar("evolution", "cycles", -10000.0, 10000.0, 0.0),
        scalar("density", "factor", 0.0, 16.0, 1.0),
        scalar("detail", "factor", 0.1, 4.0, 1.0),
        scalar("lighting", "factor", 0.0, 4.0, 1.0),
    ],
};
pub static ELLIPSE: NodeType = NodeType {
    type_id: "ellipse",
    version: 1,
    label: "Ellipse mask",
    description: "White premultiplied ellipse coverage. Feather uses normalized radial distance scaled by the smaller radius, with at least a one-pixel transition. Zero radius is transparent.",
    inputs: &[],
    parameters: &[
        scalar("x", "pixels", -1_000_000.0, 1_000_000.0, 320.0),
        scalar("y", "pixels", -1_000_000.0, 1_000_000.0, 180.0),
        scalar("radius_x", "pixels", 0.0, 8192.0, 100.0),
        scalar("radius_y", "pixels", 0.0, 8192.0, 100.0),
        scalar("feather", "pixels", 0.0, 8192.0, 1.0),
    ],
};

node!(Solid, SOLID, [], {
    fn validate(&self) -> Result<()> {
        if self.color.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&self.color[3]) {
            return fail("solid needs finite RGB and alpha in [0,1]");
        }
        Ok(())
    }
});
node!(Gradient, GRADIENT, [start_x, start_y, end_x, end_y]);
node!(Noise, NOISE, [scale]);
node!(
    FractalNoise,
    FRACTAL_NOISE,
    [scale, octaves, roughness, evolution]
);
node!(
    Fire,
    FIRE,
    [x, y, width, height, evolution, intensity, turbulence]
);
node!(
    Atmosphere,
    ATMOSPHERE,
    [x, y, width, height, evolution, density, detail, lighting]
);
node!(Ellipse, ELLIPSE, [x, y, radius_x, radius_y, feather]);
