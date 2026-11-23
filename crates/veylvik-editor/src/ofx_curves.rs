//! The curve editor for an OpenFX curve setting: one curve at a time over the plugin's key range, drawn with the host's own evaluation so what is seen is what the plugin gets; drag a point, double-click to add one, right-click to remove one.
use crate::nodes::InspectCx;
use eframe::egui;
use veylvik_document::nodes::{OfxEffect, OfxSetting};

/// The curves as stored, else the plugin's defaults.
fn current(effect: &OfxEffect, param: &veylvik_ofx::Param) -> Vec<Vec<[f64; 2]>> {
    match effect.settings.get(&param.name) {
        Some(OfxSetting::Curves(c)) => c
            .iter()
            .map(|curve| {
                curve
                    .iter()
                    .map(|[k, v]| [f64::from(*k), f64::from(*v)])
                    .collect()
            })
            .collect(),
        _ => param.curves.clone(),
    }
}
fn store(effect: &mut OfxEffect, name: &str, curves: &[Vec<[f64; 2]>]) {
    let curves = curves
        .iter()
        .map(|curve| curve.iter().map(|[k, v]| [*k as f32, *v as f32]).collect())
        .collect();
    effect
        .settings
        .insert(name.to_string(), OfxSetting::Curves(curves));
}
pub(crate) fn editor(
    ui: &mut egui::Ui,
    effect: &mut OfxEffect,
    param: &veylvik_ofx::Param,
    cx: &mut InspectCx<'_>,
) {
    let mut curves = current(effect, param);
    if curves.is_empty() {
        curves.push(Vec::new());
    }
    let chosen_id = egui::Id::new(("ofx-curve", cx.node_id, &param.name));
    let mut chosen: usize = ui
        .data(|d| d.get_temp(chosen_id))
        .unwrap_or(0)
        .min(curves.len() - 1);
    let name = |k: usize| {
        param
            .curve_labels
            .get(k)
            .cloned()
            .unwrap_or_else(|| format!("curve {}", k + 1))
    };
    ui.horizontal(|ui| {
        ui.label(&param.label)
            .on_hover_text(if param.hint.is_empty() {
                &param.name
            } else {
                &param.hint
            });
        if curves.len() > 1 {
            egui::ComboBox::from_id_salt(chosen_id)
                .selected_text(name(chosen))
                .show_ui(ui, |ui| {
                    for k in 0..curves.len() {
                        ui.selectable_value(&mut chosen, k, name(k));
                    }
                });
        }
        if effect.settings.contains_key(&param.name)
            && ui
                .small_button("Default")
                .on_hover_text("Back to the plugin's curves")
                .clicked()
        {
            effect.settings.remove(&param.name);
            cx.dirty = true;
        }
    });
    ui.data_mut(|d| d.insert_temp(chosen_id, chosen));
    let [low, high] = param.range;
    // Values shown from the lowest to the highest of all curves, at least 0 to 1.
    let values = curves.iter().flatten().map(|p| p[1]);
    let bottom = values.clone().fold(0.0f64, f64::min) - 0.05;
    let top = values.fold(1.0f64, f64::max) + 0.05;
    let size = egui::vec2(ui.available_width().min(320.0), 150.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    painter.rect_filled(rect, 3.0, visuals.extreme_bg_color);
    let to_screen = |[k, v]: [f64; 2]| {
        egui::pos2(
            rect.left() + ((k - low) / (high - low)) as f32 * rect.width(),
            rect.bottom() - ((v - bottom) / (top - bottom)) as f32 * rect.height(),
        )
    };
    let from_screen = |p: egui::Pos2| {
        [
            low + f64::from((p.x - rect.left()) / rect.width()) * (high - low),
            bottom + f64::from((rect.bottom() - p.y) / rect.height()) * (top - bottom),
        ]
    };
    for line in [0.0, 1.0] {
        if (bottom..=top).contains(&line) {
            let y = to_screen([low, line]).y;
            painter.hline(
                rect.x_range(),
                y,
                egui::Stroke::new(1.0_f32, visuals.weak_text_color().gamma_multiply(0.4)),
            );
        }
    }
    // The other curves faintly, the chosen one in full.
    for (k, curve) in curves.iter().enumerate() {
        let samples: Vec<egui::Pos2> = (0..=96)
            .map(|i| {
                let key = low + (high - low) * f64::from(i) / 96.0;
                to_screen([key, veylvik_ofx::curve_value(curve, key)])
            })
            .collect();
        let colour = if k == chosen {
            visuals.selection.bg_fill
        } else {
            visuals.weak_text_color().gamma_multiply(0.5)
        };
        painter.add(egui::Shape::line(
            samples,
            egui::Stroke::new(if k == chosen { 2.0_f32 } else { 1.0 }, colour),
        ));
    }
    let points = &mut curves[chosen];
    let near = |p: egui::Pos2, points: &[[f64; 2]]| {
        points.iter().position(|q| to_screen(*q).distance(p) < 8.0)
    };
    let drag_id = chosen_id.with("drag");
    let mut changed = false;
    // The point under where the press began: by the time a drag is noticed the pointer
    // may have moved well away from it.
    if response.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
    {
        ui.data_mut(|d| d.insert_temp(drag_id, near(p, points)));
    }
    if response.dragged()
        && let Some(Some(i)) = ui.data(|d| d.get_temp::<Option<usize>>(drag_id))
        && let Some(p) = response.interact_pointer_pos()
        && i < points.len()
    {
        // A point stays between its neighbours and within the range.
        let [mut key, value] = from_screen(p);
        let before = if i > 0 { points[i - 1][0] } else { low };
        let after = points.get(i + 1).map_or(high, |q| q[0]);
        key = key.clamp(before, after);
        points[i] = [key, value];
        changed = true;
    }
    if response.drag_stopped() {
        ui.data_mut(|d| d.remove::<Option<usize>>(drag_id));
    }
    if response.double_clicked()
        && let Some(p) = response.interact_pointer_pos()
        && near(p, points).is_none()
        && points.len() < 256
    {
        let [key, _] = from_screen(p);
        let key = key.clamp(low, high);
        let at = points.partition_point(|q| q[0] < key);
        points.insert(at, [key, veylvik_ofx::curve_value(points, key)]);
        changed = true;
    }
    if response.secondary_clicked()
        && let Some(p) = response.interact_pointer_pos()
        && let Some(i) = near(p, points)
    {
        points.remove(i);
        changed = true;
    }
    for q in points.iter() {
        painter.circle(
            to_screen(*q),
            4.0,
            visuals.selection.bg_fill,
            egui::Stroke::new(1.0_f32, visuals.strong_text_color()),
        );
    }
    response
        .on_hover_text("Drag a point; double-click to add one; right-click a point to remove it");
    if changed {
        store(effect, &param.name, &curves);
        cx.dirty = true;
    }
}
