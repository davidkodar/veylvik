//! Continuous camera motion, independent of preview or export frame rate.
use crate::{Error, Result, Time};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CameraOrbit {
    pub enabled: bool,
    pub radius: f32,
    /// Positive is above the target (scene Y points down).
    pub height: f32,
    pub start: f32,
    pub duration: f32,
    pub start_angle: f32,
    pub sweep: f32,
    pub clockwise: bool,
    pub ease: bool,
}
impl Default for CameraOrbit {
    fn default() -> Self {
        Self {
            enabled: true,
            radius: 3000.,
            height: 160.,
            start: 0.,
            duration: 10.,
            start_angle: 0.,
            sweep: 360.,
            clockwise: true,
            ease: true,
        }
    }
}
impl CameraOrbit {
    pub fn validate(&self) -> Result<()> {
        for (v, lo, hi) in [
            (self.radius, 1., 1e6),
            (self.height, -1e6, 1e6),
            (self.start, 0., 1e6),
            (self.duration, 0.001, 1e6),
            (self.start_angle, -36000., 36000.),
            (self.sweep, 0., 36000.),
        ] {
            if !v.is_finite() || !(lo..=hi).contains(&v) {
                return Err(Error(
                    "Camera orbit value is outside its supported range".into(),
                ));
            }
        }
        Ok(())
    }
    /// World-space offset from the target, evaluated at exact shutter time.
    pub fn offset(&self, time: Time) -> [f32; 3] {
        let mut t = ((time.ticks as f64 / time.rate as f64 - self.start as f64)
            / self.duration as f64)
            .clamp(0., 1.);
        if self.ease {
            t = t * t * (3. - 2. * t);
        }
        let direction = if self.clockwise { 1. } else { -1. };
        let angle = (self.start_angle as f64 + direction * self.sweep as f64 * t).to_radians();
        [
            self.radius * angle.sin() as f32,
            -self.height,
            self.radius * angle.cos() as f32,
        ]
    }
    pub fn schema() -> serde_json::Value {
        use serde_json::json;
        let number = |min: f32, max: f32| json!({"type":"number","minimum":min,"maximum":max});
        json!({"anyOf":[{"type":"null"},{"type":"object","additionalProperties":false,
            "description":"Optional procedural orbit around aim_at, or the frame centre when unset. Overrides placement position and rotation only while enabled; all fields have defaults. start/duration are seconds, angles degrees, radius/height scene pixels. Positive height is above the target. Clamps before start and after completion.",
            "properties":{"enabled":{"type":"boolean"},"radius":number(1.,1e6),"height":number(-1e6,1e6),
                "start":number(0.,1e6),"duration":number(0.001,1e6),"start_angle":number(-36000.,36000.),
                "sweep":number(0.,36000.),"clockwise":{"type":"boolean"},"ease":{"type":"boolean"}}}]})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn continuous_orbit_closes_and_reverses_without_frame_keys() {
        let mut orbit = CameraOrbit {
            ease: false,
            duration: 4.,
            ..Default::default()
        };
        let at = |s| Time { ticks: s, rate: 1 };
        let a = orbit.offset(at(0));
        let b = orbit.offset(at(4));
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() < 0.001);
        }
        assert!((orbit.offset(at(1))[0] - orbit.radius).abs() < 0.001);
        orbit.clockwise = false;
        assert!((orbit.offset(at(1))[0] + orbit.radius).abs() < 0.001);
        assert_eq!(orbit.offset(at(-1)), a);
        assert_eq!(orbit.offset(at(5)), orbit.offset(at(4)));
        orbit.duration = 0.;
        assert!(orbit.validate().is_err());
    }
}
