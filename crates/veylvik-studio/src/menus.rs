//! Application commands live in menus; contextual controls stay in their panels.
use crate::{Leave, Studio, ViewSettings, node_palette};
use eframe::egui;
use veylvik_document::{recipes::Recipe, registry};

fn item(ui: &mut egui::Ui, label: &str, shortcut: &str, enabled: bool) -> bool {
    let shortcut = crate::command_ui::menu_text(ui.ctx(), shortcut);
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
                if item(ui, "Start screen", "", true) { self.show_start(); }
                ui.separator();
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
                if item(ui, "Project files…", "", true) { self.portable.open = true; }
                if item(ui, "Render queue…", "", true) { self.go_to(crate::pages::Page::Export); self.pages.quick = false; }
                if item(ui, "Export video…", "", true) { self.ask_export(crate::pages::ExportKind::Video); }
                if item(ui, "Export current preview…", "Ctrl+E", true) { self.export_frame(); }
                if item(ui, "Export image sequence…", "Ctrl+Shift+E", true) { self.ask_export(crate::pages::ExportKind::Sequence); }
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
                if item(ui, "Undo history…", "", true) { self.log.history = true; }
                ui.separator();
                if item(ui, "Apply inspector changes", "Ctrl+Enter", self.draft_dirty) { self.keep_tryout(); }
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
                if item(ui, "Add a marker here", "M", true) { self.add_marker(); }
                let mut auto_key = self.preferences.values.auto_key;
                if ui.checkbox(&mut auto_key, "Automatic keyframes").on_hover_text("On: changing an animated setting adds a keyframe at the current position. Off: such a change is a try-out until you add the keyframe").changed() { self.toggle_auto_key(); }
                ui.separator();
                if item(ui,"Preferences…","Ctrl+,",true) {self.open_preferences();}
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
                let mut show_editor=!self.node_editor_hidden;
                if ui.checkbox(&mut show_editor,"Node / animation editor").changed() {self.node_editor_hidden = !show_editor;}
                let mut layer_bars = !self.layer_bars.hidden;
                if ui.checkbox(&mut layer_bars, "Layer bars on the timeline").on_hover_text("A bar for each footage clip and fade under the ruler: drag to slide, drag an end to trim").changed() { self.layer_bars.hidden = !layer_bars; }
                self.layout_menu(ui);
                if item(ui, "Viewer full screen", "F11", true) { let ctx = ui.ctx().clone(); self.set_viewer_fullscreen(&ctx, true); }

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
                if item(ui,"Colour management…","",true) {crate::color_controls::open(ui.ctx(),self.project.session,&self.project.document.snapshot().ocio);}
                ui.menu_button("Working colour space", |ui| {
                    use veylvik_document::color_management::WorkingSpace;
                    let current=self.project.document.snapshot().working_space;
                    for (space,label) in [(WorkingSpace::LinearRec709,"Linear Rec.709"),(WorkingSpace::Acescg,"ACEScg (AP1)")] {
                        if ui.selectable_label(current==space,label).clicked() && current!=space {
                            match self.project.apply(&[veylvik_document::Command::WorkingSpace {space}]) {Ok(())=>self.request_preview(),Err(e)=>self.fail(e)}
                            ui.close();
                        }
                    }
                    ui.weak("Compositing and grading use this space. Materials retain their authored colour domain; output remains linear Rec.709 for the viewer and existing exports.");
                });
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
                if item(ui, "Reset display settings", "", true) { self.view = ViewSettings {fast_filter:self.preferences.values.fast_view,..Default::default()}; }
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
                if item(ui, "Find a command…", "Ctrl+K", true) { let ctx = ui.ctx().clone(); self.run(crate::commands::Cmd::CommandSearch, &ctx); }
                if item(ui, "Keyboard shortcuts…", "?", true) { self.commands.editor = true; }
                if item(ui, "Messages…", "", true) { self.log.messages = true; }
                ui.separator();
                if item(ui, "About Veylvik…", "", true) { self.about.begin(); }
            });
            ui.separator();
            self.page_bar(ui);
            ui.separator();
            // Reserve only the remaining space so long filenames never push menus offscreen.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(egui::RichText::new("Render").color(crate::theme::ink(crate::theme::palette(ui.ctx()).selection))).fill(crate::theme::palette(ui.ctx()).selection)).clicked() {self.go_to(crate::pages::Page::Export);}

                let unavailable = self.project.unavailable_assets().len();
                if unavailable > 0 && ui.button(egui::RichText::new(format!("{unavailable} unavailable")).color(egui::Color32::LIGHT_RED))
                        .on_hover_text("Open Project files to locate or relink missing media").clicked() { self.portable.open = true; }
                let name = self.project.path.as_ref().and_then(|p| p.file_name())
                    .map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into());
                let title = if self.project.dirty() || self.draft_dirty { format!("{name} •") } else { name };
                if ui.ctx().content_rect().width()>1200. {
                    // Centred in the room left of the Render button, clear of the page tabs.
                    let center=ui.available_rect_before_wrap().center();
                    ui.painter().text(center,egui::Align2::CENTER_CENTER,title.chars().take(42).collect::<String>(),egui::FontId::proportional(14.),ui.visuals().text_color());
                } else {ui.add(egui::Label::new(&title).truncate()).on_hover_text(title);}

            });
        });
    }
}

