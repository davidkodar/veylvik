//! Application commands live in menus; contextual controls stay in their panels.
use crate::{Leave, Studio, ViewSettings, node_palette};
use eframe::egui;
use veylvik_document::{recipes::Recipe, registry};

fn item(ui: &mut egui::Ui, label: &str, shortcut: &str, enabled: bool) -> bool {
    let clicked = ui
        .add_enabled(enabled, egui::Button::new(label).shortcut_text(shortcut))
        .clicked();
    if clicked {
        ui.close();
    }
    clicked
}

impl Studio {
    pub(super) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.set_min_height(36.);
            ui.spacing_mut().item_spacing.x=14.;
            let side = ui.spacing().interact_size.y;
            let (mark, _) = ui.allocate_exact_size(egui::Vec2::splat(side), egui::Sense::hover());
            crate::icons::paint_mark(ui.painter(), mark, true);
            // Caps sized to the menu text so the bar keeps its height.
            let cap = ui.text_style_height(&egui::TextStyle::Button) * 0.72;
            let (word, _) = ui.allocate_exact_size(
                egui::vec2(crate::icons::WORDMARK_WIDTH * cap, side),
                egui::Sense::hover(),
            );
            let ink = ui.visuals().widgets.inactive.text_color();
            let top = word.center().y - cap / 2.0;
            crate::icons::paint_wordmark(ui.painter(), egui::pos2(word.left(), top), cap, ink);
            ui.separator();
            ui.menu_button("File", |ui| {
                if item(ui, "New composition…", "Ctrl+N", true) { self.leave(Leave::New, ui.ctx()); }
                if item(ui, "New from media…", "", true) { self.leave(Leave::NewFromMedia, ui.ctx()); }
                if item(ui, "Open…", "Ctrl+O", true) { self.leave(Leave::Open, ui.ctx()); }
                ui.separator();
                if item(ui, "Save", "Ctrl+S", true) { self.save_project(false); }
                if item(ui, "Save as…", "Ctrl+Shift+S", true) { self.save_project(true); }
                if item(ui, "Recover unsaved work…", "", true) {
                    self.recovery.refresh(); self.recovery.open = true;
                }
                ui.separator();
                if item(ui, "Import media…", "Ctrl+I", true) {
                    self.import_image(self.graph.insertion_position(ui.ctx()));
                }
                if item(ui, "Import numbered sequence…", "", true) { self.import_media(self.graph.insertion_position(ui.ctx()), true); }
                if item(ui, "Reload source images", "Ctrl+R", !self.draft_dirty) { self.reload_images(); }
                ui.separator();
                if item(ui, "Export video…", "", true) { self.video_export.open = true; }
                if item(ui, "Export current preview…", "Ctrl+E", true) { self.export_frame(); }
                if item(ui, "Export image sequence…", "Ctrl+Shift+E", true) { self.export_controls.open = true; }
                ui.separator();
                if item(ui, "Composition settings…", "", !self.draft_dirty) {
                    self.playing = false;
                    self.composition_controls.begin(self.project.document.snapshot(), false);
                }
                ui.separator();
                if item(ui, "Quit", "Q", true) { self.leave(Leave::Close, ui.ctx()); }
            });
            ui.menu_button("Edit", |ui| {
                let history = self.project.document.history();
                let ready = !self.draft_dirty && !self.graph.dragging() && !self.animation_editor.dragging();
                if item(ui, "Undo", "Ctrl+Z", ready && history.undo_steps > 0) { self.history(false); }
                if item(ui, "Redo", "Ctrl+Shift+Z", ready && history.redo_steps > 0) { self.history(true); }
                ui.separator();
                if item(ui, "Apply inspector changes", "Ctrl+Enter", self.draft_dirty) { self.apply_draft(); }
                if item(ui, "Revert inspector changes", "", self.draft_dirty) {
                    self.refresh_draft(); self.request_preview();
                }
                ui.separator();
                if item(ui, "Create effect group…", "", ready) { self.begin_group(false); }
                if item(ui, "Duplicate selected node", "Ctrl+D", ready) { self.duplicate_selected(ui.ctx()); }
                if item(ui, "Duplicate upstream branch", "Ctrl+Shift+D", ready) { self.duplicate_branch(); }
                if item(ui, "Delete selected node", "", ready) { self.delete_selected(); }
                if item(ui, "Set selected node as output", "", ready) { self.set_selected_output(); }
                ui.separator();
                if item(ui, "Find node…", "Ctrl+F", ready) { self.open_node_finder(); }
            });
            ui.menu_button("Add", |ui| {
                let ready = !self.draft_dirty && !self.graph.dragging() && !self.animation_editor.dragging();
                if item(ui, "Effect library…", "", ready) { self.open_preset_library(ui.ctx()); }
                if item(ui, "Search nodes…", "Tab", ready) { self.open_node_palette(ui.ctx()); }
                ui.separator();
                let mut chosen = None;
                ui.add_enabled_ui(ready, |ui| {
                    if item(ui, "Image or footage…", "Ctrl+I", true) { chosen = Some(node_palette::Choice::Image); }
                    if item(ui, "Text", "", true) { chosen = Some(node_palette::Choice::Node(&registry::TEXT)); }
                    if item(ui, "Text outlines…", "", true) { chosen = Some(node_palette::Choice::Text); }
                    ui.separator();
                    for (label, kinds) in ADD_MENU {
                        ui.menu_button(label, |ui| {
                            for kind in kinds {
                                if item(ui, kind.label, "", true) { chosen = Some(node_palette::Choice::Node(kind)); }
                            }
                        });
                    }
                    ui.separator();
                    for recipe in Recipe::ALL {
                        if item(ui, recipe.label(), "", true) { chosen = Some(node_palette::Choice::Effect(recipe)); }
                    }
                });
                if let Some(choice) = chosen {
                    self.playing = false;
                    self.add_from_palette(choice, self.graph.insertion_position(ui.ctx()));
                }
            });
            ui.menu_button("View", |ui| {
                ui.checkbox(&mut self.browser.visible, "Library panel");
                ui.checkbox(&mut self.inspector_visible, "Inspector panel");
                if item(ui, "Preview selected node", "", true) {
                    self.project.preview = Some(self.project.selected); self.request_preview();
                }
                if item(ui, "View composition output", "", true) {
                    self.project.preview = None; self.request_preview();
                }
                ui.separator();
                if item(ui, "Fit image", "", true) { self.navigation.fit(); }
                if item(ui, "Actual pixels (100%)", "", true) { self.navigation.actual_size(); }
                if ui.checkbox(&mut self.view.alpha, "Show alpha").clicked() { ui.close(); }
                let mut filmic = !self.project.document.snapshot().display.is_standard();
                if ui
                    .checkbox(&mut filmic, "Filmic display (ACES-style)")
                    .on_hover_text("Roll highlights off smoothly in the viewer, PNG and video instead of clipping at white. Saved with the composition; EXR stays scene-linear.")
                    .clicked()
                {
                    use veylvik_document::media::DisplayTransform;
                    let display = if filmic { DisplayTransform::Filmic } else { DisplayTransform::Standard };
                    match self.project.apply(&[veylvik_document::Command::Display { display }]) {
                        Ok(()) => self.request_preview(),
                        Err(e) => self.fail(e),
                    }
                    ui.close();
                }
                if item(ui, "Reset display settings", "", true) { self.view = ViewSettings::default(); }
                ui.separator();
                if item(ui, if self.playing { "Pause" } else { "Play" }, "Space", true) { self.toggle_play(); }
                ui.separator();
                if ui.checkbox(&mut self.statistics, "Render statistics").clicked() { ui.close(); }
            });
            ui.menu_button("Tools", |ui| {
                if item(ui, "Select subject…", "", !self.files.busy()) {
                    self.begin_subject();
                }
                ui.separator();
                if item(ui, "AI models…", "", true) {
                    self.models.show_window();
                }
            });
            ui.menu_button("Help", |ui| {
                if item(ui, "Keyboard shortcuts", "?", true) { self.help = true; }
                ui.separator();
                if item(ui, "About Veylvik…", "", true) { self.about.begin(); }
            });
            ui.separator();
            // Reserve only the remaining space so long filenames never push menus offscreen.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new("Render").fill(egui::Color32::from_rgb(110,66,174))).clicked() {self.video_export.open=true;}

