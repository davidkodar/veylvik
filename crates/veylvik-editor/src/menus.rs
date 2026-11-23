//! Application commands live in menus; contextual controls stay in their panels.
use crate::{Studio, menu_model::MenuItem};
use eframe::egui;
use veylvik_document::registry;

/// Draw `items` from the menu model; the id of an item clicked goes to `clicked`.
fn draw(ui: &mut egui::Ui, items: &[MenuItem], clicked: &mut Option<String>) {
    for item in items {
        if item.separator_before {
            ui.separator();
        }
        let hinted = |response: egui::Response| match &item.hint {
            Some(hint) => response.on_hover_text(hint),
            None => response,
        };
        if !item.children.is_empty() {
            ui.add_enabled_ui(item.enabled, |ui| {
                ui.menu_button(&item.label, |ui| draw(ui, &item.children, clicked));
            });
        } else if let Some(mut on) = item.checked {
            if hinted(ui.add_enabled(item.enabled, egui::Checkbox::new(&mut on, &item.label)))
                .changed()
            {
                *clicked = Some(item.id.clone());
            }
        } else {
            let button = egui::Button::new(&item.label)
                .shortcut_text(item.shortcut.clone().unwrap_or_default());
            if hinted(ui.add_enabled(item.enabled, button)).clicked() {
                *clicked = Some(item.id.clone());
                ui.close();
            }
        }
    }
}

impl Studio {
    pub(super) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.set_min_height(36.);
            ui.spacing_mut().item_spacing.x = 14.;
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
            // The same list a host shows in its own bar (menu_model.rs).
            let ctx = ui.ctx().clone();
            let mut clicked = None;
            for menu in self.menus(&ctx) {
                ui.menu_button(&menu.title, |ui| draw(ui, &menu.items, &mut clicked));
            }
            if let Some(id) = clicked {
                self.run_menu_item(&ctx, &id);
            }
            ui.separator();
            self.page_bar(ui);
            self.link_bar(ui);
            ui.separator();
            // Reserve only the remaining space so long filenames never push menus offscreen.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.capabilities.export
                    && ui
                        .add(
                            egui::Button::new(egui::RichText::new("Render").color(
                                crate::theme::ink(crate::theme::palette(ui.ctx()).selection),
                            ))
                            .fill(crate::theme::palette(ui.ctx()).selection),
                        )
                        .clicked()
                {
                    self.go_to(crate::pages::Page::Export);
                }

                let unavailable = self.project.unavailable_assets().len();
                // A host keeps the project's files: it relinks its own media when asked.
                let hint = if self.capabilities.project_files {
                    "Open Project files to locate or relink missing media"
                } else {
                    "Ask the host to locate or relink missing media"
                };
                if unavailable > 0
                    && ui
                        .button(
                            egui::RichText::new(format!("{unavailable} unavailable"))
                                .color(egui::Color32::LIGHT_RED),
                        )
                        .on_hover_text(hint)
                        .clicked()
                {
                    if self.capabilities.project_files {
                        self.portable.open = true;
                    } else {
                        self.relink_asked = true;
                    }
                }
                let name = self.title_name();
                let title = if self.project.dirty() || self.draft_dirty {
                    format!("{name} •")
                } else {
                    name
                };
                // Centred in the room left of the Render button when the whole name fits there
                // (the page tabs and a link's button take their room first); cut short otherwise.
                let shown: String = title.chars().take(42).collect();
                let room = ui.available_rect_before_wrap();
                let wide = ui
                    .painter()
                    .layout_no_wrap(
                        shown.clone(),
                        egui::FontId::proportional(14.),
                        ui.visuals().text_color(),
                    )
                    .size()
                    .x;
                if ui.ctx().content_rect().width() > 1200. && wide + 24. <= room.width() {
                    ui.painter().text(
                        room.center(),
                        egui::Align2::CENTER_CENTER,
                        shown,
                        egui::FontId::proportional(14.),
                        ui.visuals().text_color(),
                    );
                } else {
                    ui.add(egui::Label::new(&title).truncate())
                        .on_hover_text(title);
                }
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
            &veylvik_document::nodes::FORCE,
            &registry::DRIP,
            &registry::ATMOSPHERE,
        ],
    ),
    (
        "Masks and shapes",
        &[
            &registry::ELLIPSE,
            &registry::RECTANGLE,
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
            &registry::REPEAT,
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
            &veylvik_document::nodes::ZOOM_BLUR,
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
            &veylvik_document::nodes::OWN_SIZE,
            &registry::MODEL_3D,
            &registry::TRANSFORM_3D,
            &registry::MERGE_SCENES,
            &registry::CAMERA_3D,
            &registry::LIGHT_3D,
            &registry::ENVIRONMENT_3D,
            &registry::POINT_CLOUD,
            &registry::PARTICLE_INSTANCES,
            &registry::CAMERA_PROJECTION,
            &registry::GAUSSIAN_SPLATS,
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
