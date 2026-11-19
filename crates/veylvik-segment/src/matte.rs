//! Conversions at the model boundary: scene-linear frames become area-averaged SIDE² sRGB model
//! input, and mask logits are resampled (never a binary mask) to the frame before the sigmoid.
use crate::{Result, SIDE};
use std::path::Path;

/// Source pixels and coverage weights of each of `to` output pixels covering the source span
/// `start..start + span` (in source pixels, within `0..limit`).
fn footprints(start: f64, span: f64, limit: usize, to: usize) -> Vec<Vec<(usize, f32)>> {
    let scale = span / to as f64;
    (0..to)
        .map(|o| {
            let (a, b) = (start + o as f64 * scale, start + (o + 1) as f64 * scale);
            if scale <= 1.0 {
                return vec![((((a + b) * 0.5).max(0.0) as usize).min(limit - 1), 1.0)];
            }
            let mut taps = Vec::new();
            let mut i = a.floor().max(0.0) as usize;
            while (i as f64) < b && i < limit {
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
/// Straight sRGB bytes of scene-linear premultiplied pixels, for models.
pub fn srgb_pixels(pixels: &[[f32; 4]]) -> Vec<[u8; 3]> {
    pixels
        .iter()
        .map(|p| {
            let a = p[3].clamp(0.0, 1.0);
            let s = |c: f32| srgb(if a > 1e-6 { c / a } else { 0.0 });
            [s(p[0]), s(p[1]), s(p[2])]
        })
        .collect()
}
/// Area-averages scene-linear premultiplied `pixels` (`width` × `height`, as seen over black) to
/// SIDE² in linear light, then encodes sRGB bytes: models are trained on display-referred images.
pub fn model_frame(pixels: &[[f32; 4]], width: usize, height: usize) -> Result<Vec<u8>> {
    model_crop(
        pixels,
        width,
        height,
        [0.0, 0.0, width as f64, height as f64],
    )
}
/// As [`model_frame`] for the window `rect` (x0, y0, x1, y1 in frame pixels) of the frame.
pub fn model_crop(
    pixels: &[[f32; 4]],
    width: usize,
    height: usize,
    rect: [f64; 4],
) -> Result<Vec<u8>> {
    if width == 0 || height == 0 || pixels.len() != width * height {
        return Err("frame size does not match its pixels".into());
    }
    if !(rect[0] >= 0.0 && rect[1] >= 0.0 && rect[2] > rect[0] && rect[3] > rect[1])
        || rect[2] > width as f64 + 1e-6
        || rect[3] > height as f64 + 1e-6
    {
        return Err("model window lies outside the frame".into());
    }
    let columns = footprints(rect[0], rect[2] - rect[0], width, SIDE);
    let rows = footprints(rect[1], rect[3] - rect[1], height, SIDE);
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
/// Interior logit an opaque subject should reach: sigmoid(8) ≈ 0.9997.
const CONFIDENT: f32 = 8.0;
/// Scales an under-confident mask's logits so its interior (the median positive logit) reaches
/// [`CONFIDENT`], up to eight times. Prompted frames, where the model has no memory yet, often
/// come out this weak and would otherwise be see-through. Confident masks are left unchanged,
/// and the outline (logit zero) never moves.
pub fn normalise(logits: &mut [f32]) {
    let mut positive: Vec<f32> = logits.iter().copied().filter(|v| *v > 0.0).collect();
    if positive.len() < 16 {
        return;
    }
    let middle = positive.len() / 2;
    let (_, median, _) = positive.select_nth_unstable_by(middle, f32::total_cmp);
    let gain = (CONFIDENT / *median).clamp(1.0, 8.0);
    if gain > 1.0 {
        logits.iter_mut().for_each(|v| *v *= gain);
    }
}
/// Bilinearly resamples SIDE² logits to `width` × `height` (pixel centres aligned), then applies
/// the sigmoid: the edge stays as smooth as the model's own confidence.
pub fn alpha(logits: &[f32], width: usize, height: usize) -> Result<Vec<f32>> {
    alpha_crop(
        logits,
        width,
        height,
        [0.0, 0.0, width as f64, height as f64],
    )
}
/// As [`alpha`] for logits of the window `rect` of the frame; pixels outside it are clear.
pub fn alpha_crop(logits: &[f32], width: usize, height: usize, rect: [f64; 4]) -> Result<Vec<f32>> {
    if logits.len() != SIDE * SIDE {
        return Err("mask logits must be SIDE²".into());
    }
    // Model-space coordinate of each frame pixel centre, or None outside the window.
    let axis = |n: usize, lo: f64, hi: f64| -> Vec<Option<(usize, usize, f32)>> {
        (0..n)
            .map(|o| {
                let c = o as f64 + 0.5;
                if c < lo || c > hi {
                    return None;
                }
                let p = ((c - lo) / (hi - lo) * SIDE as f64 - 0.5).clamp(0.0, (SIDE - 1) as f64);
                let i = (p.floor() as usize).min(SIDE - 2);
                Some((i, i + 1, (p - i as f64) as f32))
            })
            .collect()
    };
    let (xs, ys) = (
        axis(width, rect[0], rect[2]),
        axis(height, rect[1], rect[3]),
    );
    let mut out = Vec::with_capacity(width * height);
    for y in &ys {
        for x in &xs {
            let (Some((y0, y1, fy)), Some((x0, x1, fx))) = (y, x) else {
                out.push(0.0);
                continue;
            };
            let at = |x: usize, y: usize| logits[y * SIDE + x];
            let top = at(*x0, *y0) + (at(*x1, *y0) - at(*x0, *y0)) * fx;
            let bottom = at(*x0, *y1) + (at(*x1, *y1) - at(*x0, *y1)) * fx;
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
    let partial = path.with_extension("png.partial");
    // Fast compression: a matte is mostly flat, so it packs well either way, and one is
    // written for every frame of the shot.
    // The encoder takes 16-bit samples in this machine's byte order and writes them as
    // the file needs.
    let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_ne_bytes()).collect();
    let file = std::io::BufWriter::new(std::fs::File::create(&partial)?);
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new_with_quality(
            file,
            image::codecs::png::CompressionType::Fast,
            image::codecs::png::FilterType::Sub,
        ),
        &bytes,
        width,
        height,
        image::ExtendedColorType::La16,
    )
    .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    std::fs::rename(partial, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_written_matte_reads_back_as_the_same_values() {
        let dir = std::env::temp_dir().join(format!("veylvik-matte-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("matte.png");
        // Values whose two bytes differ, so a byte-order mistake cannot pass.
        let alpha: Vec<f32> = (0..64).map(|i| i as f32 / 63.0 * 0.999).collect();
        write_png(&path, 8, 8, &alpha).unwrap();
        let read = image::open(&path).unwrap().into_luma_alpha16();
        for (pixel, a) in read.pixels().zip(&alpha) {
            assert_eq!(pixel.0[0], u16::MAX);
            assert_eq!(pixel.0[1], (a * 65535.0 + 0.5) as u16);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
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
    #[test]
    fn weak_masks_are_made_opaque_inside_without_moving_the_outline() {
        // Interior at +2 (alpha 0.88), outline crossing zero, outside −2.
        let mut logits: Vec<f32> = (0..SIDE * SIDE)
            .map(|i| if i % SIDE < SIDE / 2 { 2.0 } else { -2.0 })
            .collect();
        normalise(&mut logits);
        assert_eq!(logits[0], 8.0);
        assert_eq!(logits[SIDE - 1], -8.0);
        // Confident masks are untouched.
        let mut strong = vec![20.0; SIDE * SIDE];
        normalise(&mut strong);
        assert_eq!(strong[0], 20.0);
        // Very weak masks gain at most eight times.
        let mut faint = vec![0.1; SIDE * SIDE];
        normalise(&mut faint);
        assert!((faint[0] - 0.8).abs() < 1e-6);
    }
    #[test]
    fn crops_sample_only_their_window_and_map_back_to_the_same_place() {
        // A frame that is black except a white square at 100..200 × 300..400.
        let (w, h) = (400, 800);
        let pixels: Vec<[f32; 4]> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                if (100..200).contains(&x) && (300..400).contains(&y) {
                    [1.0; 4]
                } else {
                    [0.0, 0.0, 0.0, 1.0]
                }
            })
            .collect();
        // The window is exactly the square: the model sees white everywhere.
        let crop = model_crop(&pixels, w, h, [100.0, 300.0, 200.0, 400.0]).unwrap();
        assert!(crop.iter().all(|v| *v == 255));
        assert!(model_crop(&pixels, w, h, [-1.0, 0.0, 10.0, 10.0]).is_err());
        // Logits positive everywhere in the window: alpha 1 inside it, 0 outside.
        let alpha =
            alpha_crop(&vec![10.0; SIDE * SIDE], w, h, [100.0, 300.0, 200.0, 400.0]).unwrap();
        assert!(
            alpha[350 * w + 150] > 0.99
                && alpha[350 * w + 50] == 0.0
                && alpha[100 * w + 150] == 0.0
        );
    }
}
