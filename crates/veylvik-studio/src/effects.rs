//! Group authoring uses ordinary document transactions and the existing inspector draft.
use crate::{Studio, animation};
use eframe::egui;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use veylvik_document::{
    Command, Composition, NodeId,
    groups::{Control, Group},
    registry::ParameterKind,
};

#[derive(Default)]
pub struct Controls {
    pub open: bool,
    group: Option<Group>,
    members: BTreeSet<NodeId>,
    label: String,
    output: Option<NodeId>,
    error: String,
    revision: u64,
    pub edits: BTreeMap<(NodeId, String), Value>,
    pub connections: BTreeMap<(NodeId, String), NodeId>,
}
impl Controls {
    /// Shared by live draft previews and the eventual atomic commit.
    pub fn commands(&self) -> Option<Vec<Command>> {
        if self.edits.is_empty() && self.connections.is_empty() {
            return None;
        }
        Some(
            self.edits
                .iter()
                .map(|((id, control), value)| Command::SetControl {
                    id: *id,
                    control: control.clone(),
                    value: value.clone(),
                })
                .chain(
                    self.connections
                        .iter()
                        .map(|((id, input), source)| Command::ConnectGroup {
                            id: *id,
                            input: input.clone(),
                            source: *source,
                        }),
                )
                .collect(),
        )
    }

