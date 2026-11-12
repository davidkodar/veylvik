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
}
impl MaterialKind {
    pub const ALL: [Self; 6] = [
        Self::Gold,
        Self::Silver,
        Self::Aluminium,
        Self::Bronze,
        Self::Phosphor,
        Self::Wood,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Gold => "Gold",
            Self::Silver => "Silver",
            Self::Aluminium => "Aluminium",
            Self::Bronze => "Bronze",
            Self::Phosphor => "Phosphor",
            Self::Wood => "Wood",
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
