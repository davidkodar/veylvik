//! Soft coverage operations. Inputs and outputs are alpha in [0, 1].
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum MaskMode {
    Union = 0,
    Intersect = 1,
    Subtract = 2,
    Exclude = 3,
}
impl MaskMode {
    pub const ALL: [Self; 4] = [Self::Union, Self::Intersect, Self::Subtract, Self::Exclude];
    pub fn label(self) -> &'static str {
        match self {
            Self::Union => "Union",
            Self::Intersect => "Intersect",
            Self::Subtract => "Subtract",
            Self::Exclude => "Exclude overlap",
        }
    }
}
