//! Scalar authoring controls; edits remain in the inspector's transaction draft.
use eframe::egui;
use veylvik_document::{Interpolation, Scalar, Time, Track3, animation::MAX_KEYS};

/// Shared compact key controls. Navigation is consumed by Studio after committing drafts.
pub fn key_controls(ui: &mut egui::Ui, scalar: &mut Scalar, time: Time) -> bool {
    let times: Vec<Time> = scalar
        .keys()
        .map(|keys| keys.iter().map(|key| key.time).collect())
        .unwrap_or_default();
    let Some(here) = key_strip(ui, &times, scalar.keys().is_some(), time) else {
        return false;
    };
    crate::autokey::asked();
    let constant = scalar.keys().is_none();
    let result = if here {
        scalar.remove_key_at(time)
    } else {
        let added = scalar
            .at(time)
            .and_then(|value| scalar.set_key_at(time, value));
        if added.is_ok() {
            crate::autokey::first_key(scalar, constant);
        }
        added
    };
    report(ui, result)
}
/// Key controls for a whole colour or vector: one key holds all three channels.
pub fn track_key_controls(ui: &mut egui::Ui, track: &mut Track3, time: Time) -> bool {
    let times: Vec<Time> = track.keys().iter().map(|key| key.time).collect();
    let Some(here) = key_strip(ui, &times, track.is_animated(), time) else {
        return false;
    };
    crate::autokey::asked();
    let result = if here {
        track.remove_key_at(time)
    } else {
        track
            .at(time)
            .and_then(|value| track.set_key_at(time, value))
    };
    report(ui, result)
}
fn report(ui: &mut egui::Ui, result: veylvik_document::Result<()>) -> bool {
    match result {
        Ok(()) => true,
        Err(error) => {
            ui.colored_label(egui::Color32::LIGHT_RED, error.to_string());
            false
        }
    }
}
/// Previous key, the key diamond and next key for a property keyed at `times`.
/// Returns Some(key is at this frame) when the diamond was clicked.
/// The key mark with previous/next key buttons; Some(whether a key is at `time`) when
/// the mark is clicked.
pub(crate) fn key_strip(
    ui: &mut egui::Ui,
    times: &[Time],
    animated: bool,
    time: Time,
) -> Option<bool> {
    let here = times
        .iter()
        .any(|t| t.compare(time).is_ok_and(|o| o.is_eq()));
    let previous = times
        .iter()
        .rev()
        .find(|t| t.compare(time).is_ok_and(|o| o.is_lt()))
        .copied();
    let next = times
        .iter()
        .find(|t| t.compare(time).is_ok_and(|o| o.is_gt()))
        .copied();
    if ui
        .add_enabled(
            previous.is_some(),
            crate::icons::Button::new(crate::icons::Icon::Left, "Previous key"),
        )
        .on_hover_text("Previous key for this parameter")
        .clicked()
    {
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("parameter-key-seek"), previous.unwrap()));
    }
    let colour = if here {
        egui::Color32::from_rgb(239, 99, 109)
    } else if animated {
        egui::Color32::from_rgb(210, 165, 90)
    } else {
        ui.visuals().text_color()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 20.0), egui::Sense::click());
    #[cfg(test)]
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::new("key-toggle-test"), rect));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            if here {
                "Remove key at current frame"
            } else {
                "Add key at current frame"
            },
        )
    });
    let visuals = ui.style().interact(&response);
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, visuals.bg_fill);
    }
    let c = rect.center();
    let points = vec![
        c + egui::vec2(0.0, -5.0),
        c + egui::vec2(5.0, 0.0),
        c + egui::vec2(0.0, 5.0),
        c + egui::vec2(-5.0, 0.0),
    ];
    ui.painter().add(egui::Shape::convex_polygon(
        points,
        if here {
            colour
        } else {
            egui::Color32::TRANSPARENT
        },
        egui::Stroke::new(1.4_f32, colour),
    ));
    let clicked = response
        .on_hover_text(if here {
            "Remove key at this frame"
        } else {
            "Add key at this frame"
        })
        .clicked();
    if ui
        .add_enabled(
            next.is_some(),
            crate::icons::Button::new(crate::icons::Icon::Right, "Next key"),
        )
        .on_hover_text("Next key for this parameter")
        .clicked()
    {
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("parameter-key-seek"), next.unwrap()));
    }
    clicked.then_some(here)
}
/// A keyable colour (linear RGB) with its key controls.
pub fn colour_track(ui: &mut egui::Ui, label: &str, track: &mut Track3, time: Time) -> bool {
    let mut changed = false;
    ui.push_id(label, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.;
            ui.label(label);
            // egui's colour button takes and returns linear RGB.
            if let Ok(mut value) = track.at(time)
                && ui
                    .color_edit_button_rgb(&mut value)
                    .on_hover_text(format!("{label} colour"))
                    .changed()
            {
                changed |= report(ui, track.set_at(time, value.map(|v| v.clamp(0., 1.))));
            }
            changed |= track_key_controls(ui, track, time);
        });
    });
    changed
}
/// A keyable 3D vector as three number fields with one set of key controls.
pub fn vector_track(
    ui: &mut egui::Ui,
    label: &str,
    names: [&str; 3],
    track: &mut Track3,
    range: std::ops::RangeInclusive<f32>,
    time: Time,
) -> bool {
    let mut changed = false;
    ui.push_id(label, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.;
            ui.add_sized([62., 20.], egui::Label::new(label).truncate());
            if let Ok(mut value) = track.at(time) {
                let mut edited = false;
                for (name, v) in names.iter().zip(&mut value) {
                    edited |= ui
                        .add(
                            egui::DragValue::new(v)
                                .prefix(format!("{name} "))
                                .speed(0.02)
                                .range(range.clone()),
                        )
                        .changed();
                }
                if edited {
                    changed |= report(ui, track.set_at(time, value));
                }
            }
            changed |= track_key_controls(ui, track, time);
        });
    });
    changed
}