    fn begin(
        &mut self,
        doc: &Composition,
        revision: u64,
        selected: NodeId,
        existing: Option<Group>,
    ) {
        self.revision = revision;
        self.error.clear();
        self.open = true;
        if let Some(group) = existing {
            self.members = group.members.clone();
            self.label = group.label.clone();
            self.output = Some(group.output);
            self.group = Some(group);
        } else {
            self.members.clear();
            let owned: BTreeSet<_> = doc
                .groups
                .values()
                .flat_map(|g| g.members.iter().copied())
                .collect();
            let mut pending = vec![selected];
            while let Some(id) = pending.pop() {
                if !owned.contains(&id) && self.members.insert(id) {
                    pending.extend(doc.nodes[&id].inputs.values().copied());
                }
            }
            self.output = Some(selected);
            self.label = "New effect".into();
            self.group = None;
        }
    }
    fn show(&mut self, ctx: &egui::Context, doc: &Composition, revision: u64) -> Option<Group> {
        if !self.open {
            return None;
        }
        let mut open = true;
        let mut result = None;
        egui::Window::new(if self.group.is_some() { "Edit effect" } else { "Create effect" })
            .id(egui::Id::new("group-editor")).open(&mut open).collapsible(false).default_width(540.0)
            .show(ctx, |ui| {
                ui.label("Name"); ui.text_edit_singleline(&mut self.label);
                if let Some(group) = &mut self.group {
                    ui.label("Expose the controls people need. Their values and animation stay on the original nodes.");
                    egui::ScrollArea::vertical().max_height(330.0).show(ui, |ui| {
                        for id in &group.members {
                            let node = &doc.nodes[id];
                            egui::CollapsingHeader::new(&node.label).id_salt(id).show(ui, |ui| {
                                for p in node.operation.descriptor().parameters {
                                    if !matches!(p.kind, ParameterKind::Scalar | ParameterKind::Color | ParameterKind::Seed) { continue; }
                                    ui.push_id(p.name, |ui| {
                                        let key = group.controls.iter().find(|(_, c)| c.node == *id && c.parameter == p.name).map(|(k, _)| k.clone());
                                        let mut exposed = key.is_some();
                                        if ui.checkbox(&mut exposed, p.name.replace('_', " ")).changed() {
                                            if let Some(key) = &key { group.controls.remove(key); }
                                            else if group.controls.len() < veylvik_document::groups::MAX_CONTROLS {
                                                let mut number = 1;
                                                while group.controls.contains_key(&format!("control_{number}")) { number += 1; }
                                                if let Ok(control) = Control::new(doc, *id, p.name, &format!("{} {}", node.label, p.name.replace('_', " "))) {
                                                    group.controls.insert(format!("control_{number}"), control);
                                                }
                                            }
                                        }
                                        if let Some(key) = key && let Some(c) = group.controls.get_mut(&key) {
                                            ui.text_edit_singleline(&mut c.label);
                                            if matches!(p.kind, ParameterKind::Scalar) {
                                                let mut custom = c.range.is_some();
                                                if ui.checkbox(&mut custom, "Custom range").changed() { c.range = custom.then(|| [p.min.unwrap(), p.max.unwrap()]); }
                                                if let Some([min, max]) = &mut c.range {
                                                    ui.horizontal(|ui| { ui.label("Min"); ui.add(egui::DragValue::new(min).speed(0.1)); ui.label("Max"); ui.add(egui::DragValue::new(max).speed(0.1)); });
                                                }
                                            }
                                            if ui.small_button("Use current value as default").clicked() && let Ok(value) = c.value(doc) { c.default = value; }
                                        }
                                    });
                                }
                            });
                        }
                        if !group.inputs.is_empty() { ui.separator(); ui.label("Input names"); }
                        for input in group.inputs.values_mut() { ui.text_edit_singleline(&mut input.label); }
                    });
                } else {
                    ui.label("Choose nodes feeding the selected output. Uncheck source nodes to turn their connections into effect inputs.");
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        let mut nodes: Vec<_> = doc.nodes.values().collect();
                        nodes.sort_by_cached_key(|n| (Some(n.id) != self.output, n.label.to_lowercase()));
                        for node in nodes {
                            if doc.groups.values().any(|g| g.members.contains(&node.id)) { continue; }
                            let mut selected = self.members.contains(&node.id);
                            ui.horizontal(|ui| {
                                if ui.checkbox(&mut selected, &node.label).changed() {
                                    if selected { self.members.insert(node.id); } else { self.members.remove(&node.id); }
                                }
                                if Some(node.id) == self.output { ui.weak("Output"); }
                            });
                        }
                    });
                }
                if !self.error.is_empty() { ui.colored_label(egui::Color32::LIGHT_RED, &self.error); }
                ui.horizontal(|ui| {
                    if ui.button(if self.group.is_some() { "Apply" } else { "Create" }).clicked() {
                        let prepared = if revision != self.revision { Err(veylvik_document::Error("Document changed; close and reopen this dialog.".into())) }
                        else if let Some(group) = &self.group {
                            let mut group = group.clone(); group.label = self.label.clone();
                            let mut candidate = doc.clone(); candidate.groups.insert(group.id, group.clone());
                            candidate.validate().map(|_| group)
                        } else { Group::capture(doc, self.members.clone(), self.output.unwrap(), &self.label) };
                        match prepared { Ok(group) => result = Some(group), Err(error) => self.error = error.to_string() }
                    }
                    if ui.button("Cancel").clicked() { self.open = false; }
                });
            });
        self.open &= open && result.is_none();
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.open = false;
        }
        result
    }
}
impl Studio {
    pub(super) fn begin_group(&mut self, edit: bool) {
        if !self.can_change_selection()
            || self.authoring_dialog_open()
            || self.graph.dragging()
            || self.animation_editor.dragging()
        {
            return;
        }
        self.playing = false;
        let doc = self.project.document.snapshot();
        let existing = edit
            .then(|| {
                doc.groups
                    .values()
                    .find(|g| g.members.contains(&self.project.selected))
                    .cloned()
            })
            .flatten();
        self.effects.begin(
            doc,
            self.project.document.revision(),
            self.project.selected,
            existing,
        );
    }
    pub(super) fn effect_dialog(&mut self, ctx: &egui::Context) {
        if let Some(group) = self.effects.show(
            ctx,
            self.project.document.snapshot(),
            self.project.document.revision(),
        ) {
            let id = group.id;
            let output = group.output;
            match self.project.apply(&[Command::PutGroup {
                group: Box::new(group),
            }]) {
                Ok(()) => {
                    self.project.selected = output;
                    self.graph.collapse(id);
                    self.refresh_draft();
                    self.request_preview();
                }
                Err(error) => self.fail(error),
            }
        }
    }
    pub(super) fn effect_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(group) = self
            .graph
            .selected_group(self.project.document.snapshot(), self.project.selected)
            .cloned()
        else {
            return false;
        };
        ui.heading(&group.label);
        ui.weak(format!("Reusable effect · {} nodes", group.members.len()));
        // A material starter is a single-node group. Keep its finish accessible
        // without requiring users to open the group first.
        if group.members.len() == 1 {
            let mut node = self.project.document.snapshot().nodes[&group.output].clone();
            if let veylvik_document::Operation::Material(material) = &mut node.operation {
                let changed = ui
                    .add_enabled_ui(!self.draft_dirty, |ui| {
                        crate::nodes::finish_controls(ui, material)
                    })
                    .inner;
                if changed {
                    match self.project.apply(&[Command::Put {
                        node: Box::new(node),
                    }]) {
                        Ok(()) => {
                            self.refresh_draft();
                            self.request_preview();
                        }
                        Err(error) => self.fail(error),
                    }
                }
            }
        }

