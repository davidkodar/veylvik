//! Graph navigation and drawing. Pixel rendering and document commits stay outside.
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use std::collections::BTreeMap;
use veylvik_document::{Composition, NodeId, NodePosition};
const NODE_SIZE: Vec2 = egui::vec2(160.0, 60.0);
pub enum Action {
    Select {
        id: NodeId,
        preview: bool,
    },
    Place {
        id: NodeId,
        position: NodePosition,
    },
    Arrange(Vec<(NodeId, NodePosition)>),
    Connect {
        source: NodeId,
        target: NodeId,
        port: &'static str,
    },
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Socket {
    Output(NodeId),
    Input(NodeId, &'static str),
}
impl Socket {
    fn connection(self, other: Self) -> Option<Action> {
        match (self, other) {
            (Self::Output(source), Self::Input(target, port))
            | (Self::Input(target, port), Self::Output(source))
                if source != target =>
            {
                Some(Action::Connect {
                    source,
                    target,
                    port,
                })
            }
            _ => None,
        }
    }
}
struct Drag {
    id: NodeId,
    start: Pos2,
    current: Pos2,
}
pub struct Graph {
    revision: Option<u64>,
    positions: BTreeMap<NodeId, Pos2>,
    automatic: BTreeMap<NodeId, Pos2>,
    zoom: f32,
    pan: Vec2,
    canvas: Option<Rect>,
    drag: Option<Drag>,
    wire: Option<Socket>,
}
impl Default for Graph {
    fn default() -> Self {
        Self {
            revision: None,
            positions: BTreeMap::new(),
            automatic: BTreeMap::new(),
            zoom: 1.0,
            pan: Vec2::ZERO,
            canvas: None,
            drag: None,
            wire: None,
        }
    }
}
impl Graph {
    pub fn dragging(&self) -> bool {
        self.drag.is_some() || self.wire.is_some()
    }
    pub fn insertion_position(&self, ctx: &egui::Context) -> NodePosition {
        let Some(canvas) = self.canvas else {
            return NodePosition { x: 0.0, y: 0.0 };
        };
        let screen = ctx
            .pointer_hover_pos()
            .filter(|p| canvas.contains(*p))
            .unwrap_or(canvas.center());
        let world = (screen - canvas.min - self.pan) / self.zoom - NODE_SIZE * 0.5;
        let desired = egui::pos2(
            world.x.clamp(-1_000_000.0, 1_000_000.0),
            world.y.clamp(-1_000_000.0, 1_000_000.0),
        );
        let position = self.vacant_position(desired);
        NodePosition {
            x: position.x,
            y: position.y,
        }
    }
    pub fn place_branch_copy(
        &mut self,
        doc: &Composition,
        revision: u64,
        copy: &mut veylvik_document::editing::BranchCopy,
    ) -> veylvik_document::Result<()> {
        self.sync(doc, revision);
        let bounds = |ids: Vec<NodeId>| {
            ids.into_iter().fold(Rect::NOTHING, |rect, id| {
                rect.union(Rect::from_min_size(self.positions[&id], NODE_SIZE))
            })
        };
        let all = bounds(self.positions.keys().copied().collect());
        let branch = bounds(copy.mapping.keys().copied().collect());
        // Try below, right, above, then left. A separating axis guarantees no overlap.
        let candidates = [
            egui::vec2(0.0, all.max.y + 60.0 - branch.min.y),
            egui::vec2(all.max.x + 60.0 - branch.min.x, 0.0),
            egui::vec2(0.0, all.min.y - 60.0 - branch.max.y),
            egui::vec2(all.min.x - 60.0 - branch.max.x, 0.0),
        ];
        let offset = candidates
            .into_iter()
            .find(|offset| {
                copy.mapping.keys().all(|id| {
                    let p = self.positions[id] + *offset;
                    p.x.abs() <= 1_000_000.0 && p.y.abs() <= 1_000_000.0
                })
            })
            .ok_or_else(|| {
                veylvik_document::Error(
                    "No room to place the copied branch within graph coordinate limits".into(),
                )
            })?;
        for (old, new) in &copy.mapping {
            let position = self.positions[old] + offset;
            copy.nodes.get_mut(new).unwrap().position = Some(NodePosition {
                x: position.x,
                y: position.y,
            });
        }
        Ok(())
    }
    pub fn place_effect(
        &mut self,
        doc: &Composition,
        revision: u64,
        input: NodeId,
        effect: &mut veylvik_document::recipes::Effect,
        anchor: Option<NodePosition>,
    ) -> veylvik_document::Result<()> {
        self.sync(doc, revision);
        let count = effect.nodes.len();
        let anchor = anchor
            .map(|p| egui::pos2(p.x, p.y))
            .unwrap_or(self.positions[&input] + egui::vec2(195.0 * count as f32, 0.0));
        let anchor = egui::pos2(
            anchor.x.clamp(
                -1_000_000.0 + 195.0 * count.saturating_sub(1) as f32,
                1_000_000.0,
            ),
            anchor.y.clamp(-1_000_000.0, 1_000_000.0),
        );
        let base: Vec<_> = (0..count)
            .map(|index| anchor - egui::vec2(195.0 * (count - index - 1) as f32, 0.0))
            .collect();
        let bounds = base.iter().fold(Rect::NOTHING, |rect, p| {
            rect.union(Rect::from_min_size(*p, NODE_SIZE))
        });
        let all = self.positions.values().fold(Rect::NOTHING, |rect, p| {
            rect.union(Rect::from_min_size(*p, NODE_SIZE))
        });
        let candidates = [
            Vec2::ZERO,
            egui::vec2(0.0, all.max.y + 60.0 - bounds.min.y),
            egui::vec2(all.max.x + 60.0 - bounds.min.x, 0.0),
            egui::vec2(0.0, all.min.y - 60.0 - bounds.max.y),
            egui::vec2(all.min.x - 60.0 - bounds.max.x, 0.0),
        ];
        let offset = candidates
            .into_iter()
            .find(|offset| {
                base.iter().all(|p| {
                    let p = *p + *offset;
                    p.x.is_finite()
                        && p.y.is_finite()
                        && p.x.abs() <= 1_000_000.0
                        && p.y.abs() <= 1_000_000.0
                }) && self.positions.values().all(|p| {
                    !Rect::from_min_size(*p, NODE_SIZE)
                        .expand(12.0)
                        .intersects(bounds.translate(*offset))
                })
            })
            .ok_or_else(|| {
                veylvik_document::Error(
                    "No room to place the effect within graph coordinate limits".into(),
                )
            })?;
        for (node, p) in effect.nodes.iter_mut().zip(base) {
            let p = p + offset;
            node.position = Some(NodePosition { x: p.x, y: p.y });
        }
        Ok(())
    }
    fn vacant_position(&self, desired: Pos2) -> Pos2 {
        let collisions = |candidate: Pos2| {
            self.positions.values().any(|p| {
                Rect::from_min_size(*p, NODE_SIZE)
                    .expand(12.0)
                    .intersects(Rect::from_min_size(candidate, NODE_SIZE))
            })
        };
        if !collisions(desired) {
            return desired;
        }
        let mut candidates = Vec::new();
        for axis in 0..2 {
            let other = 1 - axis;
            let mut intervals: Vec<_> = self
                .positions
                .values()
                .filter(|p| {
                    desired[other] + NODE_SIZE[other] + 12.0 >= p[other]
                        && desired[other] <= p[other] + NODE_SIZE[other] + 12.0
                })
                .map(|p| {
                    (
                        p[axis] - NODE_SIZE[axis] - 12.0,
                        p[axis] + NODE_SIZE[axis] + 12.0,
                    )
                })
                .collect();
            intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut merged: Vec<(f32, f32)> = Vec::new();
            for (low, high) in intervals {
                if let Some(last) = merged.last_mut()
                    && low <= last.1
                {
                    last.1 = last.1.max(high);
                } else {
                    merged.push((low, high));
                }
            }
            if let Some((low, high)) = merged
                .into_iter()
                .find(|(low, high)| *low <= desired[axis] && desired[axis] <= *high)
            {
                for value in [low - 1.0, high + 1.0] {
                    if (-1_000_000.0..=1_000_000.0).contains(&value) {
                        let mut candidate = desired;
                        candidate[axis] = value;
                        candidates.push(candidate);
                    }
                }
            }
        }
        let visible = |p: Pos2| {
            self.canvas.is_some_and(|canvas| {
                canvas.contains_rect(Rect::from_min_size(
                    canvas.min + self.pan + p.to_vec2() * self.zoom,
                    NODE_SIZE * self.zoom,
                ))
            })
        };
        candidates
            .into_iter()
            .filter(|p| !collisions(*p))
            .min_by(|a, b| {
                visible(*b)
                    .cmp(&visible(*a))
                    .then_with(|| a.distance_sq(desired).total_cmp(&b.distance_sq(desired)))
            })
            .unwrap_or(desired)
    }
    fn reveal(&mut self, id: NodeId, canvas: Rect) {
        let Some(position) = self.positions.get(&id) else {
            return;
        };
        let rect = Rect::from_min_size(
            canvas.min + self.pan + position.to_vec2() * self.zoom,
            NODE_SIZE * self.zoom,
        );
        let visible = canvas.shrink(8.0);
        for axis in 0..2 {
            self.pan[axis] += if rect.size()[axis] > visible.size()[axis] {
                visible.center()[axis] - rect.center()[axis]
            } else if rect.min[axis] < visible.min[axis] {
                visible.min[axis] - rect.min[axis]
            } else if rect.max[axis] > visible.max[axis] {
                visible.max[axis] - rect.max[axis]
            } else {
                0.0
            };
        }
    }
    fn sync(&mut self, doc: &Composition, revision: u64) {
        if self.revision == Some(revision) {
            return;
        }
        self.wire = None;
        self.revision = Some(revision);
        self.automatic.clear();
        self.positions.clear();
        let mut depths = BTreeMap::new();
        let mut rows = BTreeMap::<usize, usize>::new();
        for id in doc.validate().unwrap_or_default() {
            let node = &doc.nodes[&id];
            let depth = node
                .inputs
                .values()
                .map(|id| depths[id] + 1)
                .max()
                .unwrap_or(0);
            depths.insert(id, depth);
            let row = rows.entry(depth).or_default();
            let automatic = egui::pos2(25.0 + depth as f32 * 195.0, 20.0 + *row as f32 * 90.0);
            *row += 1;
            self.automatic.insert(id, automatic);
            self.positions.insert(
                id,
                node.position
                    .map(|p| egui::pos2(p.x, p.y))
                    .unwrap_or(automatic),
            );
        }
    }
    fn zoom_at(&mut self, zoom: f32, point: Pos2, canvas: Rect) {
        let zoom = zoom.clamp(0.15, 2.5);
        let point = point - canvas.min;
        self.pan = point - (point - self.pan) * (zoom / self.zoom);
        self.zoom = zoom;
    }
    fn fit(&mut self, canvas: Rect) {
        let bounds = self.positions.values().fold(Rect::NOTHING, |rect, pos| {
            rect.union(Rect::from_min_size(*pos, NODE_SIZE))
        });
        if !bounds.is_finite() {
            return;
        }
        let available = (canvas.size() - egui::vec2(30.0, 30.0)).max(egui::vec2(1.0, 1.0));
        self.zoom = (available.x / bounds.width())
            .min(available.y / bounds.height())
            .clamp(0.15, 2.5);
        self.pan =
            (canvas.size() - bounds.size() * self.zoom) * 0.5 - bounds.min.to_vec2() * self.zoom;
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        doc: &Composition,
        revision: u64,
        selected: NodeId,
        accent: Color32,
    ) -> Vec<Action> {
        let newly_inserted = self.revision.is_some() && !self.positions.contains_key(&selected);
        self.sync(doc, revision);
        let mut actions = Vec::new();
        let mut fit = false;
        let mut zoom_factor = 1.0;
        ui.horizontal(|ui| {
            ui.heading("Nodes");
            if ui.button("Fit").clicked() {
                fit = true;
            }
            if ui.button("−").clicked() {
                zoom_factor = 0.8;
            }
            ui.label(format!("{:.0}%", self.zoom * 100.0));
            if ui.button("+").clicked() {
                zoom_factor = 1.25;
            }
            if ui.button("Arrange").clicked() {
                actions.push(Action::Arrange(
                    self.automatic
                        .iter()
                        .map(|(id, p)| (*id, NodePosition { x: p.x, y: p.y }))
                        .collect(),
                ));
            }
            ui.weak("Drag background to pan · Ctrl+scroll to zoom · double-click to preview");
        });
        let (canvas, response) = ui.allocate_exact_size(
            ui.available_size().max(egui::vec2(1.0, 1.0)),
            egui::Sense::click_and_drag(),
        );
        self.canvas = Some(canvas);
        if newly_inserted {
            self.reveal(selected, canvas);
        }
        if fit {
            self.fit(canvas);
        }
        if zoom_factor != 1.0 {
            self.zoom_at(self.zoom * zoom_factor, canvas.center(), canvas);
        }
        if ui.is_enabled() && response.contains_pointer() {
            let (scroll, zoom, point) = ui.input(|input| {
                (
                    input.smooth_scroll_delta,
                    input.zoom_delta(),
                    input.pointer.hover_pos().unwrap_or(canvas.center()),
                )
            });
            if zoom != 1.0 {
                self.zoom_at(self.zoom * zoom, point, canvas);
            } else {
                self.pan += scroll;
            }
        }
        let painter = ui.painter().with_clip_rect(canvas);
        painter.rect_filled(canvas, 0.0, Color32::from_rgb(18, 21, 27));
        let spacing = (32.0 * self.zoom).max(10.0);
        let offset = egui::vec2(
            self.pan.x.rem_euclid(spacing),
            self.pan.y.rem_euclid(spacing),
        );
        let mut y = canvas.top() + offset.y;
        while y < canvas.bottom() {
            let mut x = canvas.left() + offset.x;
            while x < canvas.right() {
                painter.circle_filled(egui::pos2(x, y), 0.6, Color32::from_gray(43));
                x += spacing;
            }
            y += spacing;
        }
        if let Some(drag) = &self.drag {
            self.positions.insert(drag.id, drag.current);
        }
        let mut sockets = Vec::new();
        for node in doc.nodes.values() {
            let origin = canvas.min + self.pan + self.positions[&node.id].to_vec2() * self.zoom;
            sockets.push((
                Socket::Output(node.id),
                origin + egui::vec2(160.0, 30.0) * self.zoom,
            ));
            for (index, port) in node.operation.inputs().iter().enumerate() {
                let y = 60.0 * (index + 1) as f32 / (node.operation.inputs().len() + 1) as f32;
                sockets.push((
                    Socket::Input(node.id, port),
                    origin + egui::vec2(0.0, y) * self.zoom,
                ));
            }
        }
        for node in doc.nodes.values() {
            for (index, port) in node.operation.inputs().iter().enumerate() {
                let source = &node.inputs[*port];
                let from = canvas.min
                    + self.pan
                    + (self.positions[source].to_vec2() + egui::vec2(160.0, 30.0)) * self.zoom;
                let to = canvas.min
                    + self.pan
                    + (self.positions[&node.id].to_vec2()
                        + egui::vec2(
                            0.0,
                            60.0 * (index + 1) as f32 / (node.operation.inputs().len() + 1) as f32,
                        ))
                        * self.zoom;
                let bend = egui::vec2(45.0 * self.zoom, 0.0);
                painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                    [from, from + bend, to - bend, to],
                    false,
                    Color32::TRANSPARENT,
                    egui::Stroke::new(1.5_f32, Color32::from_gray(100)),
                ));
            }
        }
        let mut node_dragged = false;
        for node in doc.nodes.values() {
            let rect = Rect::from_min_size(
                canvas.min + self.pan + self.positions[&node.id].to_vec2() * self.zoom,
                NODE_SIZE * self.zoom,
            );
            if !canvas.intersects(rect) && self.drag.as_ref().is_none_or(|drag| drag.id != node.id)
            {
                continue;
            }
            let response = ui
                .interact(
                    rect.shrink2(egui::vec2(9.0, 0.0) * self.zoom)
                        .intersect(canvas),
                    ui.id().with(node.id),
                    egui::Sense::click_and_drag(),
                )
                .on_hover_text(&node.label);
            if response.drag_started() {
                self.drag = Some(Drag {
                    id: node.id,
                    start: self.positions[&node.id],
                    current: self.positions[&node.id],
                });
            }
            if response.dragged()
                && let Some(drag) = &mut self.drag
            {
                drag.current += response.drag_delta() / self.zoom;
                drag.current.x = drag.current.x.clamp(-1_000_000.0, 1_000_000.0);
                drag.current.y = drag.current.y.clamp(-1_000_000.0, 1_000_000.0);
                node_dragged = true;
            }
            if response.drag_stopped()
                && let Some(drag) = self.drag.take()
                && drag.start.distance(drag.current) > 0.01
            {
                actions.push(Action::Place {
                    id: drag.id,
                    position: NodePosition {
                        x: drag.current.x,
                        y: drag.current.y,
                    },
                });
            }
            if response.clicked() || response.double_clicked() {
                actions.push(Action::Select {
                    id: node.id,
                    preview: response.double_clicked(),
                });
            }
            let selected = node.id == selected;
            painter.rect_filled(rect, 0.0, Color32::from_rgb(36, 41, 50));
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(
                    if selected { 2.0_f32 } else { 1.0_f32 },
                    if selected {
                        accent
                    } else {
                        Color32::from_gray(65)
                    },
                ),
                egui::StrokeKind::Inside,
            );
            let text = painter.with_clip_rect(rect.shrink(5.0 * self.zoom));
            text.text(
                rect.min + egui::vec2(10.0, 15.0) * self.zoom,
                egui::Align2::LEFT_CENTER,
                &node.label,
                egui::FontId::proportional(13.0 * self.zoom),
                Color32::WHITE,
            );
            let subtitle = if node.id == doc.output {
                "Composition output"
            } else {
                node.operation.descriptor().label
            };
            text.text(
                rect.min + egui::vec2(10.0, 41.0) * self.zoom,
                egui::Align2::LEFT_CENTER,
                subtitle,
                egui::FontId::proportional(11.0 * self.zoom),
                Color32::from_gray(160),
            );
        }
        let pointer = ui.input(|input| input.pointer.hover_pos());
        let radius = (8.0 * self.zoom).max(5.0);
        let hovered = pointer.filter(|p| canvas.contains(*p)).and_then(|p| {
            sockets
                .iter()
                .filter(|(socket, position)| {
                    position.distance(p) <= radius
                        && self
                            .wire
                            .is_none_or(|from| from.connection(*socket).is_some())
                })
                .min_by(|a, b| a.1.distance_sq(p).total_cmp(&b.1.distance_sq(p)))
                .map(|(socket, _)| *socket)
        });
        for (socket, position) in &sockets {
            if !canvas.contains(*position) {
                continue;
            }
            let hit = ui
                .interact(
                    Rect::from_center_size(*position, Vec2::splat(radius * 2.0)).intersect(canvas),
                    ui.id().with(socket),
                    egui::Sense::drag(),
                )
                .on_hover_text(match socket {
                    Socket::Output(_) => "Output",
                    Socket::Input(_, port) => port,
                });
            if hit.drag_started() {
                self.wire = Some(*socket);
                self.drag = None;
            }
            painter.circle_filled(
                *position,
                (4.0 * self.zoom).max(2.5),
                if hovered == Some(*socket) || self.wire == Some(*socket) {
                    accent
                } else {
                    Color32::from_gray(180)
                },
            );
        }
        let wiring = self.wire.is_some();
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) || !ui.is_enabled() {
            self.wire = None;
        }
        if let Some(from) = self.wire {
            if let Some(pointer) = pointer
                && let Some((_, start)) = sockets.iter().find(|(socket, _)| *socket == from)
            {
                let (start, end) = match from {
                    Socket::Output(_) => (*start, pointer),
                    Socket::Input(..) => (pointer, *start),
                };
                let bend = egui::vec2(45.0 * self.zoom, 0.0);
                painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                    [start, start + bend, end - bend, end],
                    false,
                    Color32::TRANSPARENT,
                    egui::Stroke::new(2.0_f32, accent),
                ));
            }
            if ui.input(|input| input.pointer.any_released()) {
                if let Some(action) = hovered.and_then(|to| from.connection(to)) {
                    actions.push(action);
                }
                self.wire = None;
            }
        }
        if response.dragged() && !node_dragged && self.drag.is_none() && !wiring {
            self.pan += response.drag_delta();
        }
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn node_id() -> NodeId {
        veylvik_document::Node::new(
            "test",
            veylvik_document::Operation::Solid { color: [0.0; 4] },
            &[],
        )
        .id
    }
    #[test]
    fn effect_layout_stays_inside_coordinate_limits_and_does_not_overlap() {
        let mut doc =
            veylvik_document::blank(8, 8, veylvik_document::Time::new(24, 1).unwrap()).unwrap();
        doc.nodes.get_mut(&doc.output).unwrap().position = Some(NodePosition {
            x: 1_000_000.0,
            y: 999_950.0,
        });
        let mut graph = Graph::default();
        let mut effect = veylvik_document::recipes::Recipe::Outline
            .build(&doc, doc.output)
            .unwrap();
        graph
            .place_effect(&doc, 0, doc.output, &mut effect, None)
            .unwrap();
        let mut rectangles = vec![Rect::from_min_size(graph.positions[&doc.output], NODE_SIZE)];
        for node in &effect.nodes {
            let p = node.position.unwrap();
            p.validate().unwrap();
            let rect = Rect::from_min_size(egui::pos2(p.x, p.y), NODE_SIZE);
            assert!(rectangles.iter().all(|old| !old.intersects(rect)));
            rectangles.push(rect);
        }
    }
    #[test]
    fn branch_copy_layout_preserves_offsets_and_avoids_existing_nodes() {
        let mut doc =
            veylvik_document::blank(8, 8, veylvik_document::Time::new(24, 1).unwrap()).unwrap();
        let first = doc.output;
        doc.nodes.get_mut(&first).unwrap().position = Some(NodePosition {
            x: 100.0,
            y: 999_950.0,
        });
        let mut effect = veylvik_document::Node::new(
            "Effect",
            veylvik_document::Operation::Exposure { stops: 1.0.into() },
            &[("image", first)],
        );
        effect.position = Some(NodePosition {
            x: 400.0,
            y: 999_900.0,
        });
        doc.output = effect.id;
        doc.nodes.insert(effect.id, effect);
        let mut graph = Graph::default();
        let mut copy = veylvik_document::editing::copy_branch(&doc, doc.output).unwrap();
        graph.place_branch_copy(&doc, 0, &mut copy).unwrap();
        let mut offset = None;
        for (old, new) in &copy.mapping {
            let p = copy.nodes[new].position.unwrap();
            let p = egui::pos2(p.x, p.y);
            let delta = p - graph.positions[old];
            if let Some(offset) = offset {
                assert_eq!(delta, offset);
            } else {
                offset = Some(delta);
            }
            assert!(p.x.abs() <= 1_000_000.0 && p.y.abs() <= 1_000_000.0);
            assert!(graph.positions.values().all(|old| {
                !Rect::from_min_size(*old, NODE_SIZE).intersects(Rect::from_min_size(p, NODE_SIZE))
            }));
        }
        assert!(offset.unwrap().x > 0.0); // Below would exceed the coordinate bound.
    }
    #[test]
    fn insertion_avoids_dense_nodes_and_preserves_free_positions() {
        let mut graph = Graph::default();
        let desired = egui::pos2(200.0, 100.0);
        assert_eq!(graph.vacant_position(desired), desired);
        for y in 0..12 {
            for x in 0..12 {
                graph
                    .positions
                    .insert(node_id(), egui::pos2(x as f32 * 175.0, y as f32 * 75.0));
            }
        }
        let selected = graph.vacant_position(desired);
        assert!(graph.positions.values().all(|p| {
            !Rect::from_min_size(*p, NODE_SIZE)
                .expand(12.0)
                .intersects(Rect::from_min_size(selected, NODE_SIZE))
        }));
        let unchanged = graph.positions.clone();
        assert_eq!(graph.vacant_position(selected), selected);
        assert_eq!(graph.positions, unchanged);
        graph.positions.clear();
        graph
            .positions
            .insert(node_id(), egui::pos2(1_000_000.0, 1_000_000.0));
        let position = graph.vacant_position(egui::pos2(1_000_000.0, 1_000_000.0));
        assert!(position.x <= 1_000_000.0 && position.y <= 1_000_000.0);
        assert_ne!(position, egui::pos2(1_000_000.0, 1_000_000.0));
    }
    #[test]
    fn reveal_moves_only_viewport_and_keeps_inserted_node_visible() {
        let mut graph = Graph::default();
        let id = node_id();
        graph.positions.insert(id, egui::pos2(3000.0, -2000.0));
        graph.zoom = 1.5;
        let canvas = Rect::from_min_size(egui::pos2(20.0, 600.0), egui::vec2(900.0, 200.0));
        graph.reveal(id, canvas);
        let rect = Rect::from_min_size(
            canvas.min + graph.pan + graph.positions[&id].to_vec2() * graph.zoom,
            NODE_SIZE * graph.zoom,
        );
        assert!(canvas.contains_rect(rect));
        assert_eq!(graph.positions[&id], egui::pos2(3000.0, -2000.0));
        let pan = graph.pan;
        graph.reveal(id, canvas);
        assert_eq!(graph.pan, pan);
    }
}
