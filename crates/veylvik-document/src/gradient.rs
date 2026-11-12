//! Geometry used by the grayscale gradient generator.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum GradientMode {
    Linear = 0,
    Radial = 1,
}
impl GradientMode {
    pub const ALL: [Self; 2] = [Self::Linear, Self::Radial];
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Radial => "Radial",
        }
    }
}
