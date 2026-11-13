//! Robust plane fitting: deterministic RANSAC over feature matches, then a least-squares refit.
use crate::{Result, error};
fn fail<T>(message: &str) -> Result<T> {
    Err(error(message))
}
use veylvik_document::planar::{self, Feature, Homography, Point};

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
        let Some(h) = planar::fit(&pairs) else {
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
        motion =
            planar::fit(&pairs).ok_or_else(|| error("degenerate planar feature distribution"))?;
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
