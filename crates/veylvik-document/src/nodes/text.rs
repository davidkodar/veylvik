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

node!(
    Text,
    TEXT,
    [
        size,
        x,
        y,
        tracking,
        line_height,
        rotation,
        scale_x,
        scale_y,
        wrap_width,
        box_height,
        padding
    ],
    {
        fn validate(&self) -> Result<()> {
            crate::text::validate(&self.content, &self.font, &self.language)?;
            self.typography.validate()?;
            for span in &self.typography.spans {
                if span.end > self.content.len()
                    || !self.content.is_char_boundary(span.start)
                    || !self.content.is_char_boundary(span.end)
                {
                    return fail("Character styles must use UTF-8 boundaries inside the text");
                }
            }
            if self.features.len() > 256 || !self.features.is_ascii() {
                return fail("OpenType feature list must be ASCII, at most 256 bytes");
            }
            if self.color.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&self.color[3]) {
                return fail("Text colour needs finite RGB and alpha in [0,1]");
            }
            Ok(())
        }
    }
);
