//! Graph navigation and drawing. Pixel rendering and document commits stay outside.
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use std::collections::BTreeMap;
use veylvik_document::{Composition, NodeId, NodePosition};
const NODE_SIZE: Vec2 = egui::vec2(160.0, 60.0);
pub enum Action {
    Select { id: NodeId, preview: bool },
    Place { id: NodeId, position: NodePosition },
    Arrange(Vec<(NodeId, NodePosition)>),
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
        }
    }
}
impl Graph {
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
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
        NodePosition {
            x: world.x.clamp(-1_000_000.0, 1_000_000.0),
            y: world.y.clamp(-1_000_000.0, 1_000_000.0),
        }
    }
    fn sync(&mut self, doc: &Composition, revision: u64) {
        if self.revision == Some(revision) {
            return;
        }
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
        for node in doc.nodes.values() {
            for source in node.inputs.values() {
                let from = canvas.min
                    + self.pan
                    + (self.positions[source].to_vec2() + egui::vec2(160.0, 30.0)) * self.zoom;
                let to = canvas.min
                    + self.pan
                    + (self.positions[&node.id].to_vec2() + egui::vec2(0.0, 30.0)) * self.zoom;
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
                    rect.intersect(canvas),
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
        if response.dragged() && !node_dragged && self.drag.is_none() {
            self.pan += response.drag_delta();
        }
        actions
    }
}
