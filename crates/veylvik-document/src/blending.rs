//! Scene-linear blend modes combined with source-over alpha compositing.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum BlendMode {
    Normal = 0,
    Add = 1,
    Multiply = 2,
    Screen = 3,
    Darken = 4,
    Lighten = 5,
    Difference = 6,
    /// Darkens where the background is dark and brightens where it is bright, so the
    /// background's texture shows through the foreground (paint on stone, a logo on cloth).
    Overlay = 7,
    /// A gentler overlay: the foreground tints and shades the background.
    SoftLight = 8,
}
impl BlendMode {
    pub const ALL: [Self; 9] = [
        Self::Normal,
        Self::Add,
        Self::Multiply,
        Self::Screen,
        Self::Darken,
        Self::Lighten,
        Self::Difference,
        Self::Overlay,
        Self::SoftLight,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Add => "Add",
            Self::Multiply => "Multiply",
            Self::Screen => "Screen",
            Self::Darken => "Darken",
            Self::Lighten => "Lighten",
            Self::Difference => "Difference",
            Self::Overlay => "Overlay",
            Self::SoftLight => "Soft light",
        }
    }
}
