//! Nodes that synthesize images without inputs.
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
