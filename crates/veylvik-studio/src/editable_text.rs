//! Tabbed typography inspector with shared document defaults and animation edits.
use eframe::egui;
use veylvik_document::{Operation, Scalar, TextAlignment, TextFont, Time};
#[derive(Clone, Copy, Default, PartialEq)]
enum Tab {
    #[default]
    Text,
    Layout,
    Transform,
    Style,
    Animation,
}
pub(super) fn body_id(ui: &egui::Ui) -> u8 {
    tab(ui) as u8
}
fn tab(ui: &egui::Ui) -> Tab {
    ui.ctx().data_mut(|d| {
        d.get_temp(egui::Id::new("text-inspector-tab"))
            .unwrap_or_default()
    })
}
fn parameter(
    ui: &mut egui::Ui,
    op: &mut Operation,
    name: &str,
    label: &str,
    range: std::ops::RangeInclusive<f32>,
    time: Time,
) -> bool {
    let descriptor = op.descriptor();
    let spec = descriptor
        .parameters
        .iter()
        .find(|p| p.name == name)
        .unwrap();
    let default: Scalar =
        serde_json::from_value(descriptor.default_operation()[name].clone()).unwrap();
    let value = op.scalar_mut(name).unwrap();
    let mut changed = false;
    ui.push_id(name, |ui| {
        ui.horizontal(|ui| {
            ui.add_sized(
                [(ui.available_width() - 145.0).max(64.0), 22.0],
                egui::Label::new(label),
            );
            if ui
                .small_button("↺")
                .on_hover_text("Restore the default at this frame; keep other animation keys.")
                .clicked()
            {
                match reset(value, time, default.at(time).unwrap()) {
                    Ok(()) => changed = true,
                    Err(e) => {
                        ui.colored_label(egui::Color32::LIGHT_RED, e.to_string());
                    }
                }
            }
            changed |= crate::animation::key_controls(ui, value, time);
            egui::Popup::menu(
                &ui.small_button("...")
                    .on_hover_text("Interpolation and keyframe details"),
            )
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                changed |= crate::animation::details(
                    ui,
                    label,
                    value,
                    spec.min.unwrap()..=spec.max.unwrap(),
                    time,
                );
            });
        });
        if let Ok(mut current) = value.at(time) {
            let before = current;
            ui.horizontal(|ui| {
                ui.spacing_mut().slider_width = (ui.available_width() - 88.0).max(60.0);
                let slider = ui.add(
                    egui::Slider::new(&mut current, range)
                        .show_value(false)
                        .clamping(egui::SliderClamping::Never),
                );
                let number = ui.add(
                    egui::DragValue::new(&mut current)
                        .speed(0.1)
                        .range(spec.min.unwrap()..=spec.max.unwrap()),
                );
                if slider.changed() || number.changed() {
                    match crate::handles::edit(
                        value,
                        time,
                        current - before,
                        spec.min.unwrap(),
                        spec.max.unwrap(),
                    ) {
                        Ok(()) => changed = true,
                        Err(e) => {
                            ui.colored_label(egui::Color32::LIGHT_RED, e.to_string());
                        }
                    }
                }
            });
        }
    });
    changed
}
fn reset(value: &mut Scalar, time: Time, default: f32) -> veylvik_document::Result<()> {
    if value.keys().is_some() {
        value.set_key_at(time, default)
    } else {
        *value = default.into();
        Ok(())
    }
}
pub fn header(ui: &mut egui::Ui, op: &mut Operation) -> bool {
    let Operation::Text {
        content,
        typography,
        ..
    } = op
    else {
        return false;
    };
    let before = content.clone();
    let changed = egui::ScrollArea::vertical()
        .id_salt("text-content-scroll")
        .max_height(100.0)
        .show(ui, |ui| {
            let output = egui::TextEdit::multiline(content)
                .id_salt("inspector-text-content")
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .char_limit(4096)
                .show(ui);
            if let Some(range) = output.cursor_range {
                let [a, b] = range.sorted_cursors();
                if a.index != b.index {
                    let byte = |i| {
                        content
                            .char_indices()
                            .nth(i)
                            .map_or(content.len(), |(b, _)| b)
                    };
                    ui.ctx().data_mut(|d| {
                        d.insert_temp(
                            egui::Id::new("text-selection"),
                            (byte(a.index), byte(b.index)),
                        )
                    });
                }
            }
            output.response.changed()
        })
        .inner;
    if changed {
        typography.remap(&before, content);
    }
    ui.small("Live preview · edits commit when you finish · Undo restores them");
    let mut selected = tab(ui);
    ui.horizontal_wrapped(|ui| {
        for (value, label) in [
            (Tab::Text, "Text"),
            (Tab::Layout, "Layout"),
            (Tab::Transform, "Transform"),
            (Tab::Style, "Style"),
            (Tab::Animation, "Animation"),
        ] {
            let response = ui.add(
                egui::Button::new(label)
                    .selected(selected == value)
                    .min_size(egui::vec2(0.0, 28.0)),
            );
            if response.clicked() {
                selected = value;
            }
        }
    });
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::new("text-inspector-tab"), selected));
    ui.separator();
    changed
}
pub fn controls(
    ui: &mut egui::Ui,
    op: &mut Operation,
    time: Time,
    canvas: [f32; 2],
    fonts: &mut crate::font_browser::Browser,
) -> (bool, Option<crate::font_browser::Selection>) {
    let selected = tab(ui);
    let mut changed = false;
    let mut import = None;
    match selected {
        Tab::Text => {
            if ui.button("Add material or effect…").clicked() {
                ui.ctx()
                    .data_mut(|d| d.insert_temp(egui::Id::new("text-inspector-tab"), Tab::Style));
            }
            if let Operation::Text {
                font,
                underline,
                features,
                ..
            } = op
            {
                ui.strong("Font family");
                let (edited, selection) = fonts.choose(ui, font);
                changed |= edited;
                import = selection;
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button("Add font…")
                        .on_hover_text("Install a font into Veylvik's library for all projects.")
                        .clicked()
                    {
                        import = Some(crate::font_browser::Selection::Add);
                    }
                    if ui.small_button("Reset font").clicked() {
                        **font = TextFont::default();
                        changed = true;
                    }
                    changed |= ui.checkbox(underline, "Underline").changed();
                });
                egui::Popup::menu(&ui.button("Font options"))
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        if let TextFont::Embedded { index, .. } = font.as_mut() {
                            ui.label("Collection face");
                            changed |= ui
                                .add(egui::DragValue::new(index).range(0..=1024))
                                .changed();
                            if ui.button("Reset face").clicked() {
                                *index = 0;
                                changed = true;
                            }
                        }
                        ui.label("OpenType features (e.g. kern=0, liga=0)");
                        changed |= ui.text_edit_singleline(features).changed();
                        if ui.button("Reset features").clicked() {
                            features.clear();
                            changed = true;
                        }
                    });
            }
            if let Operation::Text {
                content,
                typography,
                ..
            } = op
            {
                let range = ui
                    .ctx()
                    .data_mut(|d| d.get_temp::<(usize, usize)>(egui::Id::new("text-selection")));
                ui.collapsing("Selected characters", |ui| {
                    if let Some((start, end)) = range.filter(|(a, b)| {
                        a < b
                            && *b <= content.len()
                            && content.is_char_boundary(*a)
                            && content.is_char_boundary(*b)
                    }) {
                        ui.label(format!("Selection: {}", &content[start..end]));
                        let key = egui::Id::new("span-size");
                        let mut scale =
                            ui.ctx().data_mut(|d| d.get_temp::<f32>(key).unwrap_or(1.0));
                        ui.add(egui::Slider::new(&mut scale, 0.1..=4.0).text("Relative size"));
                        ui.ctx().data_mut(|d| d.insert_temp(key, scale));
                        let mut choice = None;
                        ui.horizontal_wrapped(|ui| {
                            for (label, font) in [
                                ("Keep font", None),
                                ("Sans regular", Some(TextFont::Sans {})),
                                ("Sans bold", Some(TextFont::Display)),
                                ("Sans italic", Some(TextFont::Italic {})),
                                ("Sans bold italic", Some(TextFont::BoldItalic {})),
                            ] {
                                if ui.button(label).clicked() {
                                    choice = Some(font);
                                }
                            }
                        });
                        if let Some(font) = choice {
                            let mut candidate = typography.clone();
                            candidate.set_span(veylvik_document::text::TextSpan {
                                start,
                                end,
                                font,
                                scale,
                            });
                            if candidate.validate().is_ok() {
                                *typography = candidate;
                                changed = true;
                            }
                        }
                    } else {
                        ui.weak("Select characters in the text box above or in the viewer.");
                    }
                    if ui.button("Clear character formatting").clicked() {
                        typography.spans.clear();
                        changed = true;
                    }
                });
            }
            changed |= parameter(ui, op, "size", "Size · px", 1.0..=512.0, time);
            changed |= parameter(
                ui,
                op,
                "tracking",
                "Letter spacing · px",
                -32.0..=100.0,
                time,
            );
        }
        Tab::Layout => {
            if let Some(warning) = ui
                .ctx()
                .data_mut(|d| d.get_temp::<String>(egui::Id::new("text-layout-warning")))
                && !warning.is_empty()
            {
                ui.colored_label(egui::Color32::YELLOW, warning);
            }
            if let Operation::Text {
                alignment,
                rtl,
                language,
                ..
            } = op
            {
                ui.strong("Alignment");
                ui.horizontal(|ui| {
                    for (value, label) in [
                        (TextAlignment::Left, "Left"),
                        (TextAlignment::Center, "Centre"),
                        (TextAlignment::Right, "Right"),
                    ] {
                        changed |= ui.selectable_value(alignment, value, label).changed();
                    }
                    if ui.small_button("↺").clicked() {
                        *alignment = TextAlignment::Center;
                        changed = true;
                    }
                });
                changed |= ui.checkbox(rtl, "Right-to-left").changed();
                ui.horizontal(|ui| {
                    ui.label("Language");
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(language)
                                .hint_text("Auto")
                                .desired_width(80.0),
                        )
                        .changed();
                    if ui.small_button("↺").clicked() {
                        language.clear();
                        *rtl = false;
                        changed = true;
                    }
                });
            }
            changed |= parameter(ui, op, "line_height", "Line spacing", 0.1..=4.0, time);
            changed |= parameter(
                ui,
                op,
                "wrap_width",
                "Wrap width · px",
                0.0..=canvas[0],
                time,
            );
            changed |= parameter(
                ui,
                op,
                "box_height",
                "Box height · px",
                0.0..=canvas[1],
                time,
            );
            changed |= parameter(ui, op, "padding", "Padding · px", 0.0..=200.0, time);
            if let Operation::Text { typography, .. } = op {
                use veylvik_document::text::Vertical;
                ui.horizontal(|ui| {
                    for (value, label) in [
                        (Vertical::Top, "Top"),
                        (Vertical::Middle, "Middle"),
                        (Vertical::Bottom, "Bottom"),
                    ] {
                        changed |= ui
                            .selectable_value(&mut typography.vertical, value, label)
                            .changed();
                    }
                });
                changed |= ui
                    .checkbox(&mut typography.auto_fit, "Shrink text to fit box")
                    .changed();
            }
            ui.weak("Width and height enable a layout box. Zero height preserves baseline layout. Long words stay intact.");
        }
        Tab::Transform => {
            if let Operation::Text { typography, .. } = op {
                ui.strong("Anchor");
                egui::Grid::new("text-anchor").show(ui, |ui| {
                    for y in 0..3 {
                        for x in 0..3 {
                            let a = Some([x as f32 * 0.5, y as f32 * 0.5]);
                            if ui.selectable_label(typography.anchor == a, "●").clicked() {
                                typography.anchor = a;
                                changed = true;
                            }
                        }
                        ui.end_row();
                    }
                });
                if ui.small_button("Baseline anchor").clicked() {
                    typography.anchor = None;
                    changed = true;
                }
                changed |= ui
                    .checkbox(&mut typography.linked_scale, "Link width and height")
                    .changed();
            }
            if ui.button("Centre in frame").clicked() {
                for (name, v) in [("x", canvas[0] * 0.5), ("y", canvas[1] * 0.5)] {
                    changed |= reset(op.scalar_mut(name).unwrap(), time, v).is_ok();
                }
                if let Operation::Text { typography, .. } = op {
                    typography.anchor = Some([0.5, 0.5]);
                }
            }
            let old_scale = op.scalar("scale_x").unwrap().at(time).unwrap_or(1.0);
            let old_height = op.scalar("scale_y").unwrap().at(time).unwrap_or(1.0);

            for (name, label, range) in [
                ("x", "Horizontal · px", 0.0..=canvas[0]),
                ("y", "Baseline · px", 0.0..=canvas[1]),
                ("rotation", "Rotation · °", -180.0..=180.0),
                ("scale_x", "Width · ×", -3.0..=3.0),
                ("scale_y", "Height · ×", -3.0..=3.0),
            ] {
                changed |= parameter(ui, op, name, label, range, time);
            }
            let new_scale = op.scalar("scale_x").unwrap().at(time).unwrap_or(1.0);
            if matches!(op,Operation::Text{typography,..} if typography.linked_scale)
                && old_scale != new_scale
            {
                let old_y = op.scalar("scale_y").unwrap().at(time).unwrap_or(1.0);
                changed |= reset(
                    op.scalar_mut("scale_y").unwrap(),
                    time,
                    if old_scale.abs() > 0.0001 {
                        old_y * new_scale / old_scale
                    } else {
                        new_scale
                    },
                )
                .is_ok();
            }
            let new_height = op.scalar("scale_y").unwrap().at(time).unwrap_or(1.0);
            if matches!(op,Operation::Text{typography,..} if typography.linked_scale)
                && old_scale == new_scale
                && old_height != new_height
            {
                changed |= reset(
                    op.scalar_mut("scale_x").unwrap(),
                    time,
                    (if old_height.abs() > 0.0001 {
                        old_scale * new_height / old_height
                    } else {
                        new_height
                    })
                    .clamp(-100.0, 100.0),
                )
                .is_ok();
            }
            ui.horizontal_wrapped(|ui| {
                for (name, label) in [("scale_x", "Mirror"), ("scale_y", "Flip")] {
                    if ui.button(label).clicked() {
                        let value = op.scalar_mut(name).unwrap();
                        if let Ok(current) = value.at(time) {
                            changed |= reset(value, time, -current).is_ok();
                        }
                    }
                }
                if ui.button("Turn 180°").clicked() {
                    changed |= crate::handles::edit(
                        op.scalar_mut("rotation").unwrap(),
                        time,
                        180.0,
                        -36000.0,
                        36000.0,
                    )
                    .is_ok();
                }
            });
            ui.weak("Or drag the handles directly in the viewer.");
        }
        Tab::Animation => {
            if let Operation::Text { typography, .. } = op {
                use veylvik_document::text::{TextMotion, TextUnit};
                ui.strong("Text animation");
                egui::ComboBox::from_id_salt("text-motion")
                    .selected_text(format!("{:?}", typography.motion))
                    .show_ui(ui, |ui| {
                        for m in [
                            TextMotion::None,
                            TextMotion::Typewriter,
                            TextMotion::Rise,
                            TextMotion::Scale,
                            TextMotion::Spin,
                        ] {
                            changed |= ui
                                .selectable_value(&mut typography.motion, m, format!("{m:?}"))
                                .changed();
                        }
                    });
                ui.horizontal_wrapped(|ui| {
                    for u in [
                        TextUnit::Character,
                        TextUnit::Word,
                        TextUnit::Line,
                        TextUnit::Whole,
                    ] {
                        changed |= ui
                            .selectable_value(&mut typography.unit, u, format!("{u:?}"))
                            .changed();
                    }
                });
                for (label, v, range, default) in [
                    ("Start · s", &mut typography.start, -86400.0..=86400.0, 0.0),
                    (
                        "Duration · s",
                        &mut typography.duration,
                        0.001..=3600.0,
                        0.5,
                    ),
                    ("Stagger · s", &mut typography.stagger, 0.0..=60.0, 0.04),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(label);
                        changed |= ui
                            .add(egui::DragValue::new(&mut *v).speed(0.01).range(range))
                            .changed();
                        if ui.small_button("↺").clicked() {
                            *v = default;
                            changed = true;
                        }
                    });
                }
                changed |= ui.checkbox(&mut typography.ease, "Ease in/out").changed();
                let mut exit = typography.exit.is_some();
                if ui.checkbox(&mut exit, "Animate out").changed() {
                    typography.exit = exit.then_some((typography.start + 3.0).min(86400.0));
                    changed = true;
                }
                if let Some(exit) = &mut typography.exit {
                    ui.horizontal(|ui| {
                        ui.label("Exit starts · s");
                        changed |= ui
                            .add(
                                egui::DragValue::new(exit)
                                    .speed(0.01)
                                    .range(typography.start..=86400.0),
                            )
                            .changed();
                    });
                }
                if ui.button("Reset animation").clicked() {
                    let d = veylvik_document::text::Typography::default();
                    typography.motion = d.motion;
                    typography.unit = d.unit;
                    typography.start = d.start;
                    typography.duration = d.duration;
                    typography.stagger = d.stagger;
                    typography.exit = d.exit;
                    typography.ease = d.ease;
                    changed = true;
                }
                ui.weak("Timing is in composition seconds. Character animation preserves shaped clusters and ligatures.");
            }
            ui.separator();
            ui.strong("Animated parameters");
            ui.weak("Previous key · Toggle key · Next key");
            for name in [
                "size", "x", "y", "rotation", "scale_x", "scale_y", "tracking",
            ] {
                let spec = *op
                    .descriptor()
                    .parameters
                    .iter()
                    .find(|p| p.name == name)
                    .unwrap();
                let defaults = op.descriptor().default_operation();
                let default: Scalar = serde_json::from_value(defaults[name].clone()).unwrap();
                changed |= crate::animation::scalar_with_default(
                    ui,
                    &name.replace('_', " "),
                    op.scalar_mut(name).unwrap(),
                    spec.min.unwrap()..=spec.max.unwrap(),
                    time,
                    default.at(time).ok(),
                );
                ui.add_space(5.0);
            }
        }
        Tab::Style => {}
    }
    if selected != Tab::Style && selected != Tab::Animation {
        ui.separator();
        let names: &[&str] = match selected {
            Tab::Text => &["size", "tracking"],
            Tab::Layout => &["line_height", "wrap_width", "box_height", "padding"],
            Tab::Transform => &["x", "y", "rotation", "scale_x", "scale_y"],
            _ => &[],
        };
        if ui.button("Reset this tab at current frame").clicked() {
            let defaults = op.descriptor().default_operation();
            for name in names {
                let default: Scalar = serde_json::from_value(defaults[*name].clone()).unwrap();
                changed |= reset(
                    op.scalar_mut(name).unwrap(),
                    time,
                    default.at(time).unwrap(),
                )
                .is_ok();
            }
            if let Operation::Text {
                font,
                underline,
                features,
                alignment,
                rtl,
                language,
                typography,
                ..
            } = op
            {
                match selected {
                    Tab::Text => {
                        **font = TextFont::default();
                        *underline = false;
                        features.clear();
                    }
                    Tab::Layout => {
                        *alignment = TextAlignment::Center;
                        *rtl = false;
                        language.clear();
                        typography.vertical = veylvik_document::text::Vertical::Top;
                        typography.auto_fit = false;
                    }
                    Tab::Transform => {
                        typography.anchor = None;
                        typography.linked_scale = false;
                    }
                    _ => {}
                }
                changed = true;
            }
        }
        if ui
            .button("Remove animation from this tab")
            .on_hover_text(
                "Keep the current values and remove their keyframes. Undo restores the keys.",
            )
            .clicked()
        {
            for name in names {
                let scalar = op.scalar_mut(name).unwrap();
                if scalar.keys().is_some()
                    && let Ok(value) = scalar.at(time)
                {
                    *scalar = value.into();
                    changed = true;
                }
            }
        }
    }
    (changed, import)
}

