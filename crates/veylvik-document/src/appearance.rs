//! Portable surface and atmosphere models shared by rendering, UI and automation.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum MaterialKind {
    Gold = 0,
    Silver = 1,
    Aluminium = 2,
    Bronze = 3,
    Phosphor = 4,
    Wood = 5,
    OakLight = 6,
    OakDark = 7,
    Marble = 8,
    BrickLight = 9,
    BrickRed = 10,
    Concrete = 11,
    Paper = 12,
    AgedPaper = 13,
    LeatherRed = 14,
    LeatherBrown = 15,
    LeatherTan = 16,
    LeatherDark = 17,
    LeatherBlack = 18,
    Plastic = 19,
    CarPaint = 20,
    CarbonFibre = 21,
    Holographic = 22,
    Texture = 23,
    Glass = 24,
    LiquidMetal = 25,
    Pearl = 26,
    Slate = 27,
    Fabric = 28,
}
impl MaterialKind {
    pub const ALL: [Self; 29] = [
        Self::Gold,
        Self::Silver,
        Self::Aluminium,
        Self::Bronze,
        Self::Phosphor,
        Self::Wood,
        Self::OakLight,
        Self::OakDark,
        Self::Marble,
        Self::BrickLight,
        Self::BrickRed,
        Self::Concrete,
        Self::Paper,
        Self::AgedPaper,
        Self::LeatherRed,
        Self::LeatherBrown,
        Self::LeatherTan,
        Self::LeatherDark,
        Self::LeatherBlack,
        Self::Plastic,
        Self::CarPaint,
        Self::CarbonFibre,
        Self::Holographic,
        Self::Texture,
        Self::Glass,
        Self::LiquidMetal,
        Self::Pearl,
        Self::Slate,
        Self::Fabric,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Gold => "Gold",
            Self::Silver => "Silver",
            Self::Aluminium => "Aluminium",
            Self::Bronze => "Bronze",
            Self::Phosphor => "Phosphor",
            Self::Wood => "Wood",
            Self::OakLight => "Light Oak",
            Self::OakDark => "Dark Oak",
            Self::Marble => "White Marble",
            Self::BrickLight => "Light Brick",
            Self::BrickRed => "Red Brick",
            Self::Concrete => "Rough Concrete",
            Self::Paper => "Clean Paper",
            Self::AgedPaper => "Weathered Paper",
            Self::LeatherRed => "Red Leather",
            Self::LeatherBrown => "Brown Leather",
            Self::LeatherTan => "Tan Leather",
            Self::LeatherDark => "Dark Leather",
            Self::LeatherBlack => "Black Leather",
            Self::Plastic => "Glossy Plastic",
            Self::CarPaint => "Car Paint",
            Self::CarbonFibre => "Carbon Fibre",
            Self::Holographic => "Holographic Foil",
            Self::Texture => "Image Texture",
            Self::Glass => "Glass",
            Self::LiquidMetal => "Liquid Metal",
            Self::Pearl => "Pearl",
            Self::Slate => "Slate",
            Self::Fabric => "Woven Fabric",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum AtmosphereKind {
    Smoke = 0,
    Soot = 1,
    Fog = 2,
    DriftingFog = 3,
    Clouds = 4,
}
impl AtmosphereKind {
    pub const ALL: [Self; 5] = [
        Self::Smoke,
        Self::Soot,
        Self::Fog,
        Self::DriftingFog,
        Self::Clouds,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Smoke => "Light smoke",
            Self::Soot => "Dense dark smoke",
            Self::Fog => "Fog layer",
            Self::DriftingFog => "Drifting fog",
            Self::Clouds => "Clouds",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum ReflectionKind {
    Metal = 0,
    Glass = 1,
}
impl ReflectionKind {
    pub const ALL: [Self; 2] = [Self::Metal, Self::Glass];
    pub fn label(self) -> &'static str {
        match self {
            Self::Metal => "Metal",
            Self::Glass => "Glass",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum PatinaKind {
    Tarnish = 0,
    Verdigris = 1,
    Rust = 2,
}
impl PatinaKind {
    pub const ALL: [Self; 3] = [Self::Tarnish, Self::Verdigris, Self::Rust];
    pub fn label(self) -> &'static str {
        match self {
            Self::Tarnish => "Tarnish",
            Self::Verdigris => "Verdigris",
            Self::Rust => "Rust",
        }
    }
}
