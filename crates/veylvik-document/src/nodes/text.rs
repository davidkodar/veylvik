//! Editable vector text.
#[allow(unused_imports)]
use crate::registry::{NodeType, Parameter, ParameterKind, other, scalar};
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

pub static TEXT: NodeType = NodeType {
    type_id: "text",
    version: 1,
    label: "Text",
    description: "Editable vector text with portable bundled or embedded fonts. Multiline alignment, tracking, line spacing and animated size/position. White alpha source for any material or colour. Explicit LTR/RTL per line; no automatic mixed-script bidi or fallback.",
    inputs: &[],
    parameters: &[
        other("content", ParameterKind::TextContent, "text"),
        other("font", ParameterKind::TextFont, "font"),
        other("alignment", ParameterKind::TextAlignment, "alignment"),
        other("rtl", ParameterKind::Boolean, "direction"),
        other("language", ParameterKind::TextLanguage, "language"),
        scalar("size", "pixels", 1.0, 1024.0, 160.0),
        scalar("x", "pixels", -32768.0, 32768.0, 480.0),
        scalar("y", "pixels", -32768.0, 32768.0, 320.0),
        scalar("tracking", "pixels", -128.0, 512.0, 0.0),
        scalar("line_height", "factor", 0.1, 4.0, 1.25),
        scalar("rotation", "degrees", -36000.0, 36000.0, 0.0),
        scalar("scale_x", "factor", -100.0, 100.0, 1.0),
        scalar("scale_y", "factor", -100.0, 100.0, 1.0),
        scalar("wrap_width", "pixels", 0.0, 8192.0, 0.0),
        scalar("box_height", "pixels", 0.0, 8192.0, 0.0),
        scalar("padding", "pixels", 0.0, 4096.0, 0.0),
        other(
            "typography",
            ParameterKind::Typography,
            "text layout and animation",
        ),
        other("underline", ParameterKind::Boolean, "style"),
        other("features", ParameterKind::TextFeatures, "OpenType"),
        Parameter {
            default: 1.0,
            ..other("color", ParameterKind::Color, "linear RGBA")
        },
    ],
};

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