/// Browsing a fill category is UI state only. Replacing a fill needs an explicit action.
fn fill_navigation(ui: &mut egui::Ui, mode: &mut u8, active: u8) -> Option<bool> {
    const LABELS: [&str; 4] = ["Colour", "Gradient", "Material", "Image / animation"];
    ui.weak(format!("Applied fill: {}", LABELS[active as usize]));
    ui.horizontal_wrapped(|ui| {
        for (value, label) in LABELS.into_iter().enumerate() {
            let response = ui.selectable_value(mode, value as u8, label);
            #[cfg(test)]
            ui.ctx().data_mut(|d| {
                d.insert_temp(egui::Id::new(("fill-tab-test", value)), response.rect)
            });
            let _ = response;
        }
    });
    if *mode != active && matches!(*mode, 0 | 1) {
        ui.weak("Browsing controls leaves the current fill and effects unchanged.");
        let response = ui
            .button(if *mode == 0 {
                "Use colour fill"
            } else {
                "Use gradient fill"
            })
            .on_hover_text(
                "Replace the current fill; keep attached effects. Undo restores the previous fill.",
            );
        #[cfg(test)]
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("fill-action-test"), response.rect));
        if response.clicked() {
            return Some(*mode == 1);
        }
    }
    None
}

impl crate::Studio {
    pub(super) fn apply_text_style(&mut self, style: Option<&str>, ctx: &egui::Context) {
        if !self.apply_draft() {
            return;
        }
        let position = Some(self.graph.insertion_position(ctx));
        match veylvik_document::presets::text_style::apply(
            self.project.document.snapshot(),
            self.draft.id,
            style,
            position,
        ) {
            Ok(commands) if commands.is_empty() => {}
            Ok(commands) => match self.project.apply(&commands) {
                Ok(()) => {
                    self.project.preview = None;
                    self.refresh_draft();
                    self.request_preview();
                }
                Err(e) => self.fail(e),
            },
            Err(e) => self.fail(e),
        }
    }
    pub(super) fn text_connections(&mut self, ui: &mut egui::Ui) {
        if !matches!(self.draft.operation, Operation::Text { .. }) || tab(ui) != Tab::Style {
            return;
        }
        self.title_style_library(ui);
        ui.strong("Fill");
        let key = egui::Id::new(("text-fill-mode", self.draft.id));
        let doc = self.project.document.snapshot();
        let material = veylvik_document::presets::text_style::current(doc, self.draft.id)
            .ok()
            .flatten();
        let source = veylvik_document::presets::text_style::fill_source(doc, self.draft.id)
            .ok()
            .flatten();
        let is_gradient = source
            .and_then(|id| doc.nodes[&id].inputs.get("image"))
            .is_some_and(|id| matches!(doc.nodes[id].operation, Operation::Gradient { .. }));
        let default = if material.is_some() {
            2u8
        } else if is_gradient {
            1
        } else if source.is_some() {
            3
        } else {
            0
        };
        let mut mode = ui
            .ctx()
            .data_mut(|d| d.get_temp::<u8>(key).unwrap_or(default));
        let fill_action = fill_navigation(ui, &mut mode, default);
        ui.ctx().data_mut(|d| d.insert_temp(key, mode));
        if let Some(gradient) = fill_action
            && self.apply_draft()
        {
            let doc = self.project.document.snapshot();
            let commands = if gradient {
                veylvik_document::presets::text_style::gradient(doc, self.draft.id)
            } else {
                veylvik_document::presets::text_style::plain(doc, self.draft.id)
            };
            match commands {
                Ok(commands) if commands.is_empty() => {}
                Ok(commands) => match self.project.apply(&commands) {
                    Ok(()) => {
                        self.refresh_draft();
                        self.request_preview();
                    }
                    Err(e) => self.fail(e),
                },
                Err(e) => self.fail(e),
            }
        }
        if mode == 1
            && default == 1
            && let Ok(Some(id)) = veylvik_document::presets::text_style::fill_source(
                self.project.document.snapshot(),
                self.draft.id,
            )
        {
            let mut node = self
                .text_edits
                .get(&id)
                .unwrap_or(&self.project.document.snapshot().nodes[&id])
                .clone();
            if let Operation::Colorize { low, high } = &mut node.operation {
                ui.strong("Gradient colours");
                let mut changed = false;
                ui.collapsing("Start colour", |ui| {
                    changed |= crate::animation::color(ui, low);
                });
                ui.collapsing("End colour", |ui| {
                    changed |= crate::animation::color(ui, high);
                });
                if changed {
                    self.text_edits.insert(id, node.clone());
                    self.draft_dirty = true;
                    self.queue_text_preview(ui.ctx());
                }
                if let Some(gradient) = node.inputs.get("image").copied() {
                    let mut source = self
                        .text_edits
                        .get(&gradient)
                        .unwrap_or(&self.project.document.snapshot().nodes[&gradient])
                        .clone();
                    if matches!(source.operation, Operation::Gradient { .. }) {
                        ui.collapsing("Gradient direction", |ui| {
                            if let Ok(time) = self.project.time() {
                                let mut changed = false;
                                for spec in source.operation.descriptor().parameters {
                                    if let Some(value) = source.operation.scalar_mut(spec.name) {
                                        changed |= crate::animation::scalar(
                                            ui,
                                            spec.name,
                                            value,
                                            spec.min.unwrap()..=spec.max.unwrap(),
                                            time,
                                        );
                                    }
                                }
                                if changed {
                                    self.text_edits.insert(gradient, source);
                                    self.draft_dirty = true;
                                    self.queue_text_preview(ui.ctx());
                                }
                            }
                        });
                    }
                }
            }
        }
        if mode == 2 {
            let mut style = None;
            let current = veylvik_document::presets::text_style::current(
                self.project.document.snapshot(),
                self.draft.id,
            );
            ui.strong("Material");
            let active = current
                .as_ref()
                .ok()
                .and_then(|id| *id)
                .and_then(|id| self.project.document.snapshot().nodes.get(&id));
            let query_id = egui::Id::new("text-material-query");
            let mut query = ui
                .ctx()
                .data_mut(|d| d.get_temp::<String>(query_id).unwrap_or_default());
            egui::ComboBox::from_id_salt("text-material")
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .width(220.0)
                .height(300.0)
                .selected_text(active.map_or("Choose a material…", |n| n.label.as_str()))
                .show_ui(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut query)
                                .hint_text("Search materials…")
                                .desired_width(160.0),
                        );
                        if ui.small_button("Clear").clicked() {
                            query.clear();
                        }
                    });
                    if ui
                        .add_enabled(active.is_some(), egui::Button::new("Remove material"))
                        .clicked()
                    {
                        style = Some(None);
                        ui.close();
                    }
                    for entry in veylvik_document::presets::builtin::ALL.iter().filter(|e| {
                        veylvik_document::presets::builtin::is_material(e.id)
                            && e.label.to_lowercase().contains(&query.to_lowercase())
                    }) {
                        if ui.button(entry.label).clicked() {
                            style = Some(Some(entry.id));
                            ui.close();
                        }
                    }
                });
            ui.ctx().data_mut(|d| d.insert_temp(query_id, query));
            ui.weak("Choose a material to apply it immediately. Your text stays editable.");
            if let Ok(Some(id)) = current {
                let mut node = self
                    .text_edits
                    .get(&id)
                    .unwrap_or(&self.project.document.snapshot().nodes[&id])
                    .clone();
                let mut edited = false;
                if let Ok(time) = self.project.time() {
                    for spec in node.operation.descriptor().parameters {
                        if node.operation.scalar(spec.name).is_some() {
                            edited |= parameter(
                                ui,
                                &mut node.operation,
                                spec.name,
                                &spec.name.replace('_', " "),
                                spec.min.unwrap()..=spec.max.unwrap(),
                                time,
                            );
                        }
                    }
                }
                if edited {
                    self.text_edits.insert(id, node);
                    self.draft_dirty = true;
                    self.queue_text_preview(ui.ctx());
                }
            }
            if let Some(style) = style {
                self.apply_text_style(style, ui.ctx());
            }
        }
        if !matches!(self.draft.operation, Operation::Text { .. }) {
            return;
        }
        ui.separator();
        if mode == 0
            && default == 0
            && let Operation::Text { color, .. } = &mut self.draft.operation
        {
            let mut changed = false;
            ui.strong("Plain fill colour");
            ui.weak("Materials use the letter shape; their colour comes from the surface.");
            let mut picker = [
                color[0].clamp(0.0, 1.0),
                color[1].clamp(0.0, 1.0),
                color[2].clamp(0.0, 1.0),
            ];
            if ui.color_edit_button_rgb(&mut picker).changed() {
                color[..3].copy_from_slice(&picker);
                changed = true;
            }
            for (index, label) in ["Red", "Green", "Blue", "Opacity"].iter().enumerate() {
                ui.push_id(label, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(*label);
                        let mut input = egui::DragValue::new(&mut color[index]).speed(0.01);
                        if index == 3 {
                            input = input.range(0.0..=1.0);
                        }
                        changed |= ui.add(input).changed();
                        if ui.small_button("↺").clicked() {
                            color[index] = 1.0;
                            changed = true;
                        }
                    });
                });
            }
            if changed {
                self.draft_dirty = true;
                self.playing = false;
                self.queue_text_preview(ui.ctx());
            }
        }
        if mode == 3 {
            let mut fill = None;
            ui.separator();
            ui.strong("Image or effect fill");
            ui.vertical(|ui| {
            ui.weak("Choose an image or animated branch to fill the letters. This replaces the current material; other effects stay connected.");
            let doc = self.project.document.snapshot();
            let excluded = downstream(doc, self.draft.id);
            egui::ComboBox::from_id_salt("text-fill-source").selected_text("Choose fill layer…").show_ui(ui, |ui| {
                for node in doc.nodes.values() {
                    if doc.groups.values().any(|g| g.members.contains(&node.id) && g.output != node.id) { continue; }
                    if excluded.contains(&node.id) { continue; }
                    let label = doc.groups.values().find(|g| g.output == node.id).map_or(node.label.as_str(), |g| g.label.as_str());
                    if ui.button(label).clicked() { fill = Some(node.id); }
                }
            });
            ui.weak("Branches that depend on this text cannot fill it: that would form a loop. Use the library to apply a material directly.");
        });
            if let Some(image) = fill {
                if self.draft_dirty {
                    self.apply_draft();
                    if self.draft_dirty {
                        return;
                    }
                }
                match veylvik_document::presets::text_style::fill(
                    self.project.document.snapshot(),
                    self.draft.id,
                    image,
                ) {
                    Ok((output, mut commands)) => {
                        let position = self.graph.insertion_position(ui.ctx());
                        for command in &mut commands {
                            if let veylvik_document::Command::Put { node } = command
                                && node.id == output
                                && !self.project.document.snapshot().nodes.contains_key(&output)
                            {
                                node.position = Some(position);
                            }
                        }
                        match self.project.apply(&commands) {
                            Ok(()) => {
                                let _ = output;
                                self.project.preview = None;
                                self.refresh_draft();
                                self.request_preview();
                            }
                            Err(e) => self.fail(e),
                        }
                    }
                    Err(e) => self.fail(e),
                }
            }
        }
        self.text_effect_stack(ui);
        if ui
            .button("Browse effects…")
            .on_hover_text("Apply glow, reflections, shine and other library effects to this text.")
            .clicked()
            && self.apply_draft()
        {
            let source = veylvik_document::presets::text_style::output(
                self.project.document.snapshot(),
                self.draft.id,
            )
            .ok();
            self.open_preset_library(ui.ctx());
            self.library.source = source;
        }
    }
}

