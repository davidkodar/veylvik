//! Indexed node navigation. Only visible result rows are laid out each frame.
use eframe::egui;
use veylvik_document::{Composition, NodeId};
struct Entry {
    id: NodeId,
    label: String,
    kind: &'static str,
    search: String,
}
fn index(doc: &Composition) -> Vec<Entry> {
    let mut entries: Vec<_> = doc
        .nodes
        .values()
        .map(|node| {
            let kind = node.operation.descriptor();
            let mut chars = node.label.chars();
            let mut label: String = chars.by_ref().take(256).collect();
            if chars.next().is_some() {
                label.push('…');
            }
            let group = doc.groups.values().find(|g| g.output == node.id);
            if let Some(group) = group {
                label = group.label.clone();
            }
            Entry {
                id: node.id,
                label,
                kind: if group.is_some() {
                    "Effect group"
                } else {
                    kind.label
                },
                search: format!(
                    "{} {} {} {} {}",
                    group.map_or("", |g| g.label.as_str()),
                    node.label,
                    kind.type_id.replace('_', " "),
                    kind.label,
                    node.id
                )
                .to_lowercase(),
            }
        })
        .collect();
    entries.sort_by_cached_key(|entry| entry.label.to_lowercase());
    entries
}
fn matches(entries: &[Entry], query: &str) -> Vec<usize> {
    let query = query.to_lowercase();
    let words: Vec<_> = query.split_whitespace().collect();
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| words.iter().all(|word| entry.search.contains(word)))
        .map(|(index, _)| index)
        .collect()
}
#[derive(Default)]
pub struct Controls {
    pub open: bool,
    entries: Vec<Entry>,
    matches: Vec<usize>,
    query: String,
    selected: usize,
    focus: bool,
}
impl Controls {
    pub fn open(&mut self, doc: &Composition, selected: NodeId) {
        self.entries = index(doc);
        self.matches = (0..self.entries.len()).collect();
        self.query.clear();
        self.selected = self
            .entries
            .iter()
            .position(|entry| entry.id == selected)
            .unwrap_or(0);
        self.open = true;
        self.focus = true;
    }
    pub fn show(&mut self, ctx: &egui::Context) -> Option<NodeId> {
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
            self.entries.clear();
            self.matches.clear();
            return None;
        }
        let mut open = self.open;
        let mut chosen = None;
        egui::Window::new("Find node")
            .id(egui::Id::new("node-finder"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(520.0)
            .show(ctx, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Search names, types or UUIDs…")
                        .char_limit(128)
                        .desired_width(f32::INFINITY),
                );
                let reveal = self.focus || response.changed() || up || down;
                if self.focus {
                    response.request_focus();
                    self.focus = false;
                }
                if response.changed() {
                    self.matches = matches(&self.entries, &self.query);
                    self.selected = 0;
                }
                if !self.matches.is_empty() {
                    if down {
                        self.selected = (self.selected + 1) % self.matches.len();
                    }
                    if up {
                        self.selected =
                            (self.selected + self.matches.len() - 1) % self.matches.len();
                    }
                }
                ui.separator();
                let height = ui.spacing().interact_size.y.max(
                    ui.text_style_height(&egui::TextStyle::Button)
                        + 2.0 * ui.spacing().button_padding.y,
                );
                let mut scroll = egui::ScrollArea::vertical().max_height(280.0);
                if reveal {
                    scroll = scroll.vertical_scroll_offset(
                        self.selected.saturating_sub(5) as f32
                            * (height + ui.spacing().item_spacing.y),
                    );
                }
                if self.matches.is_empty() {
                    ui.weak("No matching nodes");
                }
                scroll.show_rows(ui, height, self.matches.len(), |ui, rows| {
                    for row in rows {
                        let entry = &self.entries[self.matches[row]];
                        let label = format!("{} · {}", entry.label, entry.kind);
                        if ui
                            .add_sized(
                                [ui.available_width(), height],
                                egui::Button::selectable(row == self.selected, label).truncate(),
                            )
                            .clicked()
                        {
                            chosen = Some(entry.id);
                        }
                    }
                });
                ui.separator();
                if let Some(index) = self.matches.get(self.selected) {
                    let entry = &self.entries[*index];
                    ui.label(entry.kind);
                    ui.monospace(entry.id.to_string());
                    if enter {
                        chosen = Some(entry.id);
                    }
                }
                ui.weak(format!(
                    "{} matches · Up / Down choose · Enter preview · Esc cancel",
                    self.matches.len()
                ));
            });
        self.open = open && chosen.is_none();
        if !self.open {
            self.entries.clear();
            self.matches.clear();
        }
        chosen
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::{Node, Operation, Time};
    #[test]
    fn indexed_search_handles_types_ids_unicode_and_long_labels_without_truncating_search() {
        let mut doc = veylvik_document::blank(1, 1, Time::new(24, 1).unwrap()).unwrap();
        doc.nodes.get_mut(&doc.output).unwrap().label = "Ångström Background".into();
        let node = Node::new(
            &format!("{} TailMarker", "ä".repeat(300)),
            Operation::FractalNoise(veylvik_document::nodes::FractalNoise {
                seed: 1,
                scale: 32.0.into(),
                octaves: 3.0.into(),
                roughness: 0.5.into(),
                evolution: 0.0.into(),
            }),
            &[],
        );
        let id = node.id;
        doc.nodes.insert(id, node);
        let entries = index(&doc);
        assert_eq!(entries[matches(&entries, "ÅNGSTRÖM")[0]].id, doc.output);
        assert_eq!(entries[matches(&entries, "fractal TailMarker")[0]].id, id);
        assert_eq!(entries[matches(&entries, &id.to_string()[..8])[0]].id, id);
        assert_eq!(
            entries
                .iter()
                .find(|e| e.id == id)
                .unwrap()
                .label
                .chars()
                .count(),
            257
        );
        assert!(matches(&entries, "no-such-node").is_empty());
        assert_eq!(matches(&entries, "").len(), 2);
    }
}
