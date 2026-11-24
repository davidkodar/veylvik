//! Resolution-independent planar motion and robust feature consensus.
//!
//! Coordinates use image width/height as units (not aspect-corrected display
//! coordinates). Pixel matching belongs to the renderer; only feature coordinates
//! enter this solver. No text, material, or particular consumer is assumed.
use crate::{Result, Time, fail};
use serde::{Deserialize, Serialize};

pub type Point = [f64; 2];
pub type Quad = [Point; 4];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarpMode {
    Follow,
    Fit,
    Stabilize,
    /// The whole picture inside the surface without squeezing it: as large as fits,
    /// centred, by the surface's proportions on the reference frame.
    FitInside,
}
choices!(WarpMode {
    Follow,
    Fit,
    FitInside,
    Stabilize
});
impl WarpMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Follow => "Follow surface",
            Self::Fit => "Fit image to surface",
            Self::FitInside => "Fit inside surface, keeping shape",
            Self::Stabilize => "Stabilize surface",
        }
    }
}
impl Default for Track {
    fn default() -> Self {
        let reference = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
        let time = Time { ticks: 0, rate: 1 };
        Self {
            reference_time: time,
            reference,
            region: None,
            samples: vec![Sample {
                time,
                corners: reference,
                features: vec![],
                rms_pixels: 0.,
                motion: None,
            }],
        }
    }
}

pub fn schema() -> serde_json::Value {
    use serde_json::json;
    let time = json!({"type":"object","properties":{"ticks":{"type":"integer","minimum":i64::MIN,"maximum":i64::MAX},"rate":{"type":"integer","minimum":1,"maximum":u32::MAX}},"required":["ticks","rate"],"additionalProperties":false});
    let point = json!({"type":"array","minItems":2,"maxItems":2,"items":{"type":"number","minimum":-1000000,"maximum":1000000}});
    let quad = json!({"type":"array","minItems":4,"maxItems":4,"items":point});
    let feature = json!({"type":"object","properties":{"reference":point,"position":point,"correlation":{"type":"number","minimum":-1,"maximum":1},"separation":{"type":"number","minimum":0,"maximum":3},"accepted":{"type":"boolean"}},"required":["reference","position","correlation","separation","accepted"],"additionalProperties":false});
    let sample = json!({"type":"object","properties":{"time":time,"corners":quad,"features":{"type":"array","maxItems":256,"items":feature},"rms_pixels":{"type":"number","minimum":0}},"required":["time","corners","features","rms_pixels"],"additionalProperties":false});
    json!({"type":"object","properties":{"reference_time":time,"reference":quad,"samples":{"type":"array","minItems":1,"maxItems":4096,"items":sample}},"required":["reference_time","reference","samples"],"additionalProperties":false})
}

/// Save analysis as an independent graph node. No consumers or output are changed.
pub fn save(
    doc: &crate::Composition,
    source: crate::NodeId,
    data: Track,
) -> Result<(crate::NodeId, Vec<crate::Command>)> {
    data.validate()?;
    if !doc.nodes.contains_key(&source) {
        return fail("tracking source missing");
    }
    let node = crate::Node::new(
        "Planar track",
        crate::Operation::PlanarTrack(crate::nodes::PlanarTrack {
            data: Box::new(data),
        }),
        &[("image", source)],
    );
    let id = node.id;
    let commands = vec![crate::Command::Put {
        node: Box::new(node),
    }];
    crate::Document::new(doc.clone())?.prepare(0, &commands)?;
    Ok((id, commands))
}

