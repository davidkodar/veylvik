//! Choosing what a composition shows outside the editor: "Show on Edit" on a node's settings in the inspector, and a window to name, order and remove them; every change is one undoable transaction of the whole list.
use crate::Studio;
use eframe::egui;
use veylvik_document::{Command, groups::Control};

impl Studio {
    fn set_published(&mut self, published: Vec<Control>) {
        match self.project.apply(&[Command::Published { published }]) {
            Ok(()) => self.refresh_draft(),
            Err(e) => self.fail(e),
        }
    }
    /// The inspector's menu: tick a setting of the selected node to show it on a host's
    /// Edit page.
    pub(crate) fn publish_menu(&mut self, ui: &mut egui::Ui) {
        let id = self.draft.id;
        let doc = self.project.document.snapshot();
        let Some(node) = doc.nodes.get(&id) else {
            return;
        };
        let parameters: Vec<_> = node
            .operation
            .descriptor()
            .parameters
            .iter()
            .filter(|p| crate::controls::drawable(p.kind))
            .collect();
        if parameters.is_empty() {
            return;
        }
        let shown = doc.published.iter().filter(|c| c.node == id).count();
        let title = if shown > 0 {
            format!("Show on Edit ({shown})")
        } else {
            "Show on Edit".into()
        };
        let mut toggled = None;
        ui.add_enabled_ui(!self.draft_dirty, |ui| {
            ui.menu_button(title, |ui| {
                ui.weak(
                    "Settings a host such as Klipp shows on its Edit page, without opening Veylvik",
                );
                for p in &parameters {
                    let mut on = doc
                        .published
                        .iter()
                        .any(|c| c.node == id && c.parameter == p.name);
                    if ui
                        .checkbox(&mut on, crate::names::setting(p.name, p.unit))
                        .changed()
                    {
                        toggled = Some(p.name);
                    }
                }
                ui.separator();
                if ui.button("Settings shown on Edit…").clicked() {
                    self.published_open = true;
                    ui.close();
                }
            })
            .response
            .on_disabled_hover_text("Apply or revert the inspector's changes first")
            .on_hover_text(
                "Choose settings of this node to show outside the editor, on a host's Edit page",
            );
        });
        let Some(parameter) = toggled else {
            return;
        };
        let mut published = doc.published.clone();
        if let Some(at) = published
            .iter()
            .position(|c| c.node == id && c.parameter == parameter)
        {
            published.remove(at);
        } else if published.len() >= veylvik_document::groups::MAX_CONTROLS {
            self.fail("A composition shows at most 64 settings on Edit");
            return;
        } else {
            // "Title glow", not "Title glow (pixels)": the panel shows the value itself.
            let label = format!(
                "{} {}",
                node.label,
                crate::names::setting(parameter, "").to_lowercase()
            );
            match Control::new(
                doc,
                id,
                parameter,
                label.chars().take(128).collect::<String>().trim(),
            ) {
                Ok(control) => published.push(control),
                Err(e) => {
                    self.fail(e);
                    return;
                }
            }
        }
        self.set_published(published);
    }
    /// The window listing the composition's own published settings.
    pub(crate) fn published_window(&mut self, ctx: &egui::Context) {
        if !self.published_open {
            return;
        }
        let mut open = true;
        let published = self.project.document.snapshot().published.clone();
        let labels: Vec<String> = published
            .iter()
            .map(|c| {
                self.project
                    .document
                    .snapshot()
                    .nodes
                    .get(&c.node)
                    .map_or_else(String::new, |n| n.label.clone())
            })
            .collect();
        let mut next = None;
        egui::Window::new("Settings shown on Edit")
            .open(&mut open)
            .collapsible(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.weak("A host such as Klipp shows these on its Edit page, in this order, then each effect group's controls. Tick more with Show on Edit in the inspector.");
                ui.separator();
                if published.is_empty() {
                    ui.label("Nothing is shown on Edit yet.");
                }
                for (n, control) in published.iter().enumerate() {
                    ui.push_id(n, |ui| {
                        ui.horizontal(|ui| {
                            let id = ui.make_persistent_id("name");
                            let mut name = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| control.label.clone());
                            let field = ui.add(egui::TextEdit::singleline(&mut name).desired_width(220.0));
                            if field.changed() {
                                ui.data_mut(|d| d.insert_temp(id, name.clone()));
                            }
                            if field.lost_focus() {
                                ui.data_mut(|d| d.remove::<String>(id));
                                let name = name.trim();
                                if !name.is_empty() && name != control.label {
                                    let mut list = published.clone();
                                    list[n].label = name.chars().take(128).collect();
                                    next = Some(list);
                                }
                            }
                            ui.weak(format!("{} · {}", labels[n], control.parameter));
                            if ui.add_enabled(n > 0, crate::icons::Button::new(crate::icons::Icon::Up, "Show earlier")).on_hover_text("Show earlier").clicked() {
                                let mut list = published.clone();
                                list.swap(n, n - 1);
                                next = Some(list);
                            }
                            if ui.add_enabled(n + 1 < published.len(), crate::icons::Button::new(crate::icons::Icon::Down, "Show later")).on_hover_text("Show later").clicked() {
                                let mut list = published.clone();
                                list.swap(n, n + 1);
                                next = Some(list);
                            }
                            if ui.small_button("Remove").clicked() {
                                let mut list = published.clone();
                                list.remove(n);
                                next = Some(list);
                            }
                        });
                    });
                }
            });
        self.published_open = open;
        if let Some(list) = next {
            self.set_published(list);
        }
    }
}
