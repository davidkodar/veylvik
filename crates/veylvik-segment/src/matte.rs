//! Conversions at the model boundary: scene-linear frames become area-averaged SIDE² sRGB model
//! input, and mask logits are resampled (never a binary mask) to the frame before the sigmoid.
use crate::{Result, SIDE};
use std::path::Path;

/// Source pixels and coverage weights of each output pixel when resampling `from` to `to`.
fn footprints(from: usize, to: usize) -> Vec<Vec<(usize, f32)>> {
    let scale = from as f64 / to as f64;
    (0..to)
        .map(|o| {
            let (a, b) = (o as f64 * scale, (o + 1) as f64 * scale);
            if scale <= 1.0 {
                return vec![((((a + b) * 0.5) as usize).min(from - 1), 1.0)];
            }
            let mut taps = Vec::new();
            let mut i = a.floor() as usize;
            while (i as f64) < b && i < from {
                let cover = (b.min(i as f64 + 1.0) - a.max(i as f64)) / scale;
                if cover > 0.0 {
                    taps.push((i, cover as f32));
                }
                i += 1;
            }
            taps
        })
        .collect()
}
fn srgb(linear: f32) -> u8 {
    let v = linear.clamp(0.0, 1.0);
    let v = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (v * 255.0 + 0.5) as u8
}
/// Area-averages scene-linear premultiplied `pixels` (`width` × `height`, as seen over black) to
/// SIDE² in linear light, then encodes sRGB bytes: models are trained on display-referred images.
pub fn model_frame(pixels: &[[f32; 4]], width: usize, height: usize) -> Result<Vec<u8>> {
    if width == 0 || height == 0 || pixels.len() != width * height {
        return Err("frame size does not match its pixels".into());
    }
    let (columns, rows) = (footprints(width, SIDE), footprints(height, SIDE));
    // Horizontal pass into SIDE × height, then vertical.
    let mut wide = vec![[0f32; 3]; SIDE * height];
    for y in 0..height {
        let line = &pixels[y * width..(y + 1) * width];
        for (x, taps) in columns.iter().enumerate() {
            let mut sum = [0f32; 3];
            for &(i, w) in taps {
                for c in 0..3 {
                    sum[c] += line[i][c] * w;
                }
            }
            wide[y * SIDE + x] = sum;
        }
    }
    let mut out = vec![0u8; SIDE * SIDE * 3];
    for (y, taps) in rows.iter().enumerate() {
        for x in 0..SIDE {
            let mut sum = [0f32; 3];
            for &(i, w) in taps {
                for c in 0..3 {
                    sum[c] += wide[i * SIDE + x][c] * w;
                }
            }
            for c in 0..3 {
                out[(y * SIDE + x) * 3 + c] = srgb(sum[c]);
            }
        }
    }
    Ok(out)
}
/// Bilinearly resamples SIDE² logits to `width` × `height` (pixel centres aligned), then applies
/// the sigmoid: the edge stays as smooth as the model's own confidence.
pub fn alpha(logits: &[f32], width: usize, height: usize) -> Result<Vec<f32>> {
    if logits.len() != SIDE * SIDE {
        return Err("mask logits must be SIDE²".into());
    }
    let axis = |n: usize| -> Vec<(usize, usize, f32)> {
        (0..n)
            .map(|o| {
                let p =
                    ((o as f32 + 0.5) * SIDE as f32 / n as f32 - 0.5).clamp(0.0, (SIDE - 1) as f32);
                let i = (p.floor() as usize).min(SIDE - 2);
                (i, i + 1, p - i as f32)
            })
            .collect()
    };
    let (xs, ys) = (axis(width), axis(height));
    let mut out = Vec::with_capacity(width * height);
    for &(y0, y1, fy) in &ys {
        for &(x0, x1, fx) in &xs {
            let at = |x: usize, y: usize| logits[y * SIDE + x];
            let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * fx;
            let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * fx;
            let logit = top + (bottom - top) * fy;
            out.push(1.0 / (1.0 + (-logit).exp()));
        }
    }
    Ok(out)
}
/// Writes a matte as a 16-bit grey-and-alpha PNG: white with the matte as alpha, so it reads both
/// as a premultiplied white matte and as an alpha channel.
pub fn write_png(path: &Path, width: u32, height: u32, alpha: &[f32]) -> Result<()> {
    if alpha.len() != width as usize * height as usize {
        return Err("matte size does not match its pixels".into());
    }
    let data: Vec<u16> = alpha
        .iter()
        .flat_map(|a| [u16::MAX, (a.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16])
        .collect();
    let image = image::ImageBuffer::<image::LumaA<u16>, _>::from_raw(width, height, data)
        .ok_or("matte buffer has the wrong size")?;
    let partial = path.with_extension("png.partial");
    image
        .save_with_format(&partial, image::ImageFormat::Png)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    std::fs::rename(partial, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_frame_area_averages_in_linear_light_and_encodes_srgb() {
        // 2048 wide halves of 0 and 1 average to linear 0.5 only where they meet.
        let (w, h) = (2048, 4);
        let pixels: Vec<[f32; 4]> = (0..w * h)
            .map(|i| if i % w < w / 2 { [0.0; 4] } else { [1.0; 4] })
            .collect();
        let frame = model_frame(&pixels, w, h).unwrap();
        assert_eq!(frame.len(), SIDE * SIDE * 3);
        assert_eq!(frame[0], 0);
        assert_eq!(frame[(SIDE - 1) * 3], 255);
        // HDR and negative values clamp rather than wrap.
        let frame = model_frame(&[[4.0, -1.0, 0.5, 1.0]], 1, 1).unwrap();
        assert_eq!(&frame[..3], &[255, 0, srgb(0.5)]);
        assert_eq!(srgb(0.5), 188);
        assert!(model_frame(&[[0.0; 4]], 2, 1).is_err());
    }
    #[test]
    fn alpha_resamples_logits_before_the_sigmoid() {
        // A vertical edge: left negative, right positive.
        let logits: Vec<f32> = (0..SIDE * SIDE)
            .map(|i| if i % SIDE < SIDE / 2 { -6.0 } else { 6.0 })
            .collect();
        let a = alpha(&logits, 4096, 2).unwrap();
        assert!(a[0] < 0.01 && a[4095] > 0.99);
        // Upsampling 4× gives intermediate values across the edge instead of a hard step.
        let row = &a[..4096];
        let soft = row.iter().filter(|v| **v > 0.05 && **v < 0.95).count();
        assert!((1..=8).contains(&soft), "{soft}");
        assert!(row.windows(2).all(|p| p[0] <= p[1]));
    }
}
