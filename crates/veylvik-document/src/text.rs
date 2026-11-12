//! Portable typography; fonts are bundled identities or bounded embedded bytes.
use crate::{Result, fail};
use base64::Engine;
use serde::{Deserialize, Serialize};
pub const MAX_FONT_BYTES: usize = 8 * 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum TextFont {
    Sans {},
    Italic {},
    BoldItalic {},
    #[default]
    // Unit variants keep builtin references small and portable.
    Display,
    Mono {},
    Embedded {
        name: String,
        index: u32,
        data: String,
    },
}
impl TextFont {
    pub fn label(&self) -> &str {
        match self {
            Self::Sans {} => "Liberation Sans · Regular",
            Self::Italic {} => "Liberation Sans · Italic",
            Self::BoldItalic {} => "Liberation Sans · Bold Italic",
            Self::Display => "Liberation Sans · Bold",
            Self::Mono {} => "Hack · Regular",
            Self::Embedded { name, .. } => name,
        }
    }
    pub fn embedded(name: String, index: u32, bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() || bytes.len() > MAX_FONT_BYTES {
            return fail("Font must contain 1..8 MiB");
        }
        let font = Self::Embedded {
            name,
            index,
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        };
        font.validate()?;
        Ok(font)
    }
    pub fn bytes(&self) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Embedded { data, .. } => {
                if data.len() > MAX_FONT_BYTES.div_ceil(3) * 4 {
                    return fail("Embedded font exceeds 8 MiB");
                }
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .map_err(|_| crate::Error("Invalid embedded font encoding".into()))?;
                if bytes.is_empty() || bytes.len() > MAX_FONT_BYTES {
                    return fail("Font must contain 1..8 MiB");
                }
                Ok(Some(bytes))
            }
            _ => Ok(None),
        }
    }
    pub fn validate(&self) -> Result<()> {
        if let Self::Embedded { name, index, .. } = self {
            if name.is_empty() || name.len() > 256 {
                return fail("Font name must contain 1..256 bytes");
            }
            let bytes = self.bytes()?.unwrap();
            ttf_parser::Face::parse(&bytes, *index)
                .map_err(|_| crate::Error("Invalid font or font collection face index".into()))?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TextAlignment {
    Left,
    #[default]
    Center,
    Right,
}
pub fn validate(content: &str, font: &TextFont, language: &str) -> Result<()> {
    if content
        .as_bytes()
        .iter()
        .enumerate()
        .any(|(i, b)| *b == b'\r' && content.as_bytes().get(i + 1) != Some(&b'\n'))
        || content.len() > 4096
        || content.split('\n').count() > 64
        || content.chars().any(|c| {
            (c.is_control() && c != '\n' && c != '\r')
                || matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')
        })
    {
        return fail(
            "Text supports up to 4096 UTF-8 bytes and 64 lines; control characters are not supported",
        );
    }
    if language.len() > 64
        || !language
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return fail("Language must be an optional BCP-47 language tag");
    }
    font.validate()
}
pub fn font_schema() -> serde_json::Value {
    serde_json::json!({"oneOf":[
        {"type":"object","properties":{"source":{"enum":["sans","display","mono","italic","bold_italic"]}},"required":["source"],"additionalProperties":false},
        {"type":"object","properties":{"source":{"const":"embedded"},"name":{"type":"string","minLength":1,"maxLength":256},"index":{"type":"integer","minimum":0,"maximum":u32::MAX},"data":{"type":"string","maxLength":MAX_FONT_BYTES.div_ceil(3)*4}},"required":["source","name","index","data"],"additionalProperties":false}
    ]})
}
