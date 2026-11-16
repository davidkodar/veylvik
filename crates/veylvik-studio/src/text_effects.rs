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
                ("Contact shadow", "contact_shadow"),
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
                // Keep the lighting toggle visible on the effect itself, not buried
                // inside the scalar-only controls editor.
                let doc = self.project.document.snapshot();
                let shadow_id = doc.groups.values().find(|g| g.output == *id)
                    .and_then(|g| g.members.iter().find(|member| matches!(doc.nodes[member].operation, Operation::Shadow(_))).copied())
                    .or_else(|| matches!(node.operation, Operation::Shadow(_)).then_some(*id));
                if let Some(shadow_id) = shadow_id {
                    let mut target = self.text_edits.get(&shadow_id).unwrap_or(&doc.nodes[&shadow_id]).clone();
                    if let Operation::Shadow(shadow) = &mut target.operation
                        && shadow_controls(ui, shadow) {
                            if shadow_id == *id { node = target; changed = true; }
                            else { self.text_edits.insert(shadow_id, target); self.draft_dirty = true; self.queue_draft_preview(ui.ctx()); }
                    }
                } else if text_style::legacy_shadow_nodes(doc, *id).is_some() {
                    ui.weak("This shadow uses the original manual offset setup.");
                    if ui.button("Enable light-following shadow").on_hover_text("Upgrade this shadow in one undo step. Keeps strength and softness; replaces manual offsets with light direction. Enable area lights on the material first.").clicked() {
                        action = Some(("upgrade_shadow".into(), index));
                    }
                }
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
                                if matches!(self.project.document.snapshot().nodes[&control.node].operation, Operation::Shadow(_))
                                    && (control.parameter == "follow_lighting" || (control.parameter == "light" && matches!(self.text_edits.get(&control.node).unwrap_or(&self.project.document.snapshot().nodes[&control.node]).operation.scalar("light"), Some(veylvik_document::Scalar::Constant { .. })))) { continue; }

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
                            if spec.name == "light" && matches!(&node.operation, Operation::Shadow(s) if matches!(s.light, veylvik_document::Scalar::Constant { .. })) { continue; }

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
                "upgrade_shadow" => text_style::upgrade_shadow(doc, chain[index]),
                "remove" => text_style::remove_effect(doc, self.draft.id, chain[index]),
                kind => {
                    let source = chain.last().copied().unwrap_or_else(|| {
                        text_style::output(doc, self.draft.id).unwrap_or(self.draft.id)
                    });
                    if kind == "soft_shadow" {
                        text_style::shadow(doc, source)
                    } else if kind == "contact_shadow" {
                        text_style::contact_shadow(doc, source)
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

/// Shared between the text effect entry and the standalone shadow inspector.
pub(crate) fn shadow_controls(
    ui: &mut egui::Ui,
    shadow: &mut veylvik_document::nodes::Shadow,
) -> bool {
    let mut changed=ui.checkbox(&mut shadow.follow_lighting,"Follow scene lighting")
        .on_hover_text("Use the enabled area lights on the material feeding this shadow. Changes to those lights also change the shadow.").changed();
    if shadow.follow_lighting {
        changed |= crate::light_guides::source_selector(ui, &mut shadow.light);
        ui.weak("Enable area lights in Material / Studio lighting. Environment reflections alone do not cast shadows.");
    }
    changed
}
