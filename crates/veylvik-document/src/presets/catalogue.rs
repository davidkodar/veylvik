//! Built-in material looks as data: a surface family plus colours, optics and finish.
use crate::{
    MaterialKind::{self, *},
    optical::{Brushing, Palette, SurfaceOptics},
};

pub struct Entry {
    pub id: &'static str,
    pub label: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub kind: MaterialKind,
    pub colours: Option<Palette>,
    /// Metal reflectance colour; None keeps the family's optics.
    pub reflectance: Option<[f32; 3]>,
    /// Refractive index of a see-through or dielectric look (glass 1.5, ice 1.31,
    /// diamond 2.42); None keeps the family's.
    pub ior: Option<f32>,
    /// Bevel width in pixels; None uses the library's 5.
    pub bevel: Option<f32>,
    pub roughness: f32,
    pub grain: f32,
    pub brushing: Brushing,
}

const fn look(
    id: &'static str,
    label: &'static str,
    category: &'static str,
    description: &'static str,
    kind: MaterialKind,
    roughness: f32,
) -> Entry {
    Entry {
        id,
        label,
        category,
        description,
        kind,
        colours: None,
        reflectance: None,
        ior: None,
        bevel: None,
        roughness,
        grain: 0.3,
        brushing: Brushing::Smooth,
    }
}
const fn metal(
    id: &'static str,
    label: &'static str,
    description: &'static str,
    kind: MaterialKind,
    reflectance: [f32; 3],
    roughness: f32,
) -> Entry {
    Entry {
        reflectance: Some(reflectance),
        ..look(id, label, "Metal", description, kind, roughness)
    }
}
const fn painted(
    id: &'static str,
    label: &'static str,
    category: &'static str,
    description: &'static str,
    kind: MaterialKind,
    colours: Palette,
    roughness: f32,
) -> Entry {
    Entry {
        colours: Some(colours),
        ..look(id, label, category, description, kind, roughness)
    }
}
const Z: [f32; 3] = [0.0; 3];

