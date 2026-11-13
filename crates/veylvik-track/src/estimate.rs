//! Robust plane fitting: deterministic RANSAC over feature matches, then a least-squares refit.
use crate::{Result, error};
fn fail<T>(message: &str) -> Result<T> {
    Err(error(message))
}
use veylvik_document::planar::{self, Feature, Homography, Point};

/// Which transformations the plane may undergo between frames. Simpler models need fewer
/// matches and cannot invent perspective that the evidence does not support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Motion {
    /// Movement only.
    Translation,
    /// Movement, rotation and uniform scale.
    Similarity,
    /// Adds shear and non-uniform scale; parallel lines stay parallel.
    Affine,
    /// Full planar perspective.
    Perspective,
    /// Perspective when the matches clearly need it, otherwise the simplest model that
    /// explains them as well.
    #[default]
    Auto,
}
impl Motion {
    pub const ALL: [Self; 5] = [
        Self::Auto,
        Self::Perspective,
        Self::Affine,
        Self::Similarity,
        Self::Translation,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Translation => "Position",
            Self::Similarity => "Position, rotation, scale",
            Self::Affine => "Affine (shear)",
            Self::Perspective => "Perspective",
            Self::Auto => "Automatic",
        }
    }
    /// Matches that determine the model exactly, and the inliers required to accept it.
    fn sizes(self) -> (usize, usize) {
        match self {
            Self::Translation => (1, 3),
            Self::Similarity => (2, 4),
            Self::Affine => (3, 5),
            Self::Perspective | Self::Auto => (4, 6),
        }
    }
}
#[derive(Debug, Clone)]
pub struct Consensus {
    pub motion: Homography,
    pub accepted: Vec<bool>,
    pub rms_pixels: f64,
    /// The model actually fitted; never `Auto`.
    pub model: Motion,
}

