//! Video settings for immutable render-queue snapshots.
use eframe::egui;
use veylvik_document::{Composition, Time};
use veylvik_io::video_export::{Codec, Quality, Settings};
#[derive(Default)]
pub struct Controls {
    pub open: bool,
    settings: Option<Settings>,
}
impl Controls {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        doc: &Composition,
        enabled: bool,
    ) -> Option<Settings> {
        if !self.open {
            return None;
        }
        let s = self
            .settings
            .get_or_insert_with(|| Settings::for_document(doc));
        let mut open = self.open;
        let mut result = None;
        egui::Window::new("Export video")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    result = settings_panel(ui, s, doc);
                });
            });
        self.open = open && result.is_none();
        result
    }
}

fn rate_option(ui: &mut egui::Ui, current: &mut Time, value: Time, label: &str) {
    let selected = current
        .checked_sub(value)
        .is_ok_and(|delta| delta.ticks == 0);
    if ui.selectable_label(selected, label).clicked() {
        *current = value;
    }
}

fn settings_panel(ui: &mut egui::Ui, s: &mut Settings, doc: &Composition) -> Option<Settings> {
    ui.heading("Ready to share");
    ui.weak("Composition output · SDR Rec.709 · Video only");
    ui.add_space(8.);
    egui::Grid::new("video-settings")
        .num_columns(2)
        .spacing([18., 10.])
        .show(ui, |ui| {
            ui.label("Format");
            egui::ComboBox::from_id_salt("video-codec")
                .selected_text(s.codec.label())
                .show_ui(ui, |ui| {
                    for c in [Codec::H264, Codec::Hevc10] {
                        ui.selectable_value(&mut s.codec, c, c.label());
                    }
                });
            ui.end_row();
            ui.label("Quality");
            let quality_label = |q| match q {
                Quality::Draft => "Draft · faster",
                Quality::High => "High · recommended",
                Quality::Master => "Master · larger files",
            };
            egui::ComboBox::from_id_salt("video-quality")
                .selected_text(quality_label(s.quality))
                .show_ui(ui, |ui| {
                    for q in [Quality::Draft, Quality::High, Quality::Master] {
                        ui.selectable_value(&mut s.quality, q, quality_label(q));
                    }
                });
            ui.end_row();
            ui.label("Resolution");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut s.width).range(2..=8192).speed(2.));
                ui.label("×");
                ui.add(
                    egui::DragValue::new(&mut s.height)
                        .range(2..=8192)
                        .speed(2.),
                );
                if ui
                    .add(crate::icons::Button::new(
                        crate::icons::Icon::Reset,
                        "Match composition resolution",
                    ))
                    .clicked()
                {
                    s.width = doc.width;
                    s.height = doc.height;
                }
            });
            ui.end_row();
            ui.label("Size presets");
            ui.horizontal(|ui| {
                for (w, h, label) in [
                    (1280, 720, "720p"),
                    (1920, 1080, "1080p"),
                    (3840, 2160, "4K"),
                ] {
                    if ui.button(label).clicked() {
                        s.width = w;
                        s.height = h;
                    }
                }
            });
            ui.end_row();
            ui.label("Frame rate");
            egui::ComboBox::from_id_salt("video-rate")
                .selected_text(format!("{:.3} fps", s.frame_rate.seconds().unwrap_or(0.)))
                .show_ui(ui, |ui| {
                    rate_option(
                        ui,
                        &mut s.frame_rate,
                        doc.frame_rate,
                        "Composition frame rate",
                    );
                    for (n, d, label) in [
                        (24, 1, "24 fps"),
                        (25, 1, "25 fps"),
                        (30000, 1001, "29.97 fps"),
                        (30, 1, "30 fps"),
                        (50, 1, "50 fps"),
                        (60000, 1001, "59.94 fps"),
                        (60, 1, "60 fps"),
                    ] {
                        rate_option(ui, &mut s.frame_rate, Time::new(n, d).unwrap(), label);
                    }
                });
            ui.end_row();
            ui.label("Source frames");
            ui.horizontal(|ui| {
                let last = i64::from(doc.frame_count) - 1;
                ui.add(egui::DragValue::new(&mut s.start).range(0..=last));
                ui.label("to");
                ui.add(egui::DragValue::new(&mut s.end).range(0..=last));
                if ui
                    .add(crate::icons::Button::new(
                        crate::icons::Icon::Reset,
                        "Use the entire composition",
                    ))
                    .clicked()
                {
                    s.start = 0;
                    s.end = last;
                }
            });
            ui.end_row();
            ui.label("Background");
            ui.color_edit_button_rgb(&mut s.matte);
            ui.end_row();
            ui.label("Audio");
            let mut include = !s.audio.mute;
            if ui
                .checkbox(&mut include, "Include footage audio")
                .on_hover_text("Mix the sound of every clip feeding the output, at its timeline position and trims.")
                .changed()
            {
                s.audio.mute = !include;
            }
            ui.end_row();
            ui.label("Music");
            ui.horizontal(|ui| {
                let mut text = s.audio.music.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
                if ui
                    .add(egui::TextEdit::singleline(&mut text).hint_text("Optional audio file path").desired_width(220.))
                    .changed()
                {
                    s.audio.music = (!text.trim().is_empty()).then(|| std::path::PathBuf::from(text.trim()));
                }
                ui.add_enabled(
                    s.audio.music.is_some(),
                    egui::DragValue::new(&mut s.audio.music_gain_db).range(-60.0..=12.0).speed(0.2).suffix(" dB"),
                );
            });
            ui.end_row();
        });
    ui.add_space(8.);
    ui.weak("Fits the composition without stretching. Source frame range is inclusive.");
    ui.separator();
    ui.label("Hardware encoding · NVENC / VA-API");
    ui.checkbox(
        &mut s.allow_software,
        "Allow software encoding if hardware is unavailable",
    );
    ui.weak("MP4 flattens transparency onto the background and carries AAC audio. For alpha or scene-linear HDR, use Export image sequence.");
    let valid = s.times(doc);
    match &valid {
        Ok(times) => {
            ui.label(format!(
                "{} frames · {:.2} seconds",
                times.len(),
                times.len() as f64 / s.frame_rate.seconds().unwrap()
            ));
        }
        Err(e) => {
            ui.colored_label(egui::Color32::LIGHT_RED, e.to_string());
        }
    }
    ui.add_space(6.);
    let export = ui
        .add_enabled(
            valid.is_ok(),
            egui::Button::new("Choose destination and queue…").min_size(egui::vec2(250., 32.)),
        )
        .clicked();
    ui.weak("Choose a new .mp4 filename. Existing files are never replaced.");
    export.then(|| s.clone())
}
