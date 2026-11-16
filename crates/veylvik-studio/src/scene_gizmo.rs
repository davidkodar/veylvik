//! Direct manipulation in the 3D viewer: move, rotate and scale handles on the selected object, light or camera, and aim handles for spots and suns.
//!
//! Drags edit the inspector draft (keyed at the playhead when the placement is
//! animated) and apply on release, so a whole drag is one undo step.
use crate::Studio;
use crate::scene_guides::Projector;
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};
use veylvik_document::{NodeId, nodes::LightKind, track::Property};
use veylvik_scene::{Scene, math};

#[derive(Clone, Copy, PartialEq, Default)]
pub(crate) enum Mode {
    #[default]
    Move,
    Rotate,
    Scale,
}
#[derive(Clone, Copy, PartialEq)]
enum Handle {
    Axis(usize),
    Free,
    Ring(usize),
    Aim,
}
#[derive(Clone, Default)]
struct Gizmo {
    mode: Mode,
    drag: Option<Handle>,
    /// Where the aim handle points, while it is dragged.
    aim_to: Option<[f32; 3]>,
}
fn gizmo(ctx: &egui::Context) -> Gizmo {
    ctx.data_mut(|d| {
        d.get_temp::<Gizmo>(egui::Id::new("scene-gizmo"))
            .unwrap_or_default()
    })
}
fn store(ctx: &egui::Context, g: Gizmo) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("scene-gizmo"), g));
}
/// The selected thing in the scene, where it is and how its frame is turned.
struct Target {
    node: NodeId,
    origin: [f32; 3],
    /// World rotation and scale of the node.
    world: math::Mat4,
    /// Rotation and scale of its own placement (no parents).
    local: math::Mat4,
    /// Where a spot or sun points.
    aim: Option<[f32; 3]>,
    /// Cameras ignore scale.
    scalable: bool,
}
const HANDLE: f32 = 70.0;
const AXES: [Color32; 3] = [
    Color32::from_rgb(235, 85, 85),
    Color32::from_rgb(105, 210, 105),
    Color32::from_rgb(95, 150, 245),
];
fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    v.map(|x| x / l)
}
fn add(a: [f32; 3], b: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s]
}
impl Studio {
    pub(crate) fn gizmo_toolbar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut g = gizmo(&ctx);
        ui.label("Edit:");
        for (mode, label, hint) in [
            (
                Mode::Move,
                "Move (W)",
                "Drag the arrows to move along an axis, the square to move freely",
            ),
            (
                Mode::Rotate,
                "Rotate (E)",
                "Drag a ring to turn about that axis",
            ),
            (
                Mode::Scale,
                "Scale (R)",
                "Drag an axis to stretch, the square to scale evenly",
            ),
        ] {
            if ui
                .selectable_label(g.mode == mode, label)
                .on_hover_text(hint)
                .clicked()
            {
                g.mode = mode;
            }
        }
        store(&ctx, g);
    }
    fn gizmo_target(&self, scene: &Scene) -> Option<Target> {
        let id = self.project.selected;
        let rs = |node: &veylvik_document::Node| -> Option<math::Mat4> {
            let placement = match &node.operation {
                veylvik_document::Operation::Shape3d(n) => &n.placement,
                veylvik_document::Operation::Text3d(n) => &n.placement,
                veylvik_document::Operation::ImageCard(n) => &n.placement,
                veylvik_document::Operation::Light3d(n) => &n.placement,
                veylvik_document::Operation::Camera3d(n) => &n.placement,
                _ => return None,
            };
            let time = self.project.time().ok()?;
            let r = math::rotate(placement.rotation.at(time).ok()?);
            Some(
                if matches!(node.operation, veylvik_document::Operation::Camera3d(_)) {
                    r
                } else {
                    math::mul(&r, &math::scale(placement.scale.at(time).ok()?))
                },
            )
        };
        let node = &self.draft;
        if node.id != id {
            return None;
        }
        let local = rs(node)?;
        if let Some(o) = scene.objects.iter().find(|o| o.node == id) {
            return Some(Target {
                node: id,
                origin: math::point(&o.world, [0.0; 3]),
                world: o.world,
                local,
                aim: None,
                scalable: true,
            });
        }
        if let Some(l) = scene.lights.iter().find(|l| l.node == id) {
            return Some(Target {
                node: id,
                origin: l.position(),
                world: l.world,
                local,
                aim: (l.kind != LightKind::Point).then(|| l.direction()),
                scalable: false,
            });
        }
        let c = scene.cameras.iter().find(|c| c.node == Some(id))?;
        Some(Target {
            node: id,
            origin: math::point(&c.world, [0.0; 3]),
            world: c.world,
            local,
            aim: None,
            scalable: false,
        })
    }
    /// World length that spans `HANDLE` pixels at the target.
    fn handle_length(p: &Projector, t: &Target) -> Option<f32> {
        let right = math::direction(&p.camera.world, [1.0, 0.0, 0.0]);
        let a = p.point(t.origin)?;
        let b = p.point(add(t.origin, right, 1.0))?;
        Some(HANDLE / a.distance(b).max(1e-6))
    }
    /// Screen points of every handle: (handle, from, to).
    fn handles(p: &Projector, t: &Target, mode: Mode) -> Vec<(Handle, Pos2, Pos2)> {
        let Some(length) = Self::handle_length(p, t) else {
            return Vec::new();
        };
        let Some(centre) = p.point(t.origin) else {
            return Vec::new();
        };
        let mut out = vec![];
        // Move uses world axes (Y up on screen); rotate and scale the object's own.
        let axes: [[f32; 3]; 3] = match mode {
            Mode::Move => [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]],
            _ => [0, 1, 2].map(|i| {
                let mut a = [0.0; 3];
                a[i] = if i == 1 { -1.0 } else { 1.0 };
                norm(math::direction(&t.world, a))
            }),
        };
        match mode {
            Mode::Move | Mode::Scale => {
                if mode == Mode::Move || t.scalable {
                    for (i, axis) in axes.iter().enumerate() {
                        if let Some(end) = p.point(add(t.origin, *axis, length)) {
                            out.push((Handle::Axis(i), centre, end));
                        }
                    }
                    out.push((Handle::Free, centre, centre));
                }
            }
            Mode::Rotate => {
                for (i, _) in axes.iter().enumerate() {
                    // A point on the ring, for picking; the ring is drawn separately.
                    let (u, v) = ((i + 1) % 3, (i + 2) % 3);
                    if let Some(at) = p.point(add(
                        add(t.origin, axes[u], length * std::f32::consts::FRAC_1_SQRT_2),
                        axes[v],
                        length * std::f32::consts::FRAC_1_SQRT_2,
                    )) {
                        out.push((Handle::Ring(i), at, at));
                    }
                }
            }
        }
        if let Some(dir) = t.aim
            && let Some(at) = p.point(add(t.origin, dir, length * 2.0))
        {
            out.push((Handle::Aim, at, at));
        }
        out
    }
    pub(crate) fn draw_gizmo(
        &self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        p: &Projector,
        scene: &Scene,
    ) {
        let Some(t) = self.gizmo_target(scene) else {
            return;
        };
        let g = gizmo(ctx);
        let Some(length) = Self::handle_length(p, &t) else {
            return;
        };
        if g.mode == Mode::Rotate {
            for (i, colour) in AXES.iter().enumerate() {
                let (u, v) = ((i + 1) % 3, (i + 2) % 3);
                let axis = |k: usize| {
                    let mut a = [0.0; 3];
                    a[k] = 1.0;
                    norm(math::direction(&t.world, a))
                };
                let (au, av) = (axis(u), axis(v));
                let stroke = Stroke::new(
                    if g.drag == Some(Handle::Ring(i)) {
                        3.0_f32
                    } else {
                        1.8_f32
                    },
                    *colour,
                );
                for k in 0..48 {
                    let a0 = k as f32 / 48.0 * std::f32::consts::TAU;
                    let a1 = (k + 1) as f32 / 48.0 * std::f32::consts::TAU;
                    let at =
                        |a: f32| add(add(t.origin, au, length * a.cos()), av, length * a.sin());
                    p.line(painter, at(a0), at(a1), stroke);
                }
            }
        }
        for (handle, from, to) in Self::handles(p, &t, g.mode) {
            let active = g.drag == Some(handle);
            match handle {
                Handle::Axis(i) => {
                    painter.line_segment(
                        [from, to],
                        Stroke::new(if active { 3.5_f32 } else { 2.2_f32 }, AXES[i]),
                    );
                    if g.mode == Mode::Scale {
                        painter.rect_filled(
                            egui::Rect::from_center_size(to, Vec2::splat(9.0)),
                            1.0,
                            AXES[i],
                        );
                    } else {
                        let d = (to - from).normalized();
                        let side = Vec2::new(-d.y, d.x);
                        painter.add(egui::Shape::convex_polygon(
                            vec![to + d * 9.0, to + side * 5.0, to - side * 5.0],
                            AXES[i],
                            Stroke::NONE,
                        ));
                    }
                }
                Handle::Free => {
                    painter.rect_stroke(
                        egui::Rect::from_center_size(from, Vec2::splat(14.0)),
                        2.0,
                        Stroke::new(if active { 2.5_f32 } else { 1.5_f32 }, Color32::WHITE),
                        egui::StrokeKind::Middle,
                    );
                }
                Handle::Ring(_) => {}
                Handle::Aim => {
                    if let Some(origin) = p.point(t.origin) {
                        painter.line_segment(
                            [origin, from],
                            Stroke::new(1.0_f32, Color32::from_rgb(255, 210, 115)),
                        );
                    }
                    painter.circle_filled(
                        from,
                        if active { 7.0 } else { 5.5 },
                        Color32::from_rgb(255, 210, 115),
                    );
                    painter.circle_stroke(from, 9.0, Stroke::new(1.0_f32, Color32::BLACK));
                }
            }
        }
    }
    /// Handles take drags that start on them; returns true while one is in use.
    pub(crate) fn gizmo_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        canvas: egui::Rect,
        transform: crate::viewer::ViewTransform,
    ) -> bool {
        let ctx = ui.ctx().clone();
        let mut g = gizmo(&ctx);
        if response.hovered() && !ui.input(|i| i.pointer.secondary_down()) {
            for (key, mode) in [
                (egui::Key::W, Mode::Move),
                (egui::Key::E, Mode::Rotate),
                (egui::Key::R, Mode::Scale),
            ] {
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
                    g.mode = mode;
                }
            }
        }
        let Some((p, scene, _)) = self.scene_projector(&ctx, canvas, transform) else {
            store(&ctx, g);
            return false;
        };
        let Some(t) = self.gizmo_target(&scene) else {
            g.drag = None;
            store(&ctx, g);
            return false;
        };
        if response.drag_started()
            && let Some(press) = ui.input(|i| i.pointer.press_origin())
        {
            g.drag = Self::handles(&p, &t, g.mode)
                .into_iter()
                .map(|(h, from, to)| {
                    let d = match h {
                        Handle::Axis(_) => {
                            let ab = to - from;
                            let s =
                                ((press - from).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                            press.distance(from + ab * s)
                        }
                        _ => press.distance(from),
                    };
                    (h, d)
                })
                .filter(|(_, d)| *d < 10.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(h, _)| h);
            g.aim_to = None;
        }
        let Some(handle) = g.drag else {
            store(&ctx, g);
            return false;
        };
        let delta = ui.input(|i| i.pointer.delta());
        if response.dragged() && delta != Vec2::ZERO {
            self.drag_handle(&ctx, handle, delta, &p, &t, g.mode, &mut g.aim_to);
        }
        if response.drag_stopped() {
            g.drag = None;
            g.aim_to = None;
            self.apply_draft();
        }
        store(&ctx, g);
        true
    }
    #[allow(clippy::too_many_arguments)]
    fn drag_handle(
        &mut self,
        ctx: &egui::Context,
        handle: Handle,
        delta: Vec2,
        p: &Projector,
        t: &Target,
        mode: Mode,
        aim_to: &mut Option<[f32; 3]>,
    ) {
        let Ok(time) = self.project.time() else {
            return;
        };
        let Some(length) = Self::handle_length(p, t) else {
            return;
        };
        let per_pixel = length / HANDLE;
        let right = math::direction(&p.camera.world, [1.0, 0.0, 0.0]);
        let down = math::direction(&p.camera.world, [0.0, 1.0, 0.0]);
        let screen = |v: Vec2| add(add([0.0; 3], right, v.x * per_pixel), down, v.y * per_pixel);
        // World directions into the node's own placement frame (inside any parents).
        let parent_inverse = math::linear_inverse(&t.world)
            .map(|wi| math::mul(&t.local, &wi))
            .unwrap_or(math::IDENTITY);
        let along = |from: Pos2, to: Pos2| {
            let ab = to - from;
            (delta.dot(ab) / ab.length().max(1e-6), ab.length().max(1e-6))
        };
        let handles = Self::handles(p, t, mode);
        let ends = |h: Handle| {
            handles
                .iter()
                .find(|(x, ..)| *x == h)
                .map(|(_, f, t)| (*f, *t))
        };
        let edit = |studio: &mut Self, path: &str, f: &dyn Fn([f32; 3]) -> [f32; 3]| {
            if let Some(Property::Vector(track)) = studio.draft.operation.property_mut(path)
                && let Ok(value) = track.at(time)
            {
                let _ = track.set_at(time, f(value));
            }
        };
        match (mode, handle) {
            (_, Handle::Aim) => {
                let current = aim_to.unwrap_or_else(|| {
                    add(t.origin, t.aim.unwrap_or([0.0, 0.0, -1.0]), length * 2.0)
                });
                let next = add(current, screen(delta), 1.0);
                *aim_to = Some(next);
                let dir = norm(math::direction(
                    &parent_inverse,
                    norm([0, 1, 2].map(|k| next[k] - t.origin[k])),
                ));
                let pitch = dir[1].clamp(-1.0, 1.0).asin().to_degrees();
                let yaw = (-dir[0]).atan2(-dir[2]).to_degrees();
                edit(self, "placement.rotation", &|_| [pitch, yaw, 0.0]);
            }
            (Mode::Move, Handle::Free) => {
                let d = math::direction(&parent_inverse, screen(delta));
                edit(self, "placement.position", &|v| {
                    [v[0] + d[0], v[1] + d[1], v[2] + d[2]]
                });
            }
            (Mode::Move, Handle::Axis(i)) => {
                let Some((from, to)) = ends(handle) else {
                    return;
                };
                let (px, span) = along(from, to);
                let mut axis = [0.0; 3];
                axis[i] = if i == 1 { -1.0 } else { 1.0 };
                let d = math::direction(&parent_inverse, axis.map(|a| a * px * length / span));
                edit(self, "placement.position", &|v| {
                    [v[0] + d[0], v[1] + d[1], v[2] + d[2]]
                });
            }
            (Mode::Rotate, Handle::Ring(i)) => {
                // Turning follows the drag across the ring, a degree per two pixels.
                let Some(centre) = p.point(t.origin) else {
                    return;
                };
                let Some((at, _)) = ends(handle) else { return };
                let tangent = Vec2::new(-(at - centre).y, (at - centre).x).normalized();
                let degrees = delta.dot(tangent) * 0.5;
                edit(self, "placement.rotation", &|v| {
                    let mut v = v;
                    v[i] += degrees;
                    v
                });
            }
            (Mode::Scale, Handle::Axis(i)) => {
                let Some((from, to)) = ends(handle) else {
                    return;
                };
                let (px, _) = along(from, to);
                let factor = (px / 100.0).exp();
                edit(self, "placement.scale", &|v| {
                    let mut v = v;
                    v[i] *= factor;
                    v
                });
            }
            (Mode::Scale, Handle::Free) => {
                let factor = ((delta.x - delta.y) / 100.0).exp();
                edit(self, "placement.scale", &|v| v.map(|x| x * factor));
            }
            _ => return,
        }
        let _ = t.node;
        self.draft_dirty = true;
        self.playing = false;
        self.queue_draft_preview(ctx);
    }
}
