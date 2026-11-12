//! Composition format drafts are applied as one document transaction.
use eframe::egui;
use veylvik_document::{Composition, Time};
#[derive(Clone, Copy)]
pub struct Settings {
    pub width: u32,
    pub height: u32,
    pub frame_rate: Time,
    pub new: bool,
}
#[derive(Default)]
pub struct Controls {
    pub open: bool,
    settings: Option<Settings>,
}
impl Controls {
    pub fn begin(&mut self, document: &Composition, new: bool) {
        self.open = true;
        self.settings = Some(if new {
            Settings {
                width: 1920,
                height: 1080,
                frame_rate: Time { ticks: 24, rate: 1 },
                new: true,
            }
        } else {
            Settings {
                width: document.width,
                height: document.height,
                frame_rate: document.frame_rate,
                new: false,
            }
        });
    }
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Settings> {
        if !self.open {
            return None;
        }
        let mut open = self.open;
        let mut result = None;
        let mut close = false;
        let settings = self.settings.as_mut()?;
        egui::Window::new(if settings.new {
            "New composition"
        } else {
            "Composition settings"
        })
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            egui::Grid::new("composition-format").show(ui, |ui| {
                ui.label("Width");
                ui.add(
                    egui::DragValue::new(&mut settings.width)
                        .range(1..=8192)
                        .suffix(" px"),
                );
                ui.end_row();
                ui.label("Height");
                ui.add(
                    egui::DragValue::new(&mut settings.height)
                        .range(1..=8192)
                        .suffix(" px"),
                );
                ui.end_row();
                ui.label("Frame rate numerator");
                ui.add(egui::DragValue::new(&mut settings.frame_rate.ticks).range(1..=i64::MAX));
                ui.end_row();
                ui.label("Frame rate denominator");
                ui.add(egui::DragValue::new(&mut settings.frame_rate.rate).range(1..=u32::MAX));
                ui.end_row();
            });
            ui.label(format!(
                "{:.3} frames per second",
                settings.frame_rate.ticks as f64 / settings.frame_rate.rate as f64
            ));
            ui.weak(if settings.new {
                "Starts with a transparent background."
            } else {
                "Canvas changes do not scale nodes. Key times remain in seconds."
            });
            ui.horizontal(|ui| {
                if ui
                    .button(if settings.new { "Create" } else { "Apply" })
                    .clicked()
                {
                    result = Some(*settings);
                    close = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        self.open = open && !close;
        result
    }
}
