//! Procedural surface materials, reflections and glints.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, other, scalar};
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Material {
    #[serde(default = "smooth_edges_default")]
    pub smooth_edges: bool,
    #[serde(default)]
    pub brushing: crate::optical::Brushing,
    #[serde(default)]
    pub studio: crate::optical::StudioRig,
    /// None uses the material family’s optical defaults.
    #[serde(default)]
    pub optics: Option<crate::optical::SurfaceOptics>,
    /// None uses the material family’s own pattern colours.
    #[serde(default)]
    pub colours: Option<crate::optical::Palette>,
    pub kind: MaterialKind,
    pub scale: Scalar,
    pub bevel: Scalar,
    pub roughness: Scalar,
    pub lighting: Scalar,
    pub grain: Scalar,
    pub emission: Scalar,
    /// How much of the reflection comes from the connected shot rather than the
    /// studio; unused without an Environment input.
    #[serde(default = "shot_default")]
    pub shot: Scalar,
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

fn smooth_edges_default() -> bool {
    true
}
/// Studio highlights carrying the shot's colours; 1 is a pure mirror of the shot.
fn shot_default() -> Scalar {
    0.6.into()
}

pub static MATERIAL: NodeType = NodeType {
    type_id: "material",
    version: 1,
    label: "Text surface",
    description: "Procedural gold, silver, brushed aluminium, bronze, phosphor, wood, light/dark oak, marble, light/red brick, rough concrete, clean/weathered paper and five leather colours from image alpha. Surface grain, edge bevel and studio reflections; source RGB is replaced and alpha preserved. Connect footage to the optional Environment input to reflect the shot instead of the studio: roughness blurs it and dielectrics take their diffuse light from it. Not a 3D mesh or spectral renderer.",
    inputs: &["image", "environment"],
    optional: &["environment"],
    parameters: &[
        Parameter {
            default: 1.0,
            ..other("smooth_edges", ParameterKind::Boolean, "quality")
        },
        other("kind", ParameterKind::MaterialKind, "material"),
        other("brushing", ParameterKind::Brushing, "surface finish"),
        other("studio", ParameterKind::StudioRig, "lighting rig"),
        other("optics", ParameterKind::SurfaceOptics, "optical response"),
        other("colours", ParameterKind::Palette, "pattern colours"),
        scalar("scale", "pixels", 1.0, 8192.0, 180.0),
        scalar("bevel", "pixels", 0.0, 256.0, 4.0),
        scalar("roughness", "factor", 0.0, 1.0, 0.15),
        scalar("lighting", "cycles", -10000.0, 10000.0, 0.0),
        scalar("grain", "factor", 0.0, 2.0, 0.4),
        scalar("emission", "factor", 0.0, 16.0, 1.0),
        scalar("shot", "factor", 0.0, 1.0, 0.6),
    ],
};
pub static PATINA: NodeType = NodeType {
    type_id: "patina",
    version: 1,
    label: "Tarnish and dirt",
    description: "Optional patchy tarnish, copper/bronze verdigris or iron-like rust with separate dirt and scratches. Preserves source alpha. Apply after reflections to dull dirty areas, then add glints if desired.",
    inputs: &["image"],
    optional: &[],
    parameters: &[
        other("kind", ParameterKind::PatinaKind, "weathering"),
        other("seed", ParameterKind::Seed, "integer"),
        scalar("amount", "factor", 0.0, 1.0, 0.4),
        scalar("dirt", "factor", 0.0, 1.0, 0.2),
        scalar("scale", "pixels", 1.0, 8192.0, 110.0),
        scalar("wear", "factor", 0.0, 1.0, 0.3),
        scalar("scratches", "factor", 0.0, 1.0, 0.15),
    ],
};
pub static METALLIC_SHINE: NodeType = NodeType {
    type_id: "metallic_shine",
    version: 1,
    label: "Metallic shine",
    description: "A warm-white specular streak with a sharp core and soft shoulder. Preserves source alpha exactly. Apply to isolated text or a logo before compositing the background; animate X/Y to sweep. Angle is the streak normal in degrees (zero makes a vertical streak).",
    inputs: &["image"],
    optional: &[],
    parameters: &[
        scalar("x", "pixels", -32768.0, 32768.0, 480.0),
        scalar("y", "pixels", -32768.0, 32768.0, 270.0),
        scalar("width", "pixels", 1.0, 8192.0, 90.0),
        scalar("angle", "degrees", -3600.0, 3600.0, 20.0),
        scalar("intensity", "factor", 0.0, 16.0, 1.5),
    ],
};
pub static SUN_GLINTS: NodeType = NodeType {
    type_id: "sun_glints",
    version: 1,
    label: "Sun glints",
    description: "Small animated star-like specular flashes anchored to bright source pixels. Deterministic seed and four-unit evolution loop; glints can extend beyond source coverage. Threshold and density select highlights.",
    inputs: &["image"],
    optional: &[],
    parameters: &[
        other("seed", ParameterKind::Seed, "integer"),
        scalar("size", "pixels", 1.0, 512.0, 18.0),
        scalar("intensity", "factor", 0.0, 16.0, 3.0),
        scalar("threshold", "linear luminance", 0.0, 16.0, 0.65),
        scalar("evolution", "cycles", -10000.0, 10000.0, 0.0),
        scalar("density", "factor", 0.0, 1.0, 0.55),
        scalar("angle", "degrees", -3600.0, 3600.0, 15.0),
    ],
};
pub static GLASS: NodeType = NodeType {
    type_id: "glass",
    version: 1,
    label: "Glass surface",
    description: "Clear or frosted glass from image alpha and an explicit background image. Bevel normals refract the background; roughness diffuses it, tint absorbs light and dispersion separates wavelengths. Composite this cutout over the same background; add Surface reflection for an independent reflected scene.",
    inputs: &["image", "background"],
    optional: &[],
    parameters: &[
        scalar("bevel", "pixels", 0.0, 256.0, 6.0),
        scalar("refraction", "pixels", 0.0, 256.0, 18.0),
        scalar("roughness", "pixels", 0.0, 256.0, 0.0),
        scalar("ior", "factor", 1.0, 3.0, 1.5),
        scalar("transmission", "factor", 0.0, 1.0, 0.96),
        scalar("tint", "factor", 0.0, 2.0, 0.12),
        scalar("dispersion", "factor", 0.0, 1.0, 0.06),
    ],
};
pub static SURFACE_REFLECTION: NodeType = NodeType {
    type_id: "surface_reflection",
    version: 1,
    label: "Surface reflection",
    description: "Reflect a second image or animated branch in a metal/glass surface. Alpha-derived bevel normals warp the reflection; roughness softens it. Metal uses the surface tint, glass uses Fresnel weighting. Image-based 2.5D mapping, not scene ray tracing.",
    inputs: &["image", "reflection"],
    optional: &[],
    parameters: &[
        other("kind", ParameterKind::ReflectionKind, "surface"),
        scalar("bevel", "pixels", 0.0, 256.0, 5.0),
        scalar("curvature", "factor", -2.0, 2.0, 0.35),
        scalar("roughness", "pixels", 0.0, 256.0, 1.0),
        scalar("strength", "factor", 0.0, 1.0, 0.7),
        scalar("rotation", "degrees", -3600.0, 3600.0, 0.0),
        scalar("scale", "factor", 0.01, 100.0, 1.0),
        scalar("offset_x", "pixels", -32768.0, 32768.0, 0.0),
        scalar("offset_y", "pixels", -32768.0, 32768.0, 0.0),
    ],
};

node!(
    Material,
    MATERIAL,
    [scale, bevel, roughness, lighting, grain, emission, shot],
    {
        fn validate(&self) -> Result<()> {
            self.studio.validate()?;
            if let Some(optics) = self.optics {
                optics.validate()?;
            }
            if let Some(colours) = &self.colours {
                crate::optical::validate_palette(colours)?;
            }
            Ok(())
        }
    }
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

/// A water coating over an existing surface; coverage and the substrate survive.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Condensation {
    pub dispersion: Scalar,
    pub seed: u32,
    pub amount: Scalar,
    pub size: Scalar,
    pub density: Scalar,
    pub haze: Scalar,
    pub evolution: Scalar,
    pub trails: Scalar,
}
pub static CONDENSATION: NodeType = NodeType {
    type_id: "condensation",
    version: 1,
    label: "Condensation",
    description: "Water beads, fine moisture and subtle sliding trails over the input surface. Preserves its alpha and material, refracts its image locally, and inherits the nearest upstream material lighting. Uses a studio environment when none exists. Stateless procedural 2.5D coating, not a fluid simulation.",
    inputs: &["image"],
    optional: &[],
    parameters: &[
        other("seed", ParameterKind::Seed, "seed"),
        scalar("amount", "factor", 0.0, 1.0, 0.7),
        scalar("size", "pixels", 1.0, 128.0, 4.0),
        scalar("density", "factor", 0.0, 1.0, 0.55),
        scalar("haze", "factor", 0.0, 1.0, 0.18),
        scalar("dispersion", "factor", 0.0, 0.2, 0.012),
        scalar("evolution", "seconds", -10000.0, 10000.0, 0.0),
        scalar("trails", "factor", 0.0, 1.0, 0.2),
    ],
};
node!(
    Condensation,
    CONDENSATION,
    [amount, size, density, haze, evolution, trails, dispersion]
);
