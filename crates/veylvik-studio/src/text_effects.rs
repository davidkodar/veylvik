use eframe::egui;
use veylvik_document::{Command, Node, Operation, presets::text_style};
impl crate::Studio {
    pub(super) fn text_effect_stack(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.strong("Attached effects");
        let chain =
            text_style::chain(self.project.document.snapshot(), self.draft.id).unwrap_or_default();
        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            for (label, kind) in [
                ("Outline", "outline"),
                ("Shadow", "soft_shadow"),
                ("Glow", "glow"),
                ("Shine", "metallic_shine"),
                ("Bevel relief", "silver_relief"),
            ] {
                if ui.button(label).clicked() {
                    action = Some((kind.to_owned(), 0));
                }
            }
        });
        if chain.is_empty() {
            ui.weak("Add an effect here, or use Browse effects for compound presets.");
        }
        for (index, id) in chain.iter().enumerate() {
            let mut node = self
                .text_edits
                .get(id)
                .unwrap_or(&self.project.document.snapshot().nodes[id])
                .clone();
            let mut changed = false;
            ui.push_id(id, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let mut enabled = !node.bypass;
                    let label = self
                        .project
                        .document
                        .snapshot()
                        .groups
                        .values()
                        .find(|g| g.output == *id)
                        .map_or(node.label.as_str(), |g| g.label.as_str());
                    if ui.checkbox(&mut enabled, label).changed() {
                        node.bypass = !enabled;
                        changed = true;
                    }
                    if ui
                        .add_enabled(
                            index > 0,
                            crate::icons::Button::new(crate::icons::Icon::Up, "Move effect up"),
                        )
                        .clicked()
                    {
                        action = Some(("up".into(), index));
                    }
                    if ui
                        .add_enabled(
                            index + 1 < chain.len(),
                            crate::icons::Button::new(crate::icons::Icon::Down, "Move effect down"),
                        )
                        .clicked()
                    {
                        action = Some(("down".into(), index));
                    }
                    if ui.small_button("Remove").clicked() {
                        action = Some(("remove".into(), index));
                    }
                });
                ui.collapsing("Controls", |ui| {
                    if let Some(group) = self
                        .project
                        .document
                        .snapshot()
                        .groups
                        .values()
                        .find(|g| g.output == *id)
                        .cloned()
                    {
                        if let Ok(time) = self.project.time() {
                            for control in group.controls.values() {
                                let mut target = self
                                    .text_edits
                                    .get(&control.node)
                                    .unwrap_or(
                                        &self.project.document.snapshot().nodes[&control.node],
                                    )
                                    .clone();
                                let spec = target
                                    .operation
                                    .descriptor()
                                    .parameters
                                    .iter()
                                    .find(|p| p.name == control.parameter)
                                    .unwrap();
                                let edited = if let Some(value) =
                                    target.operation.scalar_mut(&control.parameter)
                                {
                                    crate::animation::scalar(
                                        ui,
                                        &control.label,
                                        value,
                                        spec.min.unwrap()..=spec.max.unwrap(),
                                        time,
                                    )
                                } else {
                                    match &mut target.operation {
                                        Operation::Colorize(
                                            veylvik_document::nodes::Colorize { high, .. },
                                        ) => crate::animation::color(ui, high),
                                        Operation::Solid(veylvik_document::nodes::Solid {
                                            color,
                                        }) => crate::animation::color(ui, color),
                                        _ => false,
                                    }
                                };
                                if edited {
                                    self.text_edits.insert(control.node, target);
                                    self.draft_dirty = true;
                                    self.queue_draft_preview(ui.ctx());
                                }
                            }
                        }
                    } else if let Ok(time) = self.project.time() {
                        for spec in node.operation.descriptor().parameters {
                            if let Some(value) = node.operation.scalar_mut(spec.name) {
                                changed |= crate::animation::scalar(
                                    ui,
                                    spec.name,
                                    value,
                                    spec.min.unwrap()..=spec.max.unwrap(),
                                    time,
                                );
                            }
                        }
                    }
                });
            });
            if changed {
                self.text_edits.insert(*id, node);
                self.draft_dirty = true;
                self.queue_draft_preview(ui.ctx());
            }
        }
        ui.weak("Shared branches remain available in the node editor.");
        if let Some((action, index)) = action {
            if !self.apply_draft() {
                return;
            }
            let doc = self.project.document.snapshot();
            let result = match action.as_str() {
                "up" => text_style::swap(doc, self.draft.id, index - 1),
                "down" => text_style::swap(doc, self.draft.id, index),
                "remove" => text_style::remove_effect(doc, self.draft.id, chain[index]),
                kind => {
                    let source = chain.last().copied().unwrap_or_else(|| {
                        text_style::output(doc, self.draft.id).unwrap_or(self.draft.id)
                    });
                    if kind == "soft_shadow" {
                        text_style::shadow(doc, source)
                    } else if kind == "outline" || kind == "glow" {
                        (|| {
                            let recipe = if kind == "outline" {
                                veylvik_document::recipes::Recipe::Outline
                            } else {
                                veylvik_document::recipes::Recipe::Glow
                            };
                            let effect = recipe.build(doc, source)?;
                            let output = effect.root;
                            let commands = effect.into_grouped_commands(doc)?;
                            veylvik_document::presets::connect_after(doc, source, output, commands)
                        })()
                    } else if kind == "metallic_shine" {
                        let spec = veylvik_document::registry::TYPES
                            .iter()
                            .find(|s| s.type_id == kind)
                            .unwrap();
                        let op: Operation =
                            serde_json::from_value(spec.default_operation()).unwrap();
                        let node = Node::new(spec.label, op, &[("image", source)]);
                        veylvik_document::presets::connect_after(
                            doc,
                            source,
                            node.id,
                            vec![Command::Put {
                                node: Box::new(node),
                            }],
                        )
                    } else {
                        (|| {
                            let preset = veylvik_document::presets::builtin::build(kind)?.adapted(
                                doc.width,
                                doc.height,
                                veylvik_document::presets::resolution::Sizing::Fit,
                            )?;
                            let inputs = preset
                                .group
                                .inputs
                                .keys()
                                .map(|k| (k.clone(), source))
                                .collect();
                            let instance = preset.instantiate(doc, &inputs, None)?;
                            veylvik_document::presets::connect_after(
                                doc,
                                source,
                                instance.output,
                                instance.commands,
                            )
                        })()
                    }
                }
            };
            match result {
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
    }
}
