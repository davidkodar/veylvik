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
