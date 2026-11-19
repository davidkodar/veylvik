//! A local-space text selection box. Only typography changes rebuild its outline;
//! moving, rotating and scaling remain cheap vector overlay operations.
use eframe::egui::{self, Color32, Rect, Stroke, Vec2};
use veylvik_document::{Operation, Result, Time};

fn bounds(ui: &egui::Ui, operation: &Operation, time: Time) -> Result<Rect> {
    let mut reference = operation.clone();
    let names: Vec<_> = reference
        .descriptor()
        .parameters
        .iter()
        .map(|p| p.name)
        .collect();
    for name in names {
        if let Some(value) = reference.scalar_mut(name) {
            *value = value.at(time)?.into();
        }
    }
    for (name, value) in [
        ("x", 0.),
        ("y", 0.),
        ("rotation", 0.),
        ("scale_x", 1.),
        ("scale_y", 1.),
    ] {
        *reference.scalar_mut(name).unwrap() = value.into();
    }
    if let Operation::Text(text) = &mut reference {
        text.typography.motion = veylvik_document::text::TextMotion::None;
    }
    let key = serde_json::to_vec(&reference).expect("serializable text");
    let id = ui.id().with("text-selection-bounds");
    if let Some((old, rect)) = ui.ctx().data(|d| d.get_temp::<(Vec<u8>, Rect)>(id))
        && old == key
    {
        return Ok(rect);
    }
    let path = veylvik_text::outline(&reference, time)
        .map_err(|e| veylvik_document::Error(e.to_string()))?;
    let b = veylvik_text::bounds(&path);
    let rect = if b.iter().all(|v| v.is_finite()) && b[2] > b[0] && b[3] > b[1] {
        Rect::from_min_max(egui::pos2(b[0], b[1]), egui::pos2(b[2], b[3]))
    } else {
        Rect::from_min_size(egui::pos2(0., -48.), egui::vec2(120., 48.))
    };
    ui.ctx().data_mut(|d| d.insert_temp(id, (key, rect)));
    Ok(rect)
}

fn sample(op: &Operation, name: &str, time: Time) -> Result<f32> {
    op.scalar(name).unwrap().at(time)
}
fn set(op: &mut Operation, name: &str, value: f32, time: Time) -> Result<()> {
    let p = op
        .descriptor()
        .parameters
        .iter()
        .find(|p| p.name == name)
        .unwrap();
    let delta = value - sample(op, name, time)?;
    crate::handles::edit(
        op.scalar_mut(name).unwrap(),
        time,
        delta,
        p.min.unwrap(),
        p.max.unwrap(),
    )
}
fn rotate(v: Vec2, angle: f32) -> Vec2 {
    let (s, c) = angle.to_radians().sin_cos();
    egui::vec2(c * v.x - s * v.y, s * v.x + c * v.y)
}

/// Quantize the visible direction, including rotation, mirroring and viewer scaling.
pub(super) fn resize_cursor(direction: Vec2) -> egui::CursorIcon {
    let sector = (direction.y.atan2(direction.x).to_degrees() / 45.).round() as i32;
    match sector.rem_euclid(4) {
        0 => egui::CursorIcon::ResizeHorizontal,
        1 => egui::CursorIcon::ResizeNwSe,
        2 => egui::CursorIcon::ResizeVertical,
        _ => egui::CursorIcon::ResizeNeSw,
    }
}

pub(super) fn rotation_cursor(ui: &egui::Ui) {
    if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::None);
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            ui.id().with("rotate-cursor"),
        ));
        let center = pointer + egui::vec2(11., 11.);
        let points: Vec<_> = (0..=24)
            .map(|i| {
                let a = (-140. + i as f32 * 270. / 24.).to_radians();
                center + egui::vec2(a.cos(), a.sin()) * 9.
            })
            .collect();
        let tip = *points.last().unwrap();
        for (width, color) in [(4.0_f32, Color32::BLACK), (2.0_f32, Color32::WHITE)] {
            painter.add(egui::Shape::line(points.clone(), Stroke::new(width, color)));
            painter.add(egui::Shape::line(
                vec![tip + egui::vec2(-1., -6.), tip, tip + egui::vec2(6., -1.)],
                Stroke::new(width, color),
            ));
        }
    }
}