pub fn scalar(
    ui: &mut egui::Ui,
    label: &str,
    scalar: &mut Scalar,
    range: std::ops::RangeInclusive<f32>,
    time: Time,
) -> bool {
    scalar_with_default(ui, label, scalar, range, time, None)
}

pub fn scalar_with_default(
    ui: &mut egui::Ui,
    label: &str,
    scalar: &mut Scalar,
    range: std::ops::RangeInclusive<f32>,
    time: Time,
    default: Option<f32>,
) -> bool {
    if !crate::inspector_tools::shown(ui.ctx(), label) {
        return false;
    }
    let mut changed = false;
    ui.push_id(label, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.;
            // An animated setting is named in the accent colour, brighter on a keyframe.
            let name = match scalar.keys() {
                Some(keys) => {
                    let on_key = keys
                        .iter()
                        .any(|key| key.time.compare(time).is_ok_and(|order| order.is_eq()));
                    let accent = crate::design::accent(ui.ctx());
                    egui::RichText::new(label).color(if on_key {
                        accent
                    } else {
                        accent.gamma_multiply(0.75)
                    })
                }
                None => egui::RichText::new(label),
            };
            ui.add_sized([88., 28.], egui::Label::new(name).truncate())
                .on_hover_text(match scalar.keys().map(<[_]>::len) {
                    Some(count) => format!("{label}\nAnimated: {count} keyframe(s)"),
                    None => label.to_owned(),
                });
            if matches!(scalar, Scalar::Expression { .. }) {
                changed |= crate::expressions::row(ui, scalar, time);
            } else if let Ok(mut value) = scalar.at(time) {
                let old = value;
                let mut edited = false;
                if ui.available_width() > 220. {
                    ui.spacing_mut().slider_width = (ui.available_width()
                        - crate::design::NUMBER_WIDTH
                        - (if default.is_some() { 5. } else { 4. }) * 25.
                        - 6.)
                        .max(24.);
                    edited |= crate::design::slider(ui, &mut value, range.clone()).changed();
                }
                edited |= crate::design::number(ui, &mut value, range.clone()).changed();
                if edited {
                    changed |= crate::handles::edit(
                        scalar,
                        time,
                        value - old,
                        *range.start(),
                        *range.end(),
                    )
                    .is_ok();
                }
            }
            if !matches!(scalar, Scalar::Expression { .. }) {
                changed |= key_controls(ui, scalar, time);
            }
            if let Some(default) = default
                && ui
                    .add(crate::icons::Button::new(
                        crate::icons::Icon::Reset,
                        "Reset to default",
                    ))
                    .on_hover_text("Reset to default at this frame; preserve other keys")
                    .clicked()
            {
                if scalar.keys().is_some() {
                    crate::autokey::asked();
                    changed |= scalar.set_key_at(time, default).is_ok();
                } else {
                    *scalar = default.into();
                    changed = true;
                }
            }
            egui::Popup::menu(&ui.add(crate::icons::Button::new(
                crate::icons::Icon::More,
                "Interpolation and keyframe details",
            )))
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                changed |= details(ui, label, scalar, range.clone(), time);
            });
        });
    });
    changed
}

