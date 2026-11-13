//! Edge-band matting around a tracked matte: a trimap marks what is surely subject, surely
//! background and the uncertain band between, a matting model resolves only that band, and the
//! result is merged back so everything outside the band stays exactly as tracked.
use crate::Result;

/// Longest side the matting model is given; larger crops are scaled down for it.
pub const MAX_SIDE: usize = 1536;
const SURE: f32 = 0.98;

/// A crop of the frame in whole pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}
/// Half-width of the uncertain band, from the subject's size: about 2% of its diagonal.
pub fn band(alpha: &[f32], width: usize) -> Option<usize> {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for (i, a) in alpha.iter().enumerate() {
        if *a > 0.5 {
            let (x, y) = (i % width, i / width);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    (x0 != usize::MAX).then(|| {
        let diagonal = (((x1 - x0) as f64).powi(2) + ((y1 - y0) as f64).powi(2)).sqrt();
        (0.02 * diagonal).round().clamp(6.0, 48.0) as usize
    })
}
/// The region holding every pixel of the matte above zero, grown by `margin` and clamped.
pub fn crop(alpha: &[f32], width: usize, height: usize, margin: usize) -> Option<Crop> {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for (i, a) in alpha.iter().enumerate() {
        if *a > 0.02 {
            let (x, y) = (i % width, i / width);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    (x0 != usize::MAX).then(|| {
        let (x0, y0) = (x0.saturating_sub(margin), y0.saturating_sub(margin));
        let (x1, y1) = ((x1 + margin).min(width - 1), (y1 + margin).min(height - 1));
        Crop {
            x: x0,
            y: y0,
            width: x1 - x0 + 1,
            height: y1 - y0 + 1,
        }
    })
}
/// Square max (or min) filter of radius `r` over a `w` × `h` mask, separably.
fn morph(mask: &[bool], w: usize, h: usize, r: usize, grow: bool) -> Vec<bool> {
    let pass = |src: &[bool], horizontal: bool| -> Vec<bool> {
        let mut out = vec![!grow; w * h];
        for y in 0..h {
            for x in 0..w {
                let (pos, len) = if horizontal { (x, w) } else { (y, h) };
                let (lo, hi) = (pos.saturating_sub(r), (pos + r).min(len - 1));
                let hit = (lo..=hi).any(|p| {
                    let i = if horizontal { y * w + p } else { p * w + x };
                    src[i] == grow
                });
                out[y * w + x] = if hit { grow } else { !grow };
            }
        }
        out
    };
    pass(&pass(mask, true), false)
}
/// Trimap of a crop of `alpha`: 255 surely subject, 0 surely background, 128 in the band of
/// half-width `band` around the edge and wherever the tracked matte is already soft.
pub fn trimap(alpha: &[f32], width: usize, crop: Crop, band: usize) -> Vec<u8> {
    let (w, h) = (crop.width, crop.height);
    let at = |x: usize, y: usize| alpha[(crop.y + y) * width + crop.x + x];
    let inside: Vec<bool> = (0..w * h).map(|i| at(i % w, i / w) > 0.5).collect();
    let grown = morph(&inside, w, h, band, true);
    let shrunk = morph(&inside, w, h, band, false);
    (0..w * h)
        .map(|i| {
            let a = at(i % w, i / w);
            if shrunk[i] && a >= SURE {
                255
            } else if !grown[i] && a <= 1.0 - SURE {
                0
            } else {
                128
            }
        })
        .collect()
}
/// Model input for a crop: sRGB colour from `srgb` (`width` × frame) with the trimap as alpha,
/// scaled down to fit [`MAX_SIDE`]. Returns the bytes and their size.
pub fn input(srgb: &[[u8; 3]], width: usize, crop: Crop, trimap: &[u8]) -> (Vec<u8>, usize, usize) {
    let scale = (MAX_SIDE as f64 / crop.width.max(crop.height) as f64).min(1.0);
    let (w, h) = (
        ((crop.width as f64 * scale).round() as usize).max(1),
        ((crop.height as f64 * scale).round() as usize).max(1),
    );
    let mut bytes = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            // Nearest sample: the trimap's three levels must stay exact.
            let sx = ((x as f64 + 0.5) / scale) as usize;
            let sy = ((y as f64 + 0.5) / scale) as usize;
            let (sx, sy) = (sx.min(crop.width - 1), sy.min(crop.height - 1));
            let c = srgb[(crop.y + sy) * width + crop.x + sx];
            bytes.extend_from_slice(&[c[0], c[1], c[2], trimap[sy * crop.width + sx]]);
        }
    }
    (bytes, w, h)
}
/// Writes matted alpha (`mw` × `mh`, covering the crop) into `alpha`, but only inside the
/// trimap's unknown band; sure pixels keep their tracked values.
pub fn merge(
    alpha: &mut [f32],
    width: usize,
    crop: Crop,
    trimap: &[u8],
    matted: &[f32],
    mw: usize,
    mh: usize,
) -> Result<()> {
    if matted.len() != mw * mh || trimap.len() != crop.width * crop.height {
        return Err("matting result does not match its crop".into());
    }
    let (sx, sy) = (
        mw as f64 / crop.width as f64,
        mh as f64 / crop.height as f64,
    );
    for y in 0..crop.height {
        for x in 0..crop.width {
            if trimap[y * crop.width + x] != 128 {
                continue;
            }
            // Bilinear sample of the matted alpha at this pixel's centre.
            let (u, v) = (
                ((x as f64 + 0.5) * sx - 0.5).clamp(0.0, (mw - 1) as f64),
                ((y as f64 + 0.5) * sy - 0.5).clamp(0.0, (mh - 1) as f64),
            );
            let (u0, v0) = (u.floor() as usize, v.floor() as usize);
            let (u1, v1) = ((u0 + 1).min(mw - 1), (v0 + 1).min(mh - 1));
            let (fu, fv) = ((u - u0 as f64) as f32, (v - v0 as f64) as f32);
            let m = |x: usize, y: usize| matted[y * mw + x];
            let top = m(u0, v0) + (m(u1, v0) - m(u0, v0)) * fu;
            let bottom = m(u0, v1) + (m(u1, v1) - m(u0, v1)) * fu;
            alpha[(crop.y + y) * width + crop.x + x] = top + (bottom - top) * fv;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A 40 × 30 frame with a hard-edged square subject at 10..30 × 5..25.
    fn square() -> Vec<f32> {
        (0..40 * 30)
            .map(|i| {
                let (x, y) = (i % 40, i / 40);
                if (10..30).contains(&x) && (5..25).contains(&y) {
                    1.0
                } else {
                    0.0
                }
            })
            .collect()
    }
    #[test]
    fn trimap_marks_a_band_around_the_edge_and_sure_regions_elsewhere() {
        let alpha = square();
        let crop = crop(&alpha, 40, 30, 4).unwrap();
        assert_eq!(
            crop,
            Crop {
                x: 6,
                y: 1,
                width: 28,
                height: 28
            }
        );
        let t = trimap(&alpha, 40, crop, 2);
        let at = |x: usize, y: usize| t[(y - crop.y) * crop.width + (x - crop.x)];
        assert_eq!(at(20, 15), 255, "interior");
        assert_eq!(at(7, 15), 0, "outside the band");
        for x in 8..=11 {
            assert_eq!(at(x, 15), 128, "band at x = {x}");
        }
        assert_eq!(at(12, 15), 255);
        assert_eq!(band(&alpha, 40), Some(6), "clamped to the minimum");
    }
    #[test]
    fn merge_changes_only_the_band_and_scales_back_from_a_smaller_input() {
        let mut alpha = square();
        let crop = crop(&alpha.clone(), 40, 30, 4).unwrap();
        let t = trimap(&alpha, 40, crop, 2);
        let srgb = vec![[10u8, 20, 30]; 40 * 30];
        let (bytes, w, h) = input(&srgb, 40, crop, &t);
        assert_eq!((w, h), (28, 28));
        assert_eq!(bytes.len(), w * h * 4);
        assert_eq!(&bytes[..4], &[10, 20, 30, 0]);
        // A matting result of 0.5 everywhere, at half the crop's size.
        merge(&mut alpha, 40, crop, &t, &vec![0.5; 14 * 14], 14, 14).unwrap();
        assert_eq!(alpha[15 * 40 + 20], 1.0, "sure interior untouched");
        assert_eq!(alpha[15 * 40 + 7], 0.0, "sure background untouched");
        assert_eq!(alpha[15 * 40 + 10], 0.5, "band takes the matted value");
        assert!(merge(&mut alpha, 40, crop, &t, &[0.5; 3], 14, 14).is_err());
    }
}
