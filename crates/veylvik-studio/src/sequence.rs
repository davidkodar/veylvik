//! Export settings and thread-safe progress for one background sequence render.
use eframe::egui;
use std::sync::atomic::{AtomicBool, AtomicU32};
/// Shared progress for tracking and segmentation workers.
#[derive(Default)]
pub struct Progress {
    pub completed: AtomicU32,
    pub total: AtomicU32,
    pub cancel: AtomicBool,
}
use veylvik_document::{Time, sequence::FrameRange};
pub struct Settings {
    pub frames: FrameRange,
    pub format: veylvik_io::FrameFormat,
    pub name: String,
}
pub struct Controls {
    pub open: bool,
    format: veylvik_io::FrameFormat,
    initialized: bool,
    start: i64,
    end: i64,
    name: String,
    /// Showing the frame range fields.
    part: bool,
}
impl Default for Controls {
    fn default() -> Self {
        Self {
            open: false,
            format: veylvik_io::FrameFormat::Png,
            initialized: false,
            start: 0,
            end: 95,
            name: "frames".into(),
            part: false,
        }
    }
}
impl Controls {
    /// The settings as a panel (the Export page and the dialog); Some when a
    /// destination is wanted.
    pub fn panel(
        &mut self,
        ui: &mut egui::Ui,
        rate: Time,
        frame_count: u32,
        enabled: bool,
    ) -> Option<Settings> {
        if !self.initialized {
            self.end = i64::from(frame_count) - 1;
            self.initialized = true;
        }
        let mut settings = None;
        let last = i64::from(frame_count) - 1;
        let part = &mut self.part;
        use crate::design::{chip, heading, option_card};
        ui.set_width(622.0_f32.min(ui.ctx().content_rect().width() - 40.0));
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        ui.add_enabled_ui(enabled, |ui| {
            heading(ui, "FORMAT");
            ui.horizontal_wrapped(|ui| {
                for (format, name, detail) in [
                    (veylvik_io::FrameFormat::Png, "PNG", "For the web and editing, with transparency"),
                    (veylvik_io::FrameFormat::Exr, "EXR", "Full HDR light, for grading and compositing"),
                ] {
                    if option_card(ui, name, detail, self.format == format)
                        .on_hover_text(format.label())
                        .clicked()
                    {
                        self.format = format;
                    }
                }
            });
            heading(ui, "RANGE");
            let whole = self.start == 0 && self.end == last;
            let custom = *part || !whole;
            ui.horizontal_wrapped(|ui| {
                if chip(ui, "Whole composition", !custom).clicked() {
                    (self.start, self.end) = (0, last);
                    *part = false;
                }
                if chip(ui, "Part of it", custom).clicked() {
                    *part = true;
                }
                if custom {
                    ui.label("Frames");
                    ui.add(egui::DragValue::new(&mut self.start).range(0..=last));
                    ui.label("to");
                    ui.add(egui::DragValue::new(&mut self.end).range(0..=last));
                }
            });
            heading(ui, "FOLDER");
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(260.0))
                    .on_hover_text("A new folder with this name is made inside the folder you choose next; it must not exist yet");
            });
            ui.add_space(6.0);
            ui.separator();
            let result = validate(self.start, self.end, rate, &self.name);
            ui.horizontal(|ui| {
                match &result {
                    Ok(frames) => {
                        let fps = rate.ticks as f64 / f64::from(rate.rate.max(1));
                        ui.label(
                            egui::RichText::new(format!(
                                "{} frames  ·  {} fps  ·  the viewed node",
                                frames.count().unwrap(),
                                if (fps - fps.round()).abs() < 1e-9 { format!("{fps:.0}") } else { format!("{fps:.3}") },
                            ))
                            .color(ui.visuals().weak_text_color()),
                        );
                    }
                    Err(error) => {
                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled_ui(result.is_ok(), |ui| crate::design::primary(ui, "Choose where to save"))
                        .inner
                        .clicked()
                    {
                        settings = Some(Settings {
                            format: self.format,
                            frames: result.clone().unwrap(),
                            name: self.name.clone(),
                        });
                    }
                });
            });
        });
        settings
    }
    /// Only the format cards (the quick export dialog).
    pub fn quick(&mut self, ui: &mut egui::Ui) {
        crate::design::heading(ui, "FORMAT");
        ui.horizontal_wrapped(|ui| {
            for (format, name, detail) in [
                (
                    veylvik_io::FrameFormat::Png,
                    "PNG",
                    "For the web and editing, with transparency",
                ),
                (
                    veylvik_io::FrameFormat::Exr,
                    "EXR",
                    "Full HDR light, for grading and compositing",
                ),
            ] {
                if crate::design::option_card(ui, name, detail, self.format == format).clicked() {
                    self.format = format;
                }
            }
        });
    }
    /// The settings as they stand, when they can be exported (the Export page's button).
    pub fn request(&mut self, rate: Time, frame_count: u32) -> Option<Settings> {
        if !self.initialized {
            self.end = i64::from(frame_count) - 1;
            self.initialized = true;
        }
        validate(self.start, self.end, rate, &self.name)
            .ok()
            .map(|frames| Settings {
                format: self.format,
                frames,
                name: self.name.clone(),
            })
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        rate: Time,
        frame_count: u32,
        enabled: bool,
    ) -> Option<Settings> {
        let mut settings = None;
        let mut open = self.open;
        if open {
            egui::Window::new("Export image sequence")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center())
                .show(ctx, |ui| {
                    settings = self.panel(ui, rate, frame_count, enabled);
                });
        }
        self.open = open && settings.is_none();
        settings
    }
}

fn validate(start: i64, end: i64, rate: Time, name: &str) -> Result<FrameRange, String> {
    if name.trim().is_empty() || matches!(name, "." | "..") || name.contains(['/', '\\', '\0']) {
        return Err("Use a single new folder name, without path separators.".into());
    }
    FrameRange::new(start, end, rate).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_folder_is_one_component_and_frame_range_is_bounded() {
        let fps = Time::new(24, 1).unwrap();
        for name in [
            "",
            " ",
            ".",
            "..",
            "../outside",
            "/tmp/output",
            "nested/name",
            "nested\\name",
            "bad\0name",
        ] {
            assert!(validate(0, 2, fps, name).is_err(), "{name:?}");
        }
        assert!(validate(0, 2, fps, "Vår animation").is_ok());
        assert!(validate(0, 10000, fps, "frames").is_err());
        assert!(validate(2, 0, fps, "frames").is_err());
    }
}
