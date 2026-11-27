//! Compare look: the viewer's frame measured against a reference picture, region by region, with plain advice (veylvik-io look); the measuring runs on a thread of its own, never on the UI thread.
use crate::Studio;
use eframe::egui;
use serde_json::{Value, json};
use std::sync::mpsc;

#[derive(Default)]
pub(crate) struct LookWindow {
    pub open: bool,
    reference: String,
    seconds: f64,
    grid: u32,
    choosing: Option<mpsc::Receiver<Result<Option<std::path::PathBuf>, String>>>,
    running: Option<mpsc::Receiver<Result<Value, String>>>,
    result: Option<Result<Value, String>>,
}
/// The numbers shown side by side for a region: (label, field, decimals).
const SHOWN: [(&str, &str, usize); 6] = [
    ("Grains per 100 px²", "grains", 0),
    ("Grain size (px)", "grain_size", 1),
    ("Stand out", "stand_out", 1),
    ("Fine detail", "fine_contrast", 4),
    ("Streaks", "streaks", 2),
    ("Lit share", "coverage", 2),
];
impl Studio {
    pub(crate) fn look_window(&mut self, ctx: &egui::Context) {
        if !self.look.open {
            return;
        }
        if self.look.grid == 0 {
            self.look.grid = 3;
        }
        if let Some(chosen) = self.look.choosing.as_ref().and_then(|r| r.try_recv().ok()) {
            self.look.choosing = None;
            match chosen {
                Ok(Some(path)) => self.look.reference = path.display().to_string(),
                Ok(None) => {}
                Err(e) => self.look.result = Some(Err(e)),
            }
        }
        if let Some(done) = self.look.running.as_ref().and_then(|r| r.try_recv().ok()) {
            self.look.running = None;
            self.look.result = Some(done);
        }
        if self.look.running.is_some() || self.look.choosing.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let mut open = true;
        let mut compare = false;
        egui::Window::new("Compare look")
            .open(&mut open)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .default_size([520.0, 560.0])
            .show(ctx, |ui| {
                ui.weak("Measures the picture in the viewer against a reference, region by region (grains, their size, fine detail, streaks, brightness, colour), and says what to change.");
                ui.horizontal(|ui| {
                    ui.label("Reference");
                    ui.add(egui::TextEdit::singleline(&mut self.look.reference).desired_width(300.0).hint_text("a picture or video, full path"));
                    if ui.add_enabled(self.look.choosing.is_none(), egui::Button::new("Choose…")).clicked() {
                        let (send, receive) = mpsc::channel();
                        let folder = std::env::current_dir().unwrap_or_default();
                        std::thread::spawn(move || {
                            let _ = send.send(crate::files::select(crate::files::Choice::Import, &folder, ""));
                        });
                        self.look.choosing = Some(receive);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("At");
                    ui.add(egui::DragValue::new(&mut self.look.seconds).range(0.0..=36000.0).speed(0.1).suffix(" s"))
                        .on_hover_text("The moment of a video or sequence to compare with (a still has only one)");
                    ui.label("Regions");
                    ui.add(egui::DragValue::new(&mut self.look.grid).range(1..=6).suffix(" across"))
                        .on_hover_text("The whole picture and the cells of a grid this many across and down");
                    let ready = !self.look.reference.trim().is_empty() && self.look.running.is_none();
                    compare = ui.add_enabled(ready, egui::Button::new("Compare")).clicked();
                    if self.look.running.is_some() {
                        ui.spinner();
                    }
                });
                ui.separator();
                match &self.look.result {
                    None => {
                        ui.weak("Pause on the frame to compare, then Compare.");
                    }
                    Some(Err(e)) => {
                        ui.colored_label(ui.visuals().error_fg_color, e);
                    }
                    Some(Ok(result)) => regions(ui, result),
                }
            });
        if !open {
            self.look.open = false;
        }
        if compare {
            self.look.result = None;
            match self.start_look() {
                Ok(receive) => self.look.running = Some(receive),
                Err(e) => self.look.result = Some(Err(e)),
            }
        }
    }
    /// The viewer's frame, as the composition's display shows it, against the reference.
    fn start_look(&self) -> Result<mpsc::Receiver<Result<Value, String>>, String> {
        let shown = self
            .displayed
            .as_ref()
            .filter(|d| !d.baked && !self.transport.playing)
            .ok_or(
                "Pause on a frame first: a playing picture is the screen's, not the renderer's",
            )?;
        let frame = shown.frame.clone();
        let renderer = self.renderer.clone();
        let doc = self.project.document.snapshot();
        let display = if doc.ocio.view.is_some() {
            veylvik_document::media::DisplayTransform::Standard
        } else {
            doc.display
        };
        let args = json!({"reference": self.look.reference.trim(), "reference_seconds": self.look.seconds, "grid": self.look.grid});
        let (send, receive) = mpsc::channel();
        std::thread::Builder::new()
            .name("veylvik-compare-look".into())
            .spawn(move || {
                let result = (|| -> Result<Value, String> {
                    let read = renderer.read_frame(&frame).map_err(|e| e.to_string())?;
                    let ours = veylvik_io::look::Seen {
                        width: read.region.width,
                        height: read.region.height,
                        pixels: veylvik_render::display::apply(display, &read.pixels),
                    };
                    veylvik_io::look::against(&renderer, &args, (&ours, None))
                        .map_err(|e| e.to_string())
                })();
                let _ = send.send(result);
            })
            .map_err(|e| e.to_string())?;
        Ok(receive)
    }
}
/// Each region: its main numbers, the reference's beside ours, and the advice.
fn regions(ui: &mut egui::Ui, result: &Value) {
    let empty = Vec::new();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (k, region) in result["regions"]
                .as_array()
                .unwrap_or(&empty)
                .iter()
                .enumerate()
            {
                let advice = region["advice"].as_array().unwrap_or(&empty);
                let name = region["name"].as_str().unwrap_or("region");
                let title = if advice.is_empty() {
                    format!("{name}: alike")
                } else {
                    format!("{name}: {} to change", advice.len())
                };
                egui::CollapsingHeader::new(title)
                    .id_salt(("look-region", k))
                    .default_open(k == 0)
                    .show(ui, |ui| {
                        egui::Grid::new(("look-numbers", k))
                            .striped(true)
                            .show(ui, |ui| {
                                ui.weak("");
                                ui.strong("Reference");
                                ui.strong("Ours");
                                ui.end_row();
                                for (label, field, decimals) in SHOWN {
                                    ui.label(label);
                                    for side in ["reference", "ours"] {
                                        let v = region[side][field].as_f64().unwrap_or(0.0);
                                        ui.monospace(format!("{v:.decimals$}"));
                                    }
                                    ui.end_row();
                                }
                                ui.label("Middle brightness");
                                for side in ["reference", "ours"] {
                                    let v = region[side]["brightness"][1].as_f64().unwrap_or(0.0);
                                    ui.monospace(format!("{v:.2}"));
                                }
                                ui.end_row();
                            });
                        for line in advice {
                            ui.label(format!("• {}", line.as_str().unwrap_or("")));
                        }
                    });
            }
        });
}