pub static ENTRIES: [Entry; 48] = [
    metal(
        "copper",
        "Copper",
        "Warm polished copper.",
        Gold,
        [0.92, 0.52, 0.36],
        0.2,
    ),
    metal(
        "rose_gold",
        "Rose Gold",
        "Pink-tinted gold, polished.",
        Gold,
        [0.96, 0.72, 0.62],
        0.16,
    ),
    Entry {
        grain: 0.1,
        ..metal(
            "chrome",
            "Chrome",
            "Mirror chrome: crisp, dark-edged reflections.",
            Silver,
            [0.55, 0.56, 0.56],
            0.04,
        )
    },
    metal(
        "black_chrome",
        "Black Chrome",
        "Smoked mirror finish.",
        Silver,
        [0.22, 0.22, 0.24],
        0.08,
    ),
    Entry {
        brushing: Brushing::Directional,
        grain: 0.65,
        ..metal(
            "brushed_steel",
            "Brushed Steel",
            "Directional brushed stainless steel.",
            Aluminium,
            [0.56, 0.57, 0.58],
            0.3,
        )
    },
    metal(
        "anodised_blue",
        "Anodised Blue",
        "Satin anodised aluminium, blue.",
        Aluminium,
        [0.25, 0.45, 0.95],
        0.32,
    ),
    metal(
        "anodised_red",
        "Anodised Red",
        "Satin anodised aluminium, red.",
        Aluminium,
        [0.9, 0.15, 0.18],
        0.32,
    ),
    metal(
        "anodised_purple",
        "Anodised Purple",
        "Satin anodised aluminium, purple.",
        Aluminium,
        [0.55, 0.25, 0.85],
        0.32,
    ),
    look(
        "holographic_foil",
        "Holographic Foil",
        "Metal",
        "Mirror foil whose rainbow shifts with the surface angle; animate Reflection motion to make it shimmer.",
        Holographic,
        0.12,
    ),
    painted(
        "gloss_red_plastic",
        "Gloss Red Plastic",
        "Glossy",
        "Moulded red plastic with a wet-look shine.",
        Plastic,
        [[0.6, 0.05, 0.04], Z, Z],
        0.18,
    ),
    painted(
        "matte_white_plastic",
        "Matte White Plastic",
        "Glossy",
        "Soft white plastic, product-shot clean.",
        Plastic,
        [[0.75, 0.75, 0.73], Z, Z],
        0.6,
    ),
    painted(
        "blue_enamel",
        "Blue Enamel",
        "Glossy",
        "Deep glossy enamel, like a badge or pin.",
        Plastic,
        [[0.03, 0.12, 0.45], Z, Z],
        0.08,
    ),
    Entry {
        grain: 0.5,
        ..painted(
            "black_rubber",
            "Black Rubber",
            "Glossy",
            "Matte black rubber.",
            Plastic,
            [[0.02, 0.02, 0.02], Z, Z],
            0.85,
        )
    },
    painted(
        "car_paint_blue",
        "Blue Car Paint",
        "Glossy",
        "Metallic blue paint with flakes under a clear coat.",
        CarPaint,
        [[0.02, 0.05, 0.25], [0.6, 0.7, 0.9], Z],
        0.12,
    ),
    painted(
        "car_paint_red",
        "Red Car Paint",
        "Glossy",
        "Candy red metallic paint.",
        CarPaint,
        [[0.35, 0.01, 0.01], [0.9, 0.5, 0.4], Z],
        0.12,
    ),
    painted(
        "car_paint_graphite",
        "Graphite Car Paint",
        "Glossy",
        "Dark grey metallic paint.",
        CarPaint,
        [[0.04, 0.045, 0.05], [0.6, 0.6, 0.62], Z],
        0.14,
    ),
    painted(
        "carbon_fibre",
        "Carbon Fibre",
        "Glossy",
        "Twill-woven carbon under clear resin.",
        CarbonFibre,
        [[0.01, 0.01, 0.012], [0.3, 0.3, 0.32], Z],
        0.2,
    ),
    painted(
        "black_marble",
        "Black Marble",
        "Natural",
        "Black stone with gold veins.",
        Marble,
        [[0.02, 0.02, 0.025], [0.8, 0.55, 0.15], Z],
        0.12,
    ),
    painted(
        "green_marble",
        "Green Marble",
        "Natural",
        "Deep green stone with pale veins.",
        Marble,
        [[0.05, 0.2, 0.12], [0.75, 0.78, 0.72], Z],
        0.15,
    ),
    painted(
        "walnut",
        "Walnut",
        "Natural",
        "Dark warm walnut grain.",
        Wood,
        [[0.03, 0.012, 0.005], [0.22, 0.1, 0.04], Z],
        0.55,
    ),
    painted(
        "ebony",
        "Ebony",
        "Natural",
        "Near-black hardwood with a subtle grain.",
        Wood,
        [[0.008, 0.006, 0.005], [0.05, 0.035, 0.025], Z],
        0.4,
    ),
    painted(
        "white_brick",
        "Painted White Brick",
        "Natural",
        "White-painted brick with grey mortar.",
        BrickLight,
        [[0.5, 0.5, 0.48], [0.7, 0.7, 0.68], [0.85, 0.85, 0.82]],
        0.8,
    ),
    painted(
        "navy_leather",
        "Navy Leather",
        "Leather",
        "Deep navy leather grain.",
        LeatherRed,
        [[0.02, 0.03, 0.09], Z, Z],
        0.65,
    ),
    painted(
        "green_leather",
        "Green Leather",
        "Leather",
        "Racing-green leather grain.",
        LeatherRed,
        [[0.02, 0.08, 0.04], Z, Z],
        0.65,
    ),
    painted(
        "kraft_paper",
        "Kraft Paper",
        "Natural",
        "Brown kraft paper fibres.",
        Paper,
        [[0.45, 0.3, 0.17], Z, Z],
        0.75,
    ),
    // See-through: they bend the shot behind them (connected automatically on text).
    Entry {
        bevel: Some(9.0),
        ..painted(
            "glass_clear",
            "Clear Glass",
            "Glass",
            "Clear glass that bends the shot behind it.",
            Glass,
            [[0.95, 0.97, 0.98], Z, Z],
            0.02,
        )
    },
    Entry {
        bevel: Some(9.0),
        ..painted(
            "glass_frosted",
            "Frosted Glass",
            "Glass",
            "Sandblasted glass: the shot behind softens to a glow.",
            Glass,
            [[0.95, 0.97, 0.98], Z, Z],
            0.45,
        )
    },
    Entry {
        bevel: Some(9.0),
        ..painted(
            "frosted_acrylic",
            "Frosted Acrylic",
            "Glass",
            "Milky blue-tinted acrylic.",
            Glass,
            [[0.8, 0.9, 1.0], Z, Z],
            0.62,
        )
    },
    Entry {
        ior: Some(1.31),
        bevel: Some(10.0),
        ..painted(
            "ice",
            "Ice",
            "Glass",
            "Cold blue ice, lightly clouded.",
            Glass,
            [[0.82, 0.94, 1.0], Z, Z],
            0.22,
        )
    },
    Entry {
        ior: Some(1.77),
        bevel: Some(8.0),
        ..painted(
            "ruby",
            "Ruby",
            "Glass",
            "Deep red gemstone.",
            Glass,
            [[0.9, 0.06, 0.12], Z, Z],
            0.03,
        )
    },
    Entry {
        ior: Some(1.77),
        bevel: Some(8.0),
        ..painted(
            "sapphire",
            "Sapphire",
            "Glass",
            "Deep blue gemstone.",
            Glass,
            [[0.1, 0.25, 0.95], Z, Z],
            0.03,
        )
    },
    Entry {
        ior: Some(1.58),
        bevel: Some(8.0),
        ..painted(
            "emerald",
            "Emerald",
            "Glass",
            "Green gemstone.",
            Glass,
            [[0.1, 0.8, 0.35], Z, Z],
            0.04,
        )
    },
    Entry {
        ior: Some(2.42),
        bevel: Some(8.0),
        ..painted(
            "diamond",
            "Diamond",
            "Glass",
            "Brilliant, colour-splitting diamond.",
            Glass,
            [[1.0, 1.0, 1.0], Z, Z],
            0.02,
        )
    },
    // Neon: glowing tubes with their bloom.
    painted(
        "neon_pink",
        "Neon Pink",
        "Neon",
        "Hot pink neon tube with its glow.",
        Phosphor,
        [[1.0, 0.1, 0.6], Z, Z],
        0.15,
    ),
    painted(
        "neon_blue",
        "Neon Blue",
        "Neon",
        "Electric blue neon tube with its glow.",
        Phosphor,
        [[0.1, 0.5, 1.0], Z, Z],
        0.15,
    ),
    painted(
        "neon_white",
        "Neon White",
        "Neon",
        "Warm white neon tube with its glow.",
        Phosphor,
        [[1.0, 0.97, 0.92], Z, Z],
        0.15,
    ),
    painted(
        "neon_red",
        "Neon Red",
        "Neon",
        "Red neon tube with its glow.",
        Phosphor,
        [[1.0, 0.08, 0.05], Z, Z],
        0.15,
    ),
    painted(
        "neon_amber",
        "Neon Amber",
        "Neon",
        "Amber neon tube with its glow.",
        Phosphor,
        [[1.0, 0.55, 0.05], Z, Z],
        0.15,
    ),
    Entry {
        grain: 1.6,
        ..look(
            "liquid_metal",
            "Liquid Metal",
            "Metal",
            "Mercury-like metal whose surface swells and ripples as Reflection motion plays.",
            LiquidMetal,
            0.05,
        )
    },
    Entry {
        grain: 1.6,
        ..metal(
            "liquid_gold",
            "Liquid Gold",
            "Molten-looking gold with slow ripples.",
            LiquidMetal,
            [0.95, 0.64, 0.21],
            0.06,
        )
    },
    look(
        "pearl",
        "Pearl",
        "Glossy",
        "Pale pearl whose sheen shifts colour with the angle.",
        Pearl,
        0.25,
    ),
    painted(
        "oil_slick",
        "Oil Slick",
        "Glossy",
        "Black surface with a rainbow oil-film sheen.",
        Pearl,
        [[0.02, 0.02, 0.025], [1.0, 1.0, 1.0], Z],
        0.1,
    ),
    look(
        "slate",
        "Slate",
        "Natural",
        "Dark layered slate.",
        Slate,
        0.7,
    ),
    painted(
        "sandstone",
        "Sandstone",
        "Natural",
        "Warm layered sandstone.",
        Slate,
        [[0.45, 0.33, 0.2], [0.65, 0.52, 0.36], Z],
        0.85,
    ),
    look(
        "denim",
        "Denim",
        "Natural",
        "Blue denim weave.",
        Fabric,
        0.8,
    ),
    painted(
        "linen",
        "Linen",
        "Natural",
        "Natural linen weave.",
        Fabric,
        [[0.62, 0.58, 0.5], [0.7, 0.66, 0.58], Z],
        0.85,
    ),
    painted(
        "red_velvet",
        "Red Velvet",
        "Natural",
        "Deep red fabric.",
        Fabric,
        [[0.35, 0.02, 0.03], [0.3, 0.015, 0.025], Z],
        0.9,
    ),
    Entry {
        bevel: Some(10.0),
        ..painted(
            "embossed_paper",
            "Embossed Paper",
            "Natural",
            "Clean paper pressed into raised letters.",
            Paper,
            [[0.88, 0.865, 0.82], Z, Z],
            0.9,
        )
    },
];

