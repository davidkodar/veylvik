//! Procedural surface materials, reflections and glints.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, PortKind, other, scalar};
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
    pub colours: Option<crate::optical::PaletteTrack>,
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
    /// How strongly a connected Normal map tilts the surface (1 = as authored).
    #[serde(default = "one")]
    pub normal_strength: Scalar,
    /// Height of a connected Height map's white, in pixels.
    #[serde(default = "height_depth_default")]
    pub height_depth: Scalar,
    /// The Normal map uses the DirectX convention (green points down).
    #[serde(default)]
    pub normal_directx: bool,
    /// Turns the grain of brushing, ridges and wear (degrees).
    #[serde(default = "nothing")]
    pub brush_angle: Scalar,
    /// Long shallow undulations along the grain that break reflections into bands.
    #[serde(default = "nothing")]
    pub ridges: Scalar,
    /// Uneven polish, a plate that is not quite flat, and faint scratches.
    #[serde(default = "nothing")]
    pub imperfections: Scalar,
}
fn nothing() -> Scalar {
    0.0.into()
}
impl Material {
    /// Whether the material is a metal (by its optics, else its family).
    pub fn is_metal(&self) -> bool {
        self.optics.map_or(
            matches!(
                self.kind,
                MaterialKind::Gold
                    | MaterialKind::Silver
                    | MaterialKind::Aluminium
                    | MaterialKind::Bronze
                    | MaterialKind::LiquidMetal
            ),
            |o| o.metallic,
        )
    }
    /// Light and finish a metal so it looks like one: strip lights to reflect, the
    /// grain turned to cross them, shallow ridges and a little wear. Roughness, grain
    /// and brushing are left alone, so a named finish (Polished, Brushed, ...) stays.
    pub fn shine(&mut self) {
        self.studio = crate::optical::StudioRig::strips();
        self.brush_angle = 65.0.into();
        self.ridges = 0.6.into();
        self.imperfections = 0.3.into();
    }
}
fn one() -> Scalar {
    1.0.into()
}
fn height_depth_default() -> Scalar {
    4.0.into()
}
/// Two rendered materials of one shape, the over layer shown through a mask and worn
/// away along the shape's edges (most at corners), like chipped paint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialLayers {
    pub seed: u32,
    pub mix: Scalar,
    pub edge_wear: Scalar,
    pub wear_width: Scalar,
    pub breakup: Scalar,
    pub noise_size: Scalar,
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
    description: "Procedural gold, silver, brushed aluminium, bronze, phosphor, wood, light/dark oak, marble, light/red brick, rough concrete, clean/weathered paper and five leather colours from image alpha. Surface grain, edge bevel and studio reflections; source RGB is replaced and alpha preserved. With the Image texture surface, an image, logo or footage on the optional Texture input becomes the surface colour, lit and bevelled like any material. Connect footage to the optional Environment input to reflect the shot instead of the studio: roughness blurs it and dielectrics take their diffuse light from it. Optional PBR maps (for example from ambientCG), placed like the texture: a Normal map (OpenGL green-up by default, DirectX with normal_directx) tilts the surface by normal_strength, a Roughness map sets roughness per pixel (Roughness 0.5 reads it as authored, lower is glossier, higher rougher) and a Height map adds relief up to height_depth pixels. Maps are read as stored 8/16-bit data, not colour. Not a 3D mesh or spectral renderer.",
    inputs: &[
        "image",
        "environment",
        "texture",
        "lights",
        "normal_map",
        "roughness_map",
        "height_map",
    ],
    optional: &[
        "environment",
        "texture",
        "lights",
        "normal_map",
        "roughness_map",
        "height_map",
    ],
    output: PortKind::Image,
    parameters: &[
        Parameter {
            default: 1.0,
            ..other("smooth_edges", ParameterKind::Boolean, "quality")
        },
        other("normal_directx", ParameterKind::Boolean, "map convention"),
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
        scalar("normal_strength", "factor", 0.0, 4.0, 1.0),
        scalar("height_depth", "pixels", 0.0, 64.0, 4.0),
        scalar("brush_angle", "degrees", -180.0, 180.0, 0.0),
        scalar("ridges", "factor", 0.0, 1.0, 0.0),
        scalar("imperfections", "factor", 0.0, 1.0, 0.0),
    ],
};
pub static MATERIAL_LAYERS: NodeType = NodeType {
    type_id: "material_layers",
    version: 1,
    label: "Layered material",
    description: "Layers two materials of the same shape: the Over material shows where the optional Mask's alpha is (everywhere without a mask), scaled by Mix, and Edge wear wears it away along the Under input's edges to reveal the Under material, most at corners, broken up by noise. Connect two Text surface (or any) renders of one title, for example red paint over steel. Coverage comes from the inputs; output alpha blends like the colour.",
    inputs: &["under", "over", "mask"],
    optional: &["mask"],
    output: PortKind::Image,
    parameters: &[
        other("seed", ParameterKind::Seed, "integer"),
        scalar("mix", "factor", 0.0, 1.0, 1.0),
        scalar("edge_wear", "factor", 0.0, 1.0, 0.5),
        scalar("wear_width", "pixels", 0.0, 128.0, 8.0),
        scalar("breakup", "factor", 0.0, 1.0, 0.6),
        scalar("noise_size", "pixels", 1.0, 4096.0, 40.0),
    ],
};
pub static PATINA: NodeType = NodeType {
    type_id: "patina",
    version: 1,
    label: "Tarnish and dirt",
    description: "Optional patchy tarnish, copper/bronze verdigris or iron-like rust with separate dirt and scratches. Preserves source alpha. Apply after reflections to dull dirty areas, then add glints if desired.",
    inputs: &["image"],
    optional: &[],
    output: PortKind::Image,
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
    output: PortKind::Image,
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
    output: PortKind::Image,
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
    output: PortKind::Image,
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
    output: PortKind::Image,
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
    [
        scale,
        bevel,
        roughness,
        lighting,
        grain,
        emission,
        shot,
        normal_strength,
        height_depth,
        brush_angle,
        ridges,
        imperfections
    ],
    {
        fn validate(&self) -> Result<()> {
            self.studio.validate()?;
            if let Some(optics) = self.optics {
                optics.validate()?;
            }
            if let Some(colours) = &self.colours {
                crate::optical::validate_palette_track(colours)?;
            }
            Ok(())
        }
    }
);
node!(Patina, PATINA, [amount, dirt, scale, wear, scratches]);
node!(
    MaterialLayers,
    MATERIAL_LAYERS,
    [mix, edge_wear, wear_width, breakup, noise_size]
);
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
    output: PortKind::Image,
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
