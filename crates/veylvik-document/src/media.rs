//! Exact-time footage placement. Source-out is exclusive; outside the clip is transparent.
use crate::{Result, Time, fail};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timing {
    pub start: Time,
    pub source_in: Time,
    pub source_out: Option<Time>,
}
impl Default for Timing {
    fn default() -> Self {
        Self {
            start: Time { ticks: 0, rate: 1 },
            source_in: Time { ticks: 0, rate: 1 },
            source_out: None,
        }
    }
}
impl Timing {
    pub fn validate(&self) -> Result<()> {
        for time in [Some(self.start), Some(self.source_in), self.source_out]
            .into_iter()
            .flatten()
        {
            if !(0.0..=86400.0).contains(&time.seconds()?) {
                return fail("Footage times must be within 0..86400 seconds");
            }
        }
        if let Some(end) = self.source_out
            && !end.compare(self.source_in)?.is_gt()
        {
            return fail("Source out must be after source in");
        }
        Ok(())
    }
    pub fn source_time(&self, time: Time, duration: Option<Time>) -> Result<Option<Time>> {
        self.validate()?;
        if time.compare(self.start)?.is_lt() {
            return Ok(None);
        }
        let source = time.checked_sub(self.start)?.checked_add(self.source_in)?;
        for end in [self.source_out, duration].into_iter().flatten() {
            if !source.compare(end)?.is_lt() {
                return Ok(None);
            }
        }
        Ok(Some(source))
    }
}
pub fn schema() -> serde_json::Value {
    let time = serde_json::json!({"type":"object","properties":{"ticks":{"type":"integer","minimum":0},"rate":{"type":"integer","minimum":1,"maximum":u32::MAX}},"required":["ticks","rate"],"additionalProperties":false});
    serde_json::json!({"type":"object","properties":{"start":time,"source_in":time,"source_out":{"anyOf":[time,{"type":"null"}]}},"required":["start","source_in","source_out"],"additionalProperties":false})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clip_boundaries_and_fractional_rates_are_exact() {
        let timing = Timing {
            start: Time::new(1001, 24000).unwrap(),
            source_in: Time::new(1, 2).unwrap(),
            source_out: Some(Time::new(1, 1).unwrap()),
        };
        assert!(
            timing
                .source_time(Time::new(0, 1).unwrap(), None)
                .unwrap()
                .is_none()
        );
        let first = timing
            .source_time(Time::new(1001, 24000).unwrap(), None)
            .unwrap()
            .unwrap();
        assert!(first.compare(Time::new(1, 2).unwrap()).unwrap().is_eq());
        assert!(
            timing
                .source_time(Time::new(13001, 24000).unwrap(), None)
                .unwrap()
                .is_none()
        );
        assert!(
            Timing {
                source_out: Some(Time::new(0, 1).unwrap()),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Timing::default()
                .source_time(Time::new(1, 1).unwrap(), Some(Time::new(1, 1).unwrap()))
                .unwrap()
                .is_none()
        );
    }
}

/// Source placement is evaluated before clipping, in the GPU input pass.
/// Legacy preserves the top-left native-pixel behavior of older documents.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    #[default]
    Legacy,
    Fit,
    Fill,
    Original,
    Stretch,
}
impl Placement {
    pub fn label(self) -> &'static str {
        match self {
            Self::Legacy => "Native pixels (top-left)",
            Self::Fit => "Fit",
            Self::Fill => "Fill",
            Self::Original => "Original size",
            Self::Stretch => "Stretch",
        }
    }
}

