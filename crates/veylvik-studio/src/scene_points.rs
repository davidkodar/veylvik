//! Point cloud guides: the 3D points of Point cloud nodes (a camera solve's features) drawn in the viewer; clicking one puts the selected shape or card on it, and in Surface mode chosen points become a shadow-catching ground or surface.
use crate::Studio;
use crate::scene_guides::Projector;
use eframe::egui::{self, Color32, Stroke};
use veylvik_document::{Command, NodeId, Operation};
use veylvik_scene::Scene;

/// Surface mode: whether clicks choose points, and the points chosen.
#[derive(Clone, Default)]
struct Surface {
    on: bool,
    chosen: Vec<[f32; 3]>,
}
fn id() -> egui::Id {
    egui::Id::new("scene-surface-pick")
}
fn surface(ctx: &egui::Context) -> Surface {
    ctx.data(|d| d.get_temp::<Surface>(id()).unwrap_or_default())
}
fn store(ctx: &egui::Context, s: Surface) {
    ctx.data_mut(|d| d.insert_temp(id(), s));
}
/// The plane through `points` (least squares): its centre and the unit normal
/// facing `eye`.
fn plane(points: &[[f32; 3]], eye: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let n = points.len() as f32;
    let c = [0, 1, 2].map(|k| points.iter().map(|p| p[k]).sum::<f32>() / n);
    let mut cov = vec![0.0f64; 9];
    for p in points {
        let d = [0, 1, 2].map(|k| f64::from(p[k] - c[k]));
        for i in 0..3 {
            for j in 0..3 {
                cov[i * 3 + j] += d[i] * d[j];
            }
        }
    }
    let (_, v) = veylvik_solve::linalg::symmetric_eigen(cov, 3);
    let mut normal = [v[0] as f32, v[3] as f32, v[6] as f32];
    let towards: f32 = (0..3).map(|k| normal[k] * (eye[k] - c[k])).sum();
    if towards < 0.0 {
        normal = normal.map(|x| -x);
    }
    (c, normal)
}
/// Rotation (degrees) that turns a Plane shape to face `normal`.
fn facing(normal: [f32; 3]) -> [f32; 3] {
    let x = (-normal[1]).clamp(-1.0, 1.0).asin();
    let y = normal[0].atan2(normal[2]);
    [x.to_degrees(), y.to_degrees(), 0.0]
}
impl Studio {
    /// The Surface toggle and, with three points chosen, what to make of them.
    pub(crate) fn surface_buttons(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut s = surface(&ctx);
        if ui
            .add(egui::Button::selectable(s.on, "Surface"))
            .on_hover_text("Click three or more solved points on a floor, table or wall, then make it a surface that catches shadows")
            .clicked()
        {
            s.on = !s.on;
            s.chosen.clear();
        }
        if s.on && s.chosen.len() >= 3 {
            let mut made = None;
            if ui
                .button("Ground here")
                .on_hover_text("Move the scene's shadow catcher onto these points")
                .clicked()
            {
                made = Some(false);
            }
            if ui
                .button("New surface")
                .on_hover_text("Add another shadow catcher on these points")
                .clicked()
            {
                made = Some(true);
            }
            if let Some(new) = made {
                match self.make_surface(&ctx, &s.chosen, new) {
                    Ok(()) => s = Surface::default(),
                    Err(e) => self.fail(e),
                }
            }
        } else if s.on {
            ui.weak(format!("{} of 3 points", s.chosen.len()));
        }
        store(&ctx, s);
    }
    /// A shadow catcher on the plane through `points`: the scene's existing one moved
    /// there, or (`new`, or none yet) a new plane merged next to the point cloud.
    fn make_surface(
        &mut self,
        ctx: &egui::Context,
        points: &[[f32; 3]],
        new: bool,
    ) -> Result<(), String> {
        use veylvik_document::{Node, nodes};
        let doc = self.project.document.snapshot();
        let time = self.project.time().map_err(|e| e.to_string())?;
        let (w, h) = (doc.width as f32, doc.height as f32);
        let eye = self
            .viewing_camera(ctx)
            .map_or([w * 0.5, h * 0.5, 1e4], |c| {
                veylvik_scene::math::point(&c.world, [0.0; 3])
            });
        let (c, normal) = plane(points, eye);
        let position = [c[0] - w * 0.5, c[1] - h * 0.5, c[2]];
        let rotation = facing(normal);
        let reach = points
            .iter()
            .map(|p| (0..3).map(|k| (p[k] - c[k]).powi(2)).sum::<f32>().sqrt())
            .fold(0.0, f32::max);
        let render = self.viewed_scene().ok_or("No 3D scene in view")?;
        let root = *doc.nodes[&render]
            .inputs
            .get("scene")
            .ok_or("Render scene needs a scene")?;
        let scene = veylvik_scene::evaluate(doc, root, time).map_err(|e| e.to_string())?;
        let existing = scene
            .objects
            .iter()
            .find(|o| o.surface.catcher)
            .map(|o| o.node);
        let mut commands = Vec::new();
        match existing.filter(|_| !new) {
            Some(id) => {
                let mut node = doc.nodes[&id].clone();
                if let Operation::Shape3d(s) = &mut node.operation {
                    s.placement
                        .position
                        .set_at(time, position)
                        .map_err(|e| e.to_string())?;
                    s.placement
                        .rotation
                        .set_at(time, rotation)
                        .map_err(|e| e.to_string())?;
                }
                commands.push(Command::Put {
                    node: Box::new(node),
                });
            }
            None => {
                let Operation::Shape3d(mut plane) =
                    serde_json::from_value(nodes::SHAPE_3D.default_operation())
                        .map_err(|e| e.to_string())?
                else {
                    return Err("shape".into());
                };
                plane.kind = nodes::ShapeKind::Plane;
                let size = (reach * 3.0).max(w * 0.5);
                plane.size_x = size.into();
                plane.size_y = size.into();
                plane.shadow_catcher = true;
                plane.placement.position = position.into();
                plane.placement.rotation = rotation.into();
                let label = if existing.is_none() {
                    "Ground (shadow catcher)"
                } else {
                    "Surface (shadow catcher)"
                };
                let plane = Node::new(label, Operation::Shape3d(plane), &[]);
                // Merge it where the points live, else into the rendered scene's root.
                let cloud = scene.points.first().map(|(id, _)| *id);
                let host = doc
                    .nodes
                    .values()
                    .find(|n| {
                        matches!(n.operation, Operation::MergeScenes(_))
                            && cloud.is_some_and(|c| n.inputs.values().any(|v| *v == c))
                    })
                    .or_else(|| {
                        doc.nodes
                            .get(&root)
                            .filter(|n| matches!(n.operation, Operation::MergeScenes(_)))
                    })
                    .cloned();
                let mut host = host.ok_or("Add a Merge scenes node to put the surface in")?;
                let port = nodes::MERGE_PORTS
                    .iter()
                    .find(|p| !host.inputs.contains_key(**p))
                    .ok_or("The scene is full (eight inputs)")?;
                host.inputs.insert((*port).into(), plane.id);
                commands.push(Command::Put {
                    node: Box::new(plane),
                });
                commands.push(Command::Put {
                    node: Box::new(host),
                });
            }
        }
        self.project.apply(&commands).map_err(|e| e.to_string())?;
        self.refresh_draft();
        self.request_preview();
        Ok(())
    }
    /// Draw the points; while one is hovered with a shape or card selected, offer to
    /// put it there and do it on click (one undo step). True when it took the click.
    pub(crate) fn scene_points(
        &mut self,
        painter: &egui::Painter,
        p: &Projector,
        scene: &Scene,
        response: &egui::Response,
    ) -> bool {
        if scene.points.is_empty() {
            return false;
        }
        let dot = Color32::from_rgba_unmultiplied(90, 230, 210, 190);
        let mut nearest: Option<(f32, egui::Pos2, [f32; 3])> = None;
        let pointer = response.hover_pos();
        for (_, world) in &scene.points {
            let Some(at) = p.point(*world) else { continue };
            if !painter.clip_rect().contains(at) {
                continue;
            }
            painter.circle_filled(at, 2.0, dot);
            if let Some(pointer) = pointer {
                let d = at.distance(pointer);
                if d < 7.0 && nearest.is_none_or(|(best, ..)| d < best) {
                    nearest = Some((d, at, *world));
                }
            }
        }
        let ctx = painter.ctx().clone();
        let mut pick = surface(&ctx);
        if pick.on {
            let chosen = Color32::from_rgb(255, 170, 60);
            for world in &pick.chosen {
                if let Some(at) = p.point(*world) {
                    painter.circle_filled(at, 4.5, chosen);
                }
            }
            let Some((_, at, world)) = nearest else {
                return response.hovered();
            };
            painter.circle_stroke(at, 6.0, Stroke::new(1.5_f32, chosen));
            if response.clicked() {
                match pick.chosen.iter().position(|q| *q == world) {
                    Some(i) => {
                        pick.chosen.remove(i);
                    }
                    None => pick.chosen.push(world),
                }
                store(&ctx, pick);
            }
            return true;
        }
        let Some((_, at, world)) = nearest else {
            return false;
        };
        painter.circle_stroke(at, 6.0, Stroke::new(1.5_f32, Color32::WHITE));
        let doc = self.project.document.snapshot();
        let selected = doc.nodes.get(&self.project.selected);
        let movable = selected
            .filter(|n| matches!(n.operation, Operation::Shape3d(_) | Operation::ImageCard(_)));
        let text = match movable {
            Some(n) => format!("Put {} here", n.label),
            None => "Select a 3D shape or image card, then click a point to put it there".into(),
        };
        painter.text(
            at + egui::vec2(10.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            text,
            egui::FontId::proportional(12.0),
            Color32::WHITE,
        );
        if response.clicked()
            && let Some(node) = movable.map(|n| n.id)
        {
            if let Err(e) = self.put_on_point(node, world) {
                self.fail(e);
            }
            return true;
        }
        false
    }
    /// Move `node` so it sits on the world point: a shape rests its base on it when
    /// upright, a card centres on it.
    fn put_on_point(&mut self, node: NodeId, world: [f32; 3]) -> Result<(), String> {
        let doc = self.project.document.snapshot();
        let time = self.project.time().map_err(|e| e.to_string())?;
        let mut node = doc.nodes.get(&node).ok_or("The object is gone")?.clone();
        let offset = [
            world[0] - doc.width as f32 * 0.5,
            world[1] - doc.height as f32 * 0.5,
            world[2],
        ];
        let placement = match &mut node.operation {
            Operation::Shape3d(s) => {
                let upright = s.placement.rotation.at(time).map_err(|e| e.to_string())?;
                let lift = if upright[0].abs() < 1.0 && upright[2].abs() < 1.0 {
                    s.size_y.at(time).map_err(|e| e.to_string())?
                        * s.placement.scale.at(time).map_err(|e| e.to_string())?[1]
                        * 0.5
                } else {
                    0.0
                };
                s.placement
                    .position
                    .set_at(time, [offset[0], offset[1] - lift, offset[2]])
                    .map_err(|e| e.to_string())?;
                true
            }
            Operation::ImageCard(c) => {
                c.placement
                    .position
                    .set_at(time, offset)
                    .map_err(|e| e.to_string())?;
                true
            }
            _ => false,
        };
        if !placement {
            return Err("Only 3D shapes and image cards can be put on a point".into());
        }
        self.project
            .apply(&[Command::Put {
                node: Box::new(node),
            }])
            .map_err(|e| e.to_string())?;
        self.refresh_draft();
        self.request_preview();
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_plane_through_floor_points_faces_up_and_turns_a_plane_shape_flat() {
        // Points on the floor (y = 500), seen from above it (y down, so smaller y is up).
        let floor = [
            [0.0, 500.0, 0.0],
            [300.0, 500.0, -200.0],
            [-250.0, 500.0, 150.0],
            [80.0, 500.0, 400.0],
        ];
        let (centre, normal) = plane(&floor, [0.0, 0.0, 1000.0]);
        assert!((centre[1] - 500.0).abs() < 1e-3);
        assert!((normal[1] + 1.0).abs() < 1e-4, "{normal:?}");
        let r = facing(normal);
        assert!((r[0] - 90.0).abs() < 0.1 && r[2] == 0.0, "{r:?}");
        // A wall facing the camera stays upright.
        let wall = [
            [0.0, 0.0, -300.0],
            [200.0, 100.0, -300.0],
            [-150.0, -80.0, -300.0],
        ];
        let (_, n) = plane(&wall, [0.0, 0.0, 1000.0]);
        let r = facing(n);
        assert!(r[0].abs() < 0.1 && r[1].abs() < 0.1, "{r:?}");
    }
}
