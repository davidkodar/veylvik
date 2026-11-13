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
}
impl WarpMode {
    pub const ALL: [Self; 3] = [Self::Follow, Self::Fit, Self::Stabilize];
    pub fn label(self) -> &'static str {
        match self {
            Self::Follow => "Follow surface",
            Self::Fit => "Fit image to surface",
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
            samples: vec![Sample {
                time,
                corners: reference,
                features: vec![],
                rms_pixels: 0.,
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
        crate::Operation::PlanarTrack {
            data: Box::new(data),
        },
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
    pub corners: Quad,
    pub features: Vec<Feature>,
    pub rms_pixels: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub reference_time: Time,
    pub reference: Quad,
    pub samples: Vec<Sample>,
}
impl Track {
    pub fn validate(&self) -> Result<()> {
        self.reference_time.seconds()?;
        validate_quad(self.reference)?;
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
            Homography::between(self.reference, sample.corners)?;
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
        let mut merged = self.clone();
        for sample in &section.samples {
            let mut sample = sample.clone();
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
        Homography::between(self.reference, previous.corners)
    }
}

#[derive(Debug, Clone)]
pub struct Consensus {
    pub motion: Homography,
    pub accepted: Vec<bool>,
    pub rms_pixels: f64,
}

/// Deterministic RANSAC followed by an all-inlier least-squares fit. Coordinates
/// are normalized; reprojection thresholds are measured in the actual image's
/// pixels, so portrait, landscape, HD and UHD get equivalent rejection rules.
pub fn estimate(features: &[Feature], size: [u32; 2], threshold_pixels: f64) -> Result<Consensus> {
    if features.len() > 256
        || size.contains(&0)
        || !threshold_pixels.is_finite()
        || !(0.1..=32.).contains(&threshold_pixels)
    {
        return fail("invalid planar estimation settings");
    }
    let eligible: Vec<usize> = features
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            (f.reference
                .into_iter()
                .chain(f.position)
                .all(|v| v.is_finite() && v.abs() <= 1e6)
                && (0.75..=1.).contains(&f.correlation)
                && (0.03..=3.).contains(&f.separation))
            .then_some(i)
        })
        .collect();
    if eligible.len() < 6 {
        return fail("not enough reliable features to track a plane");
    }
    let mask = |h: Homography| -> Vec<bool> {
        let inverse = h.inverse();
        features
            .iter()
            .enumerate()
            .map(|(i, f)| {
                eligible.contains(&i)
                    && h.project(f.reference)
                        .zip(inverse.and_then(|h| h.project(f.position)))
                        .is_some_and(|(forward, backward)| {
                            distance(forward, f.position, size).max(distance(
                                backward,
                                f.reference,
                                size,
                            )) <= threshold_pixels * threshold_pixels
                        })
            })
            .collect()
    };
    let mut best = vec![false; features.len()];
    let mut best_error = f64::INFINITY;
    let mut seed = 0x9e3779b97f4a7c15_u64;
    for _ in 0..768 {
        let mut indices = [usize::MAX; 4];
        for slot in 0..4 {
            loop {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let i = eligible[seed as usize % eligible.len()];
                if !indices[..slot].contains(&i) {
                    indices[slot] = i;
                    break;
                }
            }
        }
        let pairs: Vec<_> = indices
            .map(|i| (features[i].reference, features[i].position))
            .into();
        let Some(h) = fit(&pairs) else {
            continue;
        };
        let accepted = mask(h);
        let count = accepted.iter().filter(|&&v| v).count();
        let error: f64 = features
            .iter()
            .zip(&accepted)
            .filter(|(_, ok)| **ok)
            .map(|(f, _)| distance(h.project(f.reference).unwrap(), f.position, size))
            .sum();
        let best_count = best.iter().filter(|&&v| v).count();
        if count > best_count || (count == best_count && error < best_error) {
            best = accepted;
            best_error = error;
        }
    }
    if best.iter().filter(|&&v| v).count() < 6 {
        return fail("no consistent plane found");
    }
    let mut motion = Homography::IDENTITY;
    for _ in 0..3 {
        let pairs: Vec<_> = features
            .iter()
            .zip(&best)
            .filter(|(_, ok)| **ok)
            .map(|(f, _)| (f.reference, f.position))
            .collect();
        motion = fit(&pairs)
            .ok_or_else(|| crate::Error("degenerate planar feature distribution".into()))?;
        best = mask(motion);
        if best.iter().filter(|&&v| v).count() < 6 {
            return fail("plane lost during refinement");
        }
    }
    let count = best.iter().filter(|&&v| v).count();
    if count * 2 <= eligible.len() {
        return fail("no dominant plane; narrow the tracking region to one surface");
    }
    let error: f64 = features
        .iter()
        .zip(&best)
        .filter(|(_, ok)| **ok)
        .map(|(f, _)| distance(motion.project(f.reference).unwrap(), f.position, size))
        .sum();
    Ok(Consensus {
        motion,
        accepted: best,
        rms_pixels: (error / count as f64).sqrt(),
    })
}
fn distance(a: Point, b: Point, size: [u32; 2]) -> f64 {
    ((a[0] - b[0]) * f64::from(size[0])).powi(2) + ((a[1] - b[1]) * f64::from(size[1])).powi(2)
}

// Normalize both point clouds before solving the eight inhomogeneous equations.
// Pivoting rejects collinear samples; centroid/RMS normalization avoids 4K/8K
// coordinate conditioning problems without introducing a linear algebra runtime.
fn fit(pairs: &[(Point, Point)]) -> Option<Homography> {
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
