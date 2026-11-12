//! Scalar authoring controls; edits remain in the inspector's transaction draft.
use eframe::egui;
use veylvik_document::{Interpolation, Key, Scalar, Time, animation::MAX_KEYS};

pub fn scalar(
    ui: &mut egui::Ui,
    label: &str,
    scalar: &mut Scalar,
    range: std::ops::RangeInclusive<f32>,
    time: Time,
) -> bool {
    let mut changed = false;
    ui.push_id(label, |ui| {
        ui.label(label);
        let value = scalar.at(time);
        if let Some(mut mode) = scalar.interpolation() {
            let before = mode;
            egui::ComboBox::from_id_salt("interpolation")
                .selected_text(mode_label(mode))
                .show_ui(ui, |ui| {
                    for choice in [Interpolation::Linear, Interpolation::Step, Interpolation::Smooth] {
                        ui.selectable_value(&mut mode, choice, mode_label(choice));
                    }
                });
            if mode != before {
                scalar.set_interpolation(mode);
                changed = true;
            }
        }
        match scalar {
            Scalar::Constant { value } => {
                changed |= ui.add(egui::DragValue::new(value).speed(0.1).range(range.clone())).changed();
            }
            _ => {
                let key_count = scalar.keys_mut().unwrap().len();
                match &value {
                    Ok(value) => { ui.weak(format!("{key_count} keyframes · {value:.3} now")); }
                    Err(error) => { ui.colored_label(egui::Color32::LIGHT_RED, error.to_string()); }
                }
                let mut remove = None;
                egui::CollapsingHeader::new(format!("Edit {label} keys")).show(ui, |ui| {
                    crate::curve_preview::show(ui, scalar, time);
                    let keys = scalar.keys_mut().unwrap();
                    egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                        for (index, key) in keys.iter_mut().enumerate() {
                            ui.push_id(index, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    changed |= ui.add(egui::DragValue::new(&mut key.time.ticks)).changed();
                                    ui.label("/");
                                    changed |= ui.add(egui::DragValue::new(&mut key.time.rate).range(1..=u32::MAX)).changed();
                                    ui.label("s");
                                    changed |= ui.add(egui::DragValue::new(&mut key.value).speed(0.1).range(range.clone())).changed();
                                    if ui.small_button("Remove").clicked() { remove = Some(index); }
                                });
                            });
                        }
                    });
                });
                if let Some(index) = remove {
                    let keys = scalar.keys_mut().unwrap();
                    let removed = keys.remove(index);
                    if keys.is_empty() { *scalar = removed.value.into(); }
                    changed = true;
                }
            }
        }
        if let Ok(value) = scalar.at(time) {
            ui.horizontal_wrapped(|ui| {
                let can_add = scalar.keys_mut().is_none_or(|keys| keys.len() < MAX_KEYS && !keys.iter().any(|key| key.time.compare(time).is_ok_and(|order| order.is_eq())));
                if ui.add_enabled(can_add, egui::Button::new("Add key at playhead").small()).clicked() {
                    if let Some(keys) = scalar.keys_mut() {
                        keys.push(Key { time, value });
                        keys.sort_by(|a, b| a.time.compare(b.time).expect("validated key time"));
                    } else {
                        *scalar = Scalar::Linear { keys: vec![Key { time, value }] };
                    }
                    changed = true;
                }
                if scalar.interpolation().is_some() && ui.small_button("Make constant here").on_hover_text("Replace the animation with its value at the playhead. Apply commits the change; undo restores it.").clicked() {
                    *scalar = value.into();
                    changed = true;
                }
            });
        }
    });
    changed
}
fn mode_label(mode: Interpolation) -> &'static str {
    match mode {
        Interpolation::Linear => "Linear",
        Interpolation::Step => "Hold",
        Interpolation::Smooth => "Ease in/out",
    }
}
