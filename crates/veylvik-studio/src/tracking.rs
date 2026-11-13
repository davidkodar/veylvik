//! Guided point tracking. Analysis is a cancellable worker job; application is atomic.
use crate::{Studio, files::Completed, sequence::Progress, viewer::ViewTransform};
use eframe::egui;
use std::sync::{Arc, atomic::Ordering};
use veylvik_document::{NodeId, Operation, Time, tracking::Sample};
use veylvik_render::{Plan, RenderRequest, tracking::PointTracker};
#[derive(Default)]
pub struct Controls {
    pub open: bool,
    pub source: Option<NodeId>,
    target: Option<NodeId>,
    point: Option<[f32; 2]>,
    patch: u32,
    search: u32,
    end: i64,
    result: Option<Tracked>,
    pub active: Option<Arc<Progress>>,
    offset: [f32; 2],
    replace: bool,
}
pub struct Tracked {
    pub session: NodeId,
    pub revision: u64,
    source: NodeId,
    origin: [f32; 2],
    samples: Vec<Sample>,
    message: String,
}
impl Controls {
    pub fn overlay(
        &mut self,
        ui: &mut egui::Ui,
        canvas: egui::Rect,
        view: ViewTransform,
        image: egui::Vec2,
        frame: i64,
        rate: Time,
    ) {
        if !self.open {
            return;
        }
        let [ox, oy, sx, sy] = view.0;
        let origin = canvas.min + egui::vec2(canvas.width() * ox, canvas.height() * oy);
        let scale = egui::vec2(
            canvas.width() * sx / image.x,
            canvas.height() * sy / image.y,
        );
        let to_screen = |p: [f32; 2]| origin + egui::vec2(p[0] * scale.x, p[1] * scale.y);
        let response = ui.interact(
            canvas,
            ui.id().with("tracking-point"),
            egui::Sense::click_and_drag(),
        );
        if self.active.is_none()
            && (response.clicked() || response.dragged())
            && let Some(p) = response.interact_pointer_pos()
        {
            let p = (p - origin) / scale;
            if p.x >= 0.0 && p.y >= 0.0 && p.x < image.x && p.y < image.y {
                self.point = Some([p.x.round(), p.y.round()]);
                self.result = None;
            }
        }
        let point = self
            .result
            .as_ref()
            .and_then(|r| {
                r.samples
                    .iter()
                    .find(|s| {
                        Time::at_frame(frame, rate)
                            .is_ok_and(|t| s.time.compare(t).is_ok_and(|o| o.is_eq()))
                    })
                    .map(|s| s.position)
            })
            .or(self.point);
        if let Some(point) = point {
            let center = to_screen(point);
            let painter = ui.painter().with_clip_rect(canvas);
            for (radius, color) in [
                (self.search, egui::Color32::from_rgb(120, 160, 210)),
                (self.patch, egui::Color32::from_rgb(245, 190, 70)),
            ] {
                painter.rect_stroke(
                    egui::Rect::from_center_size(center, scale * (radius * 2 + 1) as f32),
                    0.0,
                    egui::Stroke::new(1.5_f32, color),
                    egui::StrokeKind::Middle,
                );
            }
            painter.line_segment(
                [center - egui::vec2(5.0, 0.0), center + egui::vec2(5.0, 0.0)],
                egui::Stroke::new(1.5_f32, egui::Color32::WHITE),
            );
            painter.line_segment(
                [center - egui::vec2(0.0, 5.0), center + egui::vec2(0.0, 5.0)],
                egui::Stroke::new(1.5_f32, egui::Color32::WHITE),
            );
            if let Some(result) = &self.result {
                for points in result.samples.windows(2) {
                    painter.line_segment(
                        [to_screen(points[0].position), to_screen(points[1].position)],
                        egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(245, 190, 70)),
                    );
                }
            }
        }
    }
}
impl Studio {
    pub(super) fn begin_tracking(&mut self) {
        if !self.apply_draft() || self.files.busy() {
            return;
        }
        self.playing = false;
        self.planar.open = false;
        self.tracking = Controls {
            open: true,
            source: Some(self.project.selected),
            patch: 8,
            search: 24,
            end: i64::from(self.project.document.snapshot().frame_count) - 1,
            ..Default::default()
        };
        self.project.preview = self.tracking.source;
        self.request_preview();
    }
    pub(super) fn tracking_completed(&mut self, result: Tracked) {
        self.tracking.active = None;
        if result.session != self.project.session
            || result.revision != self.project.document.revision()
        {
            self.fail("Tracking result is obsolete because the composition changed. Track again.");
            return;
        }
        self.status = result.message.clone();
        self.error = false;
        self.tracking.source = Some(result.source);
        self.tracking.result = Some(result);
        self.tracking.open = true;
    }
    pub(super) fn tracking_window(&mut self, ctx: &egui::Context) {
        if !self.tracking.open {
            return;
        }
        let mut open = true;
        let mut track = false;
        let mut apply = false;
        let mut view = false;
        let doc = self.project.document.snapshot();
        egui::Window::new("Track motion").default_pos(egui::pos2((ctx.content_rect().width()-760.0).max(20.0),100.0)).open(&mut open).collapsible(false).default_width(340.0).show(ctx, |ui| {
            ui.label("1. Choose a source and click a distinct feature in the viewer.");
            ui.add_enabled_ui(self.tracking.active.is_none(), |ui| {
                let previous = self.tracking.source;
                egui::ComboBox::from_id_salt("tracking-source").selected_text(self.tracking.source.and_then(|id| doc.nodes.get(&id)).map_or("Choose source", |n| n.label.as_str())).show_ui(ui, |ui| {
                    for node in doc.nodes.values() { ui.selectable_value(&mut self.tracking.source, Some(node.id), &node.label); }
                });
                if self.tracking.source != previous { self.tracking.point = None; self.tracking.result = None; view = true; }
                if ui.button("View source").clicked() { view = true; }
                ui.horizontal(|ui| { ui.label("Feature radius"); ui.add(egui::DragValue::new(&mut self.tracking.patch).range(3..=32).suffix(" px")); });
                ui.horizontal(|ui| { ui.label("Search radius"); ui.add(egui::DragValue::new(&mut self.tracking.search).range(1..=64).suffix(" px")); });
                ui.weak("Gold: feature. Blue: search area per frame. Position tracking only.");
                ui.separator();
                ui.label("2. Track from the current frame.");
                ui.horizontal(|ui| { ui.label(format!("From {} to", self.project.frame)); ui.add(egui::DragValue::new(&mut self.tracking.end).range(0..=i64::from(doc.frame_count)-1)); });
                ui.horizontal(|ui| {
                    if ui.button("To start").clicked() { self.tracking.end = 0; }
                    if ui.button("To end").clicked() { self.tracking.end = i64::from(doc.frame_count)-1; }
                    let valid = self.tracking.point.is_some() && self.project.frame.abs_diff(self.tracking.end) > 0 && self.project.frame.abs_diff(self.tracking.end) < 4096;
                    if ui.add_enabled(valid && !self.files.busy(), egui::Button::new(if self.tracking.end < self.project.frame { "Track backward" } else { "Track forward" })).clicked() { track = true; }
                });
            });
            if let Some(progress) = &self.tracking.active {
                let n = progress.completed.load(Ordering::Relaxed); let total = progress.total.load(Ordering::Relaxed).max(1);
                ui.add(egui::ProgressBar::new(n as f32/total as f32).text(format!("Tracking {n} / {total}")));
                if ui.button("Cancel tracking").clicked() { progress.cancel.store(true,Ordering::Relaxed); }
            }
            if let Some(result) = &self.tracking.result {
                ui.separator(); ui.label(&result.message);
                ui.label("3. Attach the accepted movement.");
                egui::ComboBox::from_id_salt("tracking-target").selected_text(self.tracking.target.and_then(|id| doc.nodes.get(&id)).map_or("Choose text, mask or layer", |n| n.label.as_str())).show_ui(ui, |ui| { for node in doc.nodes.values() { ui.selectable_value(&mut self.tracking.target,Some(node.id),&node.label); } });
                ui.horizontal(|ui| { ui.label("Offset X"); ui.add(egui::DragValue::new(&mut self.tracking.offset[0]).speed(0.5)); ui.label("Y"); ui.add(egui::DragValue::new(&mut self.tracking.offset[1]).speed(0.5)); });
                let transform = self.tracking.target.and_then(|id| doc.nodes.get(&id)).is_some_and(|n| matches!(n.operation, Operation::Transform { .. }));
                if !transform { self.tracking.replace = false; }
                ui.add_enabled(transform,egui::Checkbox::new(&mut self.tracking.replace,"Update target's position keys"));
                let fresh = result.session == self.project.session && result.revision == self.project.document.revision();
                if !fresh { ui.colored_label(egui::Color32::LIGHT_RED,"Composition changed; track again before attaching."); }
                if ui.add_enabled(fresh && result.samples.len() >= 2 && self.tracking.target.is_some() && !self.draft_dirty,egui::Button::new("Apply tracked movement")).clicked() { apply = true; }
                ui.weak("Creates editable X/Y keys. Nothing changes until Apply; Undo restores the graph.");
            }
        });
        self.tracking.open = open;
        if !open && let Some(p) = &self.tracking.active {
            p.cancel.store(true, Ordering::Relaxed);
        }
        if view {
            self.project.preview = self.tracking.source;
            self.request_preview();
        }
        if track {
            self.run_tracking(ctx);
        }
        if apply {
            let result = self.tracking.result.as_ref().unwrap();
            let attached = veylvik_document::tracking::attach(
                self.project.document.snapshot(),
                self.tracking.target.unwrap(),
                result.origin,
                &result.samples,
                self.tracking.offset,
                self.tracking.replace,
            );
            match attached {
                Ok((id, commands)) => match self.project.apply(&commands) {
                    Ok(()) => {
                        self.project.selected = id;
                        self.project.preview = None;
                        self.tracking.result = None;
                        self.tracking.open = false;
                        self.refresh_draft();
                        self.request_preview();
                    }
                    Err(e) => self.fail(e),
                },
                Err(e) => self.fail(e),
            }
        }
    }
    fn run_tracking(&mut self, ctx: &egui::Context) {
        if !self.apply_draft() {
            return;
        }
        let Some(source) = self.tracking.source else {
            return;
        };
        let Some(origin) = self.tracking.point else {
            return;
        };
        let plan = match Plan::compile_output(self.project.document.snapshot(), source) {
            Ok(p) => p,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        let (session, revision) = (self.project.session, self.project.document.revision());
        let (start, end, rate) = (
            self.project.frame,
            self.tracking.end,
            self.project.document.snapshot().frame_rate,
        );
        let (patch, search) = (self.tracking.patch, self.tracking.search);
        let renderer = self.renderer.clone();
        let inputs = self.project.inputs.clone();
        let ctx = ctx.clone();
        let progress = Arc::new(Progress::default());
        progress
            .total
            .store((start.abs_diff(end) + 1) as u32, Ordering::Relaxed);
        let worker = progress.clone();
        self.playing = false;
        self.tracking.result = None;
        let work=self.files.start(move || {
            let run=|| -> Result<Tracked,Box<dyn std::error::Error>> {
                let tracker=PointTracker::new(&renderer);
                let reference=renderer.render_gpu_cached(&plan,RenderRequest::full(Time::at_frame(start,rate)?),&inputs)?;
                let check=tracker.track(&renderer,&reference,&reference,origin,origin,[patch,search])?;
                if check.confidence < 0.75 || check.separation < 0.03 { return Err("Choose a more distinctive feature: this patch is flat or ambiguous".into()); }
                let mut samples=vec![Sample { time:Time::at_frame(start,rate)?,position:origin }];
                let mut position=origin; let step=if end>start {1} else {-1}; let mut frame=start;
                let mut message="Tracking complete".to_owned();
                worker.completed.store(1,Ordering::Relaxed);
                while frame != end {
                    if worker.cancel.load(Ordering::Relaxed) { message="Tracking cancelled; accepted frames remain available".into(); break; }
                    frame+=step;
                    let time=Time::at_frame(frame,rate)?;
                    let current=renderer.render_gpu_cached(&plan,RenderRequest::full(time),&inputs)?;
                    let found=tracker.track(&renderer,&reference,&current,origin,position,[patch,search])?;
                    if found.confidence < 0.75 || found.separation < 0.03 { message=format!("Lost feature at frame {frame} (confidence {:.0}%). Reposition there and track again.",found.confidence.max(0.0)*100.0); break; }
                    position=found.position;
                    samples.push(Sample { time,position });
                    worker.completed.store(samples.len() as u32,Ordering::Relaxed); ctx.request_repaint();
                }
                message.push_str(&format!(" · {} accepted frames",samples.len()));
                Ok(Tracked { session,revision,source,origin,samples,message })
            };
            run().map(Completed::Tracked).map_err(|e|e.to_string())
        });
        if work.is_ok() {
            self.tracking.active = Some(progress);
        }
        self.file_started(work);
    }
}