        for (key, input) in &group.inputs {
            let edit_key = (group.id, key.clone());
            let doc = self.project.document.snapshot();
            let mut source = self
                .effects
                .connections
                .get(&edit_key)
                .copied()
                .unwrap_or_else(|| group.source(doc, key).unwrap());
            let before = source;
            ui.label(&input.label);
            egui::ComboBox::from_id_salt(("group-input", key))
                .selected_text(&doc.nodes[&source].label)
                .show_ui(ui, |ui| {
                    for node in doc.nodes.values() {
                        if group.members.contains(&node.id)
                            || doc
                                .groups
                                .values()
                                .any(|g| g.members.contains(&node.id) && g.output != node.id)
                        {
                            continue;
                        }
                        let label = doc
                            .groups
                            .values()
                            .find(|g| g.output == node.id)
                            .map_or(node.label.as_str(), |g| g.label.as_str());
                        ui.selectable_value(&mut source, node.id, label);
                    }
                });
            if source != before {
                self.effects.connections.insert(edit_key, source);
                self.queue_draft_preview(ui.ctx());
                self.draft_dirty = true;
                self.playing = false;
            }
        }
        ui.separator();
        let time = self.project.time().unwrap();
        for (key, control) in &group.controls {
            let edit_key = (group.id, key.clone());
            let mut value = self
                .effects
                .edits
                .get(&edit_key)
                .cloned()
                .unwrap_or_else(|| control.value(self.project.document.snapshot()).unwrap());
            let descriptor = self.project.document.snapshot().nodes[&control.node]
                .operation
                .descriptor()
                .parameters
                .iter()
                .find(|p| p.name == control.parameter)
                .unwrap();
            let mut changed = false;
            ui.push_id(key, |ui| {
                match descriptor.kind {
                    ParameterKind::Scalar => {
                        let mut scalar = serde_json::from_value(value.clone()).unwrap();
                        if control.parameter == "light"
                            && matches!(
                                self.project.document.snapshot().nodes[&control.node].operation,
                                veylvik_document::Operation::Shadow(_)
                            )
                            && matches!(scalar, veylvik_document::Scalar::Constant { .. })
                        {
                            changed = crate::light_guides::source_selector(ui, &mut scalar);
                            value = serde_json::to_value(scalar).unwrap();
                            return;
                        }
                        let [min, max] = control
                            .range
                            .unwrap_or([descriptor.min.unwrap(), descriptor.max.unwrap()]);
                        let default = serde_json::from_value::<veylvik_document::Scalar>(
                            control.default.clone(),
                        )
                        .ok()
                        .and_then(|s| s.at(time).ok());
                        changed = animation::scalar_with_default(
                            ui,
                            &control.label,
                            &mut scalar,
                            min..=max,
                            time,
                            default,
                        );
                        value = serde_json::to_value(scalar).unwrap();
                    }
                    ParameterKind::Boolean => {
                        let mut enabled = value.as_bool().unwrap_or(false);
                        changed = ui.checkbox(&mut enabled, &control.label).changed();
                        value = enabled.into();
                    }
                    ParameterKind::Color => {
                        ui.label(&control.label);
                        let mut color = serde_json::from_value(value.clone()).unwrap();
                        changed = animation::color(ui, &mut color);
                        value = serde_json::to_value(color).unwrap();
                    }
                    ParameterKind::Seed => {
                        ui.label(&control.label);
                        let mut seed = value.as_u64().unwrap() as u32;
                        changed = ui.add(egui::DragValue::new(&mut seed)).changed();
                        value = seed.into();
                    }
                    _ => {
                        ui.label(&control.label);
                        ui.weak("Open the group to edit this control's node.");
                    }
                }
                ui.menu_button("Parameter options", |ui| {
                    if ui.small_button("Reset to default").clicked() {
                        value = control.default.clone();
                        changed = true;
                    }
                    if ui
                        .add_enabled(
                            !self.draft_dirty && !changed,
                            egui::Button::new("Show in animation"),
                        )
                        .clicked()
                    {
                        self.graph.expand(group.id);
                        self.project.selected = control.node;
                        self.refresh_draft();
                        self.animation_editor.visible = true;
                    }
                });
                ui.separator();
            });
            if changed {
                self.effects.edits.insert(edit_key, value);
                self.queue_draft_preview(ui.ctx());
                self.draft_dirty = true;
                self.playing = false;
            }
        }
        if group.controls.is_empty() {
            ui.label("Expose useful controls with Edit effect.");
        }
        ui.add_enabled_ui(!self.draft_dirty, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Edit effect…").clicked() {
                    self.begin_group(true);
                }
                if ui.button("Open group").clicked() {
                    self.graph.expand(group.id);
                }
                if ui.button("Save to library").clicked() {
                    self.save_effect_preset(group.id);
                }
                if ui.button("Duplicate effect").clicked() {
                    self.duplicate_group(group.id);
                }
                if ui.button("Set as output").clicked() {
                    self.set_selected_output();
                }
                if ui.button("Preview effect").clicked() {
                    self.project.preview = Some(group.output);
                    self.request_preview();
                }
                if ui.button("Ungroup").clicked() {
                    match self.project.apply(&[Command::Ungroup { id: group.id }]) {
                        Ok(()) => {
                            self.refresh_draft();
                            self.request_preview();
                        }
                        Err(error) => self.fail(error),
                    }
                }
            });
        });
        true
    }
    pub(super) fn duplicate_group(&mut self, id: NodeId) {
        if !self.can_change_selection() {
            return;
        }
        let doc = self.project.document.snapshot();
        let result = doc.groups[&id].duplicate(doc);
        match result {
            Ok((group, commands)) => match self.project.apply(&commands) {
                Ok(()) => {
                    self.project.selected = group.output;
                    self.refresh_draft();
                    self.request_preview();
                }
                Err(error) => self.fail(error),
            },
            Err(error) => self.fail(error),
        }
    }
}

