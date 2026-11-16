//! Navigating a 3D scene in the viewer: orbit, pan, dolly and walk a free view that only the preview uses, and turn views and walks into camera keys.
use crate::Studio;
use eframe::egui;
use veylvik_document::{Command, NodeId, Operation, Time};
use veylvik_scene::FreeView;

#[derive(Clone, Default)]
struct State {
    /// Mouse navigation on (default) while a Render scene is viewed.
    off: bool,
    /// The free view, once the user has moved; None films with the scene's camera.
    view: Option<FreeView>,
    /// Recording a walk: the view at each played frame.
    recording: Option<Vec<(Time, FreeView)>>,
    /// While walking, keep the camera pointed at the selection.
    aim: bool,
}
fn id() -> egui::Id {
    egui::Id::new("viewport-3d")
}
fn state(ctx: &egui::Context) -> State {
    ctx.data_mut(|d| d.get_temp::<State>(id()).unwrap_or_default())
}
fn store(ctx: &egui::Context, s: State) {
    ctx.data_mut(|d| d.insert_temp(id(), s));
}
/// Degrees of turn per pixel of mouse movement.
const TURN: f32 = 0.25;
impl Studio {
    /// The Render scene node in the viewer, if any.
    /// The Render scene in the viewer: the viewed node itself, or the nearest one
    /// upstream of it (so a scene composited over a backdrop still navigates).
    pub(crate) fn viewed_scene(&self) -> Option<NodeId> {
        let doc = self.project.document.snapshot();
        let mut pending =
            std::collections::VecDeque::from([self.project.preview.unwrap_or(doc.output)]);
        let mut seen = std::collections::BTreeSet::new();
        while let Some(id) = pending.pop_front() {
            if !seen.insert(id) {
                continue;
            }
            let node = doc.nodes.get(&id)?;
            if matches!(node.operation, Operation::RenderScene(_)) {
                return Some(id);
            }
            let descriptor = node.operation.descriptor();
            pending.extend(node.inputs.iter().filter_map(|(port, source)| {
                (descriptor.port_kind(port) == veylvik_document::registry::PortKind::Image)
                    .then_some(*source)
            }));
        }
        None
    }
    /// The camera the viewed Render scene films with now, and its node.
    pub(crate) fn scene_camera(&self) -> Option<(veylvik_scene::Camera, Option<NodeId>)> {
        let doc = self.project.document.snapshot();
        let render = self.viewed_scene()?;
        let node = &doc.nodes[&render];
        let Operation::RenderScene(op) = &node.operation else {
            return None;
        };
        let time = self.project.time().ok()?;
        let scene = veylvik_scene::evaluate(doc, *node.inputs.get("scene")?, time).ok()?;
        let index = op.camera.at(time).ok()?.round().clamp(0.0, 64.0) as usize;
        let camera = scene.camera(index, doc.width, doc.height);
        let node = camera
            .node
            .or_else(|| scene.cameras.first().and_then(|c| c.node));
        Some((camera, node))
    }
    /// Look through `view` (None: the scene camera) and show it.
    pub(crate) fn set_free_view(&mut self, ctx: &egui::Context, view: Option<FreeView>) {
        let mut s = state(ctx);
        s.view = view;
        if view.is_none() {
            s.recording = None;
        }
        self.scene_view = view.map(|v| v.camera());
        store(ctx, s);
        self.request_preview();
    }
    /// The free view, if one is active.
    pub(crate) fn free_view(&self, ctx: &egui::Context) -> Option<FreeView> {
        state(ctx).view
    }
    /// The camera the viewer looks through now: the free view, else the scene's.
    pub(crate) fn viewing_camera(&self, ctx: &egui::Context) -> Option<veylvik_scene::Camera> {
        self.free_view(ctx)
            .map(|v| v.camera())
            .or_else(|| self.scene_camera().map(|(c, _)| c))
    }
    /// Whether the viewer's mouse drives the 3D view instead of 2D pan and zoom.
    pub(crate) fn navigating_3d(&self, ctx: &egui::Context) -> bool {
        self.viewed_scene().is_some() && !state(ctx).off
    }
    pub(crate) fn scene_toolbar(&mut self, ui: &mut egui::Ui) {
        if self.viewed_scene().is_none() {
            return;
        }
        let ctx = ui.ctx().clone();
        let mut s = state(&ctx);
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.strong("3D");
            let mut on = !s.off;
            if ui
                .checkbox(&mut on, "Navigate")
                .on_hover_text("Drag to orbit, middle-drag or Shift-drag to pan, scroll to dolly. Hold the right button to walk: W A S D to move, Q E down and up, Shift faster, move the mouse to look. Double-click for the home view.")
                .changed()
            {
                s.off = !on;
            }
            ui.weak(if s.view.is_some() {
                "Free view (preview only; renders use the scene camera)"
            } else {
                "Scene camera"
            });
            if s.view.is_some() && ui.button("Scene camera").on_hover_text("Look through the scene's camera again").clicked() {
                s.view = None;
                s.recording = None;
                changed = true;
            }
            self.scene_guides_toolbar(ui);
            self.gizmo_toolbar(ui);
            if ui.button("Home view").on_hover_text("The view that matches the 2D frame").clicked() {
                let doc = self.project.document.snapshot();
                s.view = Some(FreeView::home(doc.width, doc.height));
                changed = true;
            }
            if ui
                .button("Frame (F)")
                .on_hover_text("Bring the selected object (or everything) into view")
                .clicked()
            {
                self.frame_focus(&mut s);
                changed = true;
            }
            ui.checkbox(&mut s.aim, "Keep aimed")
                .on_hover_text("While walking, keep looking at the selected object, so circling it is easy");
            if ui
                .add_enabled(s.view.is_some(), egui::Button::new("Camera from view"))
                .on_hover_text("Put the scene camera here (one undo step; adds a camera if the scene has none)")
                .clicked()
                && let (Some(view), Ok(time)) = (s.view, self.project.time())
            {
                let doc = self.project.document.snapshot();
                let (position, rotation) = view.placement(doc.width, doc.height);
                match self.write_camera(&[(time, position, rotation)], false) {
                    Ok(()) => self.status = "Camera moved to the view".into(),
                    Err(e) => self.fail(e),
                }
            }
            let recording = s.recording.is_some();
            if ui
                .add(egui::Button::selectable(recording, if recording { "Stop recording" } else { "Record walk" }))
                .on_hover_text("Plays from the start while you walk (hold the right button), stops at the end, then writes a smooth camera move: shake is removed and keys are thinned. One undo step.")
                .clicked()
            {
                if let Some(samples) = s.recording.take() {
                    self.finish_recording(samples);
                } else {
                    if s.view.is_none() {
                        s.view = self.current_view();
                    }
                    s.recording = Some(Vec::new());
                    if !self.playing {
                        let range = self.playback_controls.range(self.project.document.snapshot().frame_count);
                        self.project.frame = range.0;
                        self.toggle_play();
                    }
                }
            }
            self.focus_button(ui);
            self.surface_buttons(ui);
            self.shot_button(ui);
            ui.menu_button("Moves", |ui| {
                ui.weak("A smooth camera move from this view over the play range:");
                for (label, hint, kind) in [
                    ("Orbit", "Circle a quarter turn around the selection", Move::Orbit),
                    ("Push in", "Glide towards the selection", Move::PushIn),
                    ("Pull out", "Glide back to reveal the scene", Move::PullOut),
                    ("Crane up", "Rise while looking at the selection", Move::Crane),
                    ("Fly by", "Pass sideways while turning to follow the selection", Move::FlyBy),
                ] {
                    if ui.button(label).on_hover_text(hint).clicked() {
                        match self.camera_move(kind, &s) {
                            Ok(()) => self.status = format!("{label}: camera keys written"),
                            Err(e) => self.fail(e),
                        }
                        ui.close();
                    }
                }
            });
        });
        self.scene_view = s.view.map(|v| v.camera());
        store(&ctx, s);
        if changed {
            self.request_preview();
        }
    }
    /// Mouse and keyboard over the viewer canvas. True when the view changed.
    pub(crate) fn scene_interact(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        canvas: egui::Rect,
    ) -> bool {
        let ctx = ui.ctx().clone();
        let mut s = state(&ctx);
        let start = |s: &mut State| {
            if s.view.is_none() {
                let doc = self.project.document.snapshot();
                s.view = Some(match self.scene_camera() {
                    Some((camera, _)) => FreeView::from_camera(
                        &camera,
                        veylvik_scene::home_distance(
                            doc.width,
                            camera.focal_length,
                            camera.sensor_width,
                        ),
                    ),
                    None => FreeView::home(doc.width, doc.height),
                });
            }
        };
        let mut changed = false;
        if response.hovered()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F))
        {
            self.frame_focus(&mut s);
            changed = true;
        }
        if response.double_clicked() {
            let doc = self.project.document.snapshot();
            s.view = Some(FreeView::home(doc.width, doc.height));
            changed = true;
        }
        let (delta, scroll, shift, middle, primary, secondary) = ui.input(|i| {
            (
                i.pointer.delta(),
                i.smooth_scroll_delta.y,
                i.modifiers.shift,
                i.pointer.middle_down(),
                i.pointer.primary_down(),
                i.pointer.secondary_down(),
            )
        });
        if response.hovered() && scroll != 0.0 {
            start(&mut s);
            if let Some(v) = &mut s.view {
                v.dolly((-scroll * 0.002).exp());
            }
            changed = true;
        }
        if response.dragged() && delta != egui::Vec2::ZERO && !secondary {
            start(&mut s);
            if let Some(v) = &mut s.view {
                if middle || (primary && shift) {
                    v.pan(delta.x / canvas.width(), delta.y / canvas.width());
                } else {
                    v.orbit(-delta.x * TURN, delta.y * TURN);
                }
            }
            changed = true;
        }
        // Walk while the right button is held over the viewer.
        if secondary && (response.hovered() || response.dragged()) {
            start(&mut s);
            let dt = ui.input(|i| i.stable_dt).min(0.1);
            let keys = ui.input_mut(|i| {
                [
                    egui::Key::W,
                    egui::Key::S,
                    egui::Key::D,
                    egui::Key::A,
                    egui::Key::E,
                    egui::Key::Q,
                ]
                .map(|k| {
                    let down = i.key_down(k);
                    i.consume_key(egui::Modifiers::NONE, k);
                    i.consume_key(egui::Modifiers::SHIFT, k);
                    down
                })
            });
            let focus = if s.aim {
                self.focus_sphere().map(|(c, _)| c)
            } else {
                None
            };
            if let Some(v) = &mut s.view {
                let speed = v.distance * if shift { 1.5 } else { 0.5 } * dt;
                let axis = |plus: bool, minus: bool| {
                    f32::from(u8::from(plus)) - f32::from(u8::from(minus))
                };
                v.walk(
                    -delta.x * TURN,
                    delta.y * TURN,
                    axis(keys[0], keys[1]) * speed,
                    axis(keys[2], keys[3]) * speed,
                    axis(keys[5], keys[4]) * speed,
                );
                if let Some(centre) = focus {
                    let eye = v.eye();
                    v.look_at(eye, centre);
                }
            }
            changed |= delta != egui::Vec2::ZERO || keys.iter().any(|k| *k);
            ctx.request_repaint();
        }
        // Say how to move, on the picture itself.
        let walking = secondary && (response.hovered() || response.dragged());
        let guide = if walking {
            "Walking · W A S D move · Q E down / up · Shift faster · move the mouse to look"
        } else {
            "3D · drag: orbit · Shift-drag or middle-drag: pan · scroll: dolly · hold right button + W A S D: walk · double-click: home"
        };
        let painter = ui.painter().with_clip_rect(canvas);
        let mut lines = vec![guide.to_owned()];
        if s.view.is_some() {
            lines.push("Free view: preview only, renders use the scene camera".into());
        }
        if s.recording.is_some() {
            lines.push(if self.playing {
                "REC  Recording the walk".to_owned()
            } else {
                "REC  Recording: press play and walk".to_owned()
            });
        }
        let mut at = canvas.left_top() + egui::vec2(10.0, 10.0);
        for line in lines {
            let galley = painter.layout_no_wrap(
                line,
                egui::FontId::proportional(13.0),
                egui::Color32::WHITE,
            );
            painter.rect_filled(
                egui::Rect::from_min_size(at, galley.size()).expand(4.0),
                3.0,
                egui::Color32::from_black_alpha(170),
            );
            let height = galley.size().y;
            painter.galley(at, galley, egui::Color32::WHITE);
            at.y += height + 10.0;
        }
        self.scene_view = s.view.map(|v| v.camera());
        store(&ctx, s);
        changed
    }
    /// While recording and playing, sample the view at each new frame; when play
    /// stops or wraps round, write the walk.
    pub(crate) fn scene_record_tick(&mut self, ctx: &egui::Context) {
        let mut s = state(ctx);
        let (Some(samples), Some(view)) = (&mut s.recording, s.view) else {
            return;
        };
        let Ok(time) = self.project.time() else {
            return;
        };
        let wrapped = samples
            .last()
            .is_some_and(|(t, _)| time.compare(*t).is_ok_and(|o| o.is_lt()));
        if (!self.playing && !samples.is_empty()) || wrapped {
            let samples = std::mem::take(samples);
            s.recording = None;
            store(ctx, s);
            if self.playing {
                self.toggle_play();
            }
            self.finish_recording(samples);
            return;
        }
        if self.playing
            && samples
                .last()
                .is_none_or(|(t, _)| t.compare(time).is_ok_and(|o| o.is_ne()))
            && samples.len() < veylvik_document::animation::MAX_KEYS
        {
            samples.push((time, view));
            store(ctx, s);
            ctx.request_repaint();
        }
    }
    /// The eye positions recorded so far, for the guides.
    pub(crate) fn recorded_path(&self, ctx: &egui::Context) -> Vec<[f32; 3]> {
        state(ctx)
            .recording
            .map(|r| r.iter().map(|(_, v)| v.eye()).collect())
            .unwrap_or_default()
    }
    /// The free view, or one starting from the scene's camera.
    fn current_view(&self) -> Option<FreeView> {
        let doc = self.project.document.snapshot();
        Some(match self.scene_camera() {
            Some((camera, _)) => FreeView::from_camera(
                &camera,
                veylvik_scene::home_distance(doc.width, camera.focal_length, camera.sensor_width),
            ),
            None => FreeView::home(doc.width, doc.height),
        })
    }
    fn frame_focus(&self, s: &mut State) {
        if s.view.is_none() {
            s.view = self.current_view();
        }
        if let (Some(v), Some((centre, radius))) = (&mut s.view, self.focus_sphere()) {
            v.frame(centre, radius);
        }
    }
    fn finish_recording(&mut self, samples: Vec<(Time, FreeView)>) {
        if samples.len() < 2 {
            self.status = "Nothing recorded: walk while it plays".into();
            return;
        }
        let doc = self.project.document.snapshot();
        let poses: Vec<veylvik_scene::Pose> = samples
            .iter()
            .map(|(t, v)| {
                let (position, rotation) = v.placement(doc.width, doc.height);
                (*t, position, rotation)
            })
            .collect();
        // Half a second of smoothing at the composition rate; a key every 4 frames.
        let fps = doc.frame_rate.seconds().unwrap_or(24.0).max(1.0);
        let smoothed = veylvik_scene::smooth_walk(&poses, (fps * 0.5) as usize | 1, 4);
        let count = smoothed.len();
        match self.write_camera(&smoothed, true) {
            Ok(()) => self.status = format!("Recorded a smooth camera move ({count} keys)"),
            Err(e) => self.fail(e),
        }
    }
    /// A ready-made camera move from the current view over the play range.
    fn camera_move(&mut self, kind: Move, s: &State) -> Result<(), String> {
        let doc = self.project.document.snapshot();
        let start = s
            .view
            .or_else(|| self.current_view())
            .ok_or("No 3D scene in view")?;
        let focus = self.focus_sphere().map(|(c, _)| c).unwrap_or(start.target);
        let range = self.playback_controls.range(doc.frame_count);
        let frames = (range.1 - range.0).max(1);
        let ease = |t: f32| t * t * (3.0 - 2.0 * t);
        let mut poses = Vec::new();
        let mut frame = range.0;
        loop {
            let t = ease((frame - range.0) as f32 / frames as f32);
            let mut v = start;
            let eye0 = start.eye();
            match kind {
                Move::Orbit => {
                    v.look_at(eye0, focus);
                    v.orbit(90.0 * t, 0.0);
                }
                Move::PushIn | Move::PullOut => {
                    v.look_at(eye0, focus);
                    let (from, to) = if kind == Move::PushIn {
                        (1.0, 0.55)
                    } else {
                        (1.0, 1.8)
                    };
                    v.dolly(from + (to - from) * t);
                }
                Move::Crane => {
                    let rise = start.distance * 0.6 * t;
                    v.look_at([eye0[0], eye0[1] - rise, eye0[2]], focus);
                }
                Move::FlyBy => {
                    let right =
                        veylvik_scene::math::direction(&start.camera().world, [1.0, 0.0, 0.0]);
                    let side = start.distance * 0.8 * (t - 0.5);
                    v.look_at(
                        [
                            eye0[0] + right[0] * side,
                            eye0[1] + right[1] * side,
                            eye0[2] + right[2] * side,
                        ],
                        focus,
                    );
                }
            }
            let (position, rotation) = v.placement(doc.width, doc.height);
            poses.push((
                Time::at_frame(frame, doc.frame_rate).map_err(|e| e.to_string())?,
                position,
                rotation,
            ));
            if frame >= range.1 {
                break;
            }
            frame = (frame + 6).min(range.1);
        }
        // Unwrap turns without smoothing (window 1) so an orbit never spins back.
        let poses = veylvik_scene::smooth_walk(&poses, 1, 1);
        self.write_camera(&poses, true)
    }
    /// Put the scene camera at these poses: smooth keys when `keyed` (or when it is
    /// already animated), else a still placement. A scene without a camera gets one,
    /// merged into what Render scene films. One undo step.
    fn write_camera(&mut self, poses: &[veylvik_scene::Pose], keyed: bool) -> Result<(), String> {
        self.edit_scene_camera(|c| {
            for (time, position, rotation) in poses {
                if keyed {
                    c.placement
                        .position
                        .set_key_at(*time, *position)
                        .map_err(|e| e.to_string())?;
                    c.placement
                        .rotation
                        .set_key_at(*time, *rotation)
                        .map_err(|e| e.to_string())?;
                } else {
                    c.placement
                        .position
                        .set_at(*time, *position)
                        .map_err(|e| e.to_string())?;
                    c.placement
                        .rotation
                        .set_at(*time, *rotation)
                        .map_err(|e| e.to_string())?;
                }
            }
            if keyed {
                use veylvik_document::Interpolation::Smooth;
                c.placement.position.set_interpolation(Smooth);
                c.placement.rotation.set_interpolation(Smooth);
            }
            Ok(())
        })
    }
    /// Change the camera the viewed Render scene films with, in one undo step; a
    /// scene without one gets a new camera merged in front of Render scene.
    pub(crate) fn edit_scene_camera(
        &mut self,
        edit: impl FnOnce(&mut veylvik_document::nodes::Camera3d) -> Result<(), String>,
    ) -> Result<(), String> {
        use veylvik_document::{Node, nodes};
        let doc = self.project.document.snapshot();
        let render = self.viewed_scene().ok_or("No 3D scene in view")?;
        let mut commands = Vec::new();
        let (mut camera, existing) = match self.scene_camera().and_then(|(_, node)| node) {
            Some(id) => (
                doc.nodes.get(&id).ok_or("The camera is missing")?.clone(),
                true,
            ),
            None => {
                let op: Operation = serde_json::from_value(nodes::CAMERA_3D.default_operation())
                    .map_err(|e| e.to_string())?;
                (Node::new("Camera", op, &[]), false)
            }
        };
        let Operation::Camera3d(c) = &mut camera.operation else {
            return Err("Not a Camera node".into());
        };
        edit(c)?;
        let camera_id = camera.id;
        commands.push(Command::Put {
            node: Box::new(camera),
        });
        if !existing {
            // Film the scene with the new camera: merge it in front of Render scene.
            let mut render_node = doc.nodes[&render].clone();
            let root = *render_node
                .inputs
                .get("scene")
                .ok_or("Render scene needs a scene")?;
            let merge = Node::new(
                "Scene with camera",
                Operation::MergeScenes(nodes::MergeScenes {}),
                &[("scene", root), ("scene_2", camera_id)],
            );
            render_node.inputs.insert("scene".into(), merge.id);
            commands.push(Command::Put {
                node: Box::new(merge),
            });
            commands.push(Command::Put {
                node: Box::new(render_node),
            });
        }
        self.project.apply(&commands).map_err(|e| e.to_string())?;
        self.refresh_draft();
        self.request_preview();
        Ok(())
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Move {
    Orbit,
    PushIn,
    PullOut,
    Crane,
    FlyBy,
}
