//! Exact rational time and bounded scalar animation curves.
use crate::{Result, fail};
use serde::{Deserialize, Serialize};
/// Exact seconds. Structural equality is deliberately not exposed; compare rational values.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Time {
    pub ticks: i64,
    pub rate: u32,
}
impl Time {
    pub fn new(ticks: i64, rate: u32) -> Result<Self> {
        if rate == 0 {
            return fail("time rate must be positive");
        }
        Ok(Self { ticks, rate })
    }
    pub fn seconds(self) -> Result<f64> {
        Self::new(self.ticks, self.rate)?;
        Ok(self.ticks as f64 / self.rate as f64)
    }
    pub fn compare(self, other: Self) -> Result<std::cmp::Ordering> {
        Self::new(self.ticks, self.rate)?;
        Self::new(other.ticks, other.rate)?;
        Ok((self.ticks as i128 * other.rate as i128)
            .cmp(&(other.ticks as i128 * self.rate as i128)))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub time: Time,
    pub value: f32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scalar {
    Constant { value: f32 },
    Linear { keys: Vec<Key> },
    Step { keys: Vec<Key> },
    Smooth { keys: Vec<Key> },
}
impl From<f32> for Scalar {
    fn from(value: f32) -> Self {
        Self::Constant { value }
    }
}
pub const MAX_KEYS: usize = 4096;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    Linear,
    Step,
    Smooth,
}
impl Scalar {
    pub fn interpolation(&self) -> Option<Interpolation> {
        match self {
            Self::Constant { .. } => None,
            Self::Linear { .. } => Some(Interpolation::Linear),
            Self::Step { .. } => Some(Interpolation::Step),
            Self::Smooth { .. } => Some(Interpolation::Smooth),
        }
    }
    pub fn keys_mut(&mut self) -> Option<&mut Vec<Key>> {
        match self {
            Self::Constant { .. } => None,
            Self::Linear { keys } | Self::Step { keys } | Self::Smooth { keys } => Some(keys),
        }
    }
    /// Change the curve without changing its keys. Constants remain constants.
    pub fn set_interpolation(&mut self, mode: Interpolation) {
        let Some(keys) = self.keys_mut() else {
            return;
        };
        let keys = std::mem::take(keys);
        *self = match mode {
            Interpolation::Linear => Self::Linear { keys },
            Interpolation::Step => Self::Step { keys },
            Interpolation::Smooth => Self::Smooth { keys },
        };
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Constant { value } if !value.is_finite() => fail("non-finite scalar"),
            Self::Linear { keys } | Self::Step { keys } | Self::Smooth { keys } => {
                if keys.is_empty() || keys.len() > MAX_KEYS {
                    return fail(format!("animation needs 1..={MAX_KEYS} keys"));
                }
                for key in keys {
                    key.time.seconds()?;
                    if !key.value.is_finite() {
                        return fail("non-finite key");
                    }
                }
                for pair in keys.windows(2) {
                    if !pair[0].time.compare(pair[1].time)?.is_lt() {
                        return fail("keys must have strictly increasing times");
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    pub(crate) fn bounded(&self, min: f32, max: f32, name: &str) -> Result<()> {
        self.validate()?;
        let check = |value: f32| {
            if (min..=max).contains(&value) {
                Ok(())
            } else {
                fail(format!("{name} must be in {min}..={max}"))
            }
        };
        match self {
            Self::Constant { value } => check(*value),
            Self::Linear { keys } | Self::Step { keys } | Self::Smooth { keys } => {
                for key in keys {
                    check(key.value)?;
                }
                Ok(())
            }
        }
    }
    pub fn at(&self, time: Time) -> Result<f32> {
        self.validate()?;
        time.seconds()?;
        match self {
            Self::Constant { value } => Ok(*value),
            Self::Linear { keys } | Self::Step { keys } | Self::Smooth { keys } => {
                let first = &keys[0];
                if !time.compare(first.time)?.is_gt() {
                    return Ok(first.value);
                }
                for pair in keys.windows(2) {
                    if time.compare(pair[1].time)?.is_eq() {
                        return Ok(pair[1].value);
                    }
                    if time.compare(pair[1].time)?.is_lt() {
                        if matches!(self, Self::Step { .. }) {
                            return Ok(pair[0].value);
                        }
                        let a = pair[0].time;
                        let b = pair[1].time;
                        // Subtract exactly before converting; large adjacent ticks can
                        // collapse to the same f64 seconds value.
                        let elapsed = time.ticks as i128 * a.rate as i128
                            - a.ticks as i128 * time.rate as i128;
                        let span =
                            b.ticks as i128 * a.rate as i128 - a.ticks as i128 * b.rate as i128;
                        let factor =
                            elapsed as f64 / span as f64 * b.rate as f64 / time.rate as f64;
                        let factor = factor.clamp(0.0, 1.0);
                        let factor = if matches!(self, Self::Smooth { .. }) {
                            factor * factor * (3.0 - 2.0 * factor)
                        } else {
                            factor
                        };
                        return Ok((pair[0].value as f64 * (1.0 - factor)
                            + pair[1].value as f64 * factor)
                            as f32);
                    }
                }
                Ok(keys.last().unwrap().value)
            }
        }
    }
}