pub fn get(id: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|e| e.id == id)
}

impl Entry {
    /// The material this look applies, with the library's usual size and motion.
    pub fn material(&self) -> crate::nodes::Material {
        use crate::{Key, Scalar, Time};
        crate::nodes::Material {
            smooth_edges: true,
            brushing: self.brushing,
            studio: Default::default(),
            optics: match (self.reflectance, self.ior) {
                (Some(reflectance), _) => Some(SurfaceOptics {
                    metallic: true,
                    reflectance,
                    ior: 1.5,
                }),
                (None, Some(ior)) => Some(SurfaceOptics {
                    metallic: false,
                    reflectance: [0.04; 3],
                    ior,
                }),
                (None, None) => None,
            },
            colours: self.colours,
            kind: self.kind,
            scale: 180.0.into(),
            bevel: self.bevel.unwrap_or(5.0).into(),
            roughness: self.roughness.into(),
            lighting: Scalar::Smooth {
                keys: [(0, 0.0), (2, 0.12), (4, 0.0)]
                    .into_iter()
                    .map(|(ticks, value)| Key {
                        time: Time { ticks, rate: 1 },
                        value,
                        segment: None,
                    })
                    .collect(),
            },
            grain: self.grain.into(),
            emission: 1.0.into(),
            shot: 0.6.into(),
            normal_strength: 1.0.into(),
            height_depth: 4.0.into(),
            normal_directx: false,
        }
    }
    /// As a library card.
    pub fn builtin(&self) -> super::builtin::Builtin {
        super::builtin::Builtin {
            id: self.id,
            label: self.label,
            description: self.description,
        }
    }
}
