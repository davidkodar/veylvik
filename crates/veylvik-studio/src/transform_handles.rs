//! Direct affine authoring in image coordinates; viewer navigation stays separate.
use eframe::egui::{self, Color32, Rect, Stroke, Vec2};
use veylvik_document::{Operation, Result, Time};

struct Geometry {
    center: Vec2,
    pivot: Vec2,
    axes: [Vec2; 2],
    scales: Vec2,
    base: f32,
    affine: bool,
}
impl Geometry {
    fn at(operation: &Operation, image: Vec2, time: Time) -> Result<Self> {
        let sample = |name| {
            operation
                .scalar(name)
                .expect("transform parameter")
                .at(time)
        };
        let text = matches!(
            operation,
            Operation::Text(veylvik_document::nodes::Text { .. })
        );
        let affine = text
            || matches!(
                operation,
                Operation::Affine(veylvik_document::nodes::Affine { .. })
            );
        let pivot = if text {
            egui::vec2(sample("x")?, sample("y")?)
        } else if affine {
            egui::vec2(sample("pivot_x")?, sample("pivot_y")?)
        } else {
            image * 0.5
        };
        let angle = if affine {
            (sample("rotation")? as f64).rem_euclid(360.0).to_radians()
        } else {
            0.0
        };
        let (sin, cos) = angle.sin_cos();
        Ok(Self {
            center: if text {
                pivot
            } else {
                pivot + egui::vec2(sample("x")?, sample("y")?)
            },
            pivot,
            axes: [
                egui::vec2(cos as f32, sin as f32),
                egui::vec2(-sin as f32, cos as f32),
            ],
            scales: if affine {
                egui::vec2(sample("scale_x")?, sample("scale_y")?)
            } else {
                Vec2::splat(1.0)
            },
            base: image.x.min(image.y) * 0.25,
            affine,
        })
    }
    fn map(&self, point: Vec2) -> Vec2 {
        let local = (point - self.pivot) * self.scales;
        self.center + self.axes[0] * local.x + self.axes[1] * local.y
    }
}
fn edit(operation: &mut Operation, name: &str, delta: f32, time: Time) -> Result<()> {
    let parameter = operation
        .descriptor()
        .parameters
        .iter()
        .find(|p| p.name == name)
        .expect("transform parameter");
    crate::handles::edit(
        operation.scalar_mut(name).expect("transform scalar"),
        time,
        delta,
        parameter.min.unwrap(),
        parameter.max.unwrap(),
    )
}
fn move_handle(
    operation: &mut Operation,
    index: usize,
    delta: Vec2,
    pointer: Vec2,
    image: Vec2,
    time: Time,
) -> Result<()> {
    let geometry = Geometry::at(operation, image, time)?;
    let mut next = operation.clone();
    match index {
        0 => {
            edit(&mut next, "x", delta.x, time)?;
            edit(&mut next, "y", delta.y, time)?;
        }
        1 => edit(
            &mut next,
            "scale_x",
            delta.dot(geometry.axes[0]) / geometry.base,
            time,
        )?,
        2 => edit(
            &mut next,
            "scale_y",
            delta.dot(geometry.axes[1]) / geometry.base,
            time,
        )?,
        3 => {
            let after = pointer - geometry.center;
            let before = after - delta;
            // Very near the pivot, pointer angle has no stable meaning.
            if before.length_sq() >= 1.0 && after.length_sq() >= 1.0 {
                let cross = before.x as f64 * after.y as f64 - before.y as f64 * after.x as f64;
                let dot = before.x as f64 * after.x as f64 + before.y as f64 * after.y as f64;
                edit(
                    &mut next,
                    "rotation",
                    cross.atan2(dot).to_degrees() as f32,
                    time,
                )?;
            }
        }
        _ => unreachable!(),
    }
    if matches!(&next,Operation::Text(veylvik_document::nodes::Text {typography,..}) if typography.linked_scale)
        && matches!(index, 1 | 2)
    {
        let (axis, other, before, old_other) = if index == 1 {
            ("scale_x", "scale_y", geometry.scales.x, geometry.scales.y)
        } else {
            ("scale_y", "scale_x", geometry.scales.y, geometry.scales.x)
        };
        let current = next.scalar(axis).unwrap().at(time)?;
        let target = if before.abs() > 0.0001 {
            old_other * current / before
        } else {
            current
        };
        edit(&mut next, other, target - old_other, time)?;
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
    let geometry = Geometry::at(operation, image, time)?;
    let [ox, oy, sx, sy] = transform.0;
    let scale = egui::vec2(
        canvas.width() * sx / image.x,
        canvas.height() * sy / image.y,
    );
    let origin = canvas.min + egui::vec2(canvas.width() * ox, canvas.height() * oy);
    let screen = |point: Vec2| origin + point * scale;
    let center = screen(geometry.center);
    let painter = ui.painter().with_clip_rect(canvas);
    let color = crate::design::ACCENT;
    let stroke = Stroke::new(1.0_f32, color);
    let corners = [
        Vec2::ZERO,
        egui::vec2(image.x, 0.0),
        image,
        egui::vec2(0.0, image.y),
        Vec2::ZERO,
    ];
    if !matches!(
        operation,
        Operation::Text(veylvik_document::nodes::Text { .. })
    ) {
        painter.add(egui::Shape::line(
            corners.map(|point| screen(geometry.map(point))).to_vec(),
            stroke,
        ));
    }
    let mut points = vec![(center, "Drag to translate. Apply commits; Revert discards.")];
    if geometry.affine {
        for (axis, value, hint) in [
            (
                geometry.axes[0],
                geometry.scales.x,
                "Drag along the axis to scale X. Cross the center to flip.",
            ),
            (
                geometry.axes[1],
                geometry.scales.y,
                "Drag along the axis to scale Y. Cross the center to flip.",
            ),
        ] {
            let direction = (axis * scale).normalized();
            let length = (axis * scale * geometry.base * value).length().max(18.0);
            let point = center + direction * length * if value < 0.0 { -1.0 } else { 1.0 };
            painter.line_segment([center, point], stroke);
            points.push((point, hint));
        }
        let rotate = center + ((geometry.axes[0] + geometry.axes[1]) * scale).normalized() * 70.0;
        painter.line_segment([center, rotate], stroke);
        points.push((
            rotate,
            "Drag around the pivot to rotate clockwise. Apply commits; Revert discards.",
        ));
    }
    let mut changed = false;
    for (index, (point, hint)) in points.into_iter().enumerate() {
        let hit = Rect::from_center_size(point, Vec2::splat(16.0)).intersect(canvas);
        if !hit.is_positive() {
            continue;
        }
        let response = crate::handles::interact(ui, hit, ui.id().with(("transform_handle", index)))
            .on_hover_cursor(egui::CursorIcon::Grab)
            .on_hover_text(hint);
        if index == 3 {
            painter.rect_filled(Rect::from_center_size(point, Vec2::splat(9.0)), 0.0, color);
        } else {
            painter.circle_filled(point, 5.0, Color32::from_gray(25));
            painter.circle_stroke(point, 5.0, Stroke::new(1.5_f32, color));
        }
        let delta = crate::handles::drag_delta(&response);
        if response.dragged()
            && delta != Vec2::ZERO
            && let Some(pointer) = response.interact_pointer_pos()
        {
            move_handle(
                operation,
                index,
                delta / scale,
                (pointer - origin) / scale,
                image,
                time,
            )?;
            changed = true;
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::{Interpolation, Scalar};
    fn affine() -> Operation {
        Operation::Affine(veylvik_document::nodes::Affine {
            x: 3.0.into(),
            y: 4.0.into(),
            scale_x: (-2.0).into(),
            scale_y: 3.0.into(),
            rotation: 90.0.into(),
            pivot_x: 10.0.into(),
            pivot_y: 20.0.into(),
        })
    }
    #[test]
    fn geometry_and_axis_drags_match_forward_affine_with_negative_scales() {
        let time = Time::new(0, 1).unwrap();
        let image = Vec2::splat(400.0);
        let mut operation = affine();
        let geometry = Geometry::at(&operation, image, time).unwrap();
        assert!((geometry.map(egui::vec2(11.0, 22.0)) - egui::vec2(7.0, 22.0)).length() < 1e-5);
        assert_eq!(geometry.center, egui::vec2(13.0, 24.0));
        move_handle(
            &mut operation,
            1,
            egui::vec2(0.0, 20.0),
            Vec2::ZERO,
            image,
            time,
        )
        .unwrap();
        move_handle(
            &mut operation,
            2,
            egui::vec2(-10.0, 0.0),
            Vec2::ZERO,
            image,
            time,
        )
        .unwrap();
        assert!((operation.scalar("scale_x").unwrap().at(time).unwrap() + 1.8).abs() < 1e-6);
        assert!((operation.scalar("scale_y").unwrap().at(time).unwrap() - 3.1).abs() < 1e-6);
        assert_eq!(
            operation.scalar("rotation").unwrap().at(time).unwrap(),
            90.0
        );
        assert_eq!(operation.scalar("pivot_x").unwrap().at(time).unwrap(), 10.0);
        *operation.scalar_mut("scale_x").unwrap() = 0.0.into();
        move_handle(
            &mut operation,
            1,
            egui::vec2(0.0, -50.0),
            Vec2::ZERO,
            image,
            time,
        )
        .unwrap();
        assert!((operation.scalar("scale_x").unwrap().at(time).unwrap() + 0.5).abs() < 1e-6);
    }
    #[test]
    fn rotation_wraps_shortest_delta_and_keeps_animation_and_pivot() {
        let time = Time::new(0, 1).unwrap();
        let image = Vec2::splat(400.0);
        let mut operation = affine();
        let rotation = operation.scalar_mut("rotation").unwrap();
        rotation
            .set_key_at(Time::new(0, 24).unwrap(), 90.0)
            .unwrap();
        rotation.set_interpolation(Interpolation::Monotone);
        let center = Geometry::at(&operation, image, time).unwrap().center;
        let point = |degrees: f32| {
            center + egui::vec2(degrees.to_radians().cos(), degrees.to_radians().sin()) * 100.0
        };
        let (before, after) = (point(179.0), point(-179.0));
        move_handle(&mut operation, 3, after - before, after, image, time).unwrap();
        let rotation = operation.scalar("rotation").unwrap();
        assert!((rotation.at(time).unwrap() - 92.0).abs() < 1e-4);
        assert_eq!(rotation.interpolation(), Some(Interpolation::Monotone));
        assert_eq!(rotation.keys().unwrap()[0].time.rate, 24);
        let saved = serde_json::to_vec(&operation).unwrap();
        move_handle(&mut operation, 3, Vec2::splat(0.1), center, image, time).unwrap();
        assert_eq!(serde_json::to_vec(&operation).unwrap(), saved);
        assert!(
            move_handle(
                &mut operation,
                0,
                Vec2::splat(5.0),
                Vec2::ZERO,
                image,
                Time { ticks: 0, rate: 0 }
            )
            .is_err()
        );
        assert_eq!(serde_json::to_vec(&operation).unwrap(), saved);
    }
    #[test]
    fn translation_preserves_constants_and_animated_exact_keys_with_registry_bounds() {
        let time = Time::new(0, 1).unwrap();
        let image = egui::vec2(640.0, 360.0);
        let mut operation = Operation::Transform(veylvik_document::nodes::Transform {
            x: 0.0.into(),
            y: 0.0.into(),
        });
        operation
            .scalar_mut("y")
            .unwrap()
            .set_key_at(Time::new(0, 24).unwrap(), 0.0)
            .unwrap();
        let geometry = Geometry::at(&operation, image, time).unwrap();
        assert_eq!(geometry.center, image * 0.5);
        move_handle(
            &mut operation,
            0,
            egui::vec2(20.0, 30.0),
            Vec2::ZERO,
            image,
            time,
        )
        .unwrap();
        assert!(matches!(operation.scalar("x").unwrap(),Scalar::Constant{value} if *value==20.0));
        let y = operation.scalar("y").unwrap();
        assert_eq!(y.at(time).unwrap(), 30.0);
        assert_eq!(y.keys().unwrap()[0].time.rate, 24);
        move_handle(&mut operation, 0, Vec2::splat(1e8), Vec2::ZERO, image, time).unwrap();
        operation.validate().unwrap();
    }
}
