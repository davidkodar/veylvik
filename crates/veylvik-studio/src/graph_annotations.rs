//! Reading aids in the node graph: colour tags and notes on nodes, backdrops drawn around their member nodes wherever those sit, and an overview map of a graph larger than its panel. Everything here only reports actions; the document changes outside.
use super::{Action, Graph, NODE_SIZE, VisualNode};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, Vec2};
use std::collections::BTreeSet;
use veylvik_document::annotations::{self, Backdrop, Tag};
use veylvik_document::{Composition, NodeId};

pub(super) fn colour(tag: Tag) -> Color32 {
    let [r, g, b] = tag.rgb();
    Color32::from_rgb(r, g, b)
}
/// The nodes a selection of graph tiles stands for: a collapsed group is its members.
pub(super) fn node_ids(doc: &Composition, tiles: &BTreeSet<NodeId>) -> BTreeSet<NodeId> {
    let mut ids = BTreeSet::new();
    for id in tiles {
        if doc.nodes.contains_key(id) {
            ids.insert(*id);
        } else if let Some(group) = doc.groups.get(id) {
            ids.extend(group.members.iter().copied());
        }
    }
    ids
}
/// Whether the tile `id` shows any node of `backdrop`.
fn framed(doc: &Composition, backdrop: &Backdrop, id: NodeId) -> bool {
    backdrop.nodes.contains(&id)
        || doc
            .groups
            .get(&id)
            .is_some_and(|group| group.members.iter().any(|m| backdrop.nodes.contains(m)))
}
/// The backdrops with `nodes` framed by a new one; nodes leave the frames they were in.
pub(super) fn with_backdrop(doc: &Composition, nodes: BTreeSet<NodeId>) -> Vec<Backdrop> {
    let mut next = without_nodes(doc, &nodes);
    next.push(Backdrop::around(nodes));
    next
}
/// The backdrops with `nodes` taken out; a frame left empty is dropped.
pub(super) fn without_nodes(doc: &Composition, nodes: &BTreeSet<NodeId>) -> Vec<Backdrop> {
    let mut next = doc.backdrops.clone();
    for node in nodes {
        annotations::forget(&mut next, *node);
    }
    next
}
/// The overview's scale and origin: graph `bounds` fitted into `map`, centred.
pub(super) fn fitted(bounds: Rect, map: Rect) -> (f32, Vec2) {
    let scale =
        (map.width() / bounds.width().max(1.0)).min(map.height() / bounds.height().max(1.0));
    let offset = map.center().to_vec2() - bounds.center().to_vec2() * scale;
    (scale, offset)
}
impl Graph {
    fn tile(&self, canvas: Rect, id: NodeId) -> Option<Rect> {
        self.positions.get(&id).map(|position| {
            Rect::from_min_size(
                canvas.min + self.pan + position.to_vec2() * self.zoom,
                NODE_SIZE * self.zoom,
            )
        })
    }
    /// Frames behind the nodes; a click on a frame's name selects its nodes, its menu
    /// renames, recolours or removes it.
    pub(super) fn paint_backdrops(
        &mut self,
        ui: &mut egui::Ui,
        canvas: Rect,
        doc: &Composition,
        nodes: &[VisualNode],
        actions: &mut Vec<Action>,
    ) {
        for backdrop in &doc.backdrops {
            let tiles: Vec<NodeId> = nodes
                .iter()
                .filter(|node| framed(doc, backdrop, node.id))
                .map(|node| node.id)
                .collect();
            let Some(around) = tiles
                .iter()
                .filter_map(|id| self.tile(canvas, *id))
                .reduce(|a, b| a.union(b))
            else {
                continue;
            };
            let title_height = (22.0 * self.zoom).max(14.0);
            let frame = Rect::from_min_max(
                around.min - egui::vec2(16.0 * self.zoom, 12.0 * self.zoom + title_height),
                around.max + Vec2::splat(16.0 * self.zoom),
            );
            if !canvas.intersects(frame) {
                continue;
            }
            let ink = backdrop.tag.map_or(Color32::from_gray(150), colour);
            let painter = ui.painter().with_clip_rect(canvas);
            painter.rect_filled(frame, 8.0, ink.gamma_multiply(0.10));
            painter.rect_stroke(
                frame,
                8.0,
                Stroke::new(1.0_f32, ink.gamma_multiply(0.6)),
                egui::StrokeKind::Inside,
            );
            let title = Rect::from_min_size(frame.min, egui::vec2(frame.width(), title_height));
            painter.rect_filled(title, 8.0, ink.gamma_multiply(0.28));
            let name = if backdrop.label.is_empty() {
                "Backdrop"
            } else {
                backdrop.label.as_str()
            };
            let galley = painter.layout_no_wrap(
                name.to_owned(),
                FontId::proportional((13.0 * self.zoom).clamp(9.0, 16.0)),
                Color32::WHITE,
            );
            // The name goes where no other node covers it: left, else right, else the middle.
            let y = title.center().y - galley.size().y * 0.5;
            let spots = [
                title.left() + 8.0,
                title.right() - 8.0 - galley.size().x,
                title.center().x - galley.size().x * 0.5,
            ];
            let free = |x: f32| {
                let text = Rect::from_min_size(egui::pos2(x, y), galley.size());
                !nodes
                    .iter()
                    .filter(|node| !tiles.contains(&node.id))
                    .filter_map(|node| self.tile(canvas, node.id))
                    .any(|tile| tile.intersects(text))
            };
            let x = spots.into_iter().find(|x| free(*x)).unwrap_or(spots[0]);
            ui.painter().with_clip_rect(title.intersect(canvas)).galley(
                egui::pos2(x, y),
                galley,
                Color32::WHITE,
            );
            let response = ui
                .interact(
                    title.intersect(canvas),
                    ui.id().with(("backdrop", backdrop.id)),
                    egui::Sense::click(),
                )
                .on_hover_text("Click to select the nodes in this backdrop; right-click to rename, recolour or remove it");
            if response.clicked()
                && let Some(first) = tiles.first()
            {
                self.selection = tiles.iter().copied().collect();
                self.active = Some(*first);
                actions.push(Action::Select {
                    id: *first,
                    preview: false,
                });
            }
            let draft = ui.id().with(("backdrop-name", backdrop.id));
            response.context_menu(|ui| {
                let replace = |change: &dyn Fn(&mut Backdrop)| {
                    let mut next = doc.backdrops.clone();
                    if let Some(found) = next.iter_mut().find(|b| b.id == backdrop.id) {
                        change(found);
                    }
                    Action::Backdrops(next)
                };
                let mut text: String = ui.data_mut(|d| {
                    d.get_temp_mut_or_insert_with(draft, || backdrop.label.clone())
                        .clone()
                });
                ui.label("Backdrop name");
                let field = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .char_limit(annotations::MAX_LABEL)
                        .desired_width(180.0),
                );
                ui.data_mut(|d| d.insert_temp(draft, text.clone()));
                let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Rename").clicked() || entered {
                    let label = text.trim().to_owned();
                    actions.push(replace(&|b| b.label = label.clone()));
                    ui.data_mut(|d| d.remove::<String>(draft));
                    ui.close();
                }
                ui.separator();
                if let Some(tag) = tag_choice(ui, backdrop.tag) {
                    actions.push(replace(&|b| b.tag = tag));
                    ui.close();
                }
                ui.separator();
                if ui
                    .button("Remove backdrop")
                    .on_hover_text("The nodes stay; only the frame goes")
                    .clicked()
                {
                    actions.push(Action::Backdrops(
                        doc.backdrops
                            .iter()
                            .filter(|b| b.id != backdrop.id)
                            .cloned()
                            .collect(),
                    ));
                    ui.data_mut(|d| d.remove::<String>(draft));
                    ui.close();
                }
            });
        }
    }
    /// The tag, note and backdrop entries of a node's menu.
    pub(super) fn annotation_menu(
        &self,
        ui: &mut egui::Ui,
        doc: &Composition,
        node: &VisualNode,
        actions: &mut Vec<Action>,
    ) {
        if let Some(real) = doc.nodes.get(&node.id) {
            ui.menu_button("Colour tag", |ui| {
                if let Some(tag) = tag_choice(ui, real.tag) {
                    actions.push(Action::Annotate {
                        id: real.id,
                        tag,
                        note: real.note.clone(),
                    });
                    ui.close();
                }
            });
            ui.menu_button(
                if real.note.is_empty() {
                    "Add a note..."
                } else {
                    "Edit note..."
                },
                |ui| {
                    let draft = ui.id().with(("node-note", real.id));
                    let mut text: String = ui.data_mut(|d| {
                        d.get_temp_mut_or_insert_with(draft, || real.note.clone())
                            .clone()
                    });
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .char_limit(annotations::MAX_NOTE)
                            .desired_rows(3)
                            .desired_width(240.0)
                            .hint_text("What this node is for, or what is left to do"),
                    );
                    ui.data_mut(|d| d.insert_temp(draft, text.clone()));
                    ui.horizontal(|ui| {
                        let mut done = None;
                        if ui.button("Save note").clicked() {
                            done = Some(text.trim().replace('\r', ""));
                        }
                        if !real.note.is_empty() && ui.button("Remove note").clicked() {
                            done = Some(String::new());
                        }
                        if let Some(note) = done {
                            actions.push(Action::Annotate {
                                id: real.id,
                                tag: real.tag,
                                note,
                            });
                            ui.data_mut(|d| d.remove::<String>(draft));
                            ui.close();
                        }
                    });
                },
            );
        }
        let chosen = node_ids(doc, &self.selection);
        if !chosen.is_empty()
            && ui
                .button(format!("Backdrop around {} selected node(s)", self.selection.len()))
                .on_hover_text("A named, coloured frame that stays around these nodes however the graph is arranged")
                .clicked()
        {
            actions.push(Action::Backdrops(with_backdrop(doc, chosen.clone())));
            ui.close();
        }
        if doc
            .backdrops
            .iter()
            .any(|b| b.nodes.iter().any(|id| chosen.contains(id)))
            && ui.button("Take out of the backdrop").clicked()
        {
            actions.push(Action::Backdrops(without_nodes(doc, &chosen)));
            ui.close();
        }
    }
    /// A node's tag as a bar down its left side and its note beneath it.
    pub(super) fn paint_annotation(
        &self,
        ui: &egui::Ui,
        canvas: Rect,
        rect: Rect,
        doc: &Composition,
        id: NodeId,
    ) {
        let Some(node) = doc.nodes.get(&id) else {
            return;
        };
        let painter = ui.painter().with_clip_rect(canvas);
        if let Some(tag) = node.tag {
            // A band behind the node's name and a bar down its side: readable at any zoom.
            painter.rect_filled(
                Rect::from_min_size(
                    rect.min,
                    egui::vec2(rect.width(), (26.0 * self.zoom).max(8.0)),
                ),
                egui::CornerRadius {
                    nw: 5,
                    ne: 5,
                    sw: 0,
                    se: 0,
                },
                colour(tag).gamma_multiply(0.45),
            );
            painter.rect_filled(
                Rect::from_min_size(
                    rect.min,
                    egui::vec2((7.0 * self.zoom).max(4.0), rect.height()),
                ),
                egui::CornerRadius {
                    nw: 5,
                    sw: 5,
                    ne: 0,
                    se: 0,
                },
                colour(tag),
            );
        }
        if !node.note.is_empty() {
            // A folded corner says there is a note even when its words are too small to show.
            let corner = (14.0 * self.zoom).max(7.0);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    rect.right_bottom() - egui::vec2(corner, 0.0),
                    rect.right_bottom() - egui::vec2(0.0, corner),
                    rect.right_bottom(),
                ],
                Color32::from_rgb(232, 200, 110),
                Stroke::NONE,
            ));
        }
        if !node.note.is_empty() && self.zoom >= 0.4 {
            let galley = painter.layout(
                node.note.clone(),
                FontId::proportional((11.0 * self.zoom).clamp(9.0, 13.0)),
                Color32::from_rgb(232, 214, 150),
                rect.width(),
            );
            let area = Rect::from_min_size(
                rect.left_bottom() + egui::vec2(0.0, 3.0),
                egui::vec2(rect.width(), (34.0 * self.zoom).max(22.0)),
            );
            ui.painter().with_clip_rect(area.intersect(canvas)).galley(
                area.min,
                galley,
                Color32::WHITE,
            );
        }
    }
    /// A map of the whole graph in the corner while part of it is out of view; click or
    /// drag in it to look there.
    pub(super) fn overview(
        &mut self,
        ui: &mut egui::Ui,
        canvas: Rect,
        nodes: &[VisualNode],
        doc: &Composition,
    ) {
        let tiles: Vec<(NodeId, Rect)> = nodes
            .iter()
            .filter_map(|node| {
                self.positions
                    .get(&node.id)
                    .map(|p| (node.id, Rect::from_min_size(*p, NODE_SIZE)))
            })
            .collect();
        let Some(bounds) = tiles.iter().map(|(_, r)| *r).reduce(|a, b| a.union(b)) else {
            return;
        };
        // The part of the graph the panel shows, in graph coordinates.
        let view = Rect::from_min_max(
            ((-self.pan) / self.zoom).to_pos2(),
            ((canvas.size() - self.pan) / self.zoom).to_pos2(),
        );
        if tiles.len() < 2
            || view.contains_rect(bounds)
            || canvas.width() < 420.0
            || canvas.height() < 220.0
        {
            return;
        }
        let bounds = bounds.expand(40.0);
        let map = Rect::from_min_size(
            canvas.right_bottom() - egui::vec2(190.0, 124.0),
            egui::vec2(180.0, 114.0),
        );
        let (scale, offset) = fitted(bounds, map.shrink(6.0));
        let place = |p: Pos2| (p.to_vec2() * scale + offset).to_pos2();
        let response = ui
            .interact(
                map,
                ui.id().with("graph-overview"),
                egui::Sense::click_and_drag(),
            )
            .on_hover_text("The whole graph; click or drag to look at another part");
        if (response.clicked() || response.dragged())
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let target = ((pointer.to_vec2() - offset) / scale).to_pos2();
            self.pan = canvas.size() * 0.5 - target.to_vec2() * self.zoom;
        }
        let painter = ui.painter().with_clip_rect(map);
        painter.rect_filled(map, 6.0, Color32::from_rgb(12, 13, 17));
        painter.rect_stroke(
            map,
            6.0,
            Stroke::new(1.0_f32, Color32::from_gray(70)),
            egui::StrokeKind::Inside,
        );
        for (id, tile) in &tiles {
            let ink = doc.nodes.get(id).and_then(|node| node.tag).map_or(
                Color32::from_gray(if self.selection.contains(id) {
                    230
                } else {
                    130
                }),
                colour,
            );
            let small = Rect::from_min_max(place(tile.min), place(tile.max));
            painter.rect_filled(
                small.expand2((Vec2::splat(1.5) - small.size() * 0.5).max(Vec2::ZERO)),
                1.0,
                ink,
            );
        }
        painter.rect_stroke(
            Rect::from_min_max(place(view.min), place(view.max)).intersect(map),
            2.0,
            Stroke::new(1.5_f32, Color32::WHITE),
            egui::StrokeKind::Inside,
        );
    }
}
/// Eight colour buttons and None; Some(choice) when one was clicked.
fn tag_choice(ui: &mut egui::Ui, current: Option<Tag>) -> Option<Option<Tag>> {
    let mut chosen = None;
    ui.label("Colour");
    ui.horizontal_wrapped(|ui| {
        ui.set_max_width(190.0);
        for tag in Tag::ALL {
            let button = egui::Button::new("")
                .fill(colour(tag))
                .min_size(egui::vec2(20.0, 20.0))
                .stroke(Stroke::new(
                    if current == Some(tag) {
                        2.0_f32
                    } else {
                        0.0_f32
                    },
                    Color32::WHITE,
                ));
            if ui.add(button).on_hover_text(tag.label()).clicked() {
                chosen = Some(Some(tag));
            }
        }
    });
    if ui
        .add_enabled(current.is_some(), egui::Button::new("No colour"))
        .clicked()
    {
        chosen = Some(None);
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::{Node, Operation, Time};
    #[test]
    fn backdrops_take_nodes_from_older_frames_and_the_overview_fits_the_graph() {
        let mut doc = veylvik_document::blank(64, 36, Time::new(24, 1).unwrap()).unwrap();
        let mut ids = vec![doc.output];
        for name in ["A", "B"] {
            let node = Node::new(
                name,
                Operation::Solid(veylvik_document::nodes::Solid { color: [0.0; 4] }),
                &[],
            );
            ids.push(node.id);
            doc.nodes.insert(node.id, node);
        }
        doc.backdrops = with_backdrop(&doc, [ids[0], ids[1]].into());
        // Framing B with one node of the first frame moves that node over.
        doc.backdrops = with_backdrop(&doc, [ids[1], ids[2]].into());
        assert_eq!(doc.backdrops.len(), 2);
        assert_eq!(doc.backdrops[0].nodes, [ids[0]].into());
        assert_eq!(doc.backdrops[1].nodes, [ids[1], ids[2]].into());
        veylvik_document::annotations::validate(&doc).unwrap();
        // Taking the last node out drops the frame; unknown tiles stand for nothing.
        doc.backdrops = without_nodes(&doc, &[ids[0]].into());
        assert_eq!(doc.backdrops.len(), 1);
        assert!(
            framed(&doc, &doc.backdrops[0], ids[2]) && !framed(&doc, &doc.backdrops[0], ids[0])
        );
        let stranger = Node::new(
            "X",
            Operation::Solid(veylvik_document::nodes::Solid { color: [0.0; 4] }),
            &[],
        )
        .id;
        assert!(node_ids(&doc, &[stranger].into()).is_empty());
        assert_eq!(node_ids(&doc, &[ids[1]].into()), [ids[1]].into());
        // A wide graph fills the map's width and is centred in its height.
        let bounds = Rect::from_min_max(egui::pos2(-100.0, 0.0), egui::pos2(900.0, 200.0));
        let map = Rect::from_min_size(egui::pos2(500.0, 300.0), egui::vec2(100.0, 60.0));
        let (scale, offset) = fitted(bounds, map);
        assert!((scale - 0.1).abs() < 1e-6);
        let place = |p: Pos2| (p.to_vec2() * scale + offset).to_pos2();
        assert!(
            (place(bounds.min).x - 500.0).abs() < 1e-3
                && (place(bounds.max).x - 600.0).abs() < 1e-3
        );
        assert!((place(bounds.center()).y - 330.0).abs() < 1e-3);
    }
}
