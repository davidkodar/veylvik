//! Radial lens distortion (Brown-Conrady k1, k2) about the frame centre: offsets are normalised by half the frame diagonal, and distorted = ideal × (1 + k1 r² + k2 r⁴) with r the ideal radius.
use crate::linalg::V2;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Distortion {
    pub k1: f64,
    pub k2: f64,
}
impl Distortion {
    pub const NONE: Self = Self { k1: 0.0, k2: 0.0 };
    /// How much an ideal offset at squared normalised radius `r2` is scaled.
    pub fn factor(&self, r2: f64) -> f64 {
        1.0 + self.k1 * r2 + self.k2 * r2 * r2
    }
    /// The derivative of the distorted radius by the ideal radius.
    pub fn slope(&self, r2: f64) -> f64 {
        1.0 + 3.0 * self.k1 * r2 + 5.0 * self.k2 * r2 * r2
    }
    /// One to one from the centre to well past the frame corner (r = 1.5), so it can
    /// be undone everywhere in the frame.
    pub fn invertible(&self) -> bool {
        (0..=48).all(|i| {
            let r = f64::from(i) / 32.0;
            self.slope(r * r) > 0.05
        })
    }
    /// The distorted pixel offset (from the frame centre) of an ideal one.
    pub fn distort(&self, offset: V2, radius: f64) -> V2 {
        let s = self.factor((offset[0] * offset[0] + offset[1] * offset[1]) / (radius * radius));
        [offset[0] * s, offset[1] * s]
    }
    /// The ideal pixel offset whose distortion is `offset` (Newton on the radius).
    pub fn undistort(&self, offset: V2, radius: f64) -> V2 {
        let rd = (offset[0] * offset[0] + offset[1] * offset[1]).sqrt() / radius;
        if rd < 1e-12 {
            return offset;
        }
        let mut ru = rd;
        for _ in 0..30 {
            let step = (ru * self.factor(ru * ru) - rd) / self.slope(ru * ru).max(0.05);
            ru -= step;
            if step.abs() < 1e-13 {
                break;
            }
        }
        let s = ru / rd;
        [offset[0] * s, offset[1] * s]
    }
}
/// Half the frame diagonal (pixels): the unit radius of the distortion model.
pub fn radius(width: u32, height: u32) -> f64 {
    0.5 * f64::from(width).hypot(f64::from(height))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn undistort_inverts_distort() {
        let lens = Distortion {
            k1: -0.12,
            k2: 0.02,
        };
        assert!(lens.invertible());
        let r = radius(1920, 1080);
        for offset in [[0.0, 0.0], [900.0, 500.0], [-300.0, 20.0], [5.0, -530.0]] {
            let back = lens.undistort(lens.distort(offset, r), r);
            assert!((back[0] - offset[0]).abs() < 1e-7 && (back[1] - offset[1]).abs() < 1e-7);
        }
        assert!(!Distortion { k1: -0.6, k2: 0.0 }.invertible());
    }
}