/// Row-major projective transform. Its scale is arbitrary.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Homography {
    pub matrix: [f64; 9],
}
impl Homography {
    pub const IDENTITY: Self = Self {
        matrix: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
    };
    pub fn project(self, [x, y]: Point) -> Option<Point> {
        let m = self.matrix;
        let w = m[6] * x + m[7] * y + m[8];
        let p = [
            (m[0] * x + m[1] * y + m[2]) / w,
            (m[3] * x + m[4] * y + m[5]) / w,
        ];
        (w.abs() > 1e-12 && p.iter().all(|v| v.is_finite())).then_some(p)
    }
    pub fn inverse(self) -> Option<Self> {
        let m = self.matrix;
        let c = [
            m[4] * m[8] - m[5] * m[7],
            m[2] * m[7] - m[1] * m[8],
            m[1] * m[5] - m[2] * m[4],
            m[5] * m[6] - m[3] * m[8],
            m[0] * m[8] - m[2] * m[6],
            m[2] * m[3] - m[0] * m[5],
            m[3] * m[7] - m[4] * m[6],
            m[1] * m[6] - m[0] * m[7],
            m[0] * m[4] - m[1] * m[3],
        ];
        let d = m[0] * c[0] + m[1] * c[3] + m[2] * c[6];
        if !d.is_finite() || d.abs() < 1e-12 {
            return None;
        }
        Some(Self {
            matrix: c.map(|v| v / d),
        })
    }
    /// Apply `other`, then this transform.
    pub fn compose(self, other: Self) -> Self {
        let mut matrix = [0.; 9];
        for r in 0..3 {
            for c in 0..3 {
                matrix[r * 3 + c] = (0..3)
                    .map(|k| self.matrix[r * 3 + k] * other.matrix[k * 3 + c])
                    .sum();
            }
        }
        Self { matrix }
    }
    /// Reject a surface crossing the projective horizon, even if its individual
    /// corners happen to remain finite.
    pub fn transform_quad(self, quad: Quad) -> Result<Quad> {
        validate_quad(quad)?;
        let mut result = [[0.; 2]; 4];
        let mut sign = 0_f64;
        for (i, p) in quad.into_iter().enumerate() {
            let w = self.matrix[6] * p[0] + self.matrix[7] * p[1] + self.matrix[8];
            if i > 0 && sign * w <= 0. {
                return fail("surface crosses the projective horizon");
            }
            sign = w;
            result[i] = self
                .project(p)
                .ok_or_else(|| crate::Error("invalid surface projection".into()))?;
        }
        validate_quad(result)?;
        Ok(result)
    }
    pub fn between(from: Quad, to: Quad) -> Result<Self> {
        validate_quad(from)?;
        validate_quad(to)?;
        let pairs: Vec<_> = from.into_iter().zip(to).collect();
        let h =
            fit(&pairs).ok_or_else(|| crate::Error("degenerate planar correspondence".into()))?;
        h.transform_quad(from)?;
        Ok(h)
    }
}

