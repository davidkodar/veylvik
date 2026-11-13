//! Editable vector text.
#[allow(unused_imports)]
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Text {
    pub content: String,
    pub font: Box<TextFont>,
    pub alignment: TextAlignment,
    pub rtl: bool,
    pub language: String,
    pub size: Scalar,
    pub x: Scalar,
    pub y: Scalar,
    pub tracking: Scalar,
    pub line_height: Scalar,
    pub rotation: Scalar,
    pub scale_x: Scalar,
    pub scale_y: Scalar,
    pub wrap_width: Scalar,
    #[serde(default = "text::zero")]
    pub box_height: Scalar,
    #[serde(default = "text::zero")]
    pub padding: Scalar,
    #[serde(default)]
    pub typography: Box<text::Typography>,
    pub underline: bool,
    pub features: String,
    pub color: [f32; 4],
}