/// Fits `model`; `Auto` compares perspective with affine and keeps the simpler one unless
/// perspective explains clearly more matches or clearly smaller errors, then falls back to
/// simpler models when too few matches remain.
pub fn estimate(
    features: &[Feature],
    size: [u32; 2],
    threshold_pixels: f64,
    model: Motion,
) -> Result<Consensus> {
    if model != Motion::Auto {
        return estimate_model(features, size, threshold_pixels, model);
    }
    let perspective = estimate_model(features, size, threshold_pixels, Motion::Perspective);
    let affine = estimate_model(features, size, threshold_pixels, Motion::Affine);
    match (perspective, affine) {
        (Ok(p), Ok(a)) => {
            let inliers = |c: &Consensus| c.accepted.iter().filter(|&&v| v).count();
            let needs_perspective = inliers(&p) >= inliers(&a) + (inliers(&a) / 10).max(2)
                || a.rms_pixels > p.rms_pixels + 0.25;
            Ok(if needs_perspective { p } else { a })
        }
        (Ok(p), Err(_)) => Ok(p),
        (Err(_), Ok(a)) => Ok(a),
        (Err(e), Err(_)) => estimate_model(features, size, threshold_pixels, Motion::Similarity)
            .or_else(|_| estimate_model(features, size, threshold_pixels, Motion::Translation))
            .map_err(|_| e),
    }
}
/// Deterministic RANSAC followed by an all-inlier least-squares fit. Coordinates
/// are normalized; reprojection thresholds are measured in the actual image's
/// pixels, so portrait, landscape, HD and UHD get equivalent rejection rules.
fn estimate_model(
    features: &[Feature],
    size: [u32; 2],
    threshold_pixels: f64,
    model: Motion,
) -> Result<Consensus> {
    let (sample, required) = model.sizes();
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
    if eligible.len() < required {
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
        for slot in 0..sample {
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
        let pairs: Vec<_> = indices[..sample]
            .iter()
            .map(|&i| (features[i].reference, features[i].position))
            .collect();
        let Some(h) = fit(model, &pairs, size) else {
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
    if best.iter().filter(|&&v| v).count() < required {
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
        motion = fit(model, &pairs, size)
            .ok_or_else(|| error("degenerate planar feature distribution"))?;
        best = mask(motion);
        if best.iter().filter(|&&v| v).count() < required {
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
        model,
    })
}
/// Least-squares `model` through normalized correspondences. Constrained models are solved
/// in pixels, where rotation and uniform scale are meaningful for non-square frames.
fn fit(model: Motion, pairs: &[(Point, Point)], size: [u32; 2]) -> Option<Homography> {
    if matches!(model, Motion::Perspective | Motion::Auto) {
        return planar::fit(pairs);
    }
    let (w, h) = (f64::from(size[0]), f64::from(size[1]));
    let px: Vec<(Point, Point)> = pairs
        .iter()
        .map(|(a, b)| ([a[0] * w, a[1] * h], [b[0] * w, b[1] * h]))
        .collect();
    let n = px.len() as f64;
    if px.is_empty() {
        return None;
    }
    let mean = |pick: fn(&(Point, Point)) -> Point| -> Point {
        [0, 1].map(|axis| px.iter().map(|p| pick(p)[axis]).sum::<f64>() / n)
    };
    let (ca, cb) = (mean(|p| p.0), mean(|p| p.1));
    let centered: Vec<(Point, Point)> = px
        .iter()
        .map(|(a, b)| ([a[0] - ca[0], a[1] - ca[1]], [b[0] - cb[0], b[1] - cb[1]]))
        .collect();
    // Pixel-space matrix [a b c; d e f; 0 0 1] mapping reference to current.
    let m: [f64; 6] = match model {
        Motion::Translation => [1., 0., cb[0] - ca[0], 0., 1., cb[1] - ca[1]],
        Motion::Similarity => {
            let (mut sxx, mut num_a, mut num_b) = (0., 0., 0.);
            for (a, b) in &centered {
                sxx += a[0] * a[0] + a[1] * a[1];
                num_a += a[0] * b[0] + a[1] * b[1];
                num_b += a[0] * b[1] - a[1] * b[0];
            }
            if sxx < 1e-9 * n {
                return None;
            }
            let (p, q) = (num_a / sxx, num_b / sxx);
            [
                p,
                -q,
                cb[0] - (p * ca[0] - q * ca[1]),
                q,
                p,
                cb[1] - (q * ca[0] + p * ca[1]),
            ]
        }
        _ => {
            let (mut xx, mut xy, mut yy) = (0., 0., 0.);
            let (mut ux, mut uy, mut vx, mut vy) = (0., 0., 0., 0.);
            for (a, b) in &centered {
                xx += a[0] * a[0];
                xy += a[0] * a[1];
                yy += a[1] * a[1];
                ux += a[0] * b[0];
                uy += a[1] * b[0];
                vx += a[0] * b[1];
                vy += a[1] * b[1];
            }
            let det = xx * yy - xy * xy;
            if det.abs() < 1e-9 * (xx * yy).max(1e-12) {
                return None;
            }
            let (a, b) = ((ux * yy - uy * xy) / det, (uy * xx - ux * xy) / det);
            let (d, e) = ((vx * yy - vy * xy) / det, (vy * xx - vx * xy) / det);
            [
                a,
                b,
                cb[0] - (a * ca[0] + b * ca[1]),
                d,
                e,
                cb[1] - (d * ca[0] + e * ca[1]),
            ]
        }
    };
    // Back to normalized coordinates: S^-1 * M * S with S = diag(w, h, 1).
    let h_norm = Homography {
        matrix: [
            m[0],
            m[1] * h / w,
            m[2] / w,
            m[3] * w / h,
            m[4],
            m[5] / h,
            0.,
            0.,
            1.,
        ],
    };
    h_norm.inverse().map(|_| h_norm)
}
fn distance(a: Point, b: Point, size: [u32; 2]) -> f64 {
    ((a[0] - b[0]) * f64::from(size[0])).powi(2) + ((a[1] - b[1]) * f64::from(size[1])).powi(2)
}
