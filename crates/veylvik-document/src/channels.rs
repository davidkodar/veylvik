//! Channels available for converting image content into mask coverage.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum MaskChannel {
    Luminance = 0,
    Red = 1,
    Green = 2,
    Blue = 3,
    Alpha = 4,
}
impl MaskChannel {
    pub const ALL: [Self; 5] = [
        Self::Luminance,
        Self::Red,
        Self::Green,
        Self::Blue,
        Self::Alpha,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Luminance => "Luminance",
            Self::Red => "Red",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Alpha => "Alpha",
        }
    }
}