/// Resize around the opposite handle, preserving its image-space position.
fn resize(
    op: &mut Operation,
    local: Rect,
    handle: Vec2,
    delta: Vec2,
    time: Time,
    proportional: bool,
) -> Result<()> {
    let angle = sample(op, "rotation", time)?;
    let scales = egui::vec2(sample(op, "scale_x", time)?, sample(op, "scale_y", time)?);
    let opposite = local.center().to_vec2() - handle * local.size() * 0.5;
    let span = handle * local.size();
    let d = rotate(delta, -angle);
    let mut next = scales;
    if handle.x != 0. {
        next.x += d.x / span.x;
    }
    if handle.y != 0. {
        next.y += d.y / span.y;
    }
    if proportional {
        let ratio = if handle.x != 0. && scales.x.abs() > 1e-6 {
            next.x / scales.x
        } else if scales.y.abs() > 1e-6 {
            next.y / scales.y
        } else {
            1.
        };
        next = scales * ratio;
    }
    let mut edited = op.clone();
    set(&mut edited, "scale_x", next.x, time)?;
    set(&mut edited, "scale_y", next.y, time)?;
    next = egui::vec2(
        sample(&edited, "scale_x", time)?,
        sample(&edited, "scale_y", time)?,
    );
    let shift = rotate(opposite * (scales - next), angle);
    set(&mut edited, "x", sample(op, "x", time)? + shift.x, time)?;
    set(&mut edited, "y", sample(op, "y", time)? + shift.y, time)?;
    *op = edited;
    Ok(())
}

