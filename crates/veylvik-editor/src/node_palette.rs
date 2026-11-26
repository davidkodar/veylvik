//! Registry-backed node search, with insertion coordinates captured when opened.
use eframe::egui;
use veylvik_document::{
    NodePosition, Operation,
    registry::{self, NodeType},
};
#[derive(Clone, Copy)]
pub enum Choice {
    Node(&'static NodeType),
    Image,
    Text,
    Effect(veylvik_document::recipes::Recipe),
}
struct Entry {
    choice: Choice,
    label: &'static str,
    description: &'static str,
    id: &'static str,
}
fn entries(query: &str) -> Vec<Entry> {
    let mut entries: Vec<_> = registry::TYPES
        .iter()
        .map(|kind| {
            if matches!(kind.type_id, "image" | "footage") {
                Entry {
                    choice: Choice::Image,
                    label: if kind.type_id == "footage" {"Import footage…"} else {"Import image…"},
                    description: "Import video, PNG, OpenEXR or an image sequence through the file picker.",
                    id: kind.type_id,
                }
            } else {
                Entry {
                    choice: Choice::Node(kind),
                    label: kind.label,
                    description: kind.description,
                    id: kind.type_id,
                }
            }
        })
        .collect();
    entries.push(Entry {
        choice: Choice::Text,
        label: "Outline text…",
        description: "Shape text using an explicit font into a portable vector mask.",
        id: "outline_text",
    });
    entries.extend(veylvik_document::recipes::Recipe::ALL.map(|recipe| Entry {
        choice: Choice::Effect(recipe),
        label: recipe.label(),
        description: recipe.description(),
        id: recipe.id(),
    }));
    let query = query.to_lowercase();
    let words: Vec<_> = query.split_whitespace().collect();
    let rank = |entry: &Entry| {
        let title = format!("{} {}", entry.label, entry.id.replace('_', " ")).to_lowercase();
        if entry.id == query.trim() || entry.label.to_lowercase() == query.trim() {
            0
        } else if words.iter().all(|word| title.contains(word)) {
            1
        } else if words
            .iter()
            .all(|word| format!("{title} {}", entry.description.to_lowercase()).contains(word))
        {
            2
        } else {
            3
        }
    };
    entries.retain(|entry| rank(entry) < 3);
    entries.sort_by_key(rank);
    entries
}
#[derive(Default)]
pub struct Controls {
    pub open: bool,
    query: String,
    selected: usize,
    focus: bool,
    position: Option<NodePosition>,
}
impl Controls {
    pub fn open_at(&mut self, position: NodePosition) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.focus = true;
        self.position = Some(position);
    }
    pub fn show(&mut self, ctx: &egui::Context) -> Option<(Choice, NodePosition)> {
        if !self.open {
            return None;
        }
        let (up, down, enter, escape) = ctx.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if escape {
            self.open = false;
            return None;
        }
        let mut open = self.open;
        let mut chosen = None;
        egui::Window::new("Add node")
            .id(egui::Id::new("node-palette"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(440.0)
            .show(ctx, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Search nodes…")
                        .char_limit(128)
                        .desired_width(f32::INFINITY),
                );
                let reveal = self.focus || response.changed() || up || down;
                if self.focus {
                    response.request_focus();
                    self.focus = false;
                }
                if response.changed() {
                    self.selected = 0;
                }
                let matches = entries(&self.query);
                self.selected = self.selected.min(matches.len().saturating_sub(1));
                if !matches.is_empty() {
                    if down {
                        self.selected = (self.selected + 1) % matches.len();
                    }
                    if up {
                        self.selected = (self.selected + matches.len() - 1) % matches.len();
                    }
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .show(ui, |ui| {
                        if matches.is_empty() {
                            ui.weak("No matching nodes");
                        }
                        for (index, entry) in matches.iter().enumerate() {
                            let selected = index == self.selected;
                            let response = ui.selectable_label(selected, entry.label);
                            if selected && reveal {
                                response.scroll_to_me(Some(egui::Align::Center));
                            }
                            if response.clicked() {
                                chosen = Some(entry.choice);
                            }
                        }
                    });
                ui.separator();
                if let Some(entry) = matches.get(self.selected) {
                    ui.label(entry.description);
                    if enter {
                        chosen = Some(entry.choice);
                    }
                }
                ui.weak("Up / Down choose · Enter add · Esc cancel");
            });
        self.open = open && chosen.is_none();
        chosen.map(|choice| {
            (
                choice,
                self.position
                    .expect("palette opened with insertion position"),
            )
        })
    }
}

pub fn operation(kind: &NodeType, width: u32, height: u32) -> Operation {
    let mut operation: Operation =
        serde_json::from_value(kind.default_operation()).expect("validated registry defaults");
    match &mut operation {
        Operation::Text(veylvik_document::nodes::Text { x, y, size, .. }) => {
            *x = (width as f32 * 0.5).into();
            *y = (height as f32 * 0.6).into();
            *size = (height as f32 * 0.28).clamp(1.0, 4096.0).into();
        }
        Operation::MetallicShine(veylvik_document::nodes::MetallicShine {
            x, y, width: w, ..
        }) => {
            *x = (width as f32 * 0.5).into();
            *y = (height as f32 * 0.5).into();
            *w = (width as f32 * 0.094).max(1.0).into();
        }
        Operation::Atmosphere(veylvik_document::nodes::Atmosphere {
            x,
            y,
            width: w,
            height: h,
            ..
        }) => {
            *x = (width as f32 * 0.5).into();
            *y = (height as f32 * 0.93).into();
            *w = (width as f32 * 0.73).max(1.0).into();
            *h = (height as f32 * 0.83).max(1.0).into();
        }
        Operation::Material(veylvik_document::nodes::Material { scale, bevel, .. }) => {
            *scale = (width as f32 * 0.1875).max(1.0).into();
            *bevel = (width as f32 / 240.0).min(256.0).into();
        }
        Operation::SurfaceReflection(veylvik_document::nodes::SurfaceReflection {
            bevel,
            roughness,
            ..
        }) => {
            *bevel = (width as f32 / 192.0).min(256.0).into();
            *roughness = (width as f32 / 960.0).min(256.0).into();
        }
        Operation::Glass(veylvik_document::nodes::Glass {
            bevel, refraction, ..
        }) => {
            *bevel = (width as f32 / 160.0).min(256.0).into();
            *refraction = (width as f32 * 0.01875).min(256.0).into();
        }
        Operation::SunGlints(veylvik_document::nodes::SunGlints { size, .. }) => {
            *size = (width as f32 * 0.01875).clamp(1.0, 512.0).into();
        }
        Operation::MaterialLayers(veylvik_document::nodes::MaterialLayers {
            wear_width,
            noise_size,
            ..
        }) => {
            *wear_width = (width as f32 / 120.0).clamp(1.0, 128.0).into();
            *noise_size = (width as f32 / 24.0).clamp(1.0, 4096.0).into();
        }
        Operation::ContactShadow(veylvik_document::nodes::ContactShadow {
            reach, lean, ..
        }) => {
            *reach = (width as f32 / 60.0).clamp(1.0, 256.0).into();
            *lean = (width as f32 / 320.0).clamp(0.0, 512.0).into();
        }
        Operation::Patina(veylvik_document::nodes::Patina { scale, .. }) => {
            *scale = (width as f32 * 110.0 / 960.0).clamp(1.0, 8192.0).into();
        }
        Operation::Condensation(veylvik_document::nodes::Condensation { size, .. }) => {
            *size = (height as f32 * 3.2 / 540.0).clamp(1.0, 128.0).into();
        }
        Operation::Drip(veylvik_document::nodes::Drip {
            spacing,
            length,
            radius,
            ..
        }) => {
            let scale = height as f32 / 540.0;
            *spacing = (43.0 * scale).clamp(8.0, 1024.0).into();
            *length = (140.0 * scale).min(8192.0).into();
            *radius = (5.5 * scale).clamp(1.0, 256.0).into();
        }
        Operation::Dust(veylvik_document::nodes::Dust { size, .. }) => {
            *size = (1.25 * height as f32 / 540.0).clamp(0.1, 32.0).into();
        }
        Operation::Fire(veylvik_document::nodes::Fire {
            x,
            y,
            width: w,
            height: h,
            ..
        }) => {
            *x = (width as f32 * 0.5).into();
            *y = (height as f32 * 0.7).into();
            *w = (width as f32 * 0.65).into();
            *h = (height as f32 * 0.5).into();
        }
        Operation::Rectangle(veylvik_document::nodes::Rectangle {
            x,
            y,
            width: w,
            height: h,
            ..
        }) => {
            *x = (width as f32 * 0.5).into();
            *y = (height as f32 * 0.5).into();
            *w = (width as f32 * 0.4).into();
            *h = (height as f32 * 0.3).into();
        }
        Operation::Ellipse(veylvik_document::nodes::Ellipse {
            x,
            y,
            radius_x,
            radius_y,
            ..
        }) => {
            *x = (width as f32 * 0.5).into();
            *y = (height as f32 * 0.5).into();
            let radius = width.min(height) as f32 * 0.25;
            *radius_x = radius.into();
            *radius_y = radius.into();
        }
        Operation::Affine(veylvik_document::nodes::Affine {
            pivot_x, pivot_y, ..
        }) => {
            *pivot_x = (width as f32 * 0.5).into();
            *pivot_y = (height as f32 * 0.5).into();
        }
        Operation::Gradient(veylvik_document::nodes::Gradient {
            start_y,
            end_x,
            end_y,
            ..
        }) => {
            *start_y = (height as f32 * 0.5).into();
            *end_x = (width.saturating_sub(1) as f32).into();
            *end_y = (height as f32 * 0.5).into();
        }
        _ => {}
    }
    operation
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_prefers_names_supports_tokens_and_keeps_import_actions() {
        assert!(matches!(entries("image")[0].choice, Choice::Image));
        assert!(matches!(entries("TEXT")[0].choice, Choice::Node(kind) if kind.type_id=="text"));
        assert!(matches!(entries("outline text")[0].choice, Choice::Text));
        assert_eq!(entries("fractal NOI")[0].id, "fractal_noise");
        // Names rank first; nodes with a noise control (Layered material) follow.
        assert_eq!(
            entries("noise")
                .iter()
                .map(|entry| entry.id)
                .take(2)
                .collect::<Vec<_>>(),
            ["noise", "fractal_noise"]
        );
        assert!(entries("not-a-node-xyz").is_empty());
        assert_eq!(
            entries("").len(),
            registry::TYPES.len() + 1 + veylvik_document::recipes::Recipe::ALL.len()
        );
        assert!(matches!(
            entries("glow")[0].choice,
            Choice::Effect(veylvik_document::recipes::Recipe::Glow)
        ));
    }
    #[test]
    fn contextual_defaults_span_the_composition_and_validate() {
        for kind in registry::TYPES {
            if kind.type_id != "image" {
                operation(kind, 1920, 1080).validate().unwrap();
            }
        }
        let gradient = operation(&registry::GRADIENT, 1920, 1080);
        let now = veylvik_document::Time::new(0, 1).unwrap();
        assert_eq!(gradient.scalar("end_x").unwrap().at(now).unwrap(), 1919.0);
        assert_eq!(gradient.scalar("start_y").unwrap().at(now).unwrap(), 540.0);
        assert_eq!(
            operation(&registry::GRADIENT, 1, 1)
                .scalar("end_x")
                .unwrap()
                .at(now)
                .unwrap(),
            0.0
        );
    }
}