                let unavailable = self.project.unavailable_assets().len();
                if unavailable > 0 {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("{unavailable} unavailable"))
                        .on_hover_text("Select an Image node to replace its source, or use File > Reload source images after repairing the files.");
                }
                let name = self.project.path.as_ref().and_then(|p| p.file_name())
                    .map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into());
                let title = if self.project.dirty() || self.draft_dirty { format!("{name} •") } else { name };
                if ui.ctx().content_rect().width()>1200. {
                    let center=egui::pos2(ui.ctx().content_rect().center().x,ui.max_rect().center().y);
                    ui.painter().text(center,egui::Align2::CENTER_CENTER,title.chars().take(42).collect::<String>(),egui::FontId::proportional(14.),ui.visuals().text_color());
                } else {ui.add(egui::Label::new(&title).truncate()).on_hover_text(title);}

            });
        });
    }
}

/// Add menu submenus. Every registered node type appears here, except media and text,
/// which have their own Add entries (a test keeps this complete as nodes are added).
pub(crate) const ADD_MENU: [(&str, &[&registry::NodeType]); 8] = [
    (
        "Generators",
        &[
            &registry::SOLID,
            &registry::NOISE,
            &registry::FRACTAL_NOISE,
            &registry::GRADIENT,
            &registry::FIRE,
            &registry::DUST,
            &registry::DRIP,
            &registry::ATMOSPHERE,
        ],
    ),
    (
        "Masks and shapes",
        &[
            &registry::ELLIPSE,
            &registry::PATH,
            &registry::CHANNEL_MASK,
            &registry::MORPHOLOGY,
            &registry::COMBINE_MASK,
        ],
    ),
    (
        "Transform and distort",
        &[&registry::AFFINE, &registry::TRANSFORM, &registry::DISPLACE],
    ),
    (
        "Colour and filters",
        &[
            &registry::EXPOSURE,
            &registry::COLOR_CORRECT,
            &registry::LUT,
            &registry::COLORIZE,
            &registry::BLUR,
        ],
    ),
    (
        "Compositing",
        &[
            &registry::MERGE,
            &registry::BLEND,
            &registry::FADE,
            &registry::MASK,
            &registry::SURFACE_BLEND,
        ],
    ),
    (
        "Materials and light",
        &[
            &registry::MATERIAL,
            &registry::PATINA,
            &registry::GLASS,
            &registry::SURFACE_REFLECTION,
            &registry::SUN_GLINTS,
            &registry::METALLIC_SHINE,
            &veylvik_document::nodes::SHADOW,
        ],
    ),
    (
        "Keying",
        &[
            &registry::CHROMA_KEY,
            &registry::LUMA_KEY,
            &registry::REFINE_MATTE,
        ],
    ),
    (
        "Tracking",
        &[&registry::PLANAR_TRACK, &registry::PLANAR_WARP],
    ),
];

#[cfg(test)]
mod add_menu_tests {
    #[test]
    fn add_menu_offers_every_registered_node_type_once() {
        let mut listed: Vec<&str> = super::ADD_MENU
            .iter()
            .flat_map(|(_, kinds)| kinds.iter().map(|k| k.type_id))
            .collect();
        // Media and text have dedicated Add entries (Image…, Text).
        listed.extend(["image", "footage", "text"]);
        let mut sorted = listed.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), listed.len(), "a node type is listed twice");
        let mut registered: Vec<&str> = veylvik_document::registry::TYPES
            .iter()
            .map(|k| k.type_id)
            .collect();
        registered.sort_unstable();
        assert_eq!(sorted, registered, "Add menu and node registry differ");
    }
}
