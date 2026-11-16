//! Match the shot for 3D: analyse the footage behind the viewed 3D render off the UI thread (or take a clicked light), then aim a sun where its light comes from and set the environment's level, as one undo step.
use crate::Studio;
use eframe::egui;
use std::sync::{Arc, Mutex};
use veylvik_document::{Command, Node, NodeId, Operation, Time, nodes, optical::ShotLight};

/// The shot's light, a light source seen in it (pixels), and the frame size.
type Analysis = Result<(ShotLight, Option<[f32; 2]>, u32, u32), String>;
/// A running analysis and the frame it is for.
#[derive(Clone)]
struct Job {
    result: Arc<Mutex<Option<Analysis>>>,
    time: Time,
}
fn id() -> egui::Id {
    egui::Id::new("scene-shot-match")
}
fn pick_id() -> egui::Id {
    egui::Id::new("scene-shot-pick")
}
impl Studio {
    /// The footage a 3D render sits on: the background of a Merge whose foreground
    /// leads to `render`, else the scene's Environment image, else the backdrop.
    fn shot_behind(&self, render: NodeId, environment: Option<NodeId>) -> Option<NodeId> {
        let doc = self.project.document.snapshot();
        let reaches = |start: NodeId| {
            let mut pending = vec![start];
            let mut seen = std::collections::BTreeSet::new();
            while let Some(id) = pending.pop() {
                if id == render {
                    return true;
                }
                if seen.insert(id)
                    && let Some(node) = doc.nodes.get(&id)
                {
                    pending.extend(node.inputs.values().copied());
                }
            }
            false
        };
        doc.nodes
            .values()
            .filter(|n| matches!(n.operation, Operation::Merge(_)))
            .find(|n| n.inputs.get("foreground").is_some_and(|f| reaches(*f)))
            .and_then(|n| n.inputs.get("background").copied())
            .or(environment)
            .or_else(|| veylvik_document::presets::background::automatic(doc))
            .filter(|id| *id != render)
    }
    /// The Match the shot and Pick light buttons in the 3D toolbar, and applying a
    /// finished analysis.
    pub(crate) fn shot_button(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let job = ctx.data(|d| d.get_temp::<Job>(id()));
        if let Some(job) = &job {
            let done = job.result.lock().ok().and_then(|mut r| r.take());
            match done {
                Some(result) => {
                    ctx.data_mut(|d| d.remove::<Job>(id()));
                    if let Err(e) = result.and_then(|found| self.apply_shot(found, job.time)) {
                        self.fail(e);
                    }
                }
                None => {
                    ui.add(egui::Spinner::new());
                    return;
                }
            }
        }
        if ui
            .button("Match the shot")
            .on_hover_text("Aim a sun where the footage's light comes from (a sun or lamp seen in the frame, else its bright side), with its colour and strength, and set the environment's level to the shot's. It sets the selected light, the scene's first, or a new one")
            .clicked()
            && let Err(e) = self.start_shot_match(&ctx, None)
        {
            self.fail(e);
        }
        let picking = ctx.data(|d| d.get_temp::<bool>(pick_id()).unwrap_or(false));
        if ui
            .add(egui::Button::selectable(picking, "Pick light"))
            .on_hover_text(
                "Click where the light is in the frame (a sun, lamp or window) to shine from there",
            )
            .clicked()
        {
            ctx.data_mut(|d| d.insert_temp(pick_id(), !picking));
        }
    }
    /// While Pick light is on: a crosshair and, on click, a match from that pixel.
    /// True when it took the pointer.
    pub(crate) fn scene_light_pick(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        p: &crate::scene_guides::Projector,
        response: &egui::Response,
    ) -> bool {
        if !ctx.data(|d| d.get_temp::<bool>(pick_id()).unwrap_or(false)) {
            return false;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.data_mut(|d| d.insert_temp(pick_id(), false));
            return true;
        }
        let Some(at) = response.hover_pos() else {
            return true;
        };
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        let colour = egui::Color32::from_rgb(255, 220, 120);
        painter.circle_stroke(at, 10.0, egui::Stroke::new(1.5_f32, colour));
        painter.text(
            at + egui::vec2(14.0, 14.0),
            egui::Align2::LEFT_TOP,
            "Click where the light is (Esc to stop)",
            egui::FontId::proportional(12.0),
            colour,
        );
        if response.clicked() {
            ctx.data_mut(|d| d.insert_temp(pick_id(), false));
            if let Err(e) = self.start_shot_match(ctx, Some(p.to_pixel(at))) {
                self.fail(e);
            }
        }
        true
    }
    fn start_shot_match(
        &mut self,
        ctx: &egui::Context,
        picked: Option<[f32; 2]>,
    ) -> Result<(), String> {
        let doc = self.project.document.snapshot();
        let time = self.project.time().map_err(|e| e.to_string())?;
        let render = self.viewed_scene().ok_or("No 3D scene in view")?;
        let root = *doc.nodes[&render]
            .inputs
            .get("scene")
            .ok_or("Render scene needs a scene")?;
        let scene = veylvik_scene::evaluate(doc, root, time).map_err(|e| e.to_string())?;
        let shot = self
            .shot_behind(render, scene.environment.and_then(|e| e.image))
            .ok_or("No footage found behind the 3D scene: merge the render over footage, or give the scene an Environment image")?;
        let plan = veylvik_render::Plan::compile_output(doc, shot).map_err(|e| e.to_string())?;
        let (renderer, inputs, (width, height)) = (
            self.renderer.clone(),
            self.project.inputs.clone(),
            (doc.width, doc.height),
        );
        let result = Arc::new(Mutex::new(None));
        let (slot, repaint) = (result.clone(), ctx.clone());
        std::thread::spawn(move || {
            let found = renderer
                .render_frame(&plan, veylvik_render::RenderRequest::full(time), &inputs)
                .map_err(|e| e.to_string())
                .and_then(|frame| {
                    let source = picked
                        .or_else(|| veylvik_scene::source_in_frame(&frame.pixels, width, height));
                    ShotLight::analyse(&frame.pixels, width, height)
                        .map(|shot| (shot, source, width, height))
                        .ok_or_else(|| {
                            "The shot is black at this frame; move the playhead and try again."
                                .to_owned()
                        })
                });
            if let Ok(mut slot) = slot.lock() {
                *slot = Some(found);
            }
            repaint.request_repaint();
        });
        ctx.data_mut(|d| d.insert_temp(id(), Job { result, time }));
        self.status = "Matching the light to the shot…".into();
        self.error = false;
        Ok(())
    }
    fn apply_shot(
        &mut self,
        (shot, source, width, height): (ShotLight, Option<[f32; 2]>, u32, u32),
        time: Time,
    ) -> Result<(), String> {
        let doc = self.project.document.snapshot();
        let render = self.viewed_scene().ok_or("No 3D scene in view")?;
        let root = *doc.nodes[&render]
            .inputs
            .get("scene")
            .ok_or("Render scene needs a scene")?;
        let scene = veylvik_scene::evaluate(doc, root, time).map_err(|e| e.to_string())?;
        let (camera, _) = self.scene_camera().ok_or("No camera films the scene")?;
        let matched = veylvik_scene::match_shot(&shot, source, &camera, width, height);
        let selected = doc
            .nodes
            .get(&self.project.selected)
            .filter(|n| matches!(n.operation, Operation::Light3d(_)));
        let existing = selected
            .map(|n| n.id)
            .or_else(|| scene.lights.first().map(|l| l.node));
        let mut commands = Vec::new();
        let mut light = match existing.and_then(|id| doc.nodes.get(&id)) {
            Some(node) => node.clone(),
            None => {
                let operation: Operation =
                    serde_json::from_value(nodes::LIGHT_3D.default_operation())
                        .map_err(|e| e.to_string())?;
                let light = Node::new("Shot light", operation, &[]);
                let mut host = doc
                    .nodes
                    .get(&root)
                    .filter(|n| matches!(n.operation, Operation::MergeScenes(_)))
                    .cloned()
                    .ok_or("Add a Merge scenes node so the light has a place in the scene")?;
                let port = nodes::MERGE_PORTS
                    .iter()
                    .find(|p| !host.inputs.contains_key(**p))
                    .ok_or("The scene is full (eight inputs)")?;
                host.inputs.insert((*port).into(), light.id);
                commands.push(Command::Put {
                    node: Box::new(host),
                });
                light
            }
        };
        let Operation::Light3d(l) = &mut light.operation else {
            return Err("Not a light".into());
        };
        l.kind = nodes::LightKind::Sun;
        l.shadows = true;
        l.placement
            .rotation
            .set_at(time, matched.rotation)
            .map_err(|e| e.to_string())?;
        l.colour
            .set_at(time, matched.colour)
            .map_err(|e| e.to_string())?;
        veylvik_document::track::set_scalar_at(&mut l.intensity, time, matched.intensity)
            .map_err(|e| e.to_string())?;
        commands.insert(
            0,
            Command::Put {
                node: Box::new(light),
            },
        );
        if let Some(mut environment) = scene
            .environment
            .and_then(|e| doc.nodes.get(&e.node))
            .cloned()
            && let Operation::Environment3d(e) = &mut environment.operation
        {
            veylvik_document::track::set_scalar_at(&mut e.intensity, time, matched.environment)
                .map_err(|e| e.to_string())?;
            commands.push(Command::Put {
                node: Box::new(environment),
            });
        }
        self.project.apply(&commands).map_err(|e| e.to_string())?;
        self.status = match (matched.directional, matched.backlight) {
            (true, true) => "Sun placed beyond the subject, where the light in the frame is".into(),
            (true, false) => "Sun placed where the shot's light comes from, with its colour".into(),
            (false, _) => "The shot's light is soft and even: a soft sun from above".into(),
        };
        self.error = false;
        self.refresh_draft();
        self.request_preview();
        Ok(())
    }
}
