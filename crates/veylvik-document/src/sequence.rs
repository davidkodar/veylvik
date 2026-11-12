//! Inclusive frame ranges shared by batch rendering and timeline hosts.
use crate::{Result, Time, fail};
use serde::{Deserialize, Serialize};
pub const MAX_SEQUENCE_FRAMES: u32 = 10_000;
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameRange {
    pub start: i64,
    pub end: i64,
    pub frame_rate: Time,
}
impl FrameRange {
    pub fn new(start: i64, end: i64, frame_rate: Time) -> Result<Self> {
        let value = Self {
            start,
            end,
            frame_rate,
        };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(self) -> Result<()> {
        let count = self.end as i128 - self.start as i128 + 1;
        if !(1..=i128::from(MAX_SEQUENCE_FRAMES)).contains(&count) {
            return fail(format!(
                "sequence needs 1..={MAX_SEQUENCE_FRAMES} frames in ascending order"
            ));
        }
        // Preflight every timestamp so an unrepresentable interior time cannot
        // fail a long render after files have already been written.
        for frame in self.start..=self.end {
            Time::at_frame(frame, self.frame_rate)?;
        }
        Ok(())
    }
    pub fn count(self) -> Result<u32> {
        self.validate()?;
        Ok((self.end as i128 - self.start as i128 + 1) as u32)
    }
}
