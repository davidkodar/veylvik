//! Compact text editing over the same draft/transaction machinery as other nodes.
use eframe::egui;
use veylvik_document::{Operation, TextAlignment, TextFont};
pub fn controls(
    ui: &mut egui::Ui,
    op: &mut Operation,
    time: veylvik_document::Time,
) -> (bool, bool) {
    let Operation::Text {
        content,
        font,
        alignment,
        rtl,
        language,
        underline,
        features,
        color,
        rotation,
        size,
        scale_x,
        scale_y,
        ..
    } = op
    else {
        return (false, false);
    };
    ui.label("Your text");
    let mut changed = ui
        .add(
            egui::TextEdit::multiline(content)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .char_limit(4096),
        )
        .changed();
    ui.weak("Preview updates as you type. Apply keeps the edit; Revert restores it.");
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
    changed |= crate::animation::scalar(ui, "Font size (pixels)", size, 1.0..=1024.0, time);
    let import = ui
        .button("Choose font file…")
        .on_hover_text("Embed a TTF/OTF font in the project so titles remain portable.")
        .clicked();
    ui.horizontal(|ui| {
        for (value, label) in [
            (TextAlignment::Left, "Left"),
            (TextAlignment::Center, "Center"),
            (TextAlignment::Right, "Right"),
        ] {
            changed |= ui.selectable_value(alignment, value, label).changed();
        }
    });
    changed |= ui.checkbox(underline, "Underline").changed();
    ui.collapsing("Text colour", |ui| {
        changed |= crate::animation::color(ui, color);
    });
    ui.collapsing("Language and font details",|ui| {
        changed|=ui.checkbox(rtl,"Right-to-left text").changed();
        ui.label("Language tag (optional, e.g. en, sv, ar)");
        changed|=ui.text_edit_singleline(language).changed();
        if let TextFont::Embedded {index,..}=font {ui.horizontal(|ui| {ui.label("Font collection face");changed|=ui.add(egui::DragValue::new(index).range(0..=1024)).changed();});}
        ui.label("OpenType features (e.g. kern=0, liga=0, smcp=1)");
        changed|=ui.text_edit_singleline(features).changed();
        ui.weak("Features depend on the font. Each line uses one direction; mixed-direction paragraphs and font fallback are not yet supported. Wrap width 0 disables wrapping; long words remain intact.");
    });
    ui.horizontal_wrapped(|ui| {
        for (label, value) in [
            ("Mirror horizontally", scale_x),
            ("Flip vertically", scale_y),
        ] {
            if ui.button(label).clicked()
                && let Ok(current) = value.at(time)
            {
                changed |= crate::handles::edit(value, time, -2.0 * current, -100.0, 100.0).is_ok();
            }
        }
        if ui.button("Turn 180°").clicked() {
            changed |= crate::handles::edit(rotation, time, 180.0, -36000.0, 36000.0).is_ok();
        }
    });
    ui.weak("Drag the viewer handles to move, scale or rotate. Apply an effect from the library to keep this text editable beneath it.");
    for (section, names) in [
        (
            "Spacing and wrapping",
            &["tracking", "line_height", "wrap_width"][..],
        ),
        (
            "Position, rotation and scale",
            &["x", "y", "rotation", "scale_x", "scale_y"][..],
        ),
    ] {
        ui.collapsing(section, |ui| {
            for name in names {
                let parameter = op
                    .descriptor()
                    .parameters
                    .iter()
                    .find(|p| p.name == *name)
                    .unwrap();
                let label = match *name {
                    "tracking" => "Letter spacing (pixels)",
                    "line_height" => "Line spacing (factor)",
                    "wrap_width" => "Wrap width (pixels; 0 = off)",
                    "x" => "Horizontal position",
                    "y" => "Baseline position",
                    "rotation" => "Rotation (degrees)",
                    "scale_x" => "Horizontal scale",
                    _ => "Vertical scale",
                };
                changed |= crate::animation::scalar(
                    ui,
                    label,
                    op.scalar_mut(name).unwrap(),
                    parameter.min.unwrap()..=parameter.max.unwrap(),
                    time,
                );
            }
        });
    }
    (changed, import)
}

impl crate::Studio {
    pub(super) fn text_connections(&mut self, ui: &mut egui::Ui) {
        if !matches!(self.draft.operation, Operation::Text { .. }) {
            return;
        }
        let mut fill = None;
        ui.collapsing("Fill text with an image or effect", |ui| {
            ui.weak("Choose an independent image, animated branch or material surface. Its alpha and colour will be clipped to the letters. Existing effects still apply afterwards.");
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
            match veylvik_document::presets::fill_layer(
                self.project.document.snapshot(),
                self.draft.id,
                image,
            ) {
                Ok((output, mut commands)) => {
                    let position = self.graph.insertion_position(ui.ctx());
                    for command in &mut commands {
                        if let veylvik_document::Command::Put { node } = command
                            && node.id == output
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
