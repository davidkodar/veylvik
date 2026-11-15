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

pub static ENTRIES: [Entry; 25] = [
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
            optics: self.reflectance.map(|reflectance| SurfaceOptics {
                metallic: true,
                reflectance,
                ior: 1.5,
            }),
            colours: self.colours,
            kind: self.kind,
            scale: 180.0.into(),
            bevel: 5.0.into(),
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
