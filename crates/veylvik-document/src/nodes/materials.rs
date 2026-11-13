//! Procedural surface materials, reflections and glints.
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Material {
    pub kind: MaterialKind,
    pub scale: Scalar,
    pub bevel: Scalar,
    pub roughness: Scalar,
    pub lighting: Scalar,
    pub grain: Scalar,
    pub emission: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patina {
    pub kind: PatinaKind,
    pub seed: u32,
    pub amount: Scalar,
    pub dirt: Scalar,
    pub scale: Scalar,
    pub wear: Scalar,
    pub scratches: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetallicShine {
    pub x: Scalar,
    pub y: Scalar,
    pub width: Scalar,
    pub angle: Scalar,
    pub intensity: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SunGlints {
    pub seed: u32,
    pub size: Scalar,
    pub intensity: Scalar,
    pub threshold: Scalar,
    pub evolution: Scalar,
    pub density: Scalar,
    pub angle: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Glass {
    pub bevel: Scalar,
    pub refraction: Scalar,
    pub roughness: Scalar,
    pub ior: Scalar,
    pub transmission: Scalar,
    pub tint: Scalar,
    pub dispersion: Scalar,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceReflection {
    pub kind: ReflectionKind,
    pub bevel: Scalar,
    pub curvature: Scalar,
    pub roughness: Scalar,
    pub strength: Scalar,
    pub rotation: Scalar,
    pub scale: Scalar,
    pub offset_x: Scalar,
    pub offset_y: Scalar,
}

node!(
    Material,
    MATERIAL,
    [scale, bevel, roughness, lighting, grain, emission]
);
node!(Patina, PATINA, [amount, dirt, scale, wear, scratches]);
node!(
    MetallicShine,
    METALLIC_SHINE,
    [x, y, width, angle, intensity]
);
node!(
    SunGlints,
    SUN_GLINTS,
    [size, intensity, threshold, evolution, density, angle]
);
node!(
    Glass,
    GLASS,
    [
        bevel,
        refraction,
        roughness,
        ior,
        transmission,
        tint,
        dispersion
    ]
);
node!(
    SurfaceReflection,
    SURFACE_REFLECTION,
    [
        bevel, curvature, roughness, strength, rotation, scale, offset_x, offset_y
    ]
);