/// How a video's code values become scene light: its transfer curve and its gamut. `Auto`
/// follows the file's colour tags; camera log footage is usually untagged and needs a choice.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum Curve {
    #[default]
    Auto,
    Rec709,
    Srgb,
    Linear,
    Pq,
    Hlg,
    AppleLog,
    Slog3,
    Logc3,
    Logc4,
    Vlog,
}
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum Gamut {
    #[default]
    Auto,
    Rec709,
    Rec2020,
    P3D65,
    SGamut3,
    SGamut3Cine,
    ArriWideGamut3,
    ArriWideGamut4,
    VGamut,
}
impl Curve {
    pub const ALL: [Self; 11] = [
        Self::Auto,
        Self::Rec709,
        Self::Srgb,
        Self::Linear,
        Self::Pq,
        Self::Hlg,
        Self::AppleLog,
        Self::Slog3,
        Self::Logc3,
        Self::Logc4,
        Self::Vlog,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (from file)",
            Self::Rec709 => "Rec.709 video",
            Self::Srgb => "sRGB",
            Self::Linear => "Linear",
            Self::Pq => "PQ (HDR10, Dolby Vision)",
            Self::Hlg => "HLG (iPhone HDR, broadcast)",
            Self::AppleLog => "Apple Log",
            Self::Slog3 => "Sony S-Log3",
            Self::Logc3 => "ARRI LogC3",
            Self::Logc4 => "ARRI LogC4",
            Self::Vlog => "Panasonic V-Log",
        }
    }
    /// The gamut a camera curve is normally paired with, for one-click setup.
    pub fn usual_gamut(self) -> Gamut {
        match self {
            Self::AppleLog | Self::Pq | Self::Hlg => Gamut::Rec2020,
            Self::Slog3 => Gamut::SGamut3Cine,
            Self::Logc3 => Gamut::ArriWideGamut3,
            Self::Logc4 => Gamut::ArriWideGamut4,
            Self::Vlog => Gamut::VGamut,
            _ => Gamut::Auto,
        }
    }
}
impl Gamut {
    pub const ALL: [Self; 9] = [
        Self::Auto,
        Self::Rec709,
        Self::Rec2020,
        Self::P3D65,
        Self::SGamut3,
        Self::SGamut3Cine,
        Self::ArriWideGamut3,
        Self::ArriWideGamut4,
        Self::VGamut,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (from file)",
            Self::Rec709 => "Rec.709 / sRGB",
            Self::Rec2020 => "Rec.2020",
            Self::P3D65 => "P3-D65",
            Self::SGamut3 => "Sony S-Gamut3",
            Self::SGamut3Cine => "Sony S-Gamut3.Cine",
            Self::ArriWideGamut3 => "ARRI Wide Gamut 3",
            Self::ArriWideGamut4 => "ARRI Wide Gamut 4",
            Self::VGamut => "Panasonic V-Gamut",
        }
    }
}
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash,
)]
#[serde(deny_unknown_fields)]
pub struct InputColour {
    #[serde(default)]
    pub curve: Curve,
    #[serde(default)]
    pub gamut: Gamut,
}
impl InputColour {
    pub fn is_auto(&self) -> bool {
        *self == Self::default()
    }
    pub fn schema() -> serde_json::Value {
        let names = |v: serde_json::Value| v;
        serde_json::json!({"type":"object","properties":{
            "curve":{"type":"string","enum":names(serde_json::to_value(Curve::ALL).unwrap_or_default())},
            "gamut":{"type":"string","enum":names(serde_json::to_value(Gamut::ALL).unwrap_or_default())}
        },"additionalProperties":false})
    }
}

/// How scene-linear light becomes SDR display values (viewer, PNG, video). EXR output and the
/// graph itself always stay scene-linear.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayTransform {
    /// Values above 1 clip; exact for graphics made in display terms.
    #[default]
    Standard,
    /// ACES-style filmic tone curve: highlights roll off smoothly, suited to HDR and log footage.
    Filmic,
}
impl DisplayTransform {
    pub const ALL: [Self; 2] = [Self::Standard, Self::Filmic];
    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "Standard (clip at white)",
            Self::Filmic => "Filmic (ACES-style)",
        }
    }
    pub fn is_standard(&self) -> bool {
        *self == Self::Standard
    }
}
