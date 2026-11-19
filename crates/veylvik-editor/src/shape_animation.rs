//! Shape animation uses the same key controls and seek messages as scalar animation.
use eframe::egui;
use veylvik_document::{
    Key, Scalar, Time,
    vector::{PathCommand, PathMask},
};
pub fn controls(
    ui: &mut egui::Ui,
    geometry: &mut PathMask,
    time: Time,
) -> veylvik_document::Result<bool> {
    let mut changed = false;
    let mut enabled = !geometry.keys.is_empty();
    let mut result = Ok(());
    ui.horizontal(|ui| {
        if ui.checkbox(&mut enabled, "Animate shape").changed() {
            result = if enabled {
                geometry
                    .at(time)
                    .and_then(|shape| geometry.set_key(time, &shape))
            } else {
                geometry.at(time).map(|shape| *geometry = shape)
            };
            changed = true;
        }
        let here = geometry
            .keys
            .iter()
            .any(|k| k.time.compare(time).is_ok_and(|o| o.is_eq()));
        let mut proxy = if geometry.keys.is_empty() {
            Scalar::from(0.0)
        } else {
            Scalar::Linear {
                keys: geometry
                    .keys
                    .iter()
                    .map(|k| Key {
                        time: k.time,
                        value: 0.0,
                        segment: None,
                    })
                    .collect(),
            }
        };
        if crate::animation::key_controls(ui, &mut proxy, time) {
            result = if here {
                geometry.remove_key(time)
            } else {
                geometry
                    .at(time)
                    .and_then(|shape| geometry.set_key(time, &shape))
            };
            changed = true;
        }
    });
    result?;
    if !geometry.keys.is_empty() {
        ui.weak("Point and transform edits set a shape key at this frame. Disable animation to change point count or redraw.");
        let id = egui::Id::new("shape-ghosts");
        let mut ghosts = ui
            .ctx()
            .data_mut(|d| d.get_temp::<bool>(id).unwrap_or(false));
        ui.checkbox(&mut ghosts, "Neighbouring key outlines")
            .on_hover_text("Previous key: blue. Next key: violet.");
        ui.ctx().data_mut(|d| d.insert_temp(id, ghosts));
    }
    Ok(changed)
}
pub fn edit(
    ui: &mut egui::Ui,
    geometry: &mut PathMask,
    time: Time,
) -> veylvik_document::Result<bool> {
    let mut changed = controls(ui, geometry, time)?;
    let mut sampled = geometry.at(time)?;
    if crate::vector_editor::edit(ui, &mut sampled, !geometry.keys.is_empty()) {
        geometry.edit_at(time, sampled)?;
        changed = true;
    }
    Ok(changed)
}
pub fn ghosts(
    ui: &egui::Ui,
    canvas: egui::Rect,
    view: crate::viewer::ViewTransform,
    image: egui::Vec2,
    geometry: &PathMask,
    time: Time,
) {
    if !ui.ctx().data_mut(|d| {
        d.get_temp::<bool>(egui::Id::new("shape-ghosts"))
            .unwrap_or(false)
    }) {
        return;
    }
    let previous = geometry
        .keys
        .iter()
        .rev()
        .find(|k| k.time.compare(time).is_ok_and(|o| o.is_lt()));
    let next = geometry
        .keys
        .iter()
        .find(|k| k.time.compare(time).is_ok_and(|o| o.is_gt()));
    let [ox, oy, sx, sy] = view.0;
    let origin = canvas.min + egui::vec2(canvas.width() * ox, canvas.height() * oy);
    let scale = egui::vec2(
        canvas.width() * sx / image.x,
        canvas.height() * sy / image.y,
    );
    let screen = |p: [f32; 2]| origin + egui::vec2(p[0], p[1]) * scale;
    let painter = ui.painter().with_clip_rect(canvas);
    for (key, color) in [
        (previous, egui::Color32::from_rgb(65, 135, 195)),
        (next, egui::Color32::from_rgb(160, 100, 195)),
    ] {
        let Some(key) = key else {
            continue;
        };
        let stroke = egui::Stroke::new(1.0_f32, color);
        let mut first = [0.0; 2];
        let mut last = first;
        for command in &key.commands {
            match *command {
                PathCommand::Move { to } => {
                    first = to;
                    last = to;
                }
                PathCommand::Line { to } => {
                    painter.line_segment([screen(last), screen(to)], stroke);
                    last = to;
                }
                PathCommand::Quad { control, to } => {
                    painter.add(egui::epaint::QuadraticBezierShape::from_points_stroke(
                        [screen(last), screen(control), screen(to)],
                        false,
                        egui::Color32::TRANSPARENT,
                        stroke,
                    ));
                    last = to;
                }
                PathCommand::Cubic {
                    control1,
                    control2,
                    to,
                } => {
                    painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                        [screen(last), screen(control1), screen(control2), screen(to)],
                        false,
                        egui::Color32::TRANSPARENT,
                        stroke,
                    ));
                    last = to;
                }
                PathCommand::Close {} => {
                    painter.line_segment([screen(last), screen(first)], stroke);
                }
            }
        }
    }
}
