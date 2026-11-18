//! Solve camera: follow features through a footage node, solve its real camera in the background and add the scene nodes that put 3D into the shot, with a plain verdict on how well it solved.
use crate::{Studio, files::Completed, sequence::Progress};
use eframe::egui;
use std::sync::{Arc, atomic::Ordering};
use veylvik_document::{NodeId, Operation};
use veylvik_render::Plan;
use veylvik_track::camera::{Outcome, Request};

#[derive(Default)]
pub struct Controls {
    pub open: bool,
    source: Option<NodeId>,
    start: i64,
    end: i64,
    /// Use the focal length below instead of finding it.
    lens_known: bool,
    focal: f32,
    sensor: f32,
    pub active: Option<Arc<Progress>>,
    /// The last solve's report, once its nodes are added.
    report: Option<serde_json::Value>,
}
/// A finished analysis, to turn into nodes on the UI thread.
pub struct Solved {
    session: NodeId,
    revision: u64,
    request: Request,
    outcome: Outcome,
}
/// A plain verdict on a solve's average error (pixels).
fn verdict(rms: f64) -> (&'static str, egui::Color32) {
    match rms {
        r if r < 0.3 => ("Excellent", egui::Color32::from_rgb(110, 210, 120)),
        r if r < 0.6 => ("Good", egui::Color32::from_rgb(150, 210, 110)),
        r if r < 1.0 => ("Usable", egui::Color32::from_rgb(230, 200, 90)),
        _ => ("Rough: 3D may slide", egui::Color32::from_rgb(235, 120, 90)),
    }
}
impl Controls {
    /// The window's settings for a request made over live control.
    pub(crate) fn configure_live(&mut self, request: &Request) {
        *self = Self {
            open: true,
            source: Some(request.source),
            start: request.frames[0],
            end: request.frames[1],
            lens_known: request.focal_length.is_some(),
            focal: request.focal_length.unwrap_or(35.0),
            sensor: request.sensor_width,
            ..Default::default()
        };
    }
}
impl Studio {
    pub(crate) fn begin_solve_camera(&mut self) {
        if !self.apply_draft() || self.files.busy() {
            return;
        }
        self.playing = false;
        let doc = self.project.document.snapshot();
        self.solve = Controls {
            open: true,
            source: Some(self.project.selected),
            start: 0,
            end: i64::from(doc.frame_count.saturating_sub(1)),
            focal: 35.0,
            sensor: 36.0,
            ..Default::default()
        };
        self.project.preview = self.solve.source;
        self.request_preview();
    }
    pub(crate) fn solve_camera_window(&mut self, ctx: &egui::Context) {
        if !self.solve.open {
            return;
        }
        let mut open = true;
        let mut run = false;
        let mut cancel = false;
        let mut view = None;
        let mut test_cube = false;
        let last = i64::from(
            self.project
                .document
                .snapshot()
                .frame_count
                .saturating_sub(1),
        );
        let source_label = self
            .solve
            .source
            .and_then(|id| self.project.document.snapshot().nodes.get(&id))
            .map_or("nothing".to_string(), |n| n.label.clone());
        egui::Window::new("Solve camera")
            .default_width(340.)
            .collapsible(false)
            .open(&mut open)

            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.label(format!("Footage: {source_label}"));
                ui.weak("Finds the real camera of the shot, so 3D objects stay put in it. Works best with a moving camera and a textured, still background.");
                ui.horizontal(|ui| {
                    ui.label("Frames");
                    ui.add(egui::DragValue::new(&mut self.solve.start).range(0..=last));
                    ui.label("to");
                    ui.add(egui::DragValue::new(&mut self.solve.end).range(0..=last));
                });
                ui.checkbox(&mut self.solve.lens_known, "I know the lens")
                    .on_hover_text("Otherwise the focal length is measured from the shot");
                if self.solve.lens_known {
                    ui.horizontal(|ui| {
                        ui.label("Focal length");
                        ui.add(egui::DragValue::new(&mut self.solve.focal).range(1.0..=2000.0).suffix(" mm"));
                        ui.label("sensor");
                        ui.add(egui::DragValue::new(&mut self.solve.sensor).range(1.0..=100.0).suffix(" mm"));
                    });
                }
                ui.separator();
                if let Some(progress) = &self.solve.active {
                    let (done, total) = (progress.completed.load(Ordering::Relaxed), progress.total.load(Ordering::Relaxed).max(1));
                    let text = if done >= total { "Solving camera".to_string() } else { format!("Following features: frame {done} of {total}") };
                    ui.add(egui::ProgressBar::new(done as f32 / total as f32).text(text));
                    cancel = ui.button("Cancel").clicked();
                } else if ui
                    .add_enabled(self.solve.source.is_some() && self.solve.end > self.solve.start + 1 && !self.files.busy(), egui::Button::new("Solve camera"))
                    .clicked()
                {
                    run = true;
                }
                if let Some(report) = &self.solve.report {
                    let rms = report["rms_pixels"].as_f64().unwrap_or(f64::INFINITY);
                    let (word, colour) = verdict(rms);
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.colored_label(colour, egui::RichText::new(word).strong());
                        ui.label(format!("{rms:.2} px average error"));
                    });
                    ui.label(format!(
                        "{} of {} frames, {} tracks, {} points, lens {:.1} mm",
                        report["solved_frames"], report["frames"], report["tracks"], report["points"],
                        report["focal_length"].as_f64().unwrap_or(0.0)
                    ));
                    if report["tripod"].as_bool() == Some(true) {
                        ui.weak("The camera only turned (a tripod shot): it stays in place and turns.");
                    }
                    if report["lens_measured"].as_bool() == Some(false) && report["tripod"].as_bool() == Some(true) {
                        ui.weak("It barely turned, so the lens could not be measured: tick I know the lens if 3D looks the wrong size.");
                    }
                    if let Some(k1) = report["lens_distortion"]["k1"].as_f64() {
                        ui.weak(format!(
                            "The lens bends straight lines ({}, k1 {k1:.3}): the 3D is bent the same way by the Footage lens node.",
                            if k1 < 0.0 { "barrel" } else { "pincushion" }
                        ));
                    }
                    if report["ground"].is_null() {
                        ui.weak("No ground was found; add a shadow catcher plane yourself if objects need shadows.");
                    } else {
                        ui.weak("A ground that catches shadows was found and levelled.");
                    }
                    ui.horizontal(|ui| {
                        if ui.button("View 3D over footage").clicked() {
                            view = report["composite"].as_str().and_then(|s| s.parse().ok());
                        }
                        if ui.button("Add a test cube").on_hover_text("A cube on the ground, to check that 3D stays put").clicked() {
                            test_cube = true;
                        }
                    });
                }
            });
        if cancel && let Some(progress) = &self.solve.active {
            progress.cancel.store(true, Ordering::Relaxed);
        }
        if run {
            self.run_solve_camera(ctx);
        }
        if let Some(node) = view {
            self.project.preview = Some(node);
            self.request_preview();
        }
        if test_cube && let Err(e) = self.add_test_cube() {
            self.fail(e);
        }
        if !open && self.solve.active.is_none() {
            self.solve.open = false;
        }
    }
    pub(crate) fn run_solve_camera(&mut self, ctx: &egui::Context) {
        let Some(source) = self.solve.source else {
            return;
        };
        let doc = self.project.document.snapshot();
        let mut args =
            serde_json::json!({"source": source, "start": self.solve.start, "end": self.solve.end});
        if self.solve.lens_known {
            args["focal_length"] = serde_json::json!(self.solve.focal);
            args["sensor_width"] = serde_json::json!(self.solve.sensor);
        }
        let request = match Request::from_json(&args, doc.frame_count) {
            Ok(r) => r,
            Err(e) => return self.fail(e),
        };
        let plan = match Plan::compile_output(doc, source) {
            Ok(p) => p,
            Err(e) => return self.fail(e),
        };
        let (session, revision, rate) = (
            self.project.session,
            self.project.document.revision(),
            doc.frame_rate,
        );
        let renderer = self.renderer.clone();
        let inputs = self.project.inputs.clone();
        let progress = Arc::new(Progress::default());
        progress.total.store(
            (request.frames[1] - request.frames[0] + 1) as u32,
            Ordering::Relaxed,
        );
        let worker = progress.clone();
        let ctx = ctx.clone();
        self.solve.report = None;
        let work = self.files.start(move || {
            let mut report = |done: usize, _total: usize| {
                worker.completed.store(done as u32, Ordering::Relaxed);
                ctx.request_repaint();
                !worker.cancel.load(Ordering::Relaxed)
            };
            veylvik_track::camera::analyse(&renderer, &plan, &inputs, &request, rate, &mut report)
                .map(|outcome| {
                    Completed::CameraSolved(Box::new(Solved {
                        session,
                        revision,
                        request,
                        outcome,
                    }))
                })
                .map_err(|e| e.to_string())
        });
        if work.is_ok() {
            self.solve.active = Some(progress);
        }
        self.file_started(work);
    }
    pub(crate) fn camera_solved(&mut self, solved: Solved) {
        self.solve.active = None;
        if solved.session != self.project.session
            || solved.revision != self.project.document.revision()
        {
            return self.fail("The project changed while solving; solve again.");
        }
        let built = match veylvik_track::camera::scene(
            self.project.document.snapshot(),
            &solved.request,
            &solved.outcome,
        ) {
            Ok(b) => b,
            Err(e) => return self.fail(e),
        };
        if let Err(e) = self.project.apply(&built.commands) {
            return self.fail(e);
        }
        let rms = built.report["rms_pixels"].as_f64().unwrap_or(f64::INFINITY);
        self.status = format!("Camera solved: {} ({rms:.2} px)", verdict(rms).0);
        self.error = false;
        self.project.preview = built.report["composite"]
            .as_str()
            .and_then(|s| s.parse().ok());
        self.finish_live_job("solve_camera", Ok(built.report.clone()));
        self.solve.report = Some(built.report);
        self.solve.open = true;
        self.refresh_draft();
        self.request_preview();
    }
    /// A cube standing on the solved ground (or at the frame centre), merged into the
    /// solved scene, with a sun so it casts a shadow on the ground.
    fn add_test_cube(&mut self) -> Result<(), String> {
        use veylvik_document::{Command, Node, nodes};
        let report = self.solve.report.clone().ok_or("Solve the camera first")?;
        let doc = self.project.document.snapshot();
        let scene_id: NodeId = report["scene"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .ok_or("The solved scene is missing")?;
        let mut scene = doc
            .nodes
            .get(&scene_id)
            .ok_or("The solved scene is missing")?
            .clone();
        let ground = report["ground"]["node"]
            .as_str()
            .and_then(|s| s.parse::<NodeId>().ok())
            .and_then(|id| doc.nodes.get(&id));
        let size = doc.width as f32 / 8.0;
        let at = match ground.map(|g| &g.operation) {
            Some(Operation::Shape3d(g)) => {
                let p = g
                    .placement
                    .position
                    .at(self.project.time().map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                [p[0], p[1] - size * 0.5, p[2]]
            }
            _ => [0.0; 3],
        };
        let default = |kind: &veylvik_document::registry::NodeType| -> Result<Operation, String> {
            serde_json::from_value(kind.default_operation()).map_err(|e| e.to_string())
        };
        let Operation::Shape3d(mut cube) = default(&nodes::SHAPE_3D)? else {
            return Err("shape".into());
        };
        cube.size_x = size.into();
        cube.size_y = size.into();
        cube.size_z = size.into();
        cube.colour = [0.85, 0.3, 0.2].into();
        cube.placement.position = at.into();
        cube.placement.rotation = [0.0, 30.0, 0.0].into();
        let Operation::Light3d(mut sun) = default(&nodes::LIGHT_3D)? else {
            return Err("light".into());
        };
        sun.kind = nodes::LightKind::Sun;
        sun.intensity = 3.0.into();
        sun.placement.rotation = [55.0, 30.0, 0.0].into();
        let cube = Node::new("Test cube", Operation::Shape3d(cube), &[]);
        let sun = Node::new("Test sun", Operation::Light3d(sun), &[]);
        let free = |n: usize| {
            if n == 1 {
                "scene".to_string()
            } else {
                format!("scene_{n}")
            }
        };
        let mut n = 1;
        for node in [cube.id, sun.id] {
            while scene.inputs.contains_key(&free(n)) {
                n += 1;
            }
            scene.inputs.insert(free(n), node);
        }
        self.project
            .apply(&[
                Command::Put {
                    node: Box::new(cube),
                },
                Command::Put {
                    node: Box::new(sun),
                },
                Command::Put {
                    node: Box::new(scene),
                },
            ])
            .map_err(|e| e.to_string())?;
        self.refresh_draft();
        self.request_preview();
        Ok(())
    }
}
