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
            ui.strong(label);
            if ui
                .small_button("Reset")
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
            egui::Popup::menu(&ui.button("Keys"))
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    changed |= crate::animation::scalar(
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
    let Operation::Text { content, .. } = op else {
        return false;
    };
    let changed = egui::ScrollArea::vertical()
        .id_salt("text-content-scroll")
        .max_height(76.0)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(content)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .char_limit(4096),
            )
            .changed()
        })
        .inner;
    ui.small("Live preview · Apply to keep · Revert to undo draft");
    let mut selected = tab(ui);
    ui.horizontal_wrapped(|ui| {
        for (value, label) in [
            (Tab::Text, "Text"),
            (Tab::Layout, "Layout"),
            (Tab::Transform, "Transform"),
            (Tab::Style, "Style"),
        ] {
            ui.selectable_value(&mut selected, value, label);
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
) -> (bool, bool) {
    let selected = tab(ui);
    let mut changed = false;
    let mut import = false;
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
                ui.strong("Font");
                egui::ComboBox::from_id_salt("editable-font")
                    .selected_text(font.label())
                    .show_ui(ui, |ui| {
                        for value in [
                            TextFont::Display,
                            TextFont::Sans {},
                            TextFont::Italic {},
                            TextFont::BoldItalic {},
                            TextFont::Mono {},
                        ] {
                            let label = value.label().to_owned();
                            changed |= ui.selectable_value(font, value, label).changed();
                        }
                    });
                ui.horizontal_wrapped(|ui| {
                    import = ui
                        .button("Choose font…")
                        .on_hover_text("Embed a TTF/OTF/TTC font in this project.")
                        .clicked();
                    if ui.small_button("Reset font").clicked() {
                        *font = TextFont::default();
                        changed = true;
                    }
                    changed |= ui.checkbox(underline, "Underline").changed();
                });
                egui::Popup::menu(&ui.button("Font options"))
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        if let TextFont::Embedded { index, .. } = font {
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
                    if ui.small_button("Reset").clicked() {
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
                    if ui.small_button("Reset").clicked() {
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
            ui.weak("Wrap width 0 keeps manual line breaks. Long words stay intact.");
        }
        Tab::Transform => {
            for (name, label, range) in [
                ("x", "Horizontal · px", 0.0..=canvas[0]),
                ("y", "Baseline · px", 0.0..=canvas[1]),
                ("rotation", "Rotation · °", -180.0..=180.0),
                ("scale_x", "Width · ×", -3.0..=3.0),
                ("scale_y", "Height · ×", -3.0..=3.0),
            ] {
                changed |= parameter(ui, op, name, label, range, time);
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
        Tab::Style => {}
    }
    (changed, import)
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
            .selected_text(active.map_or("Plain colour", |n| n.label.as_str()))
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
                    .selectable_label(active.is_none(), "Plain colour")
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
        if let Ok(Some(id)) = current
            && ui.button("Adjust material…").clicked()
            && self.apply_draft()
        {
            self.project.selected = id;
            self.refresh_draft();
        }
        if let Some(style) = style {
            self.apply_text_style(style, ui.ctx());
        }
        if !matches!(self.draft.operation, Operation::Text { .. }) {
            return;
        }
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
        ui.separator();
        if let Operation::Text { color, .. } = &mut self.draft.operation {
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
                        if ui.small_button("Reset").clicked() {
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
                            self.project.selected = output;
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