pub fn validate_quad(q: Quad) -> Result<()> {
    if q.iter().flatten().any(|v| !v.is_finite() || v.abs() > 1e6) {
        return fail("surface coordinates must be finite and bounded");
    }
    let mut sign = 0.;
    for i in 0..4 {
        let a = q[i];
        let b = q[(i + 1) % 4];
        let c = q[(i + 2) % 4];
        let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
        if cross.abs() < 1e-10 || (i > 0 && cross * sign <= 0.) {
            return fail("surface must be a nondegenerate convex quadrilateral in perimeter order");
        }
        sign = cross;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feature {
    pub reference: Point,
    pub position: Point,
    /// Normalized patch correlation, in [-1, 1].
    pub correlation: f64,
    pub separation: f64,
    /// Consensus, not merely patch correlation, decides acceptance.
    pub accepted: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    pub time: Time,
    /// Where the reference corners appear on this frame. When `motion` is present they
    /// may be degenerate, because part of the pinned surface has passed behind the camera.
    pub corners: Quad,
    pub features: Vec<Feature>,
    pub rms_pixels: f64,
    /// Reference-to-frame motion, stored when the pinned corners no longer form a valid
    /// quad while the surface is still tracked. Renderers clip the part behind the camera.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion: Option<Homography>,
}
impl Sample {
    /// The reference-to-frame motion of this sample.
    pub fn motion(&self, reference: Quad) -> Result<Homography> {
        match self.motion {
            Some(motion) => Ok(motion),
            None => Homography::between(reference, self.corners),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub reference_time: Time,
    pub reference: Quad,
    /// Area analyzed on the reference frame when it differs from the corners: any
    /// textured part of the same plane, so corners may be small, empty or off-screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<Quad>,
    pub samples: Vec<Sample>,
}
impl Track {
    pub fn validate(&self) -> Result<()> {
        self.reference_time.seconds()?;
        validate_quad(self.reference)?;
        if let Some(region) = self.region {
            validate_quad(region)?;
        }
        if self.samples.is_empty() || self.samples.len() > 4096 {
            return fail("planar track requires 1..4096 samples");
        }
        let mut previous: Option<Time> = None;
        for sample in &self.samples {
            sample.time.seconds()?;
            if let Some(time) = previous
                && !time.compare(sample.time)?.is_lt()
            {
                return fail("planar samples must have increasing times");
            }
            previous = Some(sample.time);
            match sample.motion {
                Some(motion) => {
                    if motion.matrix.iter().any(|v| !v.is_finite()) || motion.inverse().is_none() {
                        return fail("planar sample motion must be finite and invertible");
                    }
                    if sample
                        .corners
                        .iter()
                        .flatten()
                        .any(|v| !v.is_finite() || v.abs() > 1e6)
                    {
                        return fail("planar sample corners must be finite and bounded");
                    }
                }
                None => {
                    Homography::between(self.reference, sample.corners)?;
                }
            }
            if sample.features.len() > 256
                || !sample.rms_pixels.is_finite()
                || sample.rms_pixels < 0.
            {
                return fail("invalid planar diagnostics");
            }
            for f in &sample.features {
                if f.reference
                    .into_iter()
                    .chain(f.position)
                    .any(|v| !v.is_finite() || v.abs() > 1e6)
                    || !(-1. ..=1.).contains(&f.correlation)
                    || !(0. ..=3.).contains(&f.separation)
                {
                    return fail("invalid planar feature");
                }
            }
        }
        Ok(())
    }
    /// Replace only successfully analyzed sample times, keeping the original
    /// reference basis and all untouched sections. The new reference quad is a
    /// correction at its reference time, not a new coordinate system for users.
    pub fn splice(&self, section: &Track) -> Result<Self> {
        self.validate()?;
        section.validate()?;
        let rebase = Homography::between(section.reference, self.reference)?;
        // Motion stored relative to the section's reference frame, re-expressed from ours.
        let unbase = rebase
            .inverse()
            .ok_or_else(|| crate::Error("Invalid correction reference".into()))?;
        let mut merged = self.clone();
        for sample in &section.samples {
            let mut sample = sample.clone();
            sample.motion = sample.motion.map(|motion| motion.compose(unbase));
            for feature in &mut sample.features {
                feature.reference = rebase
                    .project(feature.reference)
                    .ok_or_else(|| crate::Error("Invalid correction reference".into()))?;
            }
            if let Some(existing) = merged
                .samples
                .iter_mut()
                .find(|s| s.time.compare(sample.time).is_ok_and(|o| o.is_eq()))
            {
                *existing = sample;
            } else {
                merged.samples.push(sample);
            }
        }
        merged
            .samples
            .sort_by(|a, b| a.time.compare(b.time).expect("validated times"));
        merged.validate()?;
        Ok(merged)
    }
    /// Interpolate corner positions, not matrix coefficients. Hold at endpoints.
    /// Call `validate` when accepting persisted data, before repeated evaluation.
    pub fn motion_at(&self, time: Time) -> Result<Homography> {
        time.seconds()?;
        let first = self
            .samples
            .first()
            .ok_or_else(|| crate::Error("empty planar track".into()))?;
        let mut previous = first;
        for sample in &self.samples {
            if !sample.time.compare(time)?.is_lt() {
                let span = sample.time.checked_sub(previous.time)?.seconds()?;
                let t = if span > 0. {
                    (time.checked_sub(previous.time)?.seconds()? / span).clamp(0., 1.)
                } else {
                    0.
                };
                if previous.motion.is_some() || sample.motion.is_some() {
                    return blend(
                        previous.motion(self.reference)?,
                        sample.motion(self.reference)?,
                        t,
                    );
                }
                let mut q = previous.corners;
                for (i, p) in q.iter_mut().enumerate() {
                    for (axis, v) in p.iter_mut().enumerate() {
                        *v += (sample.corners[i][axis] - *v) * t;
                    }
                }
                return Homography::between(self.reference, q);
            }
            previous = sample;
        }
        previous.motion(self.reference)
    }
}

/// Interpolates two motions by their scale-normalized matrices, signs aligned so that a
/// projective flip of scale never passes through zero.
fn blend(a: Homography, b: Homography, t: f64) -> Result<Homography> {
    let unit = |h: Homography| {
        let norm = h.matrix.iter().map(|v| v * v).sum::<f64>().sqrt();
        h.matrix.map(|v| v / norm)
    };
    let (a, mut b) = (unit(a), unit(b));
    if a.iter().zip(&b).map(|(x, y)| x * y).sum::<f64>() < 0. {
        b = b.map(|v| -v);
    }
    let motion = Homography {
        matrix: std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t),
    };
    if motion.matrix.iter().any(|v| !v.is_finite()) || motion.inverse().is_none() {
        return fail("planar motion cannot be interpolated");
    }
    Ok(motion)
}

// Normalize both point clouds before solving the eight inhomogeneous equations.
// Pivoting rejects collinear samples; centroid/RMS normalization avoids 4K/8K
// coordinate conditioning problems without introducing a linear algebra runtime.
/// Least-squares homography through at least four correspondences; `None` when degenerate.
pub fn fit(pairs: &[(Point, Point)]) -> Option<Homography> {
    if pairs.len() < 4 {
        return None;
    }
    let normalizer = |target: bool| -> Option<Homography> {
        let points: Vec<_> = pairs
            .iter()
            .map(|p| if target { p.1 } else { p.0 })
            .collect();
        let center: Point =
            [0, 1].map(|axis| points.iter().map(|p| p[axis]).sum::<f64>() / points.len() as f64);
        let spread = (points
            .iter()
            .map(|p| (p[0] - center[0]).powi(2) + (p[1] - center[1]).powi(2))
            .sum::<f64>()
            / points.len() as f64)
            .sqrt();
        if !spread.is_finite() || spread < 1e-12 {
            return None;
        }
        let s = 2_f64.sqrt() / spread;
        Some(Homography {
            matrix: [s, 0., -s * center[0], 0., s, -s * center[1], 0., 0., 1.],
        })
    };
    let a = normalizer(false)?;
    let b = normalizer(true)?;
    let mut equations = [[0_f64; 9]; 8];
    for &(source, target) in pairs {
        let [x, y] = a.project(source)?;
        let [u, v] = b.project(target)?;
        for (row, rhs) in [
            ([x, y, 1., 0., 0., 0., -u * x, -u * y], u),
            ([0., 0., 0., x, y, 1., -v * x, -v * y], v),
        ] {
            for i in 0..8 {
                for j in 0..8 {
                    equations[i][j] += row[i] * row[j];
                }
                equations[i][8] += row[i] * rhs;
            }
        }
    }
    for col in 0..8 {
        let pivot = (col..8)
            .max_by(|&a, &b| equations[a][col].abs().total_cmp(&equations[b][col].abs()))?;
        equations.swap(col, pivot);
        let scale = equations[col][col];
        if !scale.is_finite() || scale.abs() < 1e-10 {
            return None;
        }
        for v in &mut equations[col][col..] {
            *v /= scale;
        }
        for row in 0..8 {
            if row != col {
                let scale = equations[row][col];
                let pivot_row = equations[col];
                for (value, pivot) in equations[row][col..].iter_mut().zip(&pivot_row[col..]) {
                    *value -= scale * pivot;
                }
            }
        }
    }
    let mut matrix = [1.; 9];
    for i in 0..8 {
        matrix[i] = equations[i][8];
    }
    let result = b.inverse()?.compose(Homography { matrix }).compose(a);
    result.inverse()?;
    Some(result)
}
