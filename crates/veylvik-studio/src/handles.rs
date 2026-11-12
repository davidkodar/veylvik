//! Ellipse authoring overlays. All changes stay in the inspector draft.
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
    if let Operation::Ellipse {
        x,
        y,
        radius_x,
        radius_y,
        ..
    } = &mut next
    {
        match handle {
            0 => {
                edit(x, time, delta.x, -1_000_000.0, 1_000_000.0)?;
                edit(y, time, delta.y, -1_000_000.0, 1_000_000.0)?;
            }
            1 => edit(radius_x, time, delta.x, 0.0, 8192.0)?,
            2 => edit(radius_y, time, delta.y, 0.0, 8192.0)?,
            _ => unreachable!(),
        }
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
    let Operation::Ellipse {
        x,
        y,
        radius_x,
        radius_y,
        ..
    } = operation
    else {
        return Ok(false);
    };
    let [ox, oy, sx, sy] = transform.0;
    let scale = egui::vec2(
        canvas.width() * sx / image.x,
        canvas.height() * sy / image.y,
    );
    let origin = canvas.min + egui::vec2(canvas.width() * ox, canvas.height() * oy);
    let center = origin + egui::vec2(x.at(time)?, y.at(time)?) * scale;
    let radii = egui::vec2(radius_x.at(time)?, radius_y.at(time)?) * scale;
    let points = [
        center,
        center + egui::vec2(radii.x, 0.0),
        center + egui::vec2(0.0, radii.y),
    ];
    let painter = ui.painter().with_clip_rect(canvas);
    let color = Color32::from_rgb(255, 190, 80);
    let outline: Vec<Pos2> = (0..=96)
        .map(|i| {
            let angle = i as f32 * std::f32::consts::TAU / 96.0;
            center + egui::vec2(angle.cos(), angle.sin()) * radii
        })
        .collect();
    painter.add(egui::Shape::line(outline, Stroke::new(1.0_f32, color)));
    let mut changed = false;
    for (index, point) in points.into_iter().enumerate() {
        // Offset zero-radius handles so a collapsed ellipse can be expanded.
        let point = match index {
            1 if radii.x < 18.0 => center + egui::vec2(18.0, 0.0),
            2 if radii.y < 18.0 => center + egui::vec2(0.0, 18.0),
            _ => point,
        };
        let hit = Rect::from_center_size(point, Vec2::splat(16.0)).intersect(canvas);
        if !hit.is_positive() {
            continue;
        }
        let response = ui
            .interact(hit, ui.id().with(("ellipse_handle", index)), Sense::drag())
            .on_hover_cursor(egui::CursorIcon::Grab)
            .on_hover_text(match index {
                0 => "Drag ellipse center. Apply commits; Revert discards.",
                1 => "Drag horizontal radius. Apply commits; Revert discards.",
                _ => "Drag vertical radius. Apply commits; Revert discards.",
            });
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