/// Source-space handles are valid in the scene only along a path that does not
/// move the source. A transformed shadow branch does not hide the main handles.
pub(super) fn handles_in_scene(
    doc: &veylvik_document::Composition,
    source: veylvik_document::NodeId,
) -> bool {
    let mut pending = vec![source];
    let mut visited = std::collections::BTreeSet::new();
    while let Some(id) = pending.pop() {
        if id == doc.output {
            return true;
        }
        if !visited.insert(id) {
            continue;
        }
        for node in doc.nodes.values() {
            if node.inputs.values().any(|input| *input == id)
                && !matches!(
                    node.operation,
                    Operation::Affine { .. }
                        | Operation::Transform { .. }
                        | Operation::Displace { .. }
                )
            {
                pending.push(node.id);
            }
        }
    }
    false
}

fn downstream(
    doc: &veylvik_document::Composition,
    source: veylvik_document::NodeId,
) -> std::collections::BTreeSet<veylvik_document::NodeId> {
    let mut consumers = std::collections::BTreeMap::<_, Vec<_>>::new();
    for node in doc.nodes.values() {
        for input in node.inputs.values() {
            consumers.entry(*input).or_default().push(node.id);
        }
    }
    let mut pending = vec![source];
    let mut visited = std::collections::BTreeSet::new();
    while let Some(id) = pending.pop() {
        if visited.insert(id)
            && let Some(next) = consumers.get(&id)
        {
            pending.extend(next);
        }
    }
    visited
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resetting_at_playhead_preserves_other_keys_and_plain_values_stay_constant() {
        let mut scalar: Scalar = 42.0.into();
        let zero = Time::new(0, 1).unwrap();
        reset(&mut scalar, zero, 160.0).unwrap();
        assert!(scalar.keys().is_none());
        scalar.set_key_at(zero, 80.0).unwrap();
        scalar.set_key_at(Time::new(2, 1).unwrap(), 240.0).unwrap();
        reset(&mut scalar, Time::new(1, 1).unwrap(), 160.0).unwrap();
        assert_eq!(scalar.keys().unwrap().len(), 3);
        assert_eq!(scalar.at(zero).unwrap(), 80.0);
        assert_eq!(scalar.at(Time::new(2, 1).unwrap()).unwrap(), 240.0);
    }
    #[test]
    fn source_handles_and_fill_candidates_respect_downstream_transforms() {
        use veylvik_document::*;
        let mut doc = presets::showcase::build("gold_text").unwrap().composition;
        let text = doc
            .nodes
            .values()
            .find(|n| matches!(n.operation, Operation::Text { .. }))
            .unwrap()
            .id;
        assert!(handles_in_scene(&doc, text));
        assert!(downstream(&doc, text).contains(&doc.output));
        assert!(!downstream(&doc, text).contains(&presets::background::automatic(&doc).unwrap()));
        let transform = Node::new(
            "Moved scene",
            Operation::Transform {
                x: 10.0.into(),
                y: 0.0.into(),
            },
            &[("image", doc.output)],
        );
        doc.output = transform.id;
        doc.nodes.insert(transform.id, transform);
        assert!(!handles_in_scene(&doc, text));
    }
}

