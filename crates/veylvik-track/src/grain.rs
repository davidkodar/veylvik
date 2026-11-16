//! Measuring a plate's grain for Match grain: its strength by brightness, colour and size, from the flattest parts of a frame, calibrated against Veylvik's own grain generator so the numbers reproduce the plate.
use veylvik_solve::linalg::Rng;

/// Match grain's settings: strength (standard deviation, linear light) at luminance
/// 0.02, 0.18 and 0.9; red and blue against green; how much the channels share;
/// and the blur radius in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grain {
    pub shadows: f32,
    pub midtones: f32,
    pub highlights: f32,
    pub red: f32,
    pub blue: f32,
    pub colour: f32,
    pub size: u32,
}
const BLOCK: usize = 16;
/// The blur's weights, exactly as the GPU uses them (`blur_weight`, gaussian).
fn weights(radius: u32) -> Vec<f32> {
    let r = radius as i32;
    let sigma = (radius as f32 / 3.0).max(0.5);
    (-r..=r)
        .map(|o| (-((o * o) as f32) / (2.0 * sigma * sigma)).exp())
        .collect()
}
/// Blur one channel plane separably (edges clamped).
fn blur(plane: &[f32], w: usize, h: usize, k: &[f32]) -> Vec<f32> {
    let r = (k.len() / 2) as isize;
    let sum: f32 = k.iter().sum();
    let pass = |src: &[f32], horizontal: bool| -> Vec<f32> {
        let mut out = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut t = 0.0;
                for (i, wgt) in k.iter().enumerate() {
                    let o = i as isize - r;
                    let (sx, sy) = if horizontal {
                        ((x as isize + o).clamp(0, w as isize - 1) as usize, y)
                    } else {
                        (x, (y as isize + o).clamp(0, h as isize - 1) as usize)
                    };
                    t += src[sy * w + sx] * wgt;
                }
                out[y * w + x] = t / sum;
            }
        }
        out
    };
    pass(&pass(plane, true), false)
}
/// What the estimator sees in a frame, per brightness band (shadows, mids, highlights).
struct Stats {
    /// Residual standard deviation per band and channel; None for a band with too few flat blocks.
    bands: [Option<[f32; 3]>; 3],
    /// Mean correlation between channel residuals.
    shared: f32,
    /// Neighbour correlation of the green residual.
    neighbour: f32,
}
/// The residual (image minus a smooth version) statistics of the flattest blocks.
fn stats(pixels: &[[f32; 4]], w: usize, h: usize) -> Option<Stats> {
    if w < BLOCK * 4 || h < BLOCK * 4 {
        return None;
    }
    let plane = |c: usize| -> Vec<f32> {
        pixels
            .iter()
            .map(|p| if p[3] > 1e-6 { p[c] / p[3] } else { 0.0 })
            .collect()
    };
    let smooth_k = weights(6);
    let channels: Vec<Vec<f32>> = (0..3).map(plane).collect();
    let smooth: Vec<Vec<f32>> = channels.iter().map(|c| blur(c, w, h, &smooth_k)).collect();
    let residual: Vec<Vec<f32>> = (0..3)
        .map(|c| {
            channels[c]
                .iter()
                .zip(&smooth[c])
                .map(|(a, b)| a - b)
                .collect()
        })
        .collect();
    // Blocks: brightness band and how much image structure they hold.
    let mut blocks = Vec::new();
    for by in 1..h / BLOCK - 1 {
        for bx in 1..w / BLOCK - 1 {
            let (mut lum, mut structure, mut n) = (0.0f32, 0.0f32, 0.0f32);
            for y in by * BLOCK..(by + 1) * BLOCK {
                for x in bx * BLOCK..(bx + 1) * BLOCK {
                    let i = y * w + x;
                    let l = |c: &Vec<Vec<f32>>, j: usize| {
                        0.2126 * c[0][j] + 0.7152 * c[1][j] + 0.0722 * c[2][j]
                    };
                    lum += l(&smooth, i);
                    structure += (l(&smooth, i + 1) - l(&smooth, i)).abs()
                        + (l(&smooth, i + w) - l(&smooth, i)).abs();
                    n += 1.0;
                }
            }
            let lum = lum / n;
            let band = if lum < 0.06 {
                0
            } else if lum < 0.45 {
                1
            } else {
                2
            };
            blocks.push((band, structure / n, bx, by));
        }
    }
    let mut bands = [None; 3];
    let (mut shared, mut neighbour, mut weight) = (0.0f32, 0.0f32, 0.0f32);
    for (band, slot) in bands.iter_mut().enumerate() {
        let mut chosen: Vec<_> = blocks.iter().filter(|b| b.0 == band).collect();
        if chosen.len() < 6 {
            continue;
        }
        chosen.sort_by(|a, b| a.1.total_cmp(&b.1));
        chosen.truncate((chosen.len() * 3 / 10).max(6));
        let (mut sum, mut sq, mut cross, mut lag, mut n) =
            ([0.0f64; 3], [0.0f64; 3], [0.0f64; 3], 0.0f64, 0.0f64);
        for &&(_, _, bx, by) in &chosen {
            for y in by * BLOCK..(by + 1) * BLOCK {
                for x in bx * BLOCK..(bx + 1) * BLOCK {
                    let i = y * w + x;
                    let r = [0, 1, 2].map(|c| f64::from(residual[c][i]));
                    for c in 0..3 {
                        sum[c] += r[c];
                        sq[c] += r[c] * r[c];
                    }
                    cross[0] += r[0] * r[1];
                    cross[1] += r[1] * r[2];
                    cross[2] += r[0] * r[2];
                    lag += r[1] * f64::from(residual[1][i + 1]);
                    n += 1.0;
                }
            }
        }
        let var = [0, 1, 2].map(|c| (sq[c] / n - (sum[c] / n).powi(2)).max(0.0));
        let std = var.map(|v| v.sqrt() as f32);
        let corr =
            |k: usize, a: usize, b: usize| cross[k] / n / (var[a] * var[b]).sqrt().max(1e-30);
        let count = n as f32;
        shared += count * ((corr(0, 0, 1) + corr(1, 1, 2) + corr(2, 0, 2)) / 3.0) as f32;
        neighbour += count * (lag / n / var[1].max(1e-30)) as f32;
        weight += count;
        *slot = Some(std);
    }
    (weight > 0.0).then(|| Stats {
        bands,
        shared: shared / weight,
        neighbour: neighbour / weight,
    })
}
/// Veylvik's grain at unit strength on flat mid grey: what the GPU generator makes
/// for `size` and `colour`, for calibration.
fn synthetic(size: u32, colour: f32, w: usize, h: usize) -> Vec<[f32; 4]> {
    let mut rng = Rng::new(0x6a1);
    let mut gauss = || {
        let (a, b) = (rng.unit().max(1e-12), rng.unit());
        ((-2.0 * a.ln()).sqrt() * (std::f64::consts::TAU * b).cos()) as f32
    };
    let noise: Vec<Vec<f32>> = (0..4)
        .map(|_| (0..w * h).map(|_| gauss()).collect())
        .collect();
    let k = weights(size);
    let total: f32 = k.iter().sum();
    let spread: f32 = k.iter().map(|v| (v / total).powi(2)).sum();
    let blurred: Vec<Vec<f32>> = noise
        .iter()
        .map(|n| blur(n, w, h, &k).iter().map(|v| v / spread).collect())
        .collect();
    (0..w * h)
        .map(|i| {
            let g = |c: usize| {
                0.18 + colour.sqrt() * blurred[3][i] + (1.0 - colour).sqrt() * blurred[c][i]
            };
            [g(0), g(1), g(2), 1.0]
        })
        .collect()
}
/// Measure the grain of a plate frame (premultiplied linear pixels). None when it is
/// too small or has no flat areas to measure in.
pub fn measure(pixels: &[[f32; 4]], width: u32, height: u32) -> Option<Grain> {
    let (w, h) = (width as usize, height as usize);
    if pixels.len() != w * h {
        return None;
    }
    let plate = stats(pixels, w, h)?;
    let colour = plate.shared.clamp(0.0, 1.0);
    // Size: the generator radius whose neighbour correlation, seen the same way, is closest.
    let side = 192;
    let calibration: Vec<(u32, f32, f32)> = (0..=6)
        .filter_map(|size| {
            let s = stats(&synthetic(size, colour, side, side), side, side)?;
            Some((size, s.neighbour, s.bands[1]?[1]))
        })
        .collect();
    let &(size, _, unit) = calibration.iter().min_by(|a, b| {
        (a.1 - plate.neighbour)
            .abs()
            .total_cmp(&(b.1 - plate.neighbour).abs())
    })?;
    let green = |band: usize| plate.bands[band].map(|s| s[1] / unit.max(1e-9));
    let mid = green(1).or(green(0)).or(green(2))?;
    let (all, n) = plate
        .bands
        .iter()
        .flatten()
        .fold(([0.0f32; 3], 0.0f32), |(t, n), s| {
            ([t[0] + s[0], t[1] + s[1], t[2] + s[2]], n + 1.0)
        });
    let mean = all.map(|v| v / n);
    Some(Grain {
        shadows: green(0).unwrap_or(mid * 1.3),
        midtones: mid,
        highlights: green(2).unwrap_or(mid * 0.8),
        red: (mean[0] / mean[1].max(1e-9)).clamp(0.0, 4.0),
        blue: (mean[2] / mean[1].max(1e-9)).clamp(0.0, 4.0),
        colour,
        size,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measures_back_the_grain_it_would_make() {
        // A plate: three flat bands of grey with grain of known strength, colour and size.
        let (w, h) = (288usize, 288usize);
        for (size, colour) in [(0u32, 0.2f32), (3, 0.6)] {
            let unit = synthetic(size, colour, w, h);
            let plate: Vec<[f32; 4]> = unit
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let (base, strength) = match (i / w) * 3 / h {
                        0 => (0.01, 0.02),
                        1 => (0.18, 0.012),
                        _ => (0.8, 0.008),
                    };
                    let g = |c: usize, k: f32| base + (p[c] - 0.18) * strength * k;
                    [g(0, 0.9), g(1, 1.0), g(2, 1.4), 1.0]
                })
                .collect();
            let m = measure(&plate, w as u32, h as u32).unwrap();
            assert_eq!(m.size, size, "{m:?}");
            assert!((m.midtones - 0.012).abs() < 0.0015, "{m:?}");
            assert!(
                (m.shadows - 0.02).abs() < 0.003 && (m.highlights - 0.008).abs() < 0.0015,
                "{m:?}"
            );
            assert!(
                (m.blue - 1.4).abs() < 0.15 && (m.red - 0.9).abs() < 0.1,
                "{m:?}"
            );
            assert!((m.colour - colour).abs() < 0.1, "{m:?}");
        }
    }
}
