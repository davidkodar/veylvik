//! Detail windows for the zoomed second pass: one crop size for the whole shot (the largest
//! subject extent plus a margin, so the model sees a constant scale and its memory stays
//! consistent), centred on the subject on every frame and smoothed over time.
use crate::{Point, SIDE};
use std::collections::BTreeMap;

/// x0, y0, x1, y1 in frame pixels.
pub type Rect = [f64; 4];
/// Margin around the subject, as a fraction of its extent on each side, plus a fixed minimum.
const MARGIN: f64 = 0.2;
const PAD: f64 = 24.0;
/// Below this linear gain (geometric mean of both axes) the second pass is not worth its time.
pub const MIN_GAIN: f64 = 1.3;
/// Frames on each side averaged into a window's centre.
const SMOOTHING: i64 = 4;

/// The subject's extent in frame pixels, from SIDE² logits (positive inside); `None` if empty.
pub fn bounds(logits: &[f32], width: usize, height: usize) -> Option<Rect> {
    let (mut x0, mut y0, mut x1, mut y1) = (SIDE, SIDE, 0, 0);
    for (i, v) in logits.iter().enumerate() {
        if *v > 0.0 {
            let (x, y) = (i % SIDE, i / SIDE);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    (x1 >= x0 && y1 >= y0 && x0 < SIDE).then(|| {
        let (sx, sy) = (width as f64 / SIDE as f64, height as f64 / SIDE as f64);
        [
            x0 as f64 * sx,
            y0 as f64 * sy,
            (x1 + 1) as f64 * sx,
            (y1 + 1) as f64 * sy,
        ]
    })
}
/// One window per analysed frame, or `None` when zooming would not gain enough detail (or the
/// subject was never found). `boxes` holds each frame's subject extent from the first pass.
pub fn windows(
    boxes: &BTreeMap<i64, Option<Rect>>,
    width: f64,
    height: f64,
) -> Option<BTreeMap<i64, Rect>> {
    let found: Vec<(i64, Rect)> = boxes
        .iter()
        .filter_map(|(f, b)| b.map(|b| (*f, b)))
        .collect();
    let extent = |axis: usize| {
        found
            .iter()
            .map(|(_, b)| b[axis + 2] - b[axis])
            .fold(0.0, f64::max)
    };
    let (bw, bh) = (extent(0), extent(1));
    if found.is_empty() || bw <= 0.0 || bh <= 0.0 {
        return None;
    }
    let mut w = bw * (1.0 + 2.0 * MARGIN) + 2.0 * PAD;
    let mut h = bh * (1.0 + 2.0 * MARGIN) + 2.0 * PAD;
    // Keep the crop within 1:2 and 2:1, since the model squeezes it into a square.
    w = w.max(h / 2.0);
    h = h.max(w / 2.0);
    let (w, h) = (w.min(width), h.min(height));
    if ((width / w) * (height / h)).sqrt() < MIN_GAIN {
        return None;
    }
    // Centres: the subject's own where found, else the nearest frame's that was.
    let centre = |b: &Rect| [(b[0] + b[2]) * 0.5, (b[1] + b[3]) * 0.5];
    let centres: BTreeMap<i64, [f64; 2]> = boxes
        .keys()
        .map(|f| {
            let nearest = found
                .iter()
                .min_by_key(|(g, _)| (g - f).abs())
                .map(|(_, b)| centre(b))
                .unwrap_or([width * 0.5, height * 0.5]);
            (*f, nearest)
        })
        .collect();
    Some(
        centres
            .keys()
            .map(|f| {
                let near: Vec<[f64; 2]> = centres
                    .range(f - SMOOTHING..=f + SMOOTHING)
                    .map(|(_, c)| *c)
                    .collect();
                let n = near.len() as f64;
                let c = [
                    near.iter().map(|c| c[0]).sum::<f64>() / n,
                    near.iter().map(|c| c[1]).sum::<f64>() / n,
                ];
                let x0 = (c[0] - w * 0.5).clamp(0.0, width - w);
                let y0 = (c[1] - h * 0.5).clamp(0.0, height - h);
                (*f, [x0, y0, x0 + w, y0 + h])
            })
            .collect(),
    )
}
/// Maps prompt points (0..1 of the frame) into a window, dropping those that fall outside it.
pub fn map_points(points: &[Point], window: Rect, width: f64, height: f64) -> Vec<Point> {
    points
        .iter()
        .filter_map(|p| {
            let x = (f64::from(p.x) * width - window[0]) / (window[2] - window[0]);
            let y = (f64::from(p.y) * height - window[1]) / (window[3] - window[1]);
            ((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y)).then_some(Point {
                x: x as f32,
                y: y as f32,
                positive: p.positive,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_map_model_pixels_to_the_frame() {
        let mut logits = vec![-5.0; SIDE * SIDE];
        logits[100 * SIDE + 200] = 3.0;
        logits[300 * SIDE + 250] = 3.0;
        let b = bounds(&logits, 1024, 2048).unwrap();
        assert_eq!(b, [200.0, 200.0, 251.0, 602.0]);
        assert!(bounds(&vec![-1.0; SIDE * SIDE], 1024, 1024).is_none());
    }
    #[test]
    fn windows_keep_one_size_follow_the_subject_and_stay_in_frame() {
        // A small subject walking right across a 1080 × 1920 frame.
        let boxes: BTreeMap<i64, Option<Rect>> = (0..20)
            .map(|f| {
                let x = 300.0 + f as f64 * 20.0;
                (f, (f != 7).then_some([x, 800.0, x + 120.0, 1300.0]))
            })
            .collect();
        let found = windows(&boxes, 1080.0, 1920.0).unwrap();
        assert_eq!(found.len(), 20);
        let size = |r: &Rect| (r[2] - r[0], r[3] - r[1]);
        let first = size(&found[&0]);
        assert!(
            found.values().all(|r| {
                let s = size(r);
                (s.0 - first.0).abs() < 1e-9 && (s.1 - first.1).abs() < 1e-9
            }),
            "constant scale"
        );
        // Aspect limited to 1:2, so the narrow subject gets a wider crop.
        assert!((first.0 - first.1 / 2.0).abs() < 1e-6, "{first:?}");
        for (f, r) in &found {
            assert!(r[0] >= 0.0 && r[2] <= 1080.0 && r[1] >= 0.0 && r[3] <= 1920.0);
            // Every frame's subject lies inside its window, including the frame it was missed.
            let x = 300.0 + *f as f64 * 20.0;
            assert!(r[0] <= x && x + 120.0 <= r[2], "frame {f}: {r:?}");
        }
        // Centres move smoothly.
        let centres: Vec<f64> = found.values().map(|r| (r[0] + r[2]) / 2.0).collect();
        assert!(centres.windows(2).all(|p| p[1] >= p[0]));
        // A subject filling the frame gains nothing.
        let big: BTreeMap<i64, Option<Rect>> = [(0, Some([0.0, 0.0, 1000.0, 1800.0]))].into();
        assert!(windows(&big, 1080.0, 1920.0).is_none());
        assert!(windows(&[(0, None)].into(), 1080.0, 1920.0).is_none());
    }
    #[test]
    fn points_map_into_the_window_and_outside_points_are_dropped() {
        let points = [
            Point {
                x: 0.5,
                y: 0.5,
                positive: true,
            },
            Point {
                x: 0.05,
                y: 0.5,
                positive: false,
            },
        ];
        let mapped = map_points(&points, [400.0, 800.0, 800.0, 1200.0], 1200.0, 2000.0);
        assert_eq!(mapped.len(), 1);
        assert!((mapped[0].x - 0.5).abs() < 1e-6 && (mapped[0].y - 0.5).abs() < 1e-6);
    }
}