#[cfg(test)]
mod fill_navigation_tests {
    use super::*;
    #[test]
    fn browsing_every_fill_tab_never_requests_a_document_edit() {
        let ctx = egui::Context::default();
        let mut mode = 2;
        let mut run = |events| {
            let mut action = None;
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default()
                        .show_inside(ui, |ui| action = fill_navigation(ui, &mut mode, 2));
                },
            );
            action
        };
        assert_eq!(run(vec![]), None);
        for value in [0usize, 1, 3, 2, 0] {
            let rect = ctx.data_mut(|d| {
                d.get_temp::<egui::Rect>(egui::Id::new(("fill-tab-test", value)))
                    .unwrap()
            });
            for pressed in [true, false] {
                assert_eq!(
                    run(vec![
                        egui::Event::PointerMoved(rect.center()),
                        egui::Event::PointerButton {
                            pos: rect.center(),
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE
                        }
                    ]),
                    None
                );
            }
        }
        let rect = ctx.data_mut(|d| {
            d.get_temp::<egui::Rect>(egui::Id::new("fill-action-test"))
                .unwrap()
        });
        assert_eq!(
            run(vec![
                egui::Event::PointerMoved(rect.center()),
                egui::Event::PointerButton {
                    pos: rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE
                }
            ]),
            None
        );
        assert_eq!(
            run(vec![egui::Event::PointerButton {
                pos: rect.center(),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE
            }]),
            Some(false)
        );
    }
}
