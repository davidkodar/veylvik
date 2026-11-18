//! Click to focus: pick a 3D object in the viewer (or the inspector) and the scene camera keeps it sharp, wherever it and the camera move.
use crate::Studio;
use crate::scene_guides::Projector;
use eframe::egui::{self, Color32, Pos2, Stroke};
use veylvik_document::{NodeId, Operation};
use veylvik_scene::{Scene, math};

fn id() -> egui::Id {
    egui::Id::new("scene-focus-pick")
}
/// Whether the next click in the viewer picks the object in focus.
pub(crate) fn picking(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(id()).unwrap_or(false))
}
pub(crate) fn set_picking(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(id(), on));
}
/// The 3D objects a camera can focus on, by label, for the inspector.
pub(crate) fn objects(doc: &veylvik_document::Composition) -> Vec<(NodeId, String)> {
    let mut list: Vec<(NodeId, String)> = doc
        .nodes
        .values()
        .filter(|n| {
            matches!(
                n.operation,
                Operation::Shape3d(_) | Operation::Text3d(_) | Operation::ImageCard(_)
            )
        })
        .map(|n| (n.id, n.label.clone()))
        .collect();
    list.sort_by(|a, b| a.1.cmp(&b.1));
    list
}
/// Everything an object can face: the objects, and the cameras, lights and models.
pub(crate) fn things(doc: &veylvik_document::Composition) -> Vec<(NodeId, String)> {
    let mut list: Vec<(NodeId, String)> = doc
        .nodes
        .values()
        .filter(|n| {
            matches!(
                n.operation,
                Operation::Shape3d(_)
                    | Operation::Text3d(_)
                    | Operation::ImageCard(_)
                    | Operation::Model3d(_)
                    | Operation::Camera3d(_)
                    | Operation::Light3d(_)
            )
        })
        .map(|n| (n.id, n.label.clone()))
        .collect();
    list.sort_by(|a, b| a.1.cmp(&b.1));
    list
}
/// The object under `pointer`: of those whose projected box holds it, the one with
/// the smallest box, so a cube wins over the floor it stands on.
fn under(scene: &Scene, p: &Projector, pointer: Pos2) -> Option<(NodeId, egui::Rect)> {
    scene
        .objects
        .iter()
        .filter_map(|object| {
            let (lo, hi) = object.local_bounds();
            let mut rect = egui::Rect::NOTHING;
            for i in 0..8 {
                let corner = [
                    if i & 1 == 0 { lo[0] } else { hi[0] },
                    if i & 2 == 0 { lo[1] } else { hi[1] },
                    if i & 4 == 0 { lo[2] } else { hi[2] },
                ];
                rect = rect.union(egui::Rect::from_center_size(
                    p.point(math::point(&object.world, corner))?,
                    egui::Vec2::ZERO,
                ));
            }
            rect.contains(pointer).then_some((object.node, rect))
        })
        .min_by(|a, b| a.1.area().total_cmp(&b.1.area()))
}
impl Studio {
    /// The Focus toggle in the 3D toolbar.
    pub(crate) fn focus_button(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let on = picking(&ctx);
        if ui
            .add(egui::Button::selectable(on, "Focus"))
            .on_hover_text("Click an object in the viewer to keep it in focus. Depth of field comes from the camera's F-stop; picking sets f/2.8 if it is off.")
            .clicked()
        {
            set_picking(&ctx, !on);
        }
    }
    /// While picking: outline the object under the pointer and set it on click.
    /// Returns true when it took the pointer, so guides do not select.
    pub(crate) fn scene_focus(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        p: &Projector,
        scene: &Scene,
        response: &egui::Response,
    ) -> bool {
        if !picking(ctx) {
            return false;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            set_picking(ctx, false);
            return true;
        }
        let Some(pointer) = response.hover_pos() else {
            return true;
        };
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        let colour = Color32::from_rgb(255, 205, 90);
        if let Some((node, rect)) = under(scene, p, pointer) {
            painter.rect_stroke(
                rect,
                3.0,
                Stroke::new(1.5_f32, colour),
                egui::StrokeKind::Outside,
            );
            let label = self
                .project
                .document
                .snapshot()
                .nodes
                .get(&node)
                .map_or(String::new(), |n| n.label.clone());
            painter.text(
                rect.center_top() - egui::vec2(0.0, 6.0),
                egui::Align2::CENTER_BOTTOM,
                format!("Focus on {label}"),
                egui::FontId::proportional(12.0),
                colour,
            );
            if response.clicked() {
                if let Err(e) = self.focus_on(Some(node)) {
                    self.status = e;
                }
                set_picking(ctx, false);
            }
        } else {
            painter.text(
                pointer + egui::vec2(14.0, 14.0),
                egui::Align2::LEFT_TOP,
                "Click an object to focus on it (Esc to stop)",
                egui::FontId::proportional(12.0),
                colour,
            );
        }
        true
    }
    /// Keep `node` in focus on the scene camera (adding one if the scene has none),
    /// turning depth of field on at f/2.8 if it was off. One undo step.
    pub(crate) fn focus_on(&mut self, node: Option<NodeId>) -> Result<(), String> {
        let time = self.project.time().map_err(|e| e.to_string())?;
        self.edit_scene_camera(|c| {
            c.focus_on = node;
            if node.is_some() && c.f_stop.at(time).map_err(|e| e.to_string())? <= 0.0 {
                c.f_stop = 2.8.into();
            }
            Ok(())
        })
    }
}
