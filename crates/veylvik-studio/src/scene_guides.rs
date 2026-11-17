//! Scene guides over the 3D viewer: cameras with their framing, lights, the 2D frame, a floor grid, camera paths and the selection, drawn (never rendered) through the camera the viewer looks through.
use crate::Studio;
use eframe::egui::{self, Color32, Pos2, Stroke};
use veylvik_document::{NodeId, Operation, Time, nodes::LightKind};
use veylvik_scene::{Camera, FreeView, Geometry, Scene, math};

#[derive(Clone)]
struct Show {
    cameras: bool,
    lights: bool,
    frame: bool,
    paths: bool,
    selection: bool,
    points: bool,
}
impl Default for Show {
    fn default() -> Self {
        Self {
            cameras: true,
            lights: true,
            frame: true,
            paths: true,
            selection: true,
            points: true,
        }
    }
}
/// An animated camera's route and its key positions.
type CameraPath = (NodeId, Vec<[f32; 3]>, Vec<[f32; 3]>);
/// A camera path cached per document revision.
#[derive(Clone, Default)]
struct Paths {
    revision: u64,
    paths: Vec<CameraPath>,
}
fn show(ctx: &egui::Context) -> Show {
    ctx.data_mut(|d| {
        d.get_temp::<Show>(egui::Id::new("scene-guides"))
            .unwrap_or_default()
    })
}
/// Projects scene points onto the viewer canvas through one camera.
pub(crate) struct Projector {
    pub camera: Camera,
    view: math::Mat4,
    width: u32,
    height: u32,
    canvas: egui::Rect,
    transform: [f32; 4],
}
const NEAR: f32 = 1.0;
impl Projector {
    fn to_canvas(&self, pixel: [f32; 2]) -> Pos2 {
        let [ox, oy, sx, sy] = self.transform;
        Pos2::new(
            self.canvas.min.x + self.canvas.width() * (ox + sx * pixel[0] / self.width as f32),
            self.canvas.min.y + self.canvas.height() * (oy + sy * pixel[1] / self.height as f32),
        )
    }
    /// The frame pixel under a canvas position (the inverse of `to_canvas`).
    pub(crate) fn to_pixel(&self, at: Pos2) -> [f32; 2] {
        let [ox, oy, sx, sy] = self.transform;
        [
            ((at.x - self.canvas.min.x) / self.canvas.width() - ox) / sx * self.width as f32,
            ((at.y - self.canvas.min.y) / self.canvas.height() - oy) / sy * self.height as f32,
        ]
    }
    pub(crate) fn point(&self, p: [f32; 3]) -> Option<Pos2> {
        self.camera
            .project(p, self.width, self.height)
            .map(|(pixel, _)| self.to_canvas(pixel))
    }
    /// A segment clipped against the camera's near plane.
    fn segment(&self, a: [f32; 3], b: [f32; 3]) -> Option<[Pos2; 2]> {
        let (mut va, mut vb) = (math::point(&self.view, a), math::point(&self.view, b));
        let (za, zb) = (-va[2], -vb[2]);
        if za < NEAR && zb < NEAR {
            return None;
        }
        let clip = |from: [f32; 3], to: [f32; 3], zf: f32, zt: f32| {
            let t = (NEAR - zf) / (zt - zf);
            [0, 1, 2].map(|i| from[i] + (to[i] - from[i]) * t)
        };
        if za < NEAR {
            va = clip(va, vb, za, zb);
        } else if zb < NEAR {
            vb = clip(vb, va, zb, za);
        }
        let back = math::rigid_inverse(&self.view);
        Some([
            self.point(math::point(&back, va))?,
            self.point(math::point(&back, vb))?,
        ])
    }
    pub(crate) fn line(&self, painter: &egui::Painter, a: [f32; 3], b: [f32; 3], stroke: Stroke) {
        if let Some(points) = self.segment(a, b) {
            painter.line_segment(points, stroke);
        }
    }
}
fn add(a: [f32; 3], b: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s]
}
fn tint(colour: [f32; 3]) -> Color32 {
    let m = colour.iter().copied().fold(1e-6, f32::max);
    let c = colour.map(|v| egui::ecolor::gamma_u8_from_linear_f32(v / m));
    Color32::from_rgb(c[0], c[1], c[2])
}
/// Local bounding box of an object's geometry.
pub(crate) fn bounds(geometry: &Geometry) -> ([f32; 3], [f32; 3]) {
    veylvik_scene::local_bounds(geometry)
}
impl Studio {
    /// The guides row (toggles and quick views) inside the 3D toolbar.
    pub(crate) fn scene_guides_toolbar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut s = show(&ctx);
        ui.menu_button("Guides", |ui| {
            ui.checkbox(&mut s.cameras, "Cameras and their framing");
            ui.checkbox(&mut s.lights, "Lights");
            ui.checkbox(&mut s.frame, "2D frame, floor grid and axes");
            ui.checkbox(&mut s.paths, "Camera paths");
            ui.checkbox(&mut s.selection, "Selected object");
            ui.checkbox(&mut s.points, "Points (from a camera solve)");
            ui.separator();
            ui.horizontal(|ui| {
                let all = |s: &mut Show, on: bool| {
                    *s = Show {
                        cameras: on,
                        lights: on,
                        frame: on,
                        paths: on,
                        selection: on,
                        points: on,
                    };
                };
                if ui
                    .button("Hide all")
                    .on_hover_text(
                        "See only the picture; F11 also shows the viewer alone on the whole screen",
                    )
                    .clicked()
                {
                    all(&mut s, false);
                }
                if ui.button("Show all").clicked() {
                    all(&mut s, true);
                }
            });
        });
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("scene-guides"), s));
        let doc = self.project.document.snapshot();
        let (w, h) = (doc.width, doc.height);
        let current = self
            .free_view(&ctx)
            .or_else(|| {
                self.scene_camera().map(|(c, _)| {
                    FreeView::from_camera(
                        &c,
                        veylvik_scene::home_distance(w, c.focal_length, c.sensor_width),
                    )
                })
            })
            .unwrap_or_else(|| FreeView::home(w, h));
        let turned = |yaw: f32, pitch: f32| {
            let mut v = current;
            v.yaw = yaw;
            v.pitch = pitch;
            v
        };
        let mut chosen = None;
        ui.label("View:");
        if ui
            .small_button("Front")
            .on_hover_text("Face the 2D frame")
            .clicked()
        {
            chosen = Some(Some(turned(0.0, 0.0)));
        }
        if ui
            .small_button("Top")
            .on_hover_text("Look down on the scene")
            .clicked()
        {
            chosen = Some(Some(turned(0.0, 89.0)));
        }
        if ui
            .small_button("Side")
            .on_hover_text("Look from the right")
            .clicked()
        {
            chosen = Some(Some(turned(90.0, 0.0)));
        }
        if ui
            .small_button("Camera")
            .on_hover_text("Look through the scene's camera")
            .clicked()
        {
            chosen = Some(None);
        }
        let selected_light = self.selected_light();
        if ui
            .add_enabled(selected_light.is_some(), egui::Button::new("Light").small())
            .on_hover_text("Look along the selected light: the quickest way to aim a spot or sun")
            .clicked()
            && let Some(light) = selected_light
        {
            let camera = Camera {
                node: None,
                world: light.world,
                focal_length: 30.0,
                sensor_width: 36.0,
                f_stop: 0.0,
                focus_distance: 0.0,
                focus_on: None,
            };
            chosen = Some(Some(FreeView::from_camera(
                &camera,
                veylvik_scene::home_distance(w, 50.0, 36.0) * 0.5,
            )));
        }
        if let Some(view) = chosen {
            self.set_free_view(&ctx, view);
        }
    }
    /// Centre and radius of the selected object, else of every object in view.
    pub(crate) fn focus_sphere(&self) -> Option<([f32; 3], f32)> {
        let (scene, _) = self.viewed_scene_now()?;
        let selected: Vec<_> = scene
            .objects
            .iter()
            .filter(|o| o.node == self.project.selected)
            .collect();
        let objects = if selected.is_empty() {
            scene.objects.iter().collect()
        } else {
            selected
        };
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for object in objects {
            let (a, b) = bounds(&object.geometry);
            for i in 0..8 {
                let p = math::point(
                    &object.world,
                    [
                        if i & 1 == 0 { a[0] } else { b[0] },
                        if i & 2 == 0 { a[1] } else { b[1] },
                        if i & 4 == 0 { a[2] } else { b[2] },
                    ],
                );
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        if lo[0] > hi[0] {
            return None;
        }
        let centre = [0, 1, 2].map(|k| (lo[k] + hi[k]) * 0.5);
        let radius = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2))
            .sqrt()
            * 0.5;
        Some((centre, radius))
    }
    /// The viewed scene, the camera Render scene uses, and a projector onto `canvas`.
    pub(crate) fn scene_projector(
        &self,
        ctx: &egui::Context,
        canvas: egui::Rect,
        transform: crate::viewer::ViewTransform,
    ) -> Option<(Projector, Scene, Option<NodeId>)> {
        let (scene, active) = self.viewed_scene_now()?;
        let camera = self.viewing_camera(ctx)?;
        let doc = self.project.document.snapshot();
        Some((
            Projector {
                camera,
                view: math::rigid_inverse(&camera.world),
                width: doc.width,
                height: doc.height,
                canvas,
                transform: transform.0,
            },
            scene,
            active,
        ))
    }
    fn viewed_scene_now(&self) -> Option<(Scene, Option<NodeId>)> {
        let doc = self.project.document.snapshot();
        let render = self.viewed_scene()?;
        let node = &doc.nodes[&render];
        let time = self.project.time().ok()?;
        let scene = veylvik_scene::evaluate(doc, *node.inputs.get("scene")?, time).ok()?;
        let active = self.scene_camera().and_then(|(c, _)| c.node);
        Some((scene, active))
    }
    fn selected_light(&self) -> Option<veylvik_scene::Light> {
        let (scene, _) = self.viewed_scene_now()?;
        scene
            .lights
            .into_iter()
            .find(|l| l.node == self.project.selected)
    }
    /// Camera positions along each animated camera's keys, cached per revision.
    fn camera_paths(&self, ctx: &egui::Context) -> Vec<CameraPath> {
        let doc = self.project.document.snapshot();
        let revision = self.project.document.revision();
        let id = egui::Id::new("scene-camera-paths");
        let cached: Option<Paths> = ctx.data_mut(|d| d.get_temp(id));
        if let Some(c) = cached.filter(|c| c.revision == revision) {
            return c.paths;
        }
        let mut paths = Vec::new();
        let Some(render) = self.viewed_scene() else {
            return paths;
        };
        let Some(root) = doc.nodes[&render].inputs.get("scene").copied() else {
            return paths;
        };
        let frames = doc.frame_count.clamp(1, 600);
        let step = (doc.frame_count / frames).max(1);
        let at = |frame: u32| Time::at_frame(i64::from(frame), doc.frame_rate).ok();
        for (node, n) in &doc.nodes {
            let Operation::Camera3d(c) = &n.operation else {
                continue;
            };
            if !c.placement.position.is_animated() {
                continue;
            }
            let position = |time: Time| {
                veylvik_scene::evaluate(doc, root, time).ok().and_then(|s| {
                    s.cameras
                        .iter()
                        .find(|cam| cam.node == Some(*node))
                        .map(|cam| math::point(&cam.world, [0.0; 3]))
                })
            };
            let route: Vec<[f32; 3]> = (0..doc.frame_count)
                .step_by(step as usize)
                .filter_map(|f| at(f).and_then(position))
                .collect();
            let keys: Vec<[f32; 3]> = c
                .placement
                .position
                .keys()
                .iter()
                .filter_map(|k| position(k.time))
                .collect();
            if route.len() > 1 {
                paths.push((*node, route, keys));
            }
        }
        ctx.data_mut(|d| {
            d.insert_temp(
                id,
                Paths {
                    revision,
                    paths: paths.clone(),
                },
            )
        });
        paths
    }
    /// Draw the guides; a click on one selects its node.
    pub(crate) fn scene_guides(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        canvas: egui::Rect,
        transform: crate::viewer::ViewTransform,
    ) {
        if self.viewer_fullscreen {
            return;
        }
        let ctx = ui.ctx().clone();
        let Some((scene, active)) = self.viewed_scene_now() else {
            return;
        };
        let Some(camera) = self.viewing_camera(&ctx) else {
            return;
        };
        let doc = self.project.document.snapshot();
        let (w, h) = (doc.width, doc.height);
        let s = show(&ctx);
        let p = Projector {
            camera,
            view: math::rigid_inverse(&camera.world),
            width: w,
            height: h,
            canvas,
            transform: transform.0,
        };
        let painter = ui.painter().with_clip_rect(canvas);
        let faint = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(170, 180, 200, 45));
        let (wf, hf) = (w as f32, h as f32);
        if s.frame {
            // A floor grid level with the frame's bottom edge, the 2D frame at depth 0, axes.
            let spacing = wf / 8.0;
            for i in -16..=16 {
                let o = i as f32 * spacing;
                p.line(
                    &painter,
                    [wf * 0.5 + o, hf, -wf * 2.0],
                    [wf * 0.5 + o, hf, wf * 2.0],
                    faint,
                );
                p.line(&painter, [-wf * 1.5, hf, o], [wf * 2.5, hf, o], faint);
            }
            let frame = Stroke::new(1.2_f32, Color32::from_rgba_unmultiplied(220, 225, 235, 120));
            let corners = [
                [0.0, 0.0, 0.0],
                [wf, 0.0, 0.0],
                [wf, hf, 0.0],
                [0.0, hf, 0.0],
            ];
            for i in 0..4 {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                // Dashed: eight pieces per edge.
                for k in (0..8).step_by(2) {
                    let t0 = k as f32 / 8.0;
                    let t1 = (k + 1) as f32 / 8.0;
                    let lerp = |t: f32| [0, 1, 2].map(|j| a[j] + (b[j] - a[j]) * t);
                    p.line(&painter, lerp(t0), lerp(t1), frame);
                }
            }
            let centre = [wf * 0.5, hf * 0.5, 0.0];
            let reach = wf / 16.0;
            for (axis, colour) in [
                ([1.0, 0.0, 0.0], Color32::from_rgb(230, 90, 90)),
                ([0.0, -1.0, 0.0], Color32::from_rgb(110, 210, 110)),
                ([0.0, 0.0, 1.0], Color32::from_rgb(100, 150, 240)),
            ] {
                p.line(
                    &painter,
                    centre,
                    add(centre, axis, reach),
                    Stroke::new(2.0_f32, colour),
                );
            }
        }
        let selected = self.project.selected;
        if s.selection
            && let Some(object) = scene.objects.iter().find(|o| o.node == selected)
        {
            let (lo, hi) = bounds(&object.geometry);
            let corner = |i: usize| {
                math::point(
                    &object.world,
                    [
                        if i & 1 == 0 { lo[0] } else { hi[0] },
                        if i & 2 == 0 { lo[1] } else { hi[1] },
                        if i & 4 == 0 { lo[2] } else { hi[2] },
                    ],
                )
            };
            let stroke = Stroke::new(1.5_f32, Color32::from_rgb(255, 210, 115));
            for (a, b) in [
                (0, 1),
                (2, 3),
                (4, 5),
                (6, 7),
                (0, 2),
                (1, 3),
                (4, 6),
                (5, 7),
                (0, 4),
                (1, 5),
                (2, 6),
                (3, 7),
            ] {
                p.line(&painter, corner(a), corner(b), stroke);
            }
        }
        let mut hits: Vec<(Pos2, NodeId, String)> = Vec::new();
        if s.paths {
            for (node, route, keys) in self.camera_paths(&ctx) {
                let colour = if Some(node) == active {
                    Color32::from_rgba_unmultiplied(255, 210, 115, 170)
                } else {
                    Color32::from_rgba_unmultiplied(180, 190, 210, 120)
                };
                for pair in route.windows(2) {
                    p.line(&painter, pair[0], pair[1], Stroke::new(1.5_f32, colour));
                }
                for key in keys {
                    if let Some(at) = p.point(key) {
                        painter.circle_filled(at, 3.5, colour);
                    }
                }
            }
        }
        // The walk being recorded, as it happens.
        let walk = self.recorded_path(&ctx);
        for pair in walk.windows(2) {
            p.line(
                &painter,
                pair[0],
                pair[1],
                Stroke::new(2.0_f32, Color32::from_rgb(240, 80, 80)),
            );
        }
        if s.cameras {
            for cam in &scene.cameras {
                // Looking through it: its body would cover the whole view.
                if self.free_view(&ctx).is_none() && cam.node == active {
                    continue;
                }
                let is_active = cam.node == active;
                let colour = if is_active {
                    Color32::from_rgb(255, 210, 115)
                } else {
                    Color32::from_rgb(190, 198, 215)
                };
                let stroke = Stroke::new(if is_active { 1.6_f32 } else { 1.0_f32 }, colour);
                let eye = math::point(&cam.world, [0.0; 3]);
                // The frame this camera films, where 2D layers sit at its home distance.
                let d = veylvik_scene::home_distance(w, cam.focal_length, cam.sensor_width);
                let corners = [[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]]
                    .map(|[x, y]| math::point(&cam.world, [x * wf, y * hf, -d]));
                for i in 0..4 {
                    p.line(&painter, corners[i], corners[(i + 1) % 4], stroke);
                    p.line(
                        &painter,
                        eye,
                        corners[i],
                        Stroke::new(0.8_f32, colour.gamma_multiply(0.55)),
                    );
                }
                // A small body behind the eye, sized to stay readable.
                let body = d * 0.06;
                let back = [[-0.5, -0.35], [0.5, -0.35], [0.5, 0.35], [-0.5, 0.35]]
                    .map(|[x, y]| math::point(&cam.world, [x * body, y * body, body * 0.9]));
                for i in 0..4 {
                    p.line(&painter, back[i], back[(i + 1) % 4], stroke);
                    p.line(&painter, back[i], eye, stroke);
                }
                if let (Some(at), Some(node)) = (p.point(eye), cam.node) {
                    let label = doc
                        .nodes
                        .get(&node)
                        .map_or("Camera".into(), |n| n.label.clone());
                    painter.text(
                        at + egui::vec2(8.0, -8.0),
                        egui::Align2::LEFT_BOTTOM,
                        &label,
                        egui::FontId::proportional(11.0),
                        colour,
                    );
                    hits.push((at, node, label));
                }
            }
        }
        if s.lights {
            // A light split into strips is one node: one marker, at the row's middle.
            let mut drawn = std::collections::BTreeSet::new();
            for light in &scene.lights {
                if !drawn.insert(light.node) {
                    continue;
                }
                let colour = tint(light.colour);
                let selected_light = light.node == selected;
                let ring = Stroke::new(if selected_light { 2.0_f32 } else { 1.2_f32 }, colour);
                let row: Vec<[f32; 3]> = scene
                    .lights
                    .iter()
                    .filter(|l| l.node == light.node)
                    .map(|l| l.position())
                    .collect();
                let at =
                    [0, 1, 2].map(|k| row.iter().map(|p| p[k]).sum::<f32>() / row.len() as f32);
                let dir = light.direction();
                let reach = wf * 0.18;
                match light.kind {
                    LightKind::Point | LightKind::Area => {}
                    LightKind::Spot => {
                        let tip = add(at, dir, reach);
                        let half = (light.cone * 0.5).to_radians();
                        let radius = reach * half.tan();
                        let right = math::direction(&light.world, [1.0, 0.0, 0.0]);
                        let up = math::direction(&light.world, [0.0, 1.0, 0.0]);
                        let rim = |k: usize| {
                            let a = k as f32 / 24.0 * std::f32::consts::TAU;
                            add(add(tip, right, radius * a.cos()), up, radius * a.sin())
                        };
                        for k in 0..24 {
                            p.line(&painter, rim(k), rim(k + 1), ring);
                        }
                        for k in [0, 6, 12, 18] {
                            p.line(
                                &painter,
                                at,
                                rim(k),
                                Stroke::new(0.8_f32, colour.gamma_multiply(0.6)),
                            );
                        }
                        p.line(&painter, at, tip, Stroke::new(1.0_f32, colour));
                    }
                    LightKind::Sun => {
                        let right = math::direction(&light.world, [1.0, 0.0, 0.0]);
                        for o in [-1.0, 0.0, 1.0] {
                            let from = add(at, right, o * reach * 0.15);
                            p.line(
                                &painter,
                                from,
                                add(from, dir, reach * 0.6),
                                Stroke::new(1.2_f32, colour),
                            );
                        }
                    }
                }
                if let Some(centre) = p.point(at) {
                    painter.circle_filled(centre, if selected_light { 8.0 } else { 6.5 }, colour);
                    painter.circle_stroke(centre, 11.0, ring);
                    for k in 0..8 {
                        let a = k as f32 / 8.0 * std::f32::consts::TAU;
                        let v = egui::vec2(a.cos(), a.sin());
                        painter.line_segment(
                            [centre + v * 13.0, centre + v * 17.0],
                            Stroke::new(1.0_f32, colour),
                        );
                    }
                    let label = doc
                        .nodes
                        .get(&light.node)
                        .map_or("Light".into(), |n| n.label.clone());
                    painter.text(
                        centre + egui::vec2(14.0, 10.0),
                        egui::Align2::LEFT_TOP,
                        &label,
                        egui::FontId::proportional(11.0),
                        colour,
                    );
                    hits.push((centre, light.node, label));
                }
            }
        }
        for object in &scene.objects {
            if let Some(at) = p.point(math::point(&object.world, [0.0; 3])) {
                let label = doc
                    .nodes
                    .get(&object.node)
                    .map_or(String::new(), |n| n.label.clone());
                hits.push((at, object.node, label));
            }
        }
        self.draw_gizmo(&ctx, &painter, &p, &scene);
        if self.scene_light_pick(&ctx, &painter, &p, response) {
            return;
        }
        if self.scene_focus(&ctx, &painter, &p, &scene, response) {
            return;
        }
        if s.points && self.scene_points(&painter, &p, &scene, response) {
            return;
        }
        // Hover names a guide; a click selects its node.
        if let Some(pointer) = response.hover_pos() {
            let nearest = hits
                .iter()
                .filter(|(at, ..)| at.distance(pointer) < 16.0)
                .min_by(|a, b| a.0.distance(pointer).total_cmp(&b.0.distance(pointer)));
            if let Some((at, node, label)) = nearest {
                painter.circle_stroke(*at, 15.0, Stroke::new(1.0_f32, Color32::WHITE));
                if !label.is_empty() {
                    painter.text(
                        *at + egui::vec2(0.0, 20.0),
                        egui::Align2::CENTER_TOP,
                        label,
                        egui::FontId::proportional(12.0),
                        Color32::WHITE,
                    );
                }
                if response.clicked() && self.can_change_selection() {
                    self.project.selected = *node;
                    self.refresh_draft();
                }
            }
        }
    }
}
