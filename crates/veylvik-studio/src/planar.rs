//! Guided planar analysis with explicit, undoable publication of reusable motion.
use crate::{Studio, files::Completed, sequence::Progress, viewer::ViewTransform};
use eframe::egui;
use std::sync::{Arc, Mutex, atomic::Ordering};
use veylvik_document::{
    NodeId, Operation, Time,
    planar::{Quad, Sample, Track, WarpMode},
};
use veylvik_render::Plan;
use veylvik_track::planar::{PlanarTracker, Settings};

pub struct Controls {
    pub open: bool,
    pub source: Option<NodeId>,
    pub active: Option<Arc<Progress>>,
    surface: Quad,
    /// Separate analysis area on the same plane; `None` analyzes the corners themselves.
    area: Option<Quad>,
    /// Node whose alpha marks foreground objects the tracker ignores.
    occlusion: Option<NodeId>,
    settings: Settings,
    end: i64,
    result: Option<Tracked>,
    target: Option<NodeId>,
    mode: WarpMode,
    show_features: bool,
    saved: Option<NodeId>,
    edited: bool,
    blend_surface: bool,
    look: veylvik_document::surface::Look,
    base: Option<Tracked>,
    correction_frame: Option<i64>,
    live: Arc<Mutex<Live>>,
    live_sample: Option<Sample>,
}
impl Default for Controls {
    fn default() -> Self {
        Self {
            open: false,
            source: None,
            active: None,
            surface: [[0.2, 0.2], [0.8, 0.2], [0.8, 0.8], [0.2, 0.8]],
            area: None,
            occlusion: None,
            settings: Settings::default(),
            end: 0,
            result: None,
            target: None,
            mode: WarpMode::Follow,
            show_features: true,
            saved: None,
            edited: true,
            blend_surface: true,
            look: veylvik_document::surface::Look::Painted,
            base: None,
            correction_frame: None,
            live: Arc::new(Mutex::new(Live::default())),
            live_sample: None,
        }
    }
}
#[derive(Default)]
struct Live {
    // A single replaceable GPU frame bounds memory if analysis outruns the UI.
    latest: Option<(i64, Arc<veylvik_render::GpuFrame>, Sample)>,
    quality: Vec<(Time, bool)>,
    lost: Option<i64>,
}
#[derive(Clone)]
pub struct Tracked {
    session: NodeId,
    revision: u64,
    source: NodeId,
    data: Track,
    message: String,
}
impl Controls {
    pub fn overlay(
        &mut self,
        ui: &mut egui::Ui,
        canvas: egui::Rect,
        view: ViewTransform,
        frame: i64,
        rate: Time,
    ) {
        let [ox, oy, sx, sy] = view.0;
        let origin = canvas.min + egui::vec2(canvas.width() * ox, canvas.height() * oy);
        let scale = egui::vec2(canvas.width() * sx, canvas.height() * sy);
        let screen =
            |p: [f64; 2]| origin + egui::vec2(p[0] as f32 * scale.x, p[1] as f32 * scale.y);
        let time = Time::at_frame(frame, rate).ok();
        let sample = self
            .live_sample
            .as_ref()
            .filter(|s| time.is_some_and(|t| s.time.compare(t).is_ok_and(|o| o.is_eq())))
            .or_else(|| {
                self.result.as_ref().and_then(|r| {
                    time.and_then(|t| {
                        r.data
                            .samples
                            .iter()
                            .find(|s| s.time.compare(t).is_ok_and(|o| o.is_eq()))
                    })
                })
            });
        let quad = sample
            .map(|s| s.corners)
            .or_else(|| {
                self.result.as_ref().and_then(|r| {
                    time.and_then(|t| r.data.motion_at(t).ok())
                        .and_then(|h| h.transform_quad(r.data.reference).ok())
                })
            })
            .unwrap_or(self.surface);
        let painter = ui.painter().with_clip_rect(canvas);
        for i in 0..4 {
            painter.line_segment(
                [screen(quad[i]), screen(quad[(i + 1) % 4])],
                egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(90, 210, 220)),
            );
        }
        if self.show_features
            && let Some(sample) = sample
        {
            for f in &sample.features {
                let p = screen(f.position);
                if f.accepted {
                    painter.circle_stroke(
                        p,
                        3.,
                        egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(100, 230, 150)),
                    );
                } else {
                    for sign in [-1., 1.] {
                        painter.line_segment(
                            [
                                p + egui::vec2(-3., -3. * sign),
                                p + egui::vec2(3., 3. * sign),
                            ],
                            egui::Stroke::new(1_f32, egui::Color32::from_rgb(240, 130, 100)),
                        );
                    }
                }
            }
        }
        if self.active.is_none()
            && self.result.is_none()
            && self.correction_frame.is_none_or(|f| f == frame)
        {
            let drag = |quad: &mut Quad, key: &str, color: egui::Color32, hint: &str| {
                for i in 0..4 {
                    let p = screen(quad[i]);
                    painter.rect_filled(
                        egui::Rect::from_center_size(p, egui::vec2(8., 8.)),
                        2.,
                        color,
                    );
                    let response = ui.interact(
                        egui::Rect::from_center_size(p, egui::vec2(20., 20.)).intersect(canvas),
                        ui.id().with((key, i)),
                        egui::Sense::drag(),
                    );
                    if response.dragged()
                        && let Some(pointer) = response.interact_pointer_pos()
                    {
                        let value = (pointer - origin) / scale;
                        let mut candidate = *quad;
                        candidate[i] = [
                            f64::from(value.x.clamp(0., 1.)),
                            f64::from(value.y.clamp(0., 1.)),
                        ];
                        if veylvik_document::planar::validate_quad(candidate).is_ok() {
                            *quad = candidate;
                        }
                    }
                    response.on_hover_text(hint);
                }
            };
            if let Some(area) = &mut self.area {
                for i in 0..4 {
                    painter.line_segment(
                        [screen(area[i]), screen(area[(i + 1) % 4])],
                        egui::Stroke::new(1.5_f32, AREA),
                    );
                }
                drag(
                    area,
                    "planar-area",
                    AREA,
                    "Drag to cover as much textured surface on the same plane as possible",
                );
            }
            let hint = if self.area.is_some() {
                "Drag to place the corners the layer is pinned to; they may cover an empty part of the plane"
            } else {
                "Drag to enclose the rigid surface to track"
            };
            drag(
                &mut self.surface,
                "planar-corner",
                egui::Color32::WHITE,
                hint,
            );
        }
    }
}
/// Analysis area overlay colour, distinct from the cyan corner pin.
const AREA: egui::Color32 = egui::Color32::from_rgb(250, 190, 80);
fn weak(sample: &Sample) -> bool {
    let accepted = sample.features.iter().filter(|f| f.accepted).count();
    accepted < 12 || accepted * 3 < sample.features.len() * 2 || sample.rms_pixels > 1.5
}
impl Studio {
    pub(super) fn poll_planar_preview(&mut self) {
        if self.planar.active.is_none() {
            return;
        }
        let latest = self
            .planar
            .live
            .lock()
            .ok()
            .and_then(|mut live| live.latest.take());
        if let Some((frame, image, sample)) = latest {
            self.project.frame = frame;
            self.project.preview = self.planar.source;
            self.wanted = None;
            self.displayed_ticket = None;
            self.preview_pending = false;
            self.displayed_time = Some(sample.time);
            self.displayed = Some(self.display.frame(
                &self.renderer,
                image,
                self.view,
                ViewTransform::default(),
            ));
            self.planar.live_sample = Some(sample);
        }
    }
    pub(super) fn planar_quality(&mut self, ui: &mut egui::Ui) {
        if !self.planar.open {
            return;
        }
        let doc = self.project.document.snapshot();
        let count = doc.frame_count;
        let rate = doc.frame_rate;
        let mut quality: Vec<_> = self
            .planar
            .result
            .as_ref()
            .or(self.planar.base.as_ref())
            .map(|r| r.data.samples.iter().map(|s| (s.time, weak(s))).collect())
            .unwrap_or_default();
        let lost = if let Ok(live) = self.planar.live.lock() {
            quality.extend_from_slice(&live.quality);
            live.lost
        } else {
            None
        };
        if quality.is_empty() && lost.is_none() {
            return;
        }
        let mut seek = None;
        ui.horizontal(|ui| {
            ui.label("Surface track");
            let (rect,response)=ui.allocate_exact_size(egui::vec2(ui.available_width().max(40.),14.),egui::Sense::click_and_drag());
            let width=rect.width()/count.max(1) as f32;
            let painter=ui.painter();
            painter.rect_filled(rect,2.,egui::Color32::from_gray(45));
            let x=|f:i64|rect.left()+f as f32*width;
            for (time,weak) in quality {
                if let Some(frame)=crate::key_timeline::frame(time,rate,count) {
                    painter.rect_filled(egui::Rect::from_min_size(egui::pos2(x(frame),rect.top()),egui::vec2(width.max(1.),rect.height())),0.,if weak {egui::Color32::from_rgb(220,170,70)} else {egui::Color32::from_rgb(65,155,110)});
                }
            }
            if let Some(frame)=lost {painter.rect_filled(egui::Rect::from_min_size(egui::pos2(x(frame),rect.top()),egui::vec2(width.max(3.),rect.height())),0.,egui::Color32::from_rgb(230,90,80));}
            painter.vline(x(self.project.frame)+width*0.5,rect.y_range(),(1.5,egui::Color32::WHITE));
            if self.planar.active.is_none() && (response.clicked()||response.dragged()) && let Some(p)=response.interact_pointer_pos() {
                seek=Some(((p.x-rect.left())/width).floor().clamp(0.,count.saturating_sub(1) as f32) as i64);
            }
            response.on_hover_text("Tracking quality: green = strong, amber = weak or manual, red = analysis stopped, grey = no sample. Click to inspect a frame.");
        });
        if let Some(frame) = seek
            && self.apply_draft()
        {
            self.project.frame = frame;
            self.playing = false;
            self.request_preview();
        }
    }

