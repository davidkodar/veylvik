//! A picture's look in numbers, region by region (its grains, fine contrast, brightness, colour and shimmer between two frames), measured the same way for a reference and for a render so that the two can be compared and the differences named.
#![forbid(unsafe_code)]
mod advice;
pub use advice::{advise, advise_shimmer};
use serde::Serialize;

/// Sizes and densities are given as in a frame this wide, whatever the size measured.
pub const NOMINAL_WIDTH: f32 = 1920.0;
/// A picture as it is seen: display-referred linear light (after any display transform,
/// before the screen's curve), red, green, blue and alpha, rows from the top.
#[derive(Clone, Copy)]
pub struct Picture<'a> {
    pub width: u32,
    pub height: u32,
    pub pixels: &'a [[f32; 4]],
}
/// A picture of one's own, as `resampled` makes.
pub struct Owned {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[f32; 4]>,
}
impl Owned {
    pub fn picture(&self) -> Picture<'_> {
        Picture {
            width: self.width,
            height: self.height,
            pixels: &self.pixels,
        }
    }
}
/// A part of a picture, as shares of its width and height from the top left.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, serde::Deserialize)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl Area {
    pub const WHOLE: Area = Area {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };
    /// The cells of an `n` x `n` grid, row by row.
    pub fn grid(n: u32) -> Vec<Area> {
        let s = 1.0 / n.max(1) as f32;
        (0..n * n)
            .map(|k| Area {
                x: (k % n) as f32 * s,
                y: (k / n) as f32 * s,
                width: s,
                height: s,
            })
            .collect()
    }
    pub fn check(&self) -> Result<(), String> {
        let ok = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
        if !(ok(self.x) && ok(self.y) && ok(self.width) && ok(self.height))
            || self.width <= 0.0
            || self.height <= 0.0
            || self.x + self.width > 1.0 + 1e-4
            || self.y + self.height > 1.0 + 1e-4
        {
            return Err(
                "an area is x, y, width and height as shares of the picture (0 to 1), inside it"
                    .into(),
            );
        }
        Ok(())
    }
    /// Its pixels in a picture: left, top, width, height (at least 16 x 16 where it fits).
    fn pixels(&self, width: u32, height: u32) -> (usize, usize, usize, usize) {
        let x0 = ((self.x * width as f32).floor() as usize).min(width as usize - 1);
        let y0 = ((self.y * height as f32).floor() as usize).min(height as usize - 1);
        let x1 =
            (((self.x + self.width) * width as f32).ceil() as usize).clamp(x0 + 1, width as usize);
        let y1 = (((self.y + self.height) * height as f32).ceil() as usize)
            .clamp(y0 + 1, height as usize);
        (x0, y0, x1 - x0, y1 - y0)
    }
}
/// The picture made `width` wide (and as tall as keeps its shape), each new pixel the
/// average of the light it covers, so two pictures of different sizes are measured
/// alike. A picture no wider is returned as it is.
pub fn resampled(picture: Picture<'_>, width: u32) -> Owned {
    let (w, h) = (picture.width, picture.height);
    if width >= w || w == 0 || h == 0 {
        return Owned {
            width: w,
            height: h,
            pixels: picture.pixels.to_vec(),
        };
    }
    let height = ((h as f64 * width as f64 / w as f64).round() as u32).max(1);
    // Along one axis: for each new pixel, the old pixels it covers and how much of each.
    let weights = |from: u32, to: u32| -> Vec<Vec<(usize, f32)>> {
        let step = from as f64 / to as f64;
        (0..to)
            .map(|i| {
                let (a, b) = (i as f64 * step, (i + 1) as f64 * step);
                let mut list = Vec::new();
                let mut k = a.floor() as usize;
                while (k as f64) < b && k < from as usize {
                    let cover = (b.min(k as f64 + 1.0) - a.max(k as f64)) / step;
                    if cover > 0.0 {
                        list.push((k, cover as f32));
                    }
                    k += 1;
                }
                list
            })
            .collect()
    };
    let (across, down) = (weights(w, width), weights(h, height));
    // Across first, then down.
    let mut rows = vec![[0.0f32; 4]; width as usize * h as usize];
    for y in 0..h as usize {
        for (x, list) in across.iter().enumerate() {
            let mut sum = [0.0f32; 4];
            for &(k, c) in list {
                let p = picture.pixels[y * w as usize + k];
                for i in 0..4 {
                    sum[i] += p[i] * c;
                }
            }
            rows[y * width as usize + x] = sum;
        }
    }
    let mut pixels = vec![[0.0f32; 4]; width as usize * height as usize];
    for (y, list) in down.iter().enumerate() {
        for x in 0..width as usize {
            let mut sum = [0.0f32; 4];
            for &(k, c) in list {
                let p = rows[k * width as usize + x];
                for i in 0..4 {
                    sum[i] += p[i] * c;
                }
            }
            pixels[y * width as usize + x] = sum;
        }
    }
    Owned {
        width,
        height,
        pixels,
    }
}

