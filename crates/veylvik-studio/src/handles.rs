//! Shape and gradient authoring overlays. All changes stay in the inspector draft.
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use veylvik_document::{Operation, Result, Scalar, Time};

fn edit(scalar: &mut Scalar, time: Time, delta: f32, min: f32, max: f32) -> Result<()> {
    let value = (scalar.at(time)? + delta).clamp(min, max);
    if let Scalar::Constant { value: current } = scalar {
        *current = value;
        Ok(())
    } else {
        scalar.set_key_at(time, value)
    }
}

fn move_handle(operation: &mut Operation, handle: usize, delta: Vec2, time: Time) -> Result<()> {
    let mut next = operation.clone();
    match &mut next {
        Operation::Ellipse {
            x,
            y,
            radius_x,
            radius_y,
            ..
        } => match handle {
            0 => {
                edit(x, time, delta.x, -1_000_000.0, 1_000_000.0)?;
                edit(y, time, delta.y, -1_000_000.0, 1_000_000.0)?;
            }
            1 => edit(radius_x, time, delta.x, 0.0, 8192.0)?,
            2 => edit(radius_y, time, delta.y, 0.0, 8192.0)?,
            _ => unreachable!(),
        },
        Operation::Gradient {
            start_x,
            start_y,
            end_x,
            end_y,
            ..
        } => {
            let (x, y) = if handle == 0 {
                (start_x, start_y)
            } else {
                (end_x, end_y)
            };
            edit(x, time, delta.x, -1_000_000.0, 1_000_000.0)?;
            edit(y, time, delta.y, -1_000_000.0, 1_000_000.0)?;
        }
        _ => return Ok(()),
    }
    *operation = next;
    Ok(())
}

