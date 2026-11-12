//! Graph navigation and drawing. Pixel rendering and document commits stay outside.
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use std::collections::BTreeMap;
use veylvik_document::{Composition, NodeId, NodePosition};
const NODE_SIZE: Vec2 = egui::vec2(160.0, 60.0);
pub enum Action {
    Find,
    Group,
    OpenGroup(NodeId),
    CloseGroup(NodeId),
    Add,
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
        port: String,
        group: bool,
    },
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Socket {
    Output(NodeId),
    Input(NodeId, usize),
}
impl Socket {
    fn compatible(self, other: Self) -> bool {
        matches!((self, other), (Self::Output(a), Self::Input(b, _)) | (Self::Input(b, _), Self::Output(a)) if a != b)
    }
}
pub(super) struct VisualNode {
    pub(super) id: NodeId,
    pub(super) label: String,
    pub(super) subtitle: String,
    pub(super) group: Option<NodeId>,
    pub(super) inputs: Vec<(String, NodeId)>,
}
impl VisualNode {
    fn role_colour(&self, doc: &Composition) -> Color32 {
        use veylvik_document::Operation;
        if self.group.is_some()
            && !matches!(doc.nodes[&self.id].operation, Operation::Material { .. })
        {
            Color32::from_rgb(175, 130, 205)
        } else {
            match doc.nodes[&self.id].operation {
                Operation::Text { .. } | Operation::Path { .. } | Operation::Ellipse { .. } => {
                    Color32::from_rgb(105, 170, 230)
                }
                Operation::Material { .. } | Operation::Glass { .. } | Operation::Patina { .. } => {
                    Color32::from_rgb(210, 170, 85)
                }
                Operation::Merge { .. }
                | Operation::Blend { .. }
                | Operation::Mask { .. }
                | Operation::CombineMask { .. } => Color32::from_rgb(110, 180, 195),
                ref op if op.inputs().is_empty() => Color32::from_rgb(95, 175, 150),
                _ => Color32::from_rgb(175, 130, 205),
            }
        }
    }
}
struct Drag {
    preserve_layout: bool,
    id: NodeId,
    start: Pos2,
    current: Pos2,
}
pub struct Graph {
    revision: Option<u64>,
    expanded: std::collections::BTreeSet<NodeId>,
    positions: BTreeMap<NodeId, Pos2>,
    automatic: BTreeMap<NodeId, Pos2>,
    zoom: f32,
    pan: Vec2,
    canvas: Option<Rect>,
    drag: Option<Drag>,
    wire: Option<Socket>,
    pinned: std::collections::BTreeSet<NodeId>,
    role_colours: bool,
    wire_colours: bool,
    fit_pending: bool,
    auto_arrange: bool,
    auto_view: Option<(u64, Vec2)>,
}
impl Default for Graph {
    fn default() -> Self {
        Self {
            revision: None,
            expanded: Default::default(),
            positions: BTreeMap::new(),
            automatic: BTreeMap::new(),
            zoom: 1.0,
            pan: Vec2::ZERO,
            canvas: None,
            drag: None,
            wire: None,
            pinned: Default::default(),
            role_colours: true,
            wire_colours: true,
            fit_pending: false,
            auto_arrange: false,
            auto_view: None,
        }
    }
}
impl Graph {
    pub fn expand(&mut self, id: NodeId) {
        self.expanded.insert(id);
        self.auto_view = None;
        self.revision = None;
    }
    pub fn collapse(&mut self, id: NodeId) {
        self.expanded.remove(&id);
        self.auto_view = None;
        self.revision = None;
    }
    pub fn selected_group<'a>(
        &self,
        doc: &'a Composition,
        selected: NodeId,
    ) -> Option<&'a veylvik_document::groups::Group> {
        doc.groups
            .values()
            .find(|g| g.output == selected && !self.expanded.contains(&g.id))
    }
    fn visual_nodes(&self, doc: &Composition) -> Vec<VisualNode> {
        let reachable = veylvik_document::presets::background::reachable(doc);
        let owners: BTreeMap<_, _> = doc
            .groups
            .values()
            .filter(|g| !self.expanded.contains(&g.id))
            .flat_map(|g| g.members.iter().map(move |id| (*id, g)))
            .collect();
        let source = |id: NodeId| owners.get(&id).map_or(id, |g| g.output);
        doc.nodes
            .values()
            .filter_map(|n| {
                if let Some(g) = owners.get(&n.id) {
                    if n.id != g.output {
                        return None;
                    }
                    Some(VisualNode {
                        id: n.id,
                        label: g.label.clone(),
                        subtitle: format!(
                            "{}Effect · {} nodes",
                            if reachable.contains(&g.output) {
                                ""
                            } else {
                                "Unused · "
                            },
                            g.members.len()
                        ),
                        group: Some(g.id),
                        inputs: g
                            .inputs
                            .keys()
                            .map(|name| (name.clone(), source(g.source(doc, name).unwrap())))
                            .collect(),
                    })
                } else {
                    Some(VisualNode {
                        id: n.id,
                        label: n.label.clone(),
                        subtitle: if !reachable.contains(&n.id) {
                            format!("Unused · {}", n.operation.descriptor().label)
                        } else if n.id == doc.output {
                            "Composition output".into()
                        } else {
                            n.operation.descriptor().label.into()
                        },
                        group: None,
                        inputs: n
                            .operation
                            .inputs()
                            .iter()
                            .map(|port| (port.to_string(), source(n.inputs[*port])))
                            .collect(),
                    })
                }
            })
            .collect()
    }
    pub fn focus(&mut self, doc: &Composition, revision: u64, id: NodeId) {
        for group in doc.groups.values() {
            if group.members.contains(&id) && group.output != id {
                self.expanded.insert(group.id);
            }
        }
        self.sync(doc, revision);
        if let Some(canvas) = self.canvas {
            self.reveal(id, canvas);
        }
    }
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
        self.auto_view = None;
        self.revision = Some(revision);
        let pinned_positions: Vec<_> = self
            .positions
            .iter()
            .filter(|(id, _)| {
                self.auto_arrange
                    && doc.nodes.contains_key(id)
                    && (self.pinned.contains(id)
                        || doc
                            .groups
                            .values()
                            .any(|g| self.pinned.contains(&g.output) && g.members.contains(id)))
            })
            .map(|(id, p)| (*id, *p))
            .collect();
        self.automatic.clear();
        self.positions.clear();
        let nodes = self.visual_nodes(doc);
        self.automatic = crate::graph_layout::arrange(&nodes, doc.output);
        // Keep collapsed members near their public output without counting them as visible columns.
        let mut expanded = Graph::default();
        expanded.expanded.extend(doc.groups.keys().copied());
        let raw = crate::graph_layout::arrange(&expanded.visual_nodes(doc), doc.output);
        for (id, p) in &raw {
            if self.automatic.contains_key(id) {
                continue;
            }
            let position = doc
                .groups
                .values()
                .find(|g| g.members.contains(id))
                .and_then(|g| {
                    self.automatic
                        .get(&g.output)
                        .map(|anchor| *p + (*anchor - raw[&g.output]))
                })
                .unwrap_or(*p);
            self.automatic.insert(*id, position);
        }
        for (id, node) in &doc.nodes {
            let automatic = self
                .automatic
                .get(id)
                .copied()
                .unwrap_or(egui::pos2(25.0, 20.0));
            self.positions.insert(
                *id,
                node.position
                    .map(|p| egui::pos2(p.x, p.y))
                    .unwrap_or(automatic),
            );
        }
        self.positions.extend(pinned_positions);
    }

    fn adapt_automatic(&mut self, doc: &Composition, nodes: &[VisualNode], size: Vec2) {
        let mut responsive = crate::graph_layout::arrange(nodes, doc.output);
        crate::graph_layout::adapt(&mut responsive, size);
        for group in doc.groups.values() {
            if let (Some(new), Some(old)) = (
                responsive.get(&group.output),
                self.automatic.get(&group.output),
            ) {
                let offset = *new - *old;
                for id in &group.members {
                    if !responsive.contains_key(id)
                        && let Some(position) = self.automatic.get_mut(id)
                    {
                        *position += offset;
                    }
                }
            }
        }
        self.automatic.extend(responsive);
    }
    fn arrangement(
        &self,
        doc: &Composition,
        selected: Option<NodeId>,
    ) -> Vec<(NodeId, NodePosition)> {
        let visible = self.visual_nodes(doc);
        let mut scope: std::collections::BTreeSet<_> = self.automatic.keys().copied().collect();
        if let Some(root) = selected {
            scope.clear();
            let by_id: BTreeMap<_, _> = visible.iter().map(|n| (n.id, n)).collect();
            let mut pending = vec![root];
            while let Some(id) = pending.pop() {
                if scope.insert(id)
                    && let Some(n) = by_id.get(&id)
                {
                    pending.extend(n.inputs.iter().map(|(_, id)| *id));
                }
            }
            for group in doc.groups.values() {
                if scope.contains(&group.output) {
                    scope.extend(group.members.iter().copied());
                }
            }
        }
        let mut pinned = self.pinned.clone();
        for group in doc.groups.values() {
            if pinned.contains(&group.output) {
                pinned.extend(group.members.iter().copied());
            }
        }
        scope.retain(|id| !pinned.contains(id));
        let offset = selected.map_or(Vec2::ZERO, |id| self.positions[&id] - self.automatic[&id]);
        let visible_ids: std::collections::BTreeSet<_> = visible.iter().map(|n| n.id).collect();
        let mut occupied: Vec<_> = visible_ids
            .iter()
            .filter(|id| !scope.contains(id))
            .map(|id| Rect::from_min_size(self.positions[id], NODE_SIZE).expand(12.0))
            .collect();
        let mut placed = BTreeMap::new();
        let mut order: Vec<_> = scope
            .iter()
            .filter(|id| visible_ids.contains(id))
            .copied()
            .collect();
        order.sort_by(|a, b| {
            self.automatic[a]
                .y
                .total_cmp(&self.automatic[b].y)
                .then(self.automatic[a].x.total_cmp(&self.automatic[b].x))
        });
        for id in order {
            let mut p = self.automatic[&id] + offset;
            while occupied
                .iter()
                .any(|rect| rect.intersects(Rect::from_min_size(p, NODE_SIZE)))
            {
                p.y += 100.0;
            }
            p.x = p.x.clamp(-1_000_000.0, 1_000_000.0);
            p.y = p.y.clamp(-1_000_000.0, 1_000_000.0);
            occupied.push(Rect::from_min_size(p, NODE_SIZE).expand(12.0));
            placed.insert(id, p);
        }
        for id in &scope {
            if placed.contains_key(id) {
                continue;
            }
            let p = doc
                .groups
                .values()
                .find(|g| g.members.contains(id))
                .and_then(|g| {
                    placed
                        .get(&g.output)
                        .map(|anchor| self.automatic[id] + (*anchor - self.automatic[&g.output]))
                })
                .unwrap_or(self.automatic[id] + offset);
            placed.insert(*id, p);
        }
        placed
            .into_iter()
            .filter(|(id, p)| self.positions[id] != *p)
            .map(|(id, p)| (id, NodePosition { x: p.x, y: p.y }))
            .collect()
    }
    fn zoom_at(&mut self, zoom: f32, point: Pos2, canvas: Rect) {
        let zoom = zoom.clamp(0.15, 2.5);
        let point = point - canvas.min;
        self.pan = point - (point - self.pan) * (zoom / self.zoom);
        self.zoom = zoom;
    }
    fn fit(&mut self, canvas: Rect, nodes: &[VisualNode]) {
        let bounds = nodes
            .iter()
            .map(|n| &self.positions[&n.id])
            .fold(Rect::NOTHING, |rect, pos| {
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
        let nodes = self.visual_nodes(doc);
        let mut actions = Vec::new();
        let mut fit = std::mem::take(&mut self.fit_pending);
        let mut zoom_factor = 1.0;
        ui.horizontal(|ui| {
            ui.heading("Nodes");
            if ui
                .button("Add node")
                .on_hover_text("Search and add a node (Tab)")
                .clicked()
            {
                actions.push(Action::Add);
            }
            if ui
                .button("Find")
                .on_hover_text("Find and preview a node (Ctrl+F)")
                .clicked()
            {
                actions.push(Action::Find);
            }

            if ui
                .button("Group…")
                .on_hover_text("Create a reusable effect from nodes")
                .clicked()
            {
                actions.push(Action::Group);
            }
            if let Some(g) = doc.groups.values().find(|g| g.members.contains(&selected)) {
                if self.expanded.contains(&g.id) {
                    if ui.button("Close group").clicked() {
                        actions.push(Action::CloseGroup(g.id));
                    }
                } else if ui.button("Open group").clicked() {
                    actions.push(Action::OpenGroup(g.id));
                }
            }
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
            if ui
                .button("Arrange")
                .on_hover_text(
                    "Arrange all nodes by connected branches; keep pinned nodes in place",
                )
                .clicked()
            {
                if let Some(canvas) = self.canvas { self.adapt_automatic(doc, &nodes, canvas.size()); }
                let positions = self.arrangement(doc, None);
                if !positions.is_empty() {
                    actions.push(Action::Arrange(positions));
                    self.fit_pending = true;
                } else {
                    fit = true;
                }
            }
            if ui.checkbox(&mut self.auto_arrange, "Auto arrange").on_hover_text("Adapt the graph view to node changes and pane size. Session-only; manual placements remain saved in the project.").changed() {
                self.auto_view = None;
                self.revision = None;
            }
            ui.menu_button("Layout ▾", |ui| {
                if ui.button("Arrange selected branch").clicked() {
                    let positions = self.arrangement(doc, Some(selected));
                    if !positions.is_empty() {
                        actions.push(Action::Arrange(positions));
                    }
                    ui.close();
                }
                let mut pinned = self.pinned.contains(&selected);
                if ui
                    .checkbox(&mut pinned, "Pin selected placement")
                    .on_hover_text(
                        "Keep this node in place during Arrange for this editing session",
                    )
                    .changed()
                {
                    if pinned {
                        self.pinned.insert(selected);
                    } else {
                        self.pinned.remove(&selected);
                    }
                }
                ui.separator();
                ui.checkbox(&mut self.role_colours, "Node role colours");
                ui.checkbox(&mut self.wire_colours, "Wire role colours")
                    .on_hover_text("Wires use their source node’s role colour; selected connections are brighter");
                for (label, colour) in [
                    ("Text / shapes", Color32::from_rgb(105, 170, 230)),
                    ("Images / generators", Color32::from_rgb(95, 175, 150)),
                    ("Materials", Color32::from_rgb(210, 170, 85)),
                    ("Effects", Color32::from_rgb(175, 130, 205)),
                    ("Compositing", Color32::from_rgb(110, 180, 195)),
                ] {
                    ui.colored_label(colour, label);
                }
            });
            ui.weak("Drag background to pan · Ctrl+scroll to zoom · double-click to preview");
        });
        let (canvas, response) = ui.allocate_exact_size(
            ui.available_size().max(egui::vec2(1.0, 1.0)),
            egui::Sense::click_and_drag(),
        );
        self.canvas = Some(canvas);
        if self.auto_arrange
            && !self.dragging()
            && self.auto_view != Some((revision, canvas.size()))
        {
            self.auto_view = Some((revision, canvas.size()));
            self.adapt_automatic(doc, &nodes, canvas.size());
            let positions = self.arrangement(doc, None);
            for (id, position) in positions {
                self.positions
                    .insert(id, egui::pos2(position.x, position.y));
            }
            fit = true;
        }
        if newly_inserted {
            self.reveal(selected, canvas);
        }
        if fit {
            self.fit(canvas, &nodes);
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
        for node in &nodes {
            let origin = canvas.min + self.pan + self.positions[&node.id].to_vec2() * self.zoom;
            sockets.push((
                Socket::Output(node.id),
                origin + egui::vec2(160.0, 30.0) * self.zoom,
            ));
            for (index, _) in node.inputs.iter().enumerate() {
                let y = 60.0 * (index + 1) as f32 / (node.inputs.len() + 1) as f32;
                sockets.push((
                    Socket::Input(node.id, index),
                    origin + egui::vec2(0.0, y) * self.zoom,
                ));
            }
        }
        let colours: BTreeMap<_, _> = nodes
            .iter()
            .map(|node| (node.id, node.role_colour(doc)))
            .collect();
        for node in &nodes {
            for (index, (_, source)) in node.inputs.iter().enumerate() {
                let from = canvas.min
                    + self.pan
                    + (self.positions[source].to_vec2() + egui::vec2(160.0, 30.0)) * self.zoom;
                let to = canvas.min
                    + self.pan
                    + (self.positions[&node.id].to_vec2()
                        + egui::vec2(
                            0.0,
                            60.0 * (index + 1) as f32 / (node.inputs.len() + 1) as f32,
                        ))
                        * self.zoom;
                let highlighted = node.id == selected || *source == selected;
                let colour = if self.wire_colours {
                    colours[source]
                } else {
                    Color32::from_gray(150)
                };
                let colour = if highlighted {
                    colour
                } else {
                    colour.gamma_multiply(0.65)
                };
                let bend = egui::vec2(45.0 * self.zoom, 0.0);
                painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                    [from, from + bend, to - bend, to],
                    false,
                    Color32::TRANSPARENT,
                    egui::Stroke::new(if highlighted { 2.2_f32 } else { 1.5_f32 }, colour),
                ));
            }
        }
        let mut node_dragged = false;
        for node in &nodes {
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
                let preserve_layout = self.auto_arrange;
                self.auto_arrange = false;
                self.auto_view = None;
                self.drag = Some(Drag {
                    preserve_layout,
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
                if drag.preserve_layout {
                    let mut positions = self.positions.clone();
                    positions.insert(drag.id, drag.current);
                    if let Some(group) = self.selected_group(doc, drag.id) {
                        for id in &group.members {
                            if *id != drag.id {
                                *positions.get_mut(id).unwrap() += drag.current - drag.start;
                            }
                        }
                    }
                    actions.push(Action::Arrange(
                        positions
                            .into_iter()
                            .map(|(id, p)| (id, NodePosition { x: p.x, y: p.y }))
                            .collect(),
                    ));
                } else if let Some(group) = self.selected_group(doc, drag.id) {
                    let delta = drag.current - drag.start;
                    actions.push(Action::Arrange(
                        group
                            .members
                            .iter()
                            .map(|id| {
                                let p = if *id == drag.id {
                                    drag.current
                                } else {
                                    self.positions[id] + delta
                                };
                                (*id, NodePosition { x: p.x, y: p.y })
                            })
                            .collect(),
                    ));
                } else {
                    actions.push(Action::Place {
                        id: drag.id,
                        position: NodePosition {
                            x: drag.current.x,
                            y: drag.current.y,
                        },
                    });
                }
                // Resync even after a rejected placement, so a failed gesture
                // never leaves a ghost layout in the viewport.
                self.revision = None;
            }
            if response.clicked() || response.double_clicked() {
                actions.push(Action::Select {
                    id: node.id,
                    preview: response.double_clicked(),
                });
            }
            let selected = node.id == selected;
            painter.rect_filled(rect, 0.0, Color32::from_rgb(36, 41, 50));
            if self.role_colours {
                let colour = node.role_colour(doc);
                painter.rect_filled(
                    Rect::from_min_size(rect.min, egui::vec2(rect.width(), 3.0 * self.zoom)),
                    0.0,
                    colour,
                );
            }
            if self.pinned.contains(&node.id) {
                painter.circle_filled(
                    rect.right_top() + egui::vec2(-8.0, 9.0) * self.zoom,
                    3.0 * self.zoom,
                    Color32::WHITE,
                );
            }
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
            text.text(
                rect.min + egui::vec2(10.0, 41.0) * self.zoom,
                egui::Align2::LEFT_CENTER,
                &node.subtitle,
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
                        && self.wire.is_none_or(|from| from.compatible(*socket))
                })
                .min_by(|a, b| a.1.distance_sq(p).total_cmp(&b.1.distance_sq(p)))
                .map(|(socket, _)| *socket)
        });
        for (socket, position) in &sockets {
            if !canvas.contains(*position) {
                continue;
            }
            let hit = crate::handles::interact(
                ui,
                Rect::from_center_size(*position, Vec2::splat(radius * 2.0)).intersect(canvas),
                ui.id().with(socket),
            )
            .on_hover_text(match socket {
                Socket::Output(_) => "Output",
                Socket::Input(id, index) => {
                    let node = nodes.iter().find(|n| n.id == *id).unwrap();
                    let key = &node.inputs[*index].0;
                    node.group.map_or(key.as_str(), |id| {
                        doc.groups[&id].inputs[key].label.as_str()
                    })
                }
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
                if let Some(to) = hovered.filter(|to| from.compatible(*to)) {
                    let (source, target, index) = match (from, to) {
                        (Socket::Output(a), Socket::Input(b, index))
                        | (Socket::Input(b, index), Socket::Output(a)) => (a, b, index),
                        _ => unreachable!(),
                    };
                    let node = nodes.iter().find(|n| n.id == target).unwrap();
                    actions.push(Action::Connect {
                        source,
                        target: node.group.unwrap_or(target),
                        port: node.inputs[index].0.clone(),
                        group: node.group.is_some(),
                    });
                }
                self.wire = None;
            }
        }
        if response.dragged()
            && !node_dragged
            && self.drag.is_none()
            && !wiring
            && ui.ctx().dragged_id() == Some(response.id)
            && !ui.input(|input| input.pointer.primary_pressed())
        {
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
    fn fast_socket_drag_reconnects_group_without_panning() {
        use veylvik_document::{Node, Operation, Time, groups::Group};
        let mut doc = veylvik_document::blank(8, 8, Time::new(24, 1).unwrap()).unwrap();
        let original = doc.output;
        doc.nodes.get_mut(&original).unwrap().position = Some(NodePosition { x: 25.0, y: 20.0 });
        let mut source = Node::new("Other", Operation::Solid { color: [1.0; 4] }, &[]);
        source.position = Some(NodePosition { x: 25.0, y: 110.0 });
        let source_id = source.id;
        doc.nodes.insert(source.id, source);
        let mut effect = Node::new(
            "Blur",
            Operation::Blur { radius: 4.0.into() },
            &[("image", original)],
        );
        effect.position = Some(NodePosition { x: 400.0, y: 50.0 });
        let output = effect.id;
        doc.nodes.insert(output, effect);
        let group = Group::capture(&doc, [output].into(), output, "Effect").unwrap();
        let group_id = group.id;
        doc.groups.insert(group.id, group);
        let ctx = egui::Context::default();
        let mut graph = Graph::default();
        let mut run = |events| {
            let mut actions = Vec::new();
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 400.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show_inside(ui, |ui| {
                        actions = graph.show(ui, &doc, 0, output, Color32::LIGHT_BLUE);
                    });
                },
            );
            (actions, graph.canvas.unwrap().min)
        };
        let (_, origin) = run(vec![]);
        let from = origin + egui::vec2(185.0, 140.0);
        let to = origin + egui::vec2(400.0, 80.0);
        run(vec![egui::Event::PointerMoved(from)]);
        run(vec![
            egui::Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerMoved(from + egui::vec2(40.0, 20.0)),
        ]);
        let (actions, _) = run(vec![
            egui::Event::PointerMoved(to),
            egui::Event::PointerButton {
                pos: to,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        assert_eq!(graph.pan, Vec2::ZERO);
        assert!(
            matches!(actions.as_slice(), [Action::Connect { source, target, port, group: true }] if *source == source_id && *target == group_id && port == "input_1")
        );
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

#[cfg(test)]
mod arrangement_tests {
    use super::*;
    #[test]
    fn collapsed_effects_take_one_column_and_pins_are_not_moved() {
        use veylvik_document::{Document, Time, recipes::Recipe};
        let doc = veylvik_document::blank(320, 180, Time::new(24, 1).unwrap()).unwrap();
        let source = doc.output;
        let effect = Recipe::Glow.build(&doc, source).unwrap();
        let output = effect.root;
        let commands = veylvik_document::presets::connect_after(
            &doc,
            source,
            output,
            effect.into_grouped_commands(&doc).unwrap(),
        )
        .unwrap();
        let owner = Document::new(doc).unwrap();
        let mut doc = owner.prepare(0, &commands).unwrap();
        let group = *doc.groups.keys().next().unwrap();
        let mut graph = Graph::default();
        graph.sync(&doc, 0);
        graph.auto_view = Some((0, egui::vec2(1000.0, 400.0)));
        graph.revision = None;
        graph.sync(&doc, 0);
        assert!(
            graph.auto_view.is_none(),
            "Resync must invalidate responsive view positions"
        );
        assert_eq!(
            graph.automatic[&output].x - graph.automatic[&source].x,
            220.0
        );
        graph.expand(group);
        graph.sync(&doc, 0);
        assert!(graph.automatic[&output].x - graph.automatic[&source].x > 220.0);
        doc.nodes.get_mut(&source).unwrap().position = Some(NodePosition { x: 900.0, y: 700.0 });
        graph.sync(&doc, 1);
        graph.pinned.insert(source);
        let arrangement = graph.arrangement(&doc, None);
        assert!(arrangement.iter().all(|(id, _)| *id != source));
        assert_eq!(graph.positions[&source], egui::pos2(900.0, 700.0));
    }
}