    pub(super) fn begin_planar(&mut self) {
        if !self.apply_draft() || self.files.busy() {
            return;
        }
        self.playing = false;
        self.tracking.open = false;
        let doc = self.project.document.snapshot();
        self.planar = Controls {
            open: true,
            source: Some(self.project.selected),
            end: i64::from(doc.frame_count) - 1,
            ..Default::default()
        };
        // Reopening a saved track displays its persisted diagnostics and offers
        // new consumers without rerunning analysis.
        let selected = &doc.nodes[&self.project.selected];
        let track_node = if matches!(selected.operation, Operation::PlanarWarp { .. }) {
            &doc.nodes[&selected.inputs["track"]]
        } else {
            selected
        };
        if let Operation::PlanarTrack { data } = &track_node.operation {
            let source = track_node.inputs["image"];
            self.planar.saved = Some(track_node.id);
            self.planar.edited = false;
            self.planar.source = Some(source);
            self.planar.surface = data.reference;
            self.planar.result = Some(Tracked {
                session: self.project.session,
                revision: self.project.document.revision(),
                source,
                data: (**data).clone(),
                message: "Saved planar track".into(),
            });
        }
        self.project.preview = self.planar.source;
        self.request_preview();
    }
    pub(super) fn planar_completed(&mut self, result: Tracked) {
        self.planar.active = None;
        self.planar.live_sample = None;
        if let Ok(mut live) = self.planar.live.lock() {
            live.latest = None;
        }
        if result.session != self.project.session
            || result.revision != self.project.document.revision()
        {
            self.fail("Composition changed during tracking; track again.");
            return;
        }
        self.status = result.message.clone();
        self.error = false;
        self.planar.edited = true;
        self.planar.result = Some(result);
        self.planar.base = None;
        self.planar.correction_frame = None;
        self.planar.open = true;
        self.request_preview();
    }
    pub(super) fn planar_window(&mut self, ctx: &egui::Context) {
        if !self.planar.open {
            return;
        }
        let mut open = true;
        let mut track = false;
        let mut save = false;
        let mut attach = false;
        let mut view = false;
        let doc = self.project.document.snapshot();
        egui::Window::new("Track surface").default_width(350.).default_pos(egui::pos2((ctx.content_rect().width()-370.).max(20.),100.)).open(&mut open).collapsible(false).show(ctx,|ui| {
            ui.label("Choose a rigid surface and place its four corners in the viewer.");
            ui.add_enabled_ui(self.planar.active.is_none(),|ui| {
                let old=self.planar.source;
                egui::ComboBox::from_id_salt("planar-source").selected_text(old.and_then(|id|doc.nodes.get(&id)).map_or("Source",|n|n.label.as_str())).show_ui(ui,|ui| {
                    for node in doc.nodes.values() { if !matches!(node.operation,Operation::PlanarTrack {..}|Operation::PlanarWarp {..}) { ui.selectable_value(&mut self.planar.source,Some(node.id),&node.label); } }
                });
                if old!=self.planar.source {self.planar.result=None;self.planar.base=None;self.planar.saved=None;self.planar.correction_frame=None;self.planar.live=Arc::new(Mutex::new(Live::default()));view=true;}
                if ui.button("View tracking source").clicked() {view=true;}
                ui.checkbox(&mut self.planar.show_features,"Show feature matches");
                ui.weak("Green circles: accepted. Coral crosses: rejected.");
                ui.separator();
                ui.horizontal(|ui| {ui.label(format!("Frame {} to",self.project.frame));ui.add(egui::DragValue::new(&mut self.planar.end).range(0..=i64::from(doc.frame_count)-1));});
                ui.horizontal(|ui| {if ui.button("To start").clicked() {self.planar.end=0;}
                    if ui.button("To end").clicked() {self.planar.end=i64::from(doc.frame_count)-1;}});
                ui.collapsing("Tracking settings",|ui| {
                    ui.weak("GPU pyramid: coarse motion search, full-resolution refinement.");
                    ui.horizontal(|ui| {ui.label("Feature radius");ui.add(egui::DragValue::new(&mut self.planar.settings.patch_radius).range(3..=16).suffix(" px"));});
                    ui.horizontal(|ui| {ui.label("Search per level");ui.add(egui::DragValue::new(&mut self.planar.settings.search_radius).range(1..=48).suffix(" px"));});
                    ui.horizontal(|ui| {ui.label("Match tolerance");ui.add(egui::DragValue::new(&mut self.planar.settings.tolerance_pixels).range(0.1..=32.).speed(0.1).suffix(" px"));});
                    {
                        let doc = self.project.document.snapshot();
                        let name = |id: Option<NodeId>| id.and_then(|id| doc.nodes.get(&id)).map_or("None".to_owned(), |n| n.label.clone());
                        let mut options: Vec<_> = doc.nodes.values()
                            .filter(|n| Some(n.id) != self.planar.source && matches!(n.operation,
                                Operation::Path { .. } | Operation::Ellipse { .. } | Operation::Mask {} | Operation::CombineMask { .. }
                                | Operation::ChannelMask { .. } | Operation::Morphology { .. } | Operation::Blur { .. } | Operation::Affine { .. }))
                            .map(|n| (n.label.clone(), n.id)).collect();
                        options.sort();
                        egui::ComboBox::from_label("Occlusion matte")
                            .selected_text(name(self.planar.occlusion))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.planar.occlusion, None, "None");
                                for (label, id) in options { ui.selectable_value(&mut self.planar.occlusion, Some(id), label); }
                            })
                            .response
                            .on_hover_text("A mask whose alpha covers foreground objects (people, cars, hands) crossing the surface. Matches under it are ignored on every frame, so the plane stays locked behind them. Animate the mask with the path tools.");
                    }
                    let mut separate = self.planar.area.is_some();
                    if ui.checkbox(&mut separate, "Separate tracking area")
                        .on_hover_text("Analyze a larger textured area of the same plane (amber) than the corners the layer is pinned to (cyan). Corners can then sit on blank or partly hidden parts of the surface, and tracking lasts while any of the area stays in view.")
                        .changed()
                    {
                        self.planar.area = separate.then(|| {
                            let [cx, cy] = self.planar.surface.iter().fold([0., 0.], |a, p| [a[0] + p[0] / 4., a[1] + p[1] / 4.]);
                            self.planar.surface.map(|[x, y]| [(cx + (x - cx) * 1.6).clamp(0., 1.), (cy + (y - cy) * 1.6).clamp(0., 1.)])
                        });
                    }
                    let mut reanchor = self.planar.settings.reanchor_below > 0;
                    if ui.checkbox(&mut reanchor, "Re-anchor when matches thin out")
                        .on_hover_text("Re-detect features on the latest good frame when fewer than 24 matches are accepted. Keeps walking and zooming shots tracked longer; each re-anchor can add slight drift.")
                        .changed()
                    {
                        self.planar.settings.reanchor_below = if reanchor { 24 } else { 0 };
                    }
                });
                if self.planar.result.is_some() && ui.button("Correct corners / retrack section").clicked() {
                    if let Some(result)=&self.planar.result
                        && let Ok(quad)=Time::at_frame(self.project.frame,doc.frame_rate)
                            .and_then(|time|result.data.motion_at(time))
                            .and_then(|h|h.transform_quad(result.data.reference)) {
                            self.planar.surface=quad.map(|p|p.map(|v|v.clamp(0.,1.)));
                            if veylvik_document::planar::validate_quad(self.planar.surface).is_err() { self.planar.surface=Controls::default().surface; }
                    }
                    self.planar.base=self.planar.result.take();
                    self.planar.correction_frame=Some(self.project.frame);
                    self.planar.live_sample=None;
                    self.planar.live=Arc::new(Mutex::new(Live::default()));
                }
                if let Some(frame)=self.planar.correction_frame {
                    ui.label(format!("Correction at frame {frame}. Drag the four corners, then track to the chosen endpoint. Other frames are preserved."));
                    if ui.button("Return to correction frame").clicked() {self.project.frame=frame;view=true;}
                    if ui.button("Cancel correction").clicked() {self.planar.result=self.planar.base.take();self.planar.correction_frame=None;}
                }
                let valid=self.planar.correction_frame.is_none_or(|f|f==self.project.frame) && self.planar.source.is_some() && self.project.frame.abs_diff(self.planar.end)<4096 && self.project.frame!=self.planar.end;
                if ui.add_enabled(valid && !self.files.busy(),egui::Button::new("Track surface")).clicked() {track=true;}
            });
            if let Some(p)=&self.planar.active {
                let n=p.completed.load(Ordering::Relaxed);let total=p.total.load(Ordering::Relaxed).max(1);
                ui.add(egui::ProgressBar::new(n as f32/total as f32).text(format!("Tracking {n} / {total}")));
                if ui.button("Cancel").clicked() {p.cancel.store(true,Ordering::Relaxed);}
            }
            if let Some(result)=&self.planar.result {
                ui.separator();ui.label(&result.message);
                let time=Time::at_frame(self.project.frame,doc.frame_rate).ok();
                if let Some(s)=time.and_then(|t|result.data.samples.iter().find(|s|s.time.compare(t).is_ok_and(|o|o.is_eq()))) {
                    ui.label(format!("{} / {} features accepted · {:.2} px error",s.features.iter().filter(|f|f.accepted).count(),s.features.len(),s.rms_pixels));
                }
                let fresh=result.session==self.project.session && result.revision==self.project.document.revision() && !self.draft_dirty;
                ui.add_enabled_ui(fresh,|ui| {
                    if ui.add_enabled(self.planar.edited,egui::Button::new(if self.planar.saved.is_some() {"Update reusable track"} else {"Save reusable track"})).clicked() {save=true;}
                    ui.separator();
                    egui::ComboBox::from_id_salt("planar-target").selected_text(self.planar.target.and_then(|id|doc.nodes.get(&id)).map_or("Choose text, image or layer",|n|n.label.as_str())).show_ui(ui,|ui| {for node in doc.nodes.values() {ui.selectable_value(&mut self.planar.target,Some(node.id),&node.label);}});
                    egui::ComboBox::from_id_salt("planar-mode").selected_text(self.planar.mode.label()).show_ui(ui,|ui| {for mode in WarpMode::ALL {ui.selectable_value(&mut self.planar.mode,mode,mode.label());}});
                    ui.weak(match self.planar.mode {WarpMode::Follow=>"Position the layer on the reference frame first. Its placement follows the surface.",WarpMode::Fit=>"Maps the layer's entire canvas into the four surface corners.",WarpMode::Stabilize=>"Locks the tracked source surface to its reference position."});
                    if self.planar.mode!=WarpMode::Stabilize {
                        ui.checkbox(&mut self.planar.blend_surface,"Match surface appearance");
                        if self.planar.blend_surface {
                            egui::ComboBox::from_id_salt("surface-starting-look").selected_text(self.planar.look.label()).show_ui(ui,|ui| {
                                for look in veylvik_document::surface::Look::ALL {ui.selectable_value(&mut self.planar.look,look,look.label());}
                            });
                            ui.weak("Creates editable lighting, texture, wear, softness and grain controls.");
                        }
                    }
                    if ui.add_enabled(self.planar.target.is_some() || self.planar.mode==WarpMode::Stabilize,egui::Button::new("Create tracked layer")).clicked() {attach=true;}
                });
                if !fresh {ui.colored_label(egui::Color32::LIGHT_RED,"Composition changed. Reopen the saved track or track again.");}
                ui.weak("Save and Create are undoable. Tracking alone does not change your project.");
            }
        });
        self.planar.open = open;
        if !open && let Some(p) = &self.planar.active {
            p.cancel.store(true, Ordering::Relaxed);
        }
        if view {
            self.project.preview = self.planar.source;
            self.request_preview();
        }
        if track {
            self.run_planar(ctx);
        }
        if save || attach {
            self.publish_planar(attach);
        }
    }
    fn publish_planar(&mut self, attach: bool) {
        let Some(result) = &self.planar.result else {
            return;
        };
        let doc = self.project.document.snapshot();
        let saved = if let Some(id) = self.planar.saved {
            let mut commands = Vec::new();
            if self.planar.edited {
                let Some(mut node) = doc.nodes.get(&id).cloned() else {
                    self.fail("Saved track no longer exists");
                    return;
                };
                node.operation = Operation::PlanarTrack {
                    data: Box::new(result.data.clone()),
                };
                node.inputs.insert("image".into(), result.source);
                commands.push(veylvik_document::Command::Put {
                    node: Box::new(node),
                });
            }
            Ok((id, commands))
        } else {
            veylvik_document::planar::save(doc, result.source, result.data.clone())
        };
        let (track, mut commands) = match saved {
            Ok(v) => v,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        let mut selected = track;
        if attach {
            let image = if self.planar.mode == WarpMode::Stabilize {
                result.source
            } else {
                let Some(id) = self.planar.target else {
                    return;
                };
                id
            };
            let warp = veylvik_document::Node::new(
                self.planar.mode.label(),
                Operation::PlanarWarp {
                    mode: self.planar.mode,
                },
                &[("image", image), ("track", track)],
            );
            selected = warp.id;
            let mut output = warp.id;
            commands.push(veylvik_document::Command::Put {
                node: Box::new(warp),
            });
            if self.planar.mode != WarpMode::Stabilize {
                let merge = veylvik_document::Node::new(
                    "Layer on tracked surface",
                    if self.planar.blend_surface {
                        self.planar.look.operation()
                    } else {
                        Operation::Merge {}
                    },
                    &[("background", result.source), ("foreground", output)],
                );
                output = merge.id;
                if self.planar.blend_surface {
                    selected = merge.id;
                }
                commands.push(veylvik_document::Command::Put {
                    node: Box::new(merge),
                });
            }
            commands.push(veylvik_document::Command::Output { id: output });
        }
        match self.project.apply(&commands) {
            Ok(()) => {
                self.project.selected = selected;
                self.project.preview = None;
                self.planar.open = false;
                self.planar.result = None;
                self.refresh_draft();
                self.request_preview();
            }
            Err(e) => self.fail(e),
        }
    }
    fn run_planar(&mut self, ctx: &egui::Context) {
        if !self.apply_draft() || self.files.busy() {
            return;
        }
        let Some(source) = self.planar.source else {
            return;
        };
        let plan = match Plan::compile_output(self.project.document.snapshot(), source) {
            Ok(p) => p,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        let doc = self.project.document.snapshot();
        let occlusion = match self
            .planar
            .occlusion
            .filter(|id| doc.nodes.contains_key(id))
            .map(|id| Plan::compile_output(doc, id))
            .transpose()
        {
            Ok(p) => p,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        let (session, revision) = (self.project.session, self.project.document.revision());
        let (start, end, rate) = (
            self.project.frame,
            self.planar.end,
            self.project.document.snapshot().frame_rate,
        );
        // Reanalyzing without entering correction mode still preserves existing sections.
        let base = self
            .planar
            .base
            .clone()
            .or_else(|| self.planar.result.clone());
        if base
            .as_ref()
            .is_some_and(|b| b.session != session || b.revision != revision || b.source != source)
        {
            self.fail("Composition changed. Reopen the saved track before correcting it.");
            return;
        }
        let surface = if self.planar.correction_frame.is_some() {
            self.planar.surface
        } else {
            base.as_ref()
                .and_then(|b| {
                    Time::at_frame(start, rate)
                        .ok()
                        .and_then(|t| b.data.motion_at(t).ok())
                        .and_then(|h| h.transform_quad(b.data.reference).ok())
                })
                .unwrap_or(self.planar.surface)
        };
        let settings = self.planar.settings;
        // A section correction re-tracks the corrected corners themselves.
        let region = self
            .planar
            .area
            .filter(|_| self.planar.correction_frame.is_none());
        let live = Arc::new(Mutex::new(Live::default()));
        self.planar.live = live.clone();
        self.planar.live_sample = None;
        self.planar.base = base.clone();
        let available_bytes = veylvik_document::limits::MAX_DOCUMENT_BYTES
            .saturating_sub(
                self.project
                    .document
                    .snapshot()
                    .encoded_size()
                    .unwrap_or(veylvik_document::limits::MAX_DOCUMENT_BYTES),
            )
            .saturating_sub(64 * 1024);
        let renderer = self.renderer.clone();
        let inputs = self.project.inputs.clone();
        let ctx = ctx.clone();
        let progress = Arc::new(Progress::default());
        progress
            .total
            .store((start.abs_diff(end) + 1) as u32, Ordering::Relaxed);
        let worker = progress.clone();
        self.playing = false;
        self.planar.result = None;
        let work = self.files.start(move || {
            let run = || -> Result<Tracked, Box<dyn std::error::Error>> {
                let tracker = PlanarTracker::new(&renderer);
                let mut observe =
                    |frame: i64, image: &Arc<veylvik_render::GpuFrame>, sample: &Sample| {
                        if let Ok(mut live) = live.lock() {
                            live.quality.push((sample.time, weak(sample)));
                            live.latest = Some((frame, image.clone(), sample.clone()));
                        }
                        worker.completed.fetch_add(1, Ordering::Relaxed);
                        ctx.request_repaint();
                        !worker.cancel.load(Ordering::Relaxed)
                    };
                let range = tracker.track_range(
                    &renderer,
                    &plan,
                    occlusion.as_ref().map(|plan| (plan, &inputs)),
                    &inputs,
                    surface,
                    region,
                    [start, end],
                    rate,
                    settings,
                    available_bytes,
                    &mut observe,
                )?;
                if let veylvik_track::planar::Stop::Lost { frame, .. } = range.stop
                    && let Ok(mut live) = live.lock()
                {
                    live.lost = Some(frame);
                }
                let mut data = range.data;
                let mut message = range.stop.message();
                if range.anchors > 1 {
                    message.push_str(&format!(" · re-anchored {} times", range.anchors - 1));
                }
                message.push_str(&format!(" · {} analyzed frames", data.samples.len()));
                if let Some(base) = &base {
                    data = base.data.splice(&data)?;
                    message.push_str("; other sections preserved");
                }
                Ok(Tracked {
                    session,
                    revision,
                    source,
                    data,
                    message,
                })
            };
            run()
                .map(Completed::PlanarTracked)
                .map_err(|e| e.to_string())
        });
        if work.is_ok() {
            self.planar.active = Some(progress);
        }
        self.file_started(work);
    }
}