#[cfg(test)]
mod live_preview_tests {
    use super::*;
    use veylvik_document::{Document, Operation, Scalar, Time, presets::builtin};

    #[test]
    fn group_draft_changes_preview_without_history_and_commits_as_one_edit() {
        let mut owner =
            Document::new(veylvik_document::blank(64, 64, Time::new(24, 1).unwrap()).unwrap())
                .unwrap();
        let preset = builtin::build("gold_text").unwrap();
        let bindings = preset
            .group
            .inputs
            .keys()
            .map(|key| (key.clone(), owner.snapshot().output))
            .collect();
        let instance = preset
            .instantiate(owner.snapshot(), &bindings, None)
            .unwrap();
        owner.apply(0, &instance.commands).unwrap();
        let group = &owner.snapshot().groups[&instance.group];
        let (control, target) = group
            .controls
            .iter()
            .find(|(_, c)| c.parameter == "roughness")
            .unwrap();
        let target = target.node;
        let control = control.clone();
        let revision = owner.revision();
        let steps = owner.history().undo_steps;
        let original = serde_json::to_value(owner.snapshot()).unwrap();
        let mut edits = Controls::default();
        assert!(edits.commands().is_none());
        for roughness in [0.3, 0.6, 0.9] {
            edits.edits.insert(
                (instance.group, control.clone()),
                serde_json::to_value(Scalar::from(roughness)).unwrap(),
            );
            let preview = owner.prepare(revision, &edits.commands().unwrap()).unwrap();
            let Operation::Material(m) = &preview.nodes[&target].operation else {
                panic!()
            };
            assert_eq!(m.roughness.at(Time::new(0, 1).unwrap()).unwrap(), roughness);
            assert_eq!(owner.revision(), revision);
            assert_eq!(owner.history().undo_steps, steps);
            assert_eq!(serde_json::to_value(owner.snapshot()).unwrap(), original);
        }
        let commands = edits.commands().unwrap();
        let preview = owner.prepare(revision, &commands).unwrap();
        owner.apply(revision, &commands).unwrap();
        assert_eq!(
            serde_json::to_value(owner.snapshot()).unwrap(),
            serde_json::to_value(preview).unwrap()
        );
        assert_eq!(owner.history().undo_steps, steps + 1);
        owner.undo(owner.revision()).unwrap();
        assert_eq!(serde_json::to_value(owner.snapshot()).unwrap(), original);
    }
}
