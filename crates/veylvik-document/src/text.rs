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

pub fn zero() -> crate::Scalar {
    0.0.into()
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Vertical {
    #[default]
    Top,
    Middle,
    Bottom,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextMotion {
    #[default]
    None,
    Typewriter,
    Rise,
    Scale,
    Spin,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextUnit {
    #[default]
    Character,
    Word,
    Line,
    Whole,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Typography {
    pub spans: Vec<TextSpan>,
    pub vertical: Vertical,
    pub auto_fit: bool,
    /// None preserves the historical baseline pivot. Normalized ink/box anchor otherwise.
    pub anchor: Option<[f32; 2]>,
    pub linked_scale: bool,
    pub motion: TextMotion,
    pub unit: TextUnit,
    pub start: f32,
    pub duration: f32,
    pub stagger: f32,
    /// None means hold indefinitely; otherwise start the exit at this time.
    pub exit: Option<f32>,
    pub ease: bool,
}
impl Default for Typography {
    fn default() -> Self {
        Self {
            spans: vec![],
            vertical: Vertical::Top,
            auto_fit: false,
            anchor: None,
            linked_scale: false,
            motion: TextMotion::None,
            unit: TextUnit::Character,
            start: 0.0,
            duration: 0.5,
            stagger: 0.04,
            exit: None,
            ease: true,
        }
    }
}
impl Typography {
    pub fn validate(&self) -> Result<()> {
        if self.spans.len() > 32 {
            return fail("At most 32 character style ranges are supported");
        }
        let mut previous = 0;
        for span in &self.spans {
            if span.start >= span.end
                || span.start < previous
                || span.end > 4096
                || !span.scale.is_finite()
                || !(0.1..=8.0).contains(&span.scale)
            {
                return fail("Invalid or overlapping character styles");
            }
            if let Some(font) = &span.font {
                font.validate()?;
            }
            previous = span.end;
        }
        if self
            .anchor
            .is_some_and(|a| a.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)))
            || !self.start.is_finite()
            || self.start.abs() > 86400.0
            || !self.duration.is_finite()
            || !(0.001..=3600.0).contains(&self.duration)
            || !self.stagger.is_finite()
            || !(0.0..=60.0).contains(&self.stagger)
            || self
                .exit
                .is_some_and(|v| !v.is_finite() || v < self.start || v > 86400.0)
        {
            return fail("Invalid text anchor or animation timing");
        }
        Ok(())
    }
    pub fn progress(&self, seconds: f64, unit: usize) -> f32 {
        if self.motion == TextMotion::None {
            return 1.0;
        }
        let delay = unit as f64 * self.stagger as f64;
        let entrance =
            ((seconds - self.start as f64 - delay) / self.duration as f64).clamp(0.0, 1.0) as f32;
        let exit = self.exit.map_or(1.0, |at| {
            1.0 - ((seconds - at as f64 - delay) / self.duration as f64).clamp(0.0, 1.0) as f32
        });
        let p = entrance.min(exit);
        if self.ease {
            p * p * (3.0 - 2.0 * p)
        } else {
            p
        }
    }
}
pub fn typography_schema() -> serde_json::Value {
    serde_json::json!({"type":"object","additionalProperties":false,"properties":{
        "spans":{"type":"array","maxItems":32,"items":{"type":"object","additionalProperties":false,"properties":{"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":1,"maximum":4096},"scale":{"type":"number","minimum":0.1,"maximum":8},"font":{"anyOf":[{"type":"null"},font_schema()]}},"required":["start","end","scale","font"]}},
        "vertical":{"enum":["top","middle","bottom"]},"auto_fit":{"type":"boolean"},"linked_scale":{"type":"boolean"},
        "anchor":{"anyOf":[{"type":"null"},{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number","minimum":0,"maximum":1}}]},
        "motion":{"enum":["none","typewriter","rise","scale","spin"]},"unit":{"enum":["character","word","line","whole"]},
        "start":{"type":"number","minimum":-86400,"maximum":86400},"duration":{"type":"number","minimum":0.001,"maximum":3600},"stagger":{"type":"number","minimum":0,"maximum":60},"exit":{"anyOf":[{"type":"null"},{"type":"number","minimum":-86400,"maximum":86400}]},"ease":{"type":"boolean"}
    }})
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextSpan {
    /// UTF-8 byte boundaries in content. Later spans are not allowed to overlap.
    pub start: usize,
    pub end: usize,
    pub font: Option<TextFont>,
    pub scale: f32,
}

impl Typography {
    /// Preserve character formatting across one contiguous edit, including paste.
    pub fn remap(&mut self, before: &str, after: &str) {
        if before == after {
            return;
        }
        let mut prefix = before
            .bytes()
            .zip(after.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        while !before.is_char_boundary(prefix) || !after.is_char_boundary(prefix) {
            prefix -= 1;
        }
        let mut suffix = before[prefix..]
            .bytes()
            .rev()
            .zip(after[prefix..].bytes().rev())
            .take_while(|(a, b)| a == b)
            .count();
        while !before.is_char_boundary(before.len() - suffix)
            || !after.is_char_boundary(after.len() - suffix)
        {
            suffix -= 1;
        }
        let old_end = before.len() - suffix;
        let new_end = after.len() - suffix;
        for span in &mut self.spans {
            span.start = if span.start >= old_end {
                new_end + span.start - old_end
            } else {
                span.start.min(prefix)
            };
            span.end = if span.end >= old_end {
                new_end + span.end - old_end
            } else if span.end > prefix {
                new_end
            } else {
                span.end
            };
        }
        let mut previous = 0;
        for span in &mut self.spans {
            span.start = span.start.max(previous);
            previous = previous.max(span.end);
        }
        self.spans
            .retain(|s| s.start < s.end && s.end <= after.len());
    }
    pub fn set_span(&mut self, span: TextSpan) {
        let mut retained = Vec::new();
        for old in self.spans.drain(..) {
            if old.end <= span.start || old.start >= span.end {
                retained.push(old);
                continue;
            }
            if old.start < span.start {
                let mut left = old.clone();
                left.end = span.start;
                retained.push(left);
            }
            if old.end > span.end {
                let mut right = old;
                right.start = span.end;
                retained.push(right);
            }
        }
        retained.push(span);
        retained.sort_by_key(|s| s.start);
        self.spans = retained;
    }
}