fn luma(p: &[f32; 4]) -> f32 {
    (0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]).max(0.0)
}
fn encoded(linear: f32) -> f32 {
    let v = linear.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}
/// Pixels darker than this (on the screen's 0 to 1 scale) count as the background.
const DARK: f32 = 0.04;
/// Colours less saturated than this count as grey, in no hue.
const GREY: f32 = 0.15;
/// The hue bands colour is sorted into: a name and where it starts, in degrees round the
/// colour wheel (red at 0); each runs to where the next starts.
pub const BANDS: [(&str, f32); 8] = [
    ("red", 345.0),
    ("orange", 15.0),
    ("yellow", 40.0),
    ("green", 70.0),
    ("teal", 150.0),
    ("blue", 200.0),
    ("violet", 255.0),
    ("magenta", 290.0),
];
fn band(hue: f32) -> usize {
    let mut best = 0;
    for (k, (_, start)) in BANDS.iter().enumerate().skip(1) {
        if hue >= *start {
            best = k;
        }
    }
    if hue >= BANDS[0].1 || hue < BANDS[1].1 {
        0
    } else {
        best
    }
}
/// How much of a region's lit pixels fall in one hue band, and how saturated they are there.
#[derive(Clone, Debug, Serialize)]
pub struct Hue {
    pub band: &'static str,
    /// Share of the lit pixels (0 to 1).
    pub share: f32,
    /// Their median saturation on the screen (0 to 1).
    pub saturation: f32,
}
/// A region's look.
#[derive(Clone, Debug, Serialize)]
pub struct Look {
    /// Distinct grains (points of light brighter than their surroundings) in each 100 x 100
    /// pixels, as in a frame 1920 wide.
    pub grains: f32,
    /// How many times brighter than its surroundings a grain is (median).
    pub stand_out: f32,
    /// A grain's size across, in pixels as in a frame 1920 wide (median): where its light
    /// is at least half its peak.
    pub grain_size: f32,
    /// Fine detail: how much the light varies from its surroundings' average (standard
    /// deviation, linear light). High in crisp glitter, low in a soft haze.
    pub fine_contrast: f32,
    /// Brightness on the screen (0 to 1) at the 10th, 50th, 90th and 99.5th percentiles.
    pub brightness: [f32; 4],
    /// Share of the region that is lit, brighter than a dark background.
    pub coverage: f32,
    /// The background's brightness on the screen (mean of the dark pixels).
    pub background: f32,
    /// Lit pixels by hue band, the bands with any share.
    pub hues: Vec<Hue>,
    /// Share of the lit pixels that are grey (in no hue).
    pub grey: f32,
}
fn median(values: &mut [f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_unstable_by(f32::total_cmp);
    values[values.len() / 2]
}
/// A region's luminance (linear light) as its own grid.
fn region_luma(picture: Picture<'_>, area: &Area) -> (Vec<f32>, usize, usize) {
    let (x0, y0, w, h) = area.pixels(picture.width, picture.height);
    let mut out = Vec::with_capacity(w * h);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            out.push(luma(&picture.pixels[y * picture.width as usize + x]));
        }
    }
    (out, w, h)
}
/// The mean of each pixel's (2r+1)^2 neighbourhood, clamped at the edges.
fn box_mean(values: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    // A summed-area table.
    let mut sat = vec![0.0f64; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0.0f64;
        for x in 0..w {
            row += f64::from(values[y * w + x]);
            sat[(y + 1) * (w + 1) + x + 1] = sat[y * (w + 1) + x + 1] + row;
        }
    }
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let (x0, y0) = (x.saturating_sub(r), y.saturating_sub(r));
            let (x1, y1) = ((x + r + 1).min(w), (y + r + 1).min(h));
            let sum = sat[y1 * (w + 1) + x1] - sat[y0 * (w + 1) + x1] - sat[y1 * (w + 1) + x0]
                + sat[y0 * (w + 1) + x0];
            out[y * w + x] = (sum / ((x1 - x0) * (y1 - y0)) as f64) as f32;
        }
    }
    out
}
/// The look of `area` of a picture.
pub fn look(picture: Picture<'_>, area: &Area) -> Look {
    let nominal = NOMINAL_WIDTH / picture.width as f32;
    let (lum, w, h) = region_luma(picture, area);
    let around = box_mean(&lum, w, h, 7);
    // Grains: brightest in their 3 x 3, clearly above their surroundings.
    let (mut ratios, mut sizes) = (Vec::new(), Vec::new());
    let mut count = 0usize;
    for y in 0..h {
        for x in 0..w {
            let v = lum[y * w + x];
            if v <= 0.004 || v <= 1.5 * around[y * w + x] + 0.002 {
                continue;
            }
            let peak = (y.saturating_sub(1)..(y + 2).min(h))
                .all(|j| (x.saturating_sub(1)..(x + 2).min(w)).all(|i| lum[j * w + i] <= v));
            if !peak {
                continue;
            }
            count += 1;
            ratios.push(v / around[y * w + x].max(1e-4));
            // Its size: pixels within 4 of it at least half as bright.
            let mut half = 0usize;
            for j in y.saturating_sub(4)..(y + 5).min(h) {
                for i in x.saturating_sub(4)..(x + 5).min(w) {
                    half += usize::from(lum[j * w + i] >= 0.5 * v);
                }
            }
            sizes.push(2.0 * (half as f32 / std::f32::consts::PI).sqrt() * nominal);
        }
    }
    let n = (w * h) as f32;
    let fine = {
        let d: Vec<f32> = lum.iter().zip(&around).map(|(a, b)| a - b).collect();
        let mean = d.iter().sum::<f32>() / n;
        (d.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n).sqrt()
    };
    // On the screen: brightness, what is lit, and its colour.
    let (x0, y0, _, _) = area.pixels(picture.width, picture.height);
    let mut seen = Vec::with_capacity(w * h);
    let (mut dark_sum, mut dark) = (0.0f32, 0usize);
    let mut per_band: Vec<Vec<f32>> = vec![Vec::new(); BANDS.len()];
    let (mut lit, mut grey) = (0usize, 0usize);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let p = picture.pixels[y * picture.width as usize + x];
            let s = encoded(luma(&p));
            seen.push(s);
            if s <= DARK {
                dark_sum += s;
                dark += 1;
                continue;
            }
            lit += 1;
            let (r, g, b) = (encoded(p[0]), encoded(p[1]), encoded(p[2]));
            let (hi, lo) = (r.max(g).max(b), r.min(g).min(b));
            let saturation = if hi > 0.0 { (hi - lo) / hi } else { 0.0 };
            if saturation < GREY {
                grey += 1;
                continue;
            }
            let hue = (3.0f32.sqrt() * (g - b))
                .atan2(2.0 * r - g - b)
                .to_degrees()
                .rem_euclid(360.0);
            per_band[band(hue)].push(saturation);
        }
    }
    seen.sort_unstable_by(f32::total_cmp);
    let at = |share: f32| seen[((seen.len() - 1) as f32 * share).round() as usize];
    let hues = per_band
        .iter_mut()
        .enumerate()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| Hue {
            band: BANDS[k].0,
            share: v.len() as f32 / lit.max(1) as f32,
            saturation: median(v),
        })
        .collect();
    Look {
        // A 100 x 100 patch at 1920 wide is (100 / nominal) squared pixels here.
        grains: count as f32 / n * 10_000.0 / (nominal * nominal),
        stand_out: median(&mut ratios),
        grain_size: median(&mut sizes),
        fine_contrast: fine,
        brightness: [at(0.10), at(0.50), at(0.90), at(0.995)],
        coverage: lit as f32 / n,
        background: if dark > 0 {
            dark_sum / dark as f32
        } else {
            0.0
        },
        hues,
        grey: grey as f32 / lit.max(1) as f32,
    }
}
/// How a region glitters over time, from two frames in a row.
#[derive(Clone, Debug, Serialize)]
pub struct Shimmer {
    /// Share of the region bright on the screen (above 0.35) in either frame.
    pub bright: f32,
    /// Of those, the share that is less than half as bright in the other frame.
    pub changing: f32,
}
/// The shimmer of `area` between two frames of the same size.
pub fn shimmer(first: Picture<'_>, next: Picture<'_>, area: &Area) -> Result<Shimmer, String> {
    if first.width != next.width || first.height != next.height {
        return Err("shimmer needs two frames of the same size".into());
    }
    let (a, w, h) = region_luma(first, area);
    let (b, _, _) = region_luma(next, area);
    let (mut bright, mut changing) = (0usize, 0usize);
    for (x, y) in a.iter().zip(&b) {
        let (x, y) = (encoded(*x), encoded(*y));
        let (hi, lo) = (x.max(y), x.min(y));
        if hi > 0.35 {
            bright += 1;
            changing += usize::from(lo < 0.5 * hi);
        }
    }
    Ok(Shimmer {
        bright: bright as f32 / (w * h) as f32,
        changing: if bright > 0 {
            changing as f32 / bright as f32
        } else {
            0.0
        },
    })
}

#[cfg(test)]
mod tests;
