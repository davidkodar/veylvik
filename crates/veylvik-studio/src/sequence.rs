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
        }
    }
}
impl Controls {
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
            if !self.initialized {
                self.end = i64::from(frame_count) - 1;
                self.initialized = true;
            }
            egui::Window::new("Export image sequence").open(&mut open).collapsible(false).show(ctx, |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    ui.label("Export the current preview node at the composition frame rate.");
                    egui::ComboBox::from_id_salt("sequence-format").selected_text(self.format.label()).show_ui(ui,|ui| {
                        for format in [veylvik_io::FrameFormat::Png,veylvik_io::FrameFormat::Exr] { ui.selectable_value(&mut self.format,format,format.label()); }
                    });
                    ui.horizontal(|ui| {
                        ui.label("First frame"); ui.add(egui::DragValue::new(&mut self.start));
                        ui.label("Last frame (inclusive)"); ui.add(egui::DragValue::new(&mut self.end));
                    });
                    ui.horizontal(|ui| { ui.label("New folder name"); ui.text_edit_singleline(&mut self.name); });
                    ui.weak("Choose an existing parent folder. The named export folder must not exist.");
                    let result = validate(self.start, self.end, rate, &self.name);
                    match &result {
                        Ok(frames) => { ui.label(format!("{} frames · {} / {} fps", frames.count().unwrap(), rate.ticks, rate.rate)); }
                        Err(error) => { ui.colored_label(egui::Color32::LIGHT_RED, error); }
                    }
                    if ui.add_enabled(result.is_ok(), egui::Button::new("Choose parent folder and queue…")).clicked() {
                        settings = Some(Settings { format:self.format,frames: result.unwrap(), name: self.name.clone() });
                    }
                });
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