pub fn show(
    ui: &egui::Ui,
    canvas: Rect,
    transform: crate::viewer::ViewTransform,
    image: Vec2,
    operation: &mut Operation,
    time: Time,
) -> Result<bool> {
    let [ox, oy, sx, sy] = transform.0;
    let scale = egui::vec2(
        canvas.width() * sx / image.x,
        canvas.height() * sy / image.y,
    );
    let origin = canvas.min + egui::vec2(canvas.width() * ox, canvas.height() * oy);
    let painter = ui.painter().with_clip_rect(canvas);
    let color = Color32::from_rgb(255, 190, 80);
    let ellipse = |center: Pos2, radii: Vec2| {
        let outline: Vec<Pos2> = (0..=96)
            .map(|i| {
                let angle = i as f32 * std::f32::consts::TAU / 96.0;
                center + egui::vec2(angle.cos(), angle.sin()) * radii
            })
            .collect();
        painter.add(egui::Shape::line(outline, Stroke::new(1.0_f32, color)));
    };
    let points = match operation {
        Operation::Ellipse {
            x,
            y,
            radius_x,
            radius_y,
            ..
        } => {
            let center = origin + egui::vec2(x.at(time)?, y.at(time)?) * scale;
            let radii = egui::vec2(radius_x.at(time)?, radius_y.at(time)?) * scale;
            ellipse(center, radii);
            vec![
                (
                    center,
                    "Drag ellipse center. Apply commits; Revert discards.",
                ),
                (
                    center + egui::vec2(radii.x.max(18.0), 0.0),
                    "Drag horizontal radius. Apply commits; Revert discards.",
                ),
                (
                    center + egui::vec2(0.0, radii.y.max(18.0)),
                    "Drag vertical radius. Apply commits; Revert discards.",
                ),
            ]
        }
        Operation::Gradient {
            mode,
            start_x,
            start_y,
            end_x,
            end_y,
        } => {
            let start = egui::vec2(start_x.at(time)?, start_y.at(time)?);
            let end = egui::vec2(end_x.at(time)?, end_y.at(time)?);
            let center = origin + start * scale;
            let endpoint = origin + end * scale;
            painter.line_segment([center, endpoint], Stroke::new(1.0_f32, color));
            if *mode == veylvik_document::GradientMode::Radial {
                ellipse(center, Vec2::splat((end - start).length()) * scale);
            }
            let delta = endpoint - center;
            let endpoint = if delta.length() < 18.0 {
                center
                    + if delta == Vec2::ZERO {
                        egui::vec2(18.0, 0.0)
                    } else {
                        delta.normalized() * 18.0
                    }
            } else {
                endpoint
            };
            vec![
                (
                    center,
                    "Drag gradient start. Apply commits; Revert discards.",
                ),
                (
                    endpoint,
                    "Drag gradient end. Apply commits; Revert discards.",
                ),
            ]
        }
        _ => return Ok(false),
    };
    let mut changed = false;
    // Keep handles separated for zero-radius ellipses and coincident endpoints.
    for (index, (point, hint)) in points.into_iter().enumerate() {
        let hit = Rect::from_center_size(point, Vec2::splat(16.0)).intersect(canvas);
        if !hit.is_positive() {
            continue;
        }
        let response = ui
            .interact(hit, ui.id().with(("geometry_handle", index)), Sense::drag())
            .on_hover_cursor(egui::CursorIcon::Grab)
            .on_hover_text(hint);
        painter.circle_filled(point, 5.0, Color32::from_gray(25));
        painter.circle_stroke(point, 5.0, Stroke::new(1.5_f32, color));
        if response.dragged() && response.drag_delta() != Vec2::ZERO {
            move_handle(operation, index, response.drag_delta() / scale, time)?;
            changed = true;
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::{Interpolation, Key};
    #[test]
    fn gradient_handles_edit_endpoints_independently() {
        let mut operation = Operation::Gradient {
            mode: veylvik_document::GradientMode::Radial,
            start_x: 10.0.into(),
            start_y: 20.0.into(),
            end_x: 100.0.into(),
            end_y: 200.0.into(),
        };
        let time = Time::new(0, 1).unwrap();
        move_handle(&mut operation, 1, egui::vec2(-5.0, 8.0), time).unwrap();
        assert_eq!(operation.scalar("start_x").unwrap().at(time).unwrap(), 10.0);
        assert_eq!(operation.scalar("end_x").unwrap().at(time).unwrap(), 95.0);
        assert_eq!(operation.scalar("end_y").unwrap().at(time).unwrap(), 208.0);
        let start = operation.scalar_mut("start_x").unwrap();
        start.set_key_at(time, 10.0).unwrap();
        start.set_interpolation(Interpolation::Monotone);
        move_handle(
            &mut operation,
            0,
            egui::vec2(2.0, 3.0),
            Time::new(1, 1).unwrap(),
        )
        .unwrap();
        let start = operation.scalar("start_x").unwrap();
        assert_eq!(start.interpolation(), Some(Interpolation::Monotone));
        assert_eq!(start.keys().unwrap().len(), 2);
        assert_eq!(start.at(time).unwrap(), 10.0);
        assert_eq!(start.at(Time::new(1, 1).unwrap()).unwrap(), 12.0);
    }
    #[test]
    fn handle_edits_keep_constants_and_preserve_exact_animation() {
        let mut operation = Operation::Ellipse {
            x: 10.0.into(),
            y: Scalar::Smooth {
                keys: vec![Key {
                    time: Time { ticks: 0, rate: 24 },
                    value: 20.0,
                }],
            },
            radius_x: 100.0.into(),
            radius_y: 50.0.into(),
            feather: 1.0.into(),
        };
        let time = Time { ticks: 0, rate: 1 };
        move_handle(&mut operation, 0, egui::vec2(5.0, -2.0), time).unwrap();
        assert!(
            matches!(operation.scalar("x").unwrap(), Scalar::Constant { value } if *value == 15.0)
        );
        let y = operation.scalar("y").unwrap();
        assert_eq!(y.interpolation(), Some(Interpolation::Smooth));
        assert_eq!(y.keys().unwrap()[0].time.rate, 24);
        assert_eq!(y.at(time).unwrap(), 18.0);
        move_handle(&mut operation, 1, egui::vec2(-200.0, 0.0), time).unwrap();
        assert_eq!(operation.scalar("radius_x").unwrap().at(time).unwrap(), 0.0);
        let before = serde_json::to_vec(&operation).unwrap();
        assert!(
            move_handle(
                &mut operation,
                0,
                Vec2::splat(3.0),
                Time { ticks: 0, rate: 0 }
            )
            .is_err()
        );
        assert_eq!(serde_json::to_vec(&operation).unwrap(), before);
    }
}
