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
            if kind.type_id == "image" {
                Entry {
                    choice: Choice::Image,
                    label: "Import image…",
                    description: "Import a PNG or OpenEXR image through the file picker.",
                    id: "image",
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
        id: "text",
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
        if words.iter().all(|word| title.contains(word)) {
            0
        } else if words
            .iter()
            .all(|word| format!("{title} {}", entry.description.to_lowercase()).contains(word))
        {
            1
        } else {
            2
        }
    };
    entries.retain(|entry| rank(entry) < 2);
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
        Operation::Fire {
            x,
            y,
            width: w,
            height: h,
            ..
        } => {
            *x = (width as f32 * 0.5).into();
            *y = (height as f32 * 0.7).into();
            *w = (width as f32 * 0.65).into();
            *h = (height as f32 * 0.5).into();
        }
        Operation::Ellipse {
            x,
            y,
            radius_x,
            radius_y,
            ..
        } => {
            *x = (width as f32 * 0.5).into();
            *y = (height as f32 * 0.5).into();
            let radius = width.min(height) as f32 * 0.25;
            *radius_x = radius.into();
            *radius_y = radius.into();
        }
        Operation::Affine {
            pivot_x, pivot_y, ..
        } => {
            *pivot_x = (width as f32 * 0.5).into();
            *pivot_y = (height as f32 * 0.5).into();
        }
        Operation::Gradient {
            start_y,
            end_x,
            end_y,
            ..
        } => {
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
        assert!(matches!(entries("TEXT")[0].choice, Choice::Text));
        assert_eq!(entries("fractal NOI")[0].id, "fractal_noise");
        assert_eq!(
            entries("noise")
                .iter()
                .map(|entry| entry.id)
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