pub fn show(
    ui: &egui::Ui,
    canvas: Rect,
    view: crate::viewer::ViewTransform,
    image: Vec2,
    op: &mut Operation,
    time: Time,
) -> Result<bool> {
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::new("text-box-moving"), false));
    let local = bounds(ui, op, time)?;
    let angle = sample(op, "rotation", time)?;
    let pivot = egui::vec2(sample(op, "x", time)?, sample(op, "y", time)?);
    let scales = egui::vec2(sample(op, "scale_x", time)?, sample(op, "scale_y", time)?);
    let [ox, oy, sx, sy] = view.0;
    let zoom = egui::vec2(
        canvas.width() * sx / image.x,
        canvas.height() * sy / image.y,
    );
    let origin = canvas.min + egui::vec2(canvas.width() * ox, canvas.height() * oy);
    let screen = |p: Vec2| origin + (pivot + rotate(p * scales, angle)) * zoom;
    let painter = ui.painter().with_clip_rect(canvas);
    let stroke = Stroke::new(1.0_f32, crate::design::accent(ui.ctx()));
    let corners = [
        local.left_top(),
        local.right_top(),
        local.right_bottom(),
        local.left_bottom(),
        local.left_top(),
    ];
    painter.add(egui::Shape::line(
        corners.map(|p| screen(p.to_vec2())).to_vec(),
        stroke,
    ));
    let center = screen(local.center().to_vec2());
    let top = screen(egui::vec2(local.center().x, local.top()));
    let outward = (top - center).normalized();
    let outward = if outward == Vec2::ZERO {
        egui::vec2(0., -1.)
    } else {
        outward
    };
    let rotation = top + outward * 28.;
    painter.line_segment([top, rotation], stroke);
    let handles = [
        egui::vec2(-1., -1.),
        egui::vec2(0., -1.),
        egui::vec2(1., -1.),
        egui::vec2(1., 0.),
        egui::vec2(1., 1.),
        egui::vec2(0., 1.),
        egui::vec2(-1., 1.),
        egui::vec2(-1., 0.),
    ];
    let mut changed = false;
    // The whole box moves the text: its border and everything inside it. A double-click
    // inside still edits the words (that is read from the pointer, not from this area).
    let border_id = ui.id().with("text-box-border");
    let pointer = ui.input(|i| {
        if i.pointer.primary_pressed() {
            i.pointer.press_origin()
        } else {
            i.pointer.hover_pos()
        }
    });
    let border_hit = pointer.filter(|p| {
        let on_border = corners.windows(2).any(|edge| {
            let a = screen(edge[0].to_vec2());
            let b = screen(edge[1].to_vec2());
            let span = b - a;
            let t = ((*p - a).dot(span) / span.length_sq().max(0.001)).clamp(0., 1.);
            p.distance(a + span * t) <= 5.
        });
        // Inside the (possibly rotated) box: on the same side of all four edges.
        let turns: Vec<f32> = corners
            .windows(2)
            .map(|edge| {
                let a = screen(edge[0].to_vec2());
                let b = screen(edge[1].to_vec2());
                (b - a).x * (*p - a).y - (b - a).y * (*p - a).x
            })
            .collect();
        let inside = turns.iter().all(|t| *t >= 0.0) || turns.iter().all(|t| *t <= 0.0);
        canvas.contains(*p)
            && (on_border || inside)
            && !handles.iter().any(|h| {
                let point = screen(local.center().to_vec2() + *h * local.size() * 0.5);
                Rect::from_center_size(point, Vec2::splat(20.)).contains(*p)
            })
            && p.distance(rotation) > 12.
            && p.distance(center) > 12.
    });
    let hit = border_hit.map_or(Rect::NOTHING, |p| {
        Rect::from_center_size(p, Vec2::splat(10.)).intersect(canvas)
    });
    let border = crate::handles::interact(ui, hit, border_id)
        .on_hover_text("Move text · drag anywhere on it · double-click to edit the words");
    if border.hovered() || border.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
    }
    if border.dragged() {
        let delta = crate::handles::drag_delta(&border) / zoom;
        if delta != Vec2::ZERO {
            let mut next = op.clone();
            set(&mut next, "x", pivot.x + delta.x, time)?;
            set(&mut next, "y", pivot.y + delta.y, time)?;
            *op = next;
            changed = true;
            ui.ctx()
                .data_mut(|d| d.insert_temp(egui::Id::new("text-box-moving"), true));
        }
    }
    // Move from the centre cross; the interior remains available for double-click text editing.
    for (index, handle) in handles.into_iter().chain([Vec2::ZERO; 2]).enumerate() {
        let point = if index < 8 {
            screen(local.center().to_vec2() + handle * local.size() * 0.5)
        } else if index == 8 {
            rotation
        } else {
            center
        };
        let hit = Rect::from_center_size(point, Vec2::splat(16.)).intersect(canvas);
        if !hit.is_positive() {
            continue;
        }
        let hint = if index == 8 {
            "Rotate text · snaps near 45-degree angles · Alt: free · Shift: 15-degree steps"
        } else if index == 9 {
            "Move text"
        } else {
            "Resize text · opposite handle stays fixed · Shift preserves proportions"
        };
        let response = crate::handles::interact(ui, hit, ui.id().with(("text-box-handle", index)))
            .on_hover_text(hint);
        let cursor = if index < 8 {
            resize_cursor(rotate(handle * scales, angle) * zoom)
        } else if index == 9 {
            egui::CursorIcon::Move
        } else {
            egui::CursorIcon::Crosshair
        };
        if response.hovered() || response.dragged() {
            if index == 8 {
                rotation_cursor(ui);
            } else {
                ui.ctx().set_cursor_icon(cursor);
            }
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), hint)
        });
        if index == 8 {
            painter.circle_filled(point, 4., crate::design::canvas(ui.ctx()));
            painter.circle_stroke(point, 4., stroke);
        } else if index == 9 {
            painter.line_segment(
                [point - egui::vec2(6., 0.), point + egui::vec2(6., 0.)],
                stroke,
            );
            painter.line_segment(
                [point - egui::vec2(0., 6.), point + egui::vec2(0., 6.)],
                stroke,
            );
        } else {
            painter.rect(
                point_rect(point),
                0.,
                Color32::from_rgb(24, 26, 32),
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        let delta = crate::handles::drag_delta(&response) / zoom;
        if response.dragged() && delta != Vec2::ZERO {
            if index < 8 {
                let linked = matches!(op,Operation::Text(t) if t.typography.linked_scale);
                resize(
                    op,
                    local,
                    handle,
                    delta,
                    time,
                    linked || ui.input(|i| i.modifiers.shift),
                )?;
            } else if index == 9 {
                ui.ctx()
                    .data_mut(|d| d.insert_temp(egui::Id::new("text-box-moving"), true));
                let mut next = op.clone();
                set(&mut next, "x", pivot.x + delta.x, time)?;
                set(&mut next, "y", pivot.y + delta.y, time)?;
                *op = next;
            } else if let Some(pointer) = response.interact_pointer_pos() {
                let after = (pointer - origin) / zoom - pivot;
                let before = after - delta;
                if before.length_sq() > 1. && after.length_sq() > 1. {
                    let delta = (before.x * after.y - before.y * after.x)
                        .atan2(before.dot(after))
                        .to_degrees();
                    let target = crate::transform_handles::rotation_drag(&response, angle, delta);
                    set(op, "rotation", target, time)?;
                }
            }
            changed = true;
        }
        if response.drag_stopped() {
            ui.ctx()
                .data_mut(|d| d.remove::<f32>(response.id.with("raw-rotation")));
        }
    }
    Ok(changed)
}
fn point_rect(p: egui::Pos2) -> Rect {
    Rect::from_center_size(p, Vec2::splat(7.))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text() -> Operation {
        veylvik_document::presets::showcase::build("gold_text")
            .unwrap()
            .composition
            .nodes
            .into_values()
            .find(|n| matches!(n.operation, Operation::Text(_)))
            .unwrap()
            .operation
    }
    fn mapped(op: &Operation, p: Vec2, t: Time) -> Vec2 {
        egui::vec2(sample(op, "x", t).unwrap(), sample(op, "y", t).unwrap())
            + rotate(
                p * egui::vec2(
                    sample(op, "scale_x", t).unwrap(),
                    sample(op, "scale_y", t).unwrap(),
                ),
                sample(op, "rotation", t).unwrap(),
            )
    }
    #[test]
    fn opposite_corner_stays_fixed_when_rotated_mirrored_and_keyed() {
        let time = Time::new(1, 1).unwrap();
        let mut op = text();
        *op.scalar_mut("rotation").unwrap() = 37.0.into();
        *op.scalar_mut("scale_x").unwrap() = (-1.5).into();
        op.scalar_mut("scale_y")
            .unwrap()
            .set_key_at(Time::new(0, 1).unwrap(), 2.)
            .unwrap();
        let local = Rect::from_min_max(egui::pos2(-300., -70.), egui::pos2(200., 40.));
        let anchor = mapped(&op, local.left_top().to_vec2(), time);
        resize(
            &mut op,
            local,
            Vec2::splat(1.),
            egui::vec2(50., 20.),
            time,
            true,
        )
        .unwrap();
        assert!((mapped(&op, local.left_top().to_vec2(), time) - anchor).length() < 0.001);
        assert!(
            (sample(&op, "scale_x", time).unwrap() / sample(&op, "scale_y", time).unwrap() + 0.75)
                .abs()
                < 1e-5
        );
        assert_eq!(op.scalar("scale_y").unwrap().keys().unwrap().len(), 2);
        assert_eq!(
            op.scalar("scale_y")
                .unwrap()
                .at(Time::new(0, 1).unwrap())
                .unwrap(),
            2.
        );
    }
    #[test]
    fn side_handle_changes_only_its_axis_when_unlinked() {
        let time = Time::new(0, 1).unwrap();
        let mut op = text();
        let local = Rect::from_min_max(egui::pos2(-100., -60.), egui::pos2(100., 0.));
        let before = sample(&op, "scale_y", time).unwrap();
        let anchor = mapped(&op, egui::vec2(-100., -30.), time);
        resize(
            &mut op,
            local,
            egui::vec2(1., 0.),
            egui::vec2(100., 20.),
            time,
            false,
        )
        .unwrap();
        assert_eq!(sample(&op, "scale_y", time).unwrap(), before);
        assert!((mapped(&op, egui::vec2(-100., -30.), time) - anchor).length() < 0.001);
    }
}
