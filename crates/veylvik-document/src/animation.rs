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
    /// Convert an absolute frame index at a rational frames-per-second rate.
    /// Reduce in i128 before narrowing to avoid avoidable intermediate overflow.
    pub fn at_frame(frame: i64, frame_rate: Time) -> Result<Self> {
        if frame_rate.ticks <= 0 || frame_rate.rate == 0 {
            return fail("frame rate must be positive");
        }
        let numerator = frame as i128 * frame_rate.rate as i128;
        let denominator = frame_rate.ticks as i128;
        let (mut a, mut b) = (numerator.abs(), denominator);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        let ticks = i64::try_from(numerator / a)
            .map_err(|_| crate::Error("frame time exceeds timestamp range".into()))?;
        let rate = u32::try_from(denominator / a)
            .map_err(|_| crate::Error("frame time denominator exceeds timestamp range".into()))?;
        Self::new(ticks, rate)
    }

    /// Exact rational addition, reduced before narrowing to the storage types.
    pub fn checked_add(self, other: Self) -> Result<Self> {
        self.combine(other, false)
    }
    /// Exact rational subtraction, including i64::MIN without negating it.
    pub fn checked_sub(self, other: Self) -> Result<Self> {
        self.combine(other, true)
    }
    fn combine(self, other: Self, subtract: bool) -> Result<Self> {
        self.seconds()?;
        other.seconds()?;
        let a = i128::from(self.ticks) * i128::from(other.rate);
        let b = i128::from(other.ticks) * i128::from(self.rate);
        let numerator = if subtract { a - b } else { a + b };
        let denominator = i128::from(self.rate) * i128::from(other.rate);
        let (mut a, mut b) = (numerator.abs(), denominator);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        Self::new(
            i64::try_from(numerator / a).map_err(|_| crate::Error("time overflow".into()))?,
            u32::try_from(denominator / a)
                .map_err(|_| crate::Error("time denominator overflow".into()))?,
        )
    }

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
    /// Optional interpolation for the segment starting at this key. Omitted keys
    /// inherit the scalar's interpolation, preserving existing documents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment: Option<Segment>,
    pub time: Time,
    pub value: f32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scalar {
    Constant {
        value: f32,
    },
    Linear {
        keys: Vec<Key>,
    },
    Step {
        keys: Vec<Key>,
    },
    Smooth {
        keys: Vec<Key>,
    },
    Monotone {
        keys: Vec<Key>,
    },
    /// A formula of time, frame and other parameters (see `crate::expression`); links
    /// are stored by node id. `resolved` is attached by the document after each change.
    Expression {
        expression: String,
        #[serde(skip)]
        resolved: Option<std::sync::Arc<crate::expression::Resolved>>,
    },
}
impl From<f32> for Scalar {
    fn from(value: f32) -> Self {
        Self::Constant { value }
    }
}
/// A handle's horizontal reach is a fraction of the adjacent key interval;
/// value is an offset from its endpoint. Handles travel with their endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Handle {
    pub time: f32,
    pub value: f32,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Segment {
    Linear,
    Hold,
    Smooth,
    Auto,
    Bezier { outgoing: Handle, incoming: Handle },
}
impl Segment {
    pub fn validate(self) -> Result<()> {
        if let Self::Bezier {
            outgoing: a,
            incoming: b,
        } = self
            && (!a.time.is_finite()
                || !b.time.is_finite()
                || !a.value.is_finite()
                || !b.value.is_finite()
                || a.time < 0.0
                || b.time < 0.0
                || f64::from(a.time) + f64::from(b.time) > 1.0)
        {
            return fail("Bezier handles must be finite, nonnegative in time, and not cross");
        }
        Ok(())
    }
}
pub const MAX_KEYS: usize = 4096;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    Linear,
    Step,
    Smooth,
    Monotone,
}
impl Scalar {
    pub fn interpolation(&self) -> Option<Interpolation> {
        match self {
            Self::Constant { .. } | Self::Expression { .. } => None,
            Self::Linear { .. } => Some(Interpolation::Linear),
            Self::Step { .. } => Some(Interpolation::Step),
            Self::Smooth { .. } => Some(Interpolation::Smooth),
            Self::Monotone { .. } => Some(Interpolation::Monotone),
        }
    }
    pub fn keys(&self) -> Option<&[Key]> {
        match self {
            Self::Constant { .. } | Self::Expression { .. } => None,
            Self::Linear { keys }
            | Self::Step { keys }
            | Self::Smooth { keys }
            | Self::Monotone { keys } => Some(keys),
        }
    }
    /// Insert or update a key at an exact rational time. Existing equivalent
    /// timestamps retain their representation and curves retain interpolation.
    /// Constants become a single-key linear curve. Failure leaves self unchanged.
    pub fn set_key_at(&mut self, time: Time, value: f32) -> Result<()> {
        self.validate()?;
        if let Self::Expression { .. } = self {
            return fail("this value follows an expression; remove the expression to set keys");
        }
        time.seconds()?;
        if !value.is_finite() {
            return fail("non-finite key");
        }
        let mut candidate = self.clone();
        if let Some(keys) = candidate.keys_mut() {
            match keys.binary_search_by(|key| key.time.compare(time).expect("validated times")) {
                Ok(index) => keys[index].value = value,
                Err(index) => {
                    if keys.len() == MAX_KEYS {
                        return fail(format!("animation exceeds {MAX_KEYS} keys"));
                    }
                    keys.insert(
                        index,
                        Key {
                            segment: None,
                            time,
                            value,
                        },
                    );
                }
            }
        } else {
            candidate = Self::Linear {
                keys: vec![Key {
                    segment: None,
                    time,
                    value,
                }],
            };
        }
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    /// Remove an exact-time key. Removing the final key preserves its value as
    /// a constant. Missing keys and invalid input leave the scalar unchanged.
    pub fn remove_key_at(&mut self, time: Time) -> Result<()> {
        self.validate()?;
        time.seconds()?;
        let mut candidate = self.clone();
        let Some(keys) = candidate.keys_mut() else {
            return fail("scalar has no keyframes");
        };
        let Ok(index) =
            keys.binary_search_by(|key| key.time.compare(time).expect("validated times"))
        else {
            return fail("no keyframe at requested time");
        };
        let removed = keys.remove(index);
        if keys.is_empty() {
            candidate = Self::Constant {
                value: removed.value,
            };
        }
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    pub fn keys_mut(&mut self) -> Option<&mut Vec<Key>> {
        match self {
            Self::Constant { .. } | Self::Expression { .. } => None,
            Self::Linear { keys }
            | Self::Step { keys }
            | Self::Smooth { keys }
            | Self::Monotone { keys } => Some(keys),
        }
    }
    /// Change the curve without changing its keys. Constants remain constants.
    pub fn set_interpolation(&mut self, mode: Interpolation) {
        let Some(keys) = self.keys_mut() else {
            return;
        };
        let mut keys = std::mem::take(keys);
        for key in &mut keys {
            key.segment = None;
        }
        *self = match mode {
            Interpolation::Linear => Self::Linear { keys },
            Interpolation::Step => Self::Step { keys },
            Interpolation::Smooth => Self::Smooth { keys },
            Interpolation::Monotone => Self::Monotone { keys },
        };
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Constant { value } if !value.is_finite() => fail("non-finite scalar"),
            Self::Expression { expression, .. } => crate::expression::validate(expression),
            Self::Linear { keys }
            | Self::Step { keys }
            | Self::Smooth { keys }
            | Self::Monotone { keys } => {
                if keys.is_empty() || keys.len() > MAX_KEYS {
                    return fail(format!("animation needs 1..={MAX_KEYS} keys"));
                }
                for key in keys {
                    key.time.seconds()?;
                    if let Some(segment) = key.segment {
                        segment.validate()?;
                    }
                    if !key.value.is_finite() {
                        return fail("non-finite key");
                    }
                }
                for pair in keys.windows(2) {
                    if let Some(Segment::Bezier { outgoing, incoming }) = pair[0].segment {
                        for value in [
                            f64::from(pair[0].value) + f64::from(outgoing.value),
                            f64::from(pair[1].value) + f64::from(incoming.value),
                        ] {
                            if value.abs() > f64::from(f32::MAX) {
                                return fail("Bezier control exceeds value range");
                            }
                        }
                    }
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
            // Expressions are clamped to the range when sampled.
            Self::Expression { .. } => Ok(()),
            Self::Linear { keys }
            | Self::Step { keys }
            | Self::Smooth { keys }
            | Self::Monotone { keys } => {
                for key in keys {
                    check(key.value)?;
                }
                for pair in keys.windows(2) {
                    if let Some(Segment::Bezier { outgoing, incoming }) = pair[0].segment {
                        for value in [
                            f64::from(pair[0].value) + f64::from(outgoing.value),
                            f64::from(pair[1].value) + f64::from(incoming.value),
                        ] {
                            if value < f64::from(min) || value > f64::from(max) {
                                return fail(format!(
                                    "{name} Bezier controls must be in {min}..={max}"
                                ));
                            }
                        }
                    }
                }
                Ok(())
            }
        }
    }
    /// Compile monotone segments once for bounded normalized sampling in plots.
    /// Returns segments when either the curve or an individual interval uses Auto.
    pub fn monotone_segments(&self) -> Result<Option<Vec<MonotoneSegment>>> {
        self.validate()?;
        let Some(keys) = self.keys() else {
            return Ok(None);
        };
        if !matches!(self, Self::Monotone { .. })
            && !keys.iter().any(|k| k.segment == Some(Segment::Auto))
        {
            return Ok(None);
        }
        Ok(Some(
            (0..keys.len() - 1)
                .map(|index| monotone_segment(keys, index))
                .collect(),
        ))
    }
    /// Freeze a validated curve for repeated sampling without revalidation.
    pub fn compile(&self) -> Result<CompiledScalar> {
        let segments = self.monotone_segments()?;
        Ok(CompiledScalar {
            scalar: self.clone(),
            segments,
        })
    }
    pub fn at(&self, time: Time) -> Result<f32> {
        self.validate()?;
        self.sample_validated(time, None)
    }
    fn sample_validated(&self, time: Time, segments: Option<&[MonotoneSegment]>) -> Result<f32> {
        time.seconds()?;
        if let Self::Expression {
            expression,
            resolved,
        } = self
        {
            return crate::expression::evaluate(expression, resolved.as_deref(), time);
        }
        let Some(keys) = self.keys() else {
            let Self::Constant { value } = self else {
                unreachable!()
            };
            return Ok(*value);
        };
        let upper =
            keys.partition_point(|key| key.time.compare(time).expect("validated times").is_lt());
        if upper == 0 {
            return Ok(keys[0].value);
        }
        if upper == keys.len() {
            return Ok(keys[upper - 1].value);
        }
        if keys[upper].time.compare(time)?.is_eq() {
            return Ok(keys[upper].value);
        }
        let a = &keys[upper - 1];
        let b = &keys[upper];
        // Subtract exactly before converting; large adjacent ticks can collapse
        // to the same f64 seconds value.
        let elapsed =
            time.ticks as i128 * a.time.rate as i128 - a.time.ticks as i128 * time.rate as i128;
        let span =
            b.time.ticks as i128 * a.time.rate as i128 - a.time.ticks as i128 * b.time.rate as i128;
        let factor =
            (elapsed as f64 / span as f64 * b.time.rate as f64 / time.rate as f64).clamp(0.0, 1.0);
        self.sample_interval(upper - 1, factor, segments)
    }
    /// Resolve an outgoing segment without changing inherited interpolation.
    pub fn segment_mode(&self, index: usize) -> Option<Segment> {
        let keys = self.keys()?;
        let key = keys.get(index)?;
        keys.get(index + 1)?;
        Some(key.segment.unwrap_or(match self.interpolation()? {
            Interpolation::Linear => Segment::Linear,
            Interpolation::Step => Segment::Hold,
            Interpolation::Smooth => Segment::Smooth,
            Interpolation::Monotone => Segment::Auto,
        }))
    }
    /// Convert a segment to editable handles. Linear, Smooth and Auto preserve
    /// their shape (to f32 handle precision); Hold starts with flat handles.
    pub fn bezier_segment(&self, index: usize) -> Result<Segment> {
        self.validate()?;
        let mode = self
            .segment_mode(index)
            .ok_or_else(|| crate::Error("missing curve segment".into()))?;
        if let Segment::Bezier { .. } = mode {
            return Ok(mode);
        }
        let keys = self.keys().unwrap();
        let (out, incoming) = match mode {
            Segment::Linear => {
                let delta = f64::from(keys[index + 1].value) - f64::from(keys[index].value);
                (delta / 3.0, -delta / 3.0)
            }
            Segment::Auto => {
                let segment = monotone_segment(keys, index);
                (segment.tangents[0] / 3.0, -segment.tangents[1] / 3.0)
            }
            _ => (0.0, 0.0),
        };
        let segment = Segment::Bezier {
            outgoing: Handle {
                time: 1.0 / 3.0,
                value: out as f32,
            },
            incoming: Handle {
                time: 1.0 / 3.0,
                value: incoming as f32,
            },
        };
        segment.validate()?;
        Ok(segment)
    }
    fn sample_interval(
        &self,
        index: usize,
        fraction: f64,
        segments: Option<&[MonotoneSegment]>,
    ) -> Result<f32> {
        if !fraction.is_finite() {
            return fail("non-finite curve fraction");
        }
        let mode = self
            .segment_mode(index)
            .ok_or_else(|| crate::Error("missing curve segment".into()))?;
        let keys = self.keys().unwrap();
        let a = f64::from(keys[index].value);
        let b = f64::from(keys[index + 1].value);
        let t = fraction.clamp(0.0, 1.0);
        if t == 0.0 {
            return Ok(a as f32);
        }
        if t == 1.0 {
            return Ok(b as f32);
        }
        let value = match mode {
            Segment::Hold => a,
            Segment::Linear => a * (1.0 - t) + b * t,
            Segment::Smooth => {
                let t = t * t * (3.0 - 2.0 * t);
                a * (1.0 - t) + b * t
            }
            Segment::Auto => {
                return segments
                    .map_or_else(|| monotone_segment(keys, index), |s| s[index])
                    .sample(t);
            }
            Segment::Bezier { outgoing, incoming } => {
                // Monotone time coordinates permit bounded bisection, including
                // flat endpoint derivatives. Fixed iterations make it deterministic.
                let (mut lo, mut hi) = (0.0, 1.0);
                for _ in 0..48 {
                    let u = (lo + hi) * 0.5;
                    if cubic(
                        0.0,
                        f64::from(outgoing.time),
                        1.0 - f64::from(incoming.time),
                        1.0,
                        u,
                    ) < t
                    {
                        lo = u;
                    } else {
                        hi = u;
                    }
                }
                cubic(
                    a,
                    a + f64::from(outgoing.value),
                    b + f64::from(incoming.value),
                    b,
                    (lo + hi) * 0.5,
                )
            }
        };
        if !(value as f32).is_finite() {
            return fail("non-finite curve sample");
        }
        Ok(value as f32)
    }
}

/// Immutable validated animation with binary-search lookup and precomputed
/// monotone segments. Later authoring changes do not affect this snapshot.
#[derive(Debug, Clone)]
pub struct CompiledScalar {
    scalar: Scalar,
    segments: Option<Vec<MonotoneSegment>>,
}
fn cubic(a: f64, b: f64, c: f64, d: f64, t: f64) -> f64 {
    let s = 1.0 - t;
    s * s * s * a + 3.0 * s * s * t * b + 3.0 * s * t * t * c + t * t * t * d
}
impl CompiledScalar {
    /// Sample a segment at normalized time using the same evaluator as rendering.
    pub fn sample_segment(&self, index: usize, fraction: f64) -> Result<f32> {
        self.scalar
            .sample_interval(index, fraction, self.segments.as_deref())
    }
    pub fn at(&self, time: Time) -> Result<f32> {
        self.scalar.sample_validated(time, self.segments.as_deref())
    }
}

/// A validated monotone cubic segment in normalized interval coordinates.
#[derive(Debug, Clone, Copy)]
pub struct MonotoneSegment {
    values: [f64; 2],
    tangents: [f64; 2],
}
impl MonotoneSegment {
    /// Sample within the segment, holding endpoint values outside 0..=1.
    pub fn sample(self, fraction: f64) -> Result<f32> {
        if !fraction.is_finite() {
            return fail("non-finite curve fraction");
        }
        let [a, b] = self.values;
        if fraction <= 0.0 {
            return Ok(a as f32);
        }
        if fraction >= 1.0 {
            return Ok(b as f32);
        }
        let t = fraction;
        let t2 = t * t;
        let t3 = t2 * t;
        let value = (2.0 * t3 - 3.0 * t2 + 1.0) * a
            + (t3 - 2.0 * t2 + t) * self.tangents[0]
            + (-2.0 * t3 + 3.0 * t2) * b
            + (t3 - t2) * self.tangents[1];
        // Preserve registered parameter bounds even at floating-point extremes.
        Ok(value.clamp(a.min(b), a.max(b)) as f32)
    }
}
fn duration(a: Time, b: Time) -> f64 {
    let ticks = b.ticks as i128 * a.rate as i128 - a.ticks as i128 * b.rate as i128;
    ticks as f64 / a.rate as f64 / b.rate as f64
}
fn secant(a: &Key, b: &Key) -> f64 {
    (b.value as f64 - a.value as f64) / duration(a.time, b.time)
}
fn tangent(keys: &[Key], index: usize) -> f64 {
    if index == 0 {
        return secant(&keys[0], &keys[1]);
    }
    if index == keys.len() - 1 {
        return secant(&keys[index - 1], &keys[index]);
    }
    let left = secant(&keys[index - 1], &keys[index]);
    let right = secant(&keys[index], &keys[index + 1]);
    if left == 0.0 || right == 0.0 || left.is_sign_positive() != right.is_sign_positive() {
        return 0.0;
    }
    // Weighted harmonic interior slopes (Fritsch–Butland); endpoint slopes
    // use the adjacent secant. Unlike per-interval smoothstep, neighboring
    // segments share a derivative without overshooting their key values.
    let h_left = duration(keys[index - 1].time, keys[index].time);
    let h_right = duration(keys[index].time, keys[index + 1].time);
    let w_left = 2.0 * h_right + h_left;
    let w_right = h_right + 2.0 * h_left;
    (w_left + w_right) / (w_left / left + w_right / right)
}
fn monotone_segment(keys: &[Key], index: usize) -> MonotoneSegment {
    let span = duration(keys[index].time, keys[index + 1].time);
    MonotoneSegment {
        values: [keys[index].value as f64, keys[index + 1].value as f64],
        tangents: [tangent(keys, index) * span, tangent(keys, index + 1) * span],
    }
}