pub fn details(
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
                    for choice in [Interpolation::Linear, Interpolation::Step, Interpolation::Smooth, Interpolation::Monotone] {
                        ui.selectable_value(&mut mode, choice, mode_label(choice));
                    }
                }).response.on_hover_text("Set interpolation for the entire parameter; replaces per-segment easing when applied.");
            if mode != before {
                scalar.set_interpolation(mode);
                changed = true;
            }
        }
        match scalar {
            Scalar::Constant { value } => {
                changed |= ui.add(egui::DragValue::new(value).speed(0.1).range(range.clone())).changed();
            }
            Scalar::Expression { .. } => {
                ui.horizontal(|ui| changed |= crate::expressions::row(ui, scalar, time));
            }
            _ => {
                let key_count = scalar.keys().unwrap().len();
                match &value {
                    Ok(value) => {
                        ui.weak(format!("{key_count} keyframes"));
                        let mut edited = *value;
                        let can_edit = key_count < MAX_KEYS || scalar.keys().unwrap().iter().any(|key| key.time.compare(time).is_ok_and(|order| order.is_eq()));
                        ui.horizontal_wrapped(|ui| {
                            ui.label("At playhead");
                            if ui.add_enabled(can_edit, egui::DragValue::new(&mut edited).speed(0.1).range(range.clone()))
                                .on_hover_text("Change or add a key at the current frame. Apply commits the edit.").changed() {
                                crate::autokey::asked();
                                match scalar.set_key_at(time, edited) {
                                    Ok(()) => changed = true,
                                    Err(error) => { ui.colored_label(egui::Color32::LIGHT_RED, error.to_string()); }
                                }
                            }
                        });
                    }
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
        changed |= crate::expressions::section(ui, scalar, time);
        if let Ok(value) = scalar.at(time)
            && !matches!(scalar, Scalar::Expression { .. })
        {
            ui.horizontal_wrapped(|ui| {
                let can_add = scalar.keys().is_none_or(|keys| keys.len() < MAX_KEYS && !keys.iter().any(|key| key.time.compare(time).is_ok_and(|order| order.is_eq())));
                if ui.add_enabled(can_add, egui::Button::new("Add key at playhead").small()).clicked() {
                    crate::autokey::asked();
                    let constant = scalar.keys().is_none();
                    match scalar.set_key_at(time, value) {
                        Ok(()) => {
                            crate::autokey::first_key(scalar, constant);
                            changed = true;
                        }
                        Err(error) => { ui.colored_label(egui::Color32::LIGHT_RED, error.to_string()); }
                    }
                }
                let at_key = scalar.keys().is_some_and(|keys| keys.iter().any(|key| key.time.compare(time).is_ok_and(|order| order.is_eq())));
                if scalar.keys().is_some() && ui.add_enabled(at_key, egui::Button::new("Remove key at playhead").small()).clicked() {
                    match scalar.remove_key_at(time) {
                        Ok(()) => changed = true,
                        Err(error) => { ui.colored_label(egui::Color32::LIGHT_RED, error.to_string()); }
                    }
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
pub(super) fn mode_label(mode: Interpolation) -> &'static str {
    match mode {
        Interpolation::Linear => "Linear",
        Interpolation::Step => "Hold",
        Interpolation::Smooth => "Ease in/out",
        Interpolation::Monotone => "Auto smooth",
    }
}

/// Scene-linear HDR color controls shared by solids and color mapping.
pub fn color(ui: &mut egui::Ui, color: &mut [f32; 4]) -> bool {
    let mut changed = false;
    // The widget performs a color-space round trip even without interaction.
    // Keep it isolated from authored HDR, negative and zero-alpha RGB values.
    let mut picker = [color[0], color[1], color[2]];
    for value in &mut picker {
        *value = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
    ui.horizontal(|ui| {
        ui.label("Pick color");
        if ui.color_edit_button_rgb(&mut picker)
            .on_hover_text("Pick standard colors in the 0–1 range. Alpha stays unchanged. Use numeric controls for HDR or negative RGB.")
            .changed() {
            color[..3].copy_from_slice(&picker);
            changed = true;
        }
    });
    for (index, label) in ["Red", "Green", "Blue", "Alpha"].iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(*label);
            let mut value = egui::DragValue::new(&mut color[index]).speed(0.01);
            if index == 3 {
                value = value.range(0.0..=1.0);
            }
            changed |= ui.add(value).changed();
        });
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diamond_adds_and_removes_only_the_current_key() {
        let ctx = egui::Context::default();
        let time = Time::new(1, 1).unwrap();
        let mut scalar: Scalar = 12.0.into();
        scalar.set_key_at(Time::new(0, 1).unwrap(), 4.0).unwrap();
        let mut run = |events| {
            let mut changed = false;
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(500.0, 200.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.horizontal(|ui| {
                        changed = key_controls(ui, &mut scalar, time);
                    });
                },
            );
            (changed, scalar.clone())
        };
        run(vec![]);
        let rect = ctx.data_mut(|d| {
            d.get_temp::<egui::Rect>(egui::Id::new("key-toggle-test"))
                .unwrap()
        });
        for expected in [2, 1] {
            for pressed in [true, false] {
                let (changed, value) = run(vec![
                    egui::Event::PointerMoved(rect.center()),
                    egui::Event::PointerButton {
                        pos: rect.center(),
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
                assert_eq!(changed, !pressed);
                if !pressed {
                    assert_eq!(value.keys().unwrap().len(), expected);
                    assert_eq!(value.keys().unwrap()[0].value, 4.0);
                }
            }
        }
    }
}
