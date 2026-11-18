//! Numbers read back from a rendered frame, so a picture can be judged without looking at it: brightness percentiles as displayed, scene-linear averages and peaks, and where the non-transparent pixels lie.
use crate::Result;
use serde_json::{Value, json};

/// Alpha above this counts as covered.
const COVERED: f32 = 0.005;
fn srgb(linear: f32) -> f32 {
    let v = linear.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}
fn luma(pixel: &[f32; 4]) -> f32 {
    0.2126 * pixel[0] + 0.7152 * pixel[1] + 0.0722 * pixel[2]
}
/// Statistics of a `width` x `height` frame whose top-left pixel sits at (`left`, `top`)
/// of the composition. `linear` holds the scene-linear premultiplied pixels and
/// `shown` the same pixels after the display transform (linear light, before the
/// sRGB curve).
pub fn measure(
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    linear: &[[f32; 4]],
    shown: &[[f32; 4]],
) -> Result<Value> {
    let count = width as usize * height as usize;
    if count == 0 || linear.len() != count || shown.len() != count {
        return Err("measure needs matching, non-empty pixels".into());
    }
    let mut sum = [0.0_f64; 4];
    let (mut peak, mut peak_at) = (f32::NEG_INFINITY, 0);
    let (mut covered, mut over) = (0_usize, 0_usize);
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (i, pixel) in linear.iter().enumerate() {
        for (total, value) in sum.iter_mut().zip(pixel) {
            *total += f64::from(*value);
        }
        let y = luma(pixel);
        if y > peak {
            (peak, peak_at) = (y, i);
        }
        over += usize::from(pixel[..3].iter().any(|v| *v > 1.0));
        if pixel[3] > COVERED {
            covered += 1;
            let (x, y) = (i as u32 % width, i as u32 / width);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    let mut seen: Vec<f32> = shown.iter().map(|p| srgb(luma(p))).collect();
    seen.sort_unstable_by(f32::total_cmp);
    let at = |share: f64| seen[((count - 1) as f64 * share).round() as usize];
    let mean = sum.map(|v| (v / count as f64) as f32);
    let bounds = (covered > 0)
        .then(|| json!({"x":left + x0,"y":top + y0,"width":x1 - x0 + 1,"height":y1 - y0 + 1}));
    Ok(json!({
        "region":{"x":left,"y":top,"width":width,"height":height},
        "displayed_luma":{"p2":at(0.02),"p10":at(0.10),"p25":at(0.25),"p50":at(0.50),"p75":at(0.75),"p90":at(0.90),"p98":at(0.98),"min":seen[0],"max":seen[count - 1]},
        "linear":{"mean":mean,"peak_luma":peak,"peak_at":{"x":left + peak_at as u32 % width,"y":top + peak_at as u32 / width},"above_one":over as f64 / count as f64},
        "coverage":covered as f64 / count as f64,
        "bounds":bounds,
        "notes":"displayed_luma is 0..1 after the display transform and the sRGB curve; linear values are scene-linear premultiplied; bounds frames every pixel with alpha above 0.005, in composition pixels"
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_bright_square_is_found_and_weighed() {
        let (w, h) = (8_u32, 4_u32);
        let mut pixels = vec![[0.0_f32; 4]; 32];
        for y in 1..3 {
            for x in 2..6 {
                pixels[(y * w + x) as usize] = [2.0, 2.0, 2.0, 1.0];
            }
        }
        pixels[(2 * w + 5) as usize] = [4.0, 4.0, 4.0, 1.0];
        let m = measure(100, 50, w, h, &pixels, &pixels).unwrap();
        assert_eq!(m["bounds"], json!({"x":102,"y":51,"width":4,"height":2}));
        assert_eq!(m["coverage"], json!(0.25));
        assert_eq!(m["linear"]["above_one"], json!(0.25));
        assert_eq!(m["linear"]["peak_at"], json!({"x":105,"y":52}));
        assert_eq!(m["displayed_luma"]["p50"], json!(0.0));
        assert!(m["displayed_luma"]["max"].as_f64().unwrap() > 0.9999);
        assert!(measure(0, 0, w, h, &pixels[1..], &pixels).is_err());
        let clear = vec![[0.0_f32; 4]; 32];
        assert!(measure(0, 0, w, h, &clear, &clear).unwrap()["bounds"].is_null());
    }
}
