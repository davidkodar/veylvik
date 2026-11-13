//! Subject selection: clicks in the viewer prompt a local model in a worker process for an
//! instant matte preview; tracking runs once on the file worker and publishes one undo step.
use crate::{Studio, files::Completed, sequence::Progress, viewer::ViewTransform};
use eframe::egui;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, atomic::Ordering, mpsc},
};
use veylvik_document::{Command, Node, NodeId, Operation, Time};
use veylvik_render::{Plan, RenderRequest};
use veylvik_segment::{Point, analysis, matte, models, worker::Worker};

/// The model used for clicks and tracking.
const MODEL: &str = "edgetam";
/// The model that refines the edge band.
const MATTING: &str = "vitmatte";
/// Longest side of the preview overlay, in pixels.
const PREVIEW_SIDE: usize = 960;
type Shared = Arc<Mutex<Option<Worker>>>;

struct Preview {
    frame: i64,
    texture: egui::TextureHandle,
    present: bool,
    milliseconds: f64,
}
struct Rendered {
    frame: i64,
    image: egui::ColorImage,
    present: bool,
    milliseconds: f64,
}
/// A finished analysis, published by `subject_completed`.
pub struct Segmented {
    session: NodeId,
    revision: u64,
    source: NodeId,
    manifest: PathBuf,
    media: veylvik_io::footage::Media,
    outcome: analysis::Outcome,
}
/// Nodes made by the last analysis, for the one-click results.
#[derive(Clone, Copy)]
struct Done {
    source: NodeId,
    cutout: NodeId,
}
#[derive(Default)]
pub struct Controls {
    pub open: bool,
    pub source: Option<NodeId>,
    pub active: Option<Arc<Progress>>,
    points: Vec<Point>,
    /// Composition frame the points were placed on.
    frame: i64,
    end: i64,
    backward: bool,
    /// Second, zoomed pass for small parts.
    detail: bool,
    /// Edge-band matting for hair and soft edges, when the model is ready.
    matting: bool,
    show_mask: bool,
    worker: Shared,
    preview: Option<Preview>,
    pending: Option<mpsc::Receiver<Result<Rendered, String>>>,
    /// Points changed while a preview was running; preview again when it finishes.
    again: bool,
    message: Option<(String, bool)>,
    done: Option<Done>,
}
impl Controls {
    /// Places points on left click (subject) and right or Alt click (exclude), and draws the
    /// preview matte and the points. Returns true when the points changed.
    pub fn overlay(
        &mut self,
        ui: &egui::Ui,
        canvas: egui::Rect,
        view: ViewTransform,
        frame: i64,
    ) -> bool {
        let [ox, oy, sx, sy] = view.0;
        let origin = canvas.min + egui::vec2(canvas.width() * ox, canvas.height() * oy);
        let scale = egui::vec2(canvas.width() * sx, canvas.height() * sy);
        let painter = ui.painter().with_clip_rect(canvas);
        if self.show_mask
            && let Some(preview) = self.preview.as_ref().filter(|p| p.frame == frame)
        {
            painter.image(
                preview.texture.id(),
                egui::Rect::from_min_size(origin, scale),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        if frame == self.frame {
            for point in &self.points {
                let p = origin + egui::vec2(point.x * scale.x, point.y * scale.y);
                let colour = if point.positive {
                    egui::Color32::from_rgb(90, 220, 140)
                } else {
                    egui::Color32::from_rgb(240, 90, 90)
                };
                painter.circle_filled(p, 6.0, egui::Color32::from_black_alpha(160));
                painter.circle_stroke(p, 6.0, egui::Stroke::new(2.0_f32, colour));
                let bar = egui::vec2(3.5, 0.0);
                painter.line_segment([p - bar, p + bar], egui::Stroke::new(2.0_f32, colour));
                if point.positive {
                    let bar = egui::vec2(0.0, 3.5);
                    painter.line_segment([p - bar, p + bar], egui::Stroke::new(2.0_f32, colour));
                }
            }
        }
        if self.active.is_some() {
            return false;
        }
        let (pointer, primary, secondary, alt) = ui.input(|i| {
            (
                i.pointer.hover_pos(),
                i.pointer.button_clicked(egui::PointerButton::Primary),
                i.pointer.button_clicked(egui::PointerButton::Secondary),
                i.modifiers.alt,
            )
        });
        let Some(p) = pointer.filter(|p| canvas.contains(*p)) else {
            return false;
        };
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        if !(primary || secondary) {
            return false;
        }
        let n = (p - origin) / scale;
        if !(0.0..=1.0).contains(&n.x) || !(0.0..=1.0).contains(&n.y) {
            return false;
        }
        // Points belong to one frame; clicking on another frame starts a new selection.
        if frame != self.frame || self.points.len() >= veylvik_segment::MAX_POINTS {
            self.points.clear();
            self.frame = frame;
        }
        self.points.push(Point {
            x: n.x,
            y: n.y,
            positive: primary && !alt,
        });
        self.done = None;
        true
    }
}
fn overlay_image(alpha: &[f32], width: usize, height: usize) -> egui::ColorImage {
    let pixels = alpha
        .iter()
        .map(|a| {
            let a = (a.clamp(0.0, 1.0) * 140.0) as u8;
            egui::Color32::from_rgba_unmultiplied(70, 190, 255, a)
        })
        .collect();
    egui::ColorImage::new([width, height], pixels)
}
/// The `veylvik` executable that hosts the worker, next to this editor.
fn worker_exe() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name("veylvik");
    if exe.exists() {
        Ok(exe)
    } else {
        Err(format!(
            "{} is missing; build or install the veylvik command",
            exe.display()
        ))
    }
}
fn ensure(worker: &mut Option<Worker>) -> Result<&mut Worker, String> {
    if worker.is_none() {
        *worker = Some(Worker::spawn(&worker_exe()?, MODEL).map_err(|e| e.to_string())?);
    }
    worker
        .as_mut()
        .ok_or_else(|| "segmentation worker unavailable".into())
}

impl Studio {
    pub(super) fn begin_subject(&mut self) {
        if !self.apply_draft() {
            return;
        }
        self.playing = false;
        let doc = self.project.document.snapshot();
        self.subject.source = Some(self.project.selected);
        self.subject.open = true;
        self.subject.show_mask = true;
        self.subject.detail = true;
        self.subject.matting = true;
        self.models.status(MATTING);
        self.subject.points.clear();
        self.subject.preview = None;
        self.subject.done = None;
        self.subject.message = None;
        self.subject.frame = self.project.frame;
        self.subject.end = i64::from(doc.frame_count) - 1;
        self.project.preview = self.subject.source;
        self.models.status(MODEL);
        self.request_preview();
    }
    /// Releases the worker and its graphics memory. Never waits on the UI thread: a running job
    /// holds its own handle and the worker stops when that job finishes.
    fn close_subject(&mut self) {
        self.subject.open = false;
        self.subject.pending = None;
        self.subject.worker = Shared::default();
    }
    fn subject_ready(&mut self) -> bool {
        self.model_ready(MODEL)
    }
    fn model_ready(&mut self, id: &str) -> bool {
        self.models
            .status(id)
            .is_some_and(|s| s.installed && s.enabled)
    }
    /// Renders the prompted frame and asks the worker for its matte, off the UI thread.
    pub(super) fn preview_subject(&mut self, ctx: &egui::Context) {
        if self.subject.pending.is_some() {
            self.subject.again = true;
            return;
        }
        let Some(source) = self.subject.source else {
            return;
        };
        if self.subject.points.is_empty() || !self.subject_ready() {
            self.subject.preview = None;
            return;
        }
        if !self.subject.points.iter().any(|p| p.positive) {
            self.subject.message = Some((
                "Left-click the subject first; right-click excludes.".into(),
                false,
            ));
            return;
        }
        let doc = self.project.document.snapshot();
        let plan = match Plan::compile_output(doc, source) {
            Ok(plan) => plan,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        let (width, height, rate) = (doc.width as usize, doc.height as usize, doc.frame_rate);
        let (frame, points) = (self.subject.frame, self.subject.points.clone());
        let (renderer, inputs, worker) = (
            self.renderer.clone(),
            self.project.inputs.clone(),
            self.subject.worker.clone(),
        );
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("veylvik-subject".into())
            .spawn(move || {
                let run = || -> Result<Rendered, String> {
                    let started = std::time::Instant::now();
                    let time = Time::at_frame(frame, rate).map_err(|e| e.to_string())?;
                    let rendered = renderer
                        .render_frame(&plan, RenderRequest::full(time), &inputs)
                        .map_err(|e| e.to_string())?;
                    let input = matte::model_frame(&rendered.pixels, width, height)
                        .map_err(|e| e.to_string())?;
                    let mut guard = worker.lock().map_err(|_| "worker lock poisoned")?;
                    let mask = ensure(&mut guard)?
                        .frame(&input, Some(&points))
                        .map_err(|e| e.to_string())?;
                    drop(guard);
                    let scale = PREVIEW_SIDE as f64 / width.max(height) as f64;
                    let (w, h) = (
                        ((width as f64 * scale).round() as usize).max(1),
                        ((height as f64 * scale).round() as usize).max(1),
                    );
                    let alpha = if mask.present() {
                        matte::alpha(&mask.logits, w, h).map_err(|e| e.to_string())?
                    } else {
                        vec![0.0; w * h]
                    };
                    Ok(Rendered {
                        frame,
                        image: overlay_image(&alpha, w, h),
                        present: mask.present(),
                        milliseconds: started.elapsed().as_secs_f64() * 1000.0,
                    })
                };
                let _ = sender.send(run());
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) => self.subject.pending = Some(receiver),
            Err(e) => self.fail(e),
        }
    }
    pub(super) fn poll_subject(&mut self, ctx: &egui::Context) {
        let Some(receiver) = &self.subject.pending else {
            return;
        };
        let Ok(result) = receiver.try_recv() else {
            return;
        };
        self.subject.pending = None;
        match result {
            Ok(rendered) => {
                self.subject.message = (!rendered.present).then(|| {
                    (
                        "No subject found there; click on the subject itself.".into(),
                        false,
                    )
                });
                self.subject.preview = Some(Preview {
                    frame: rendered.frame,
                    texture: ctx.load_texture(
                        "subject-preview",
                        rendered.image,
                        egui::TextureOptions::LINEAR,
                    ),
                    present: rendered.present,
                    milliseconds: rendered.milliseconds,
                });
            }
            Err(e) => self.subject.message = Some((e, true)),
        }
        if std::mem::take(&mut self.subject.again) {
            self.preview_subject(ctx);
        }
    }
    fn track_subject(&mut self, ctx: &egui::Context) {
        if !self.apply_draft() || self.files.busy() {
            return;
        }
        let Some(source) = self.subject.source else {
            return;
        };
        let use_matting = self.subject.matting && self.model_ready(MATTING);
        let doc = self.project.document.snapshot();
        let plan = match Plan::compile_output(doc, source) {
            Ok(plan) => plan,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        let request = analysis::Request {
            points: self.subject.points.clone(),
            frames: [self.subject.frame, self.subject.end],
            back_to: (self.subject.backward
                && self.subject.frame > 0
                && self.subject.end >= self.subject.frame)
                .then_some(0),
            detail: self.subject.detail,
            matting: use_matting.then_some(MATTING),
        };
        let total = match analysis::schedule(&request, doc.frame_count) {
            Ok(schedule) => schedule.len() * if request.detail { 2 } else { 1 },
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        // Mattes go beside a saved project, otherwise into Veylvik's data folder.
        let parent = match &self.project.path {
            Some(path) => path.parent().map(PathBuf::from),
            None => models::root()
                .ok()
                .and_then(|r| r.parent().map(PathBuf::from)),
        };
        let Some(parent) = parent else {
            self.fail("No folder is available for the subject matte");
            return;
        };
        let dir = analysis::unique_dir(&parent, source);
        let (session, revision) = (self.project.session, self.project.document.revision());
        let doc = doc.clone();
        let (renderer, inputs, worker) = (
            self.renderer.clone(),
            self.project.inputs.clone(),
            self.subject.worker.clone(),
        );
        let progress = Arc::new(Progress::default());
        progress.total.store(total as u32, Ordering::Relaxed);
        let observer = progress.clone();
        let ctx = ctx.clone();
        let work = self.files.start(move || {
            let mut guard = worker.lock().map_err(|_| "worker lock poisoned")?;
            let worker = ensure(&mut guard)?;
            let outcome = analysis::analyse(
                &renderer,
                &plan,
                &inputs,
                &doc,
                &request,
                worker,
                &dir,
                &mut |done, _| {
                    observer.completed.store(done as u32, Ordering::Relaxed);
                    ctx.request_repaint();
                    !observer.cancel.load(Ordering::Relaxed)
                },
            )
            .map_err(|e| e.to_string())?;
            drop(guard);
            let media = veylvik_io::footage::load(&outcome.manifest, 128 * 1024 * 1024)
                .map_err(|e| e.to_string())?;
            Ok(Completed::Segmented(Box::new(Segmented {
                session,
                revision,
                source,
                manifest: outcome.manifest.clone(),
                media,
                outcome,
            })))
        });
        if work.is_ok() {
            self.subject.active = Some(progress);
        }
        self.file_started(work);
    }
    pub(super) fn subject_completed(&mut self, result: Segmented) {
        self.subject.active = None;
        if result.session != self.project.session
            || result.revision != self.project.document.revision()
        {
            self.fail("Composition changed during subject tracking; track again.");
            return;
        }
        let Some(asset) = result.manifest.to_str().map(str::to_owned) else {
            self.fail("Matte path is not UTF-8");
            return;
        };
        let added = match analysis::matte_nodes(
            self.project.document.snapshot(),
            result.source,
            asset.clone(),
            result.outcome.first_frame,
        ) {
            Ok(added) => added,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        if let Err(e) = self.project.apply(&added.commands) {
            self.fail(e);
            return;
        }
        result.media.insert(&mut self.project.inputs, asset);
        let o = &result.outcome;
        self.subject.done = Some(Done {
            source: result.source,
            cutout: added.cutout,
        });
        self.subject.message = Some((
            format!(
                "{}Tracked frames {}–{} ({} frames, {:.0} ms each){}{}{}.",
                if o.cancelled { "Stopped early. " } else { "" },
                o.first_frame,
                o.last_frame,
                o.frames,
                o.milliseconds_per_frame,
                if o.absent > 0 {
                    format!("; subject absent on {} frames", o.absent)
                } else {
                    String::new()
                },
                o.zoom
                    .map(|z| format!("; detail pass at {z:.1}× zoom"))
                    .unwrap_or_default(),
                if o.matting.is_some() {
                    format!(
                        "; hair and soft edges matted ({:.0} ms a frame)",
                        o.matting_milliseconds
                    )
                } else {
                    String::new()
                }
            ),
            false,
        ));
        self.status = "Subject tracked: Subject matte and Subject cutout added".into();
        self.error = false;
        self.project.selected = added.cutout;
        self.project.preview = Some(added.cutout);
        self.subject.show_mask = false;
        self.refresh_draft();
        self.request_preview();
    }
    /// One-click uses of the cutout, each a single undo step.
    fn subject_result(&mut self, kind: usize) {
        let Some(done) = self.subject.done else {
            return;
        };
        let doc = self.project.document.snapshot();
        if !doc.nodes.contains_key(&done.cutout) || !doc.nodes.contains_key(&done.source) {
            self.fail("The subject cutout no longer exists; track again.");
            return;
        }
        let mut commands = Vec::new();
        let (output, selected) = match kind {
            // Keep the subject: everything else transparent.
            0 => (done.cutout, done.cutout),
            // Replace the background with a colour that can be edited or rewired.
            1 => {
                let backdrop = Node::new(
                    "New background",
                    Operation::Solid(veylvik_document::nodes::Solid {
                        color: [0.03, 0.18, 0.08, 1.0],
                    }),
                    &[],
                );
                let merge = Node::new(
                    "Subject over new background",
                    Operation::Merge(veylvik_document::nodes::Merge {}),
                    &[("background", backdrop.id), ("foreground", done.cutout)],
                );
                let ids = (merge.id, backdrop.id);
                commands.push(Command::Put {
                    node: Box::new(backdrop),
                });
                commands.push(Command::Put {
                    node: Box::new(merge),
                });
                ids
            }
            // Layers behind the subject: a starter title between the shot and the subject.
            _ => {
                let mut text = crate::node_palette::operation(
                    &veylvik_document::registry::TEXT,
                    doc.width,
                    doc.height,
                );
                // Sized from the shorter side so it fits portrait frames too, and set at chest
                // height where the subject covers part of it.
                if let Operation::Text(veylvik_document::nodes::Text { y, size, .. }) = &mut text {
                    *size = (doc.width.min(doc.height) as f32 * 0.16).into();
                    *y = (doc.height as f32 * 0.42).into();
                }
                let title = Node::new("Behind the subject", text, &[]);
                let behind = Node::new(
                    "Layer behind subject",
                    Operation::Merge(veylvik_document::nodes::Merge {}),
                    &[("background", done.source), ("foreground", title.id)],
                );
                let front = Node::new(
                    "Subject in front",
                    Operation::Merge(veylvik_document::nodes::Merge {}),
                    &[("background", behind.id), ("foreground", done.cutout)],
                );
                let ids = (front.id, title.id);
                for node in [title, behind, front] {
                    commands.push(Command::Put {
                        node: Box::new(node),
                    });
                }
                ids
            }
        };
        commands.push(Command::Output { id: output });
        match self.project.apply(&commands) {
            Ok(()) => {
                self.project.selected = selected;
                self.project.preview = None;
                self.refresh_draft();
                self.request_preview();
                self.status = match kind {
                    0 => "The output is now the subject alone",
                    1 => "Background replaced; edit New background or connect an image",
                    _ => "Added a title behind the subject; edit it or connect any layer",
                }
                .into();
            }
            Err(e) => self.fail(e),
        }
    }
    pub(super) fn subject_window(&mut self, ctx: &egui::Context) {
        self.poll_subject(ctx);
        if !self.subject.open {
            return;
        }
        let ready = self.subject_ready();
        let status = self.models.status(MODEL);
        let installing = self.models.busy(MODEL);
        let mut install = false;
        let matting_ready = self.model_ready(MATTING);
        let matting_installing = self.models.busy(MATTING);
        let mut install_matting = false;
        let mut open = true;
        let (mut track, mut cancel, mut result, mut changed, mut models) =
            (false, false, None, false, false);
        let doc = self.project.document.snapshot();
        let source_label = self
            .subject
            .source
            .and_then(|id| doc.nodes.get(&id))
            .map_or("—".to_owned(), |n| n.label.clone());
        let last = i64::from(doc.frame_count) - 1;
        egui::Window::new("Select subject")
            .open(&mut open)
            .default_width(340.0)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(format!("Source: {source_label}"));
                if !ready {
                    ui.add_space(4.0);
                    if status.is_some_and(|s| s.installed) {
                        ui.label("EdgeTAM is switched off. Switch it on in AI models to select subjects.");
                    } else {
                        ui.label("Subject selection uses the EdgeTAM model. It runs on this computer; nothing is uploaded.");
                        if installing {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Installing… (progress in AI models)");
                            });
                        } else if ui
                            .button("Install EdgeTAM (65 MB)")
                            .on_hover_text("Download from Hugging Face and verify every file (Apache-2.0)")
                            .clicked()
                        {
                            install = true;
                        }
                    }
                    if ui.button("AI models…").clicked() {
                        models = true;
                    }
                    return;
                }
                ui.weak("Click the subject in the viewer. Right-click or Alt-click excludes an area. A few clicks spread over the subject (head, body, legs) select a whole person reliably.");
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{} point{} on frame {}",
                        self.subject.points.len(),
                        if self.subject.points.len() == 1 { "" } else { "s" },
                        self.subject.frame
                    ));
                    if ui.add_enabled(!self.subject.points.is_empty(), egui::Button::new("Undo point")).clicked() {
                        self.subject.points.pop();
                        changed = true;
                    }
                    if ui.add_enabled(!self.subject.points.is_empty(), egui::Button::new("Clear")).clicked() {
                        self.subject.points.clear();
                        changed = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.subject.show_mask, "Show mask");
                    if self.subject.pending.is_some() {
                        ui.spinner();
                    } else if let Some(p) = &self.subject.preview {
                        ui.weak(if p.present {
                            format!("Preview in {:.0} ms", p.milliseconds)
                        } else {
                            "No subject at these points".into()
                        });
                    }
                });
                ui.separator();
                if let Some(progress) = &self.subject.active {
                    let (done, total) = (
                        progress.completed.load(Ordering::Relaxed),
                        progress.total.load(Ordering::Relaxed).max(1),
                    );
                    let detail = self.subject.detail;
                    let text = if detail && done * 2 < total {
                        format!("Locating the subject: {done} of {} frames", total / 2)
                    } else if detail {
                        format!("Detail pass: {} of {} frames", done - total / 2, total / 2)
                    } else {
                        format!("Tracking {done} of {total} frames")
                    };
                    ui.add(egui::ProgressBar::new(done as f32 / total as f32).text(text));
                    if ui.button("Stop").on_hover_text("Keep the frames tracked so far").clicked() {
                        cancel = true;
                    }
                } else {
                    ui.horizontal(|ui| {
                        ui.label("Track to frame");
                        ui.add(egui::DragValue::new(&mut self.subject.end).range(0..=last));
                    });
                    ui.add_enabled(
                        self.subject.frame > 0,
                        egui::Checkbox::new(&mut self.subject.backward, "Also track back to the start"),
                    );
                    ui.horizontal(|ui| {
                        ui.add_enabled(
                            matting_ready,
                            egui::Checkbox::new(&mut self.subject.matting, "Hair and soft edges"),
                        )
                        .on_hover_text("Refine the band around the outline with the ViTMatte matting model, for hair, fur and motion blur");
                        if !matting_ready && !matting_installing && ui.button("Install ViTMatte (104 MB)").clicked() {
                            install_matting = true;
                        } else if matting_installing {
                            ui.spinner();
                        }
                    });
                    ui.checkbox(&mut self.subject.detail, "Detail pass for small parts")
                        .on_hover_text("Track a second time, zoomed in on the subject, so feet, hands and hair get more model pixels. About twice the analysis time; skipped automatically when the subject already fills the frame.");
                    let can = !self.subject.points.is_empty()
                        && self.subject.points.iter().any(|p| p.positive)
                        && !self.files.busy();
                    if ui
                        .add_enabled(can, egui::Button::new("Track subject"))
                        .on_hover_text("Analyse the shot once and add a Subject matte and a refined Subject cutout")
                        .clicked()
                    {
                        track = true;
                    }
                }
                if self.subject.done.is_some() && self.subject.active.is_none() {
                    ui.separator();
                    ui.label("Use the subject:");
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Keep subject").on_hover_text("Make the cut-out subject the output").clicked() {
                            result = Some(0);
                        }
                        if ui.button("Replace background").on_hover_text("Put the subject over a new background").clicked() {
                            result = Some(1);
                        }
                        if ui.button("Put layers behind").on_hover_text("Add a title between the shot and the subject").clicked() {
                            result = Some(2);
                        }
                    });
                    ui.weak("Fine-tune the edge on Subject cutout: Radius, Detail and Decontaminate.");
                }
                if let Some((message, error)) = &self.subject.message {
                    if *error {
                        ui.colored_label(ui.visuals().error_fg_color, message);
                    } else {
                        ui.weak(message);
                    }
                }
            });
        if models {
            self.models.show_window();
        }
        if install && let Ok(model) = models::find(MODEL) {
            self.models.install(model, ctx);
        }
        if install_matting && let Ok(model) = models::find(MATTING) {
            self.models.install(model, ctx);
        }
        if changed {
            self.subject.done = None;
            self.preview_subject(ctx);
        }
        if cancel && let Some(progress) = &self.subject.active {
            progress.cancel.store(true, Ordering::Relaxed);
        }
        if track {
            self.track_subject(ctx);
        }
        if let Some(kind) = result {
            self.subject_result(kind);
        }
        if !open {
            if let Some(progress) = &self.subject.active {
                progress.cancel.store(true, Ordering::Relaxed);
            }
            self.close_subject();
        }
    }
}
