//! Read-only curve plots use exact rational differences for the horizontal axis.
use eframe::egui;
use veylvik_document::{Interpolation, Scalar, Time};
struct Plot {
    points: Vec<[f32; 2]>,
    keys: Vec<[f32; 2]>,
    playhead: Option<f32>,
    min: f64,
    max: f64,
}
fn relative(time: Time, first: Time, last: Time) -> Option<f64> {
    let span = last.ticks as i128 * first.rate as i128 - first.ticks as i128 * last.rate as i128;
    if span == 0 {
        return None;
    }
    let elapsed = time.ticks as i128 * first.rate as i128 - first.ticks as i128 * time.rate as i128;
    Some(elapsed as f64 / span as f64 * last.rate as f64 / time.rate as f64)
}
fn plot(scalar: &Scalar, time: Time, width: f32) -> Option<Plot> {
    scalar.validate().ok()?;
    time.seconds().ok()?;
    let (keys, mode) = match scalar {
        Scalar::Linear { keys } => (keys, Interpolation::Linear),
        Scalar::Step { keys } => (keys, Interpolation::Step),
        Scalar::Smooth { keys } => (keys, Interpolation::Smooth),
        _ => return None,
    };
    let first = keys.first()?.time;
    let last = keys.last()?.time;
    let mut min = keys
        .iter()
        .map(|k| k.value as f64)
        .fold(f64::INFINITY, f64::min);
    let mut max = keys
        .iter()
        .map(|k| k.value as f64)
        .fold(f64::NEG_INFINITY, f64::max);
    let padding = if min == max {
        (min.abs() * 0.05).max(0.5)
    } else {
        (max - min) * 0.08
    };
    min -= padding;
    max += padding;
    let y = |value: f64| (1.0 - (value - min) / (max - min)) as f32;
    let key_points: Vec<_> = keys
        .iter()
        .map(|k| {
            [
                relative(k.time, first, last).unwrap_or(0.5) as f32,
                y(k.value as f64),
            ]
        })
        .collect();
    let mut points = vec![key_points[0]];
    for (index, pair) in keys.windows(2).enumerate() {
        let a = key_points[index];
        let b = key_points[index + 1];
        match mode {
            Interpolation::Step => {
                points.push([b[0], a[1]]);
                points.push(b);
            }
            Interpolation::Linear => points.push(b),
            Interpolation::Smooth => {
                // At most 32 samples per segment; total scales with canvas width
                // plus key count, not width multiplied by key count.
                let samples =
                    (((b[0] - a[0]) * width.clamp(1.0, 2048.0) / 4.0).ceil() as usize).clamp(1, 32);
                for step in 1..=samples {
                    let t = step as f64 / samples as f64;
                    let eased = t * t * (3.0 - 2.0 * t);
                    points.push([
                        a[0] + (b[0] - a[0]) * t as f32,
                        y(pair[0].value as f64 * (1.0 - eased) + pair[1].value as f64 * eased),
                    ]);
                }
            }
        }
    }
    if keys.len() == 1 {
        points = vec![[0.0, key_points[0][1]], [1.0, key_points[0][1]]];
    }
    let playhead = relative(time, first, last)
        .or_else(|| time.compare(first).ok().filter(|o| o.is_eq()).map(|_| 0.5))
        .filter(|v| (0.0..=1.0).contains(v))
        .map(|v| v as f32);
    Some(Plot {
        points,
        keys: key_points,
        playhead,
        min,
        max,
    })
}
pub fn show(ui: &mut egui::Ui, scalar: &Scalar, time: Time) {
    let Some(plot) = plot(scalar, time, ui.available_width()) else {
        return;
    };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(1.0), 100.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
    let graph = rect.shrink2(egui::vec2(6.0, 16.0));
    let point = |p: [f32; 2]| {
        egui::pos2(
            graph.left() + p[0] * graph.width(),
            graph.top() + p[1] * graph.height(),
        )
    };
    for y in [0.0, 0.5, 1.0] {
        painter.line_segment(
            [point([0.0, y]), point([1.0, y])],
            egui::Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );
    }
    painter.add(egui::Shape::line(
        plot.points.into_iter().map(point).collect(),
        egui::Stroke::new(1.5_f32, ui.visuals().selection.stroke.color),
    ));
    let mut previous_x = f32::NEG_INFINITY;
    for key in plot.keys {
        let center = point(key);
        if center.x - previous_x >= 3.0 {
            painter.circle_filled(center, 2.5, ui.visuals().text_color());
            previous_x = center.x;
        }
    }
    if let Some(x) = plot.playhead {
        painter.line_segment(
            [point([x, 0.0]), point([x, 1.0])],
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(245, 185, 75)),
        );
    }
    let font = egui::FontId::monospace(10.0);
    painter.text(
        rect.left_top() + egui::vec2(4.0, 2.0),
        egui::Align2::LEFT_TOP,
        format!("{:.3}", plot.max),
        font.clone(),
        ui.visuals().weak_text_color(),
    );
    painter.text(
        rect.left_bottom() + egui::vec2(4.0, -2.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{:.3}", plot.min),
        font,
        ui.visuals().weak_text_color(),
    );
    response.on_hover_text("First to last key; dots are keys and the amber line is the playhead. Edit the values below, then Apply.");
}
#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::Key;
    #[test]
    fn plots_preserve_large_adjacent_times_modes_and_invalid_drafts() {
        let first = Time::new(i64::MAX - 2, 1).unwrap();
        let last = Time::new(i64::MAX, 1).unwrap();
        let keys = vec![
            Key {
                time: first,
                value: -10.0,
            },
            Key {
                time: last,
                value: 10.0,
            },
        ];
        let now = Time::new(i64::MAX - 1, 1).unwrap();
        let linear = plot(&Scalar::Linear { keys: keys.clone() }, now, 200.0).unwrap();
        assert_eq!(linear.playhead, Some(0.5));
        assert_eq!(linear.keys[0][0], 0.0);
        assert_eq!(linear.keys[1][0], 1.0);
        assert_eq!(linear.points.len(), 2);
        let hold = plot(&Scalar::Step { keys: keys.clone() }, now, 200.0).unwrap();
        assert_eq!(hold.points[1], [1.0, hold.points[0][1]]);
        assert_eq!(hold.points.len(), 3);
        let smooth = plot(&Scalar::Smooth { keys: keys.clone() }, now, 200.0).unwrap();
        assert!(
            (smooth.points[8][1]
                - (1.0 - (-10.0 + 20.0 * 0.15625 - smooth.min) / (smooth.max - smooth.min)) as f32)
                .abs()
                < 1e-6
        );
        assert_eq!(
            relative(
                Time::new(12, 24).unwrap(),
                Time::new(0, 1).unwrap(),
                Time::new(1, 1).unwrap()
            ),
            Some(0.5)
        );
        assert!(
            plot(
                &Scalar::Linear {
                    keys: vec![keys[1].clone(), keys[0].clone()]
                },
                now,
                200.0
            )
            .is_none()
        );
        let single = plot(
            &Scalar::Smooth {
                keys: vec![Key {
                    time: first,
                    value: f32::MAX,
                }],
            },
            now,
            200.0,
        )
        .unwrap();
        assert!(single.points.iter().flatten().all(|v| v.is_finite()));
        assert_eq!(single.points[0][1], single.points[1][1]);
    }
}