/// Add menu submenus. Every registered node type appears here, except media and text,
/// which have their own Add entries (a test keeps this complete as nodes are added).
pub(crate) const ADD_MENU: [(&str, &[&registry::NodeType]); 10] = [
    (
        "Generators",
        &[
            &registry::SOLID,
            &registry::NOISE,
            &registry::FRACTAL_NOISE,
            &registry::GRADIENT,
            &registry::FIRE,
            &registry::DUST,
            &registry::PARTICLES,
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
        &[
            &registry::AFFINE,
            &registry::TRANSFORM,
            &registry::DISPLACE,
            &registry::LENS_DISTORTION,
            &registry::GRID_WARP,
            &registry::SHAPE_WARP,
        ],
    ),
    (
        "Colour and filters",
        &[
            &registry::EXPOSURE,
            &registry::COLOR_CORRECT,
            &registry::LUT,
            &registry::OCIO,
            &registry::COLORIZE,
            &registry::BLUR,
            &registry::BLOOM,
            &veylvik_document::nodes::LIGHT_WRAP,
            &registry::SHADER,
            &registry::MATCH_GRAIN,
            &registry::LENS_BLUR,
        ],
    ),
    (
        "Time and motion",
        &[
            &registry::MOTION_BLUR,
            &registry::TIME_REMAP,
            &registry::OPTICAL_FLOW,
            &veylvik_document::nodes::SOUND,
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
        "3D",
        &[
            &registry::SHAPE_3D,
            &registry::TEXT_3D,
            &registry::IMAGE_CARD,
            &registry::MODEL_3D,
            &registry::TRANSFORM_3D,
            &registry::MERGE_SCENES,
            &registry::CAMERA_3D,
            &registry::LIGHT_3D,
            &registry::ENVIRONMENT_3D,
            &registry::POINT_CLOUD,
            &registry::RENDER_SCENE,
        ],
    ),
    (
        "Materials and light",
        &[
            &registry::MATERIAL,
            &registry::CONDENSATION,
            &registry::PATINA,
            &registry::MATERIAL_LAYERS,
            &registry::GLASS,
            &registry::SURFACE_REFLECTION,
            &registry::SUN_GLINTS,
            &registry::METALLIC_SHINE,
            &registry::LIGHT_RIG,
            &veylvik_document::nodes::SHADOW,
            &veylvik_document::nodes::CONTACT_SHADOW,
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
