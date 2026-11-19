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
    /// The settings beyond the delivery cards (More settings in the export dialog).
    pub fn details(&mut self, ui: &mut egui::Ui, doc: &Composition) {
        let s = self
            .settings
            .get_or_insert_with(|| Settings::for_document(doc));
        details(ui, s, doc);
    }
    /// What the current settings will make, or why they cannot be exported.
    pub fn summary(&mut self, doc: &Composition) -> Result<String, String> {
        let s = self
            .settings
            .get_or_insert_with(|| Settings::for_document(doc));
        let times = s.times(doc).map_err(|e| e.to_string())?;
        let fps = s.frame_rate.seconds().unwrap_or(1.0);
        Ok(format!(
            "{} × {}  ·  {}  ·  {:.1} s",
            s.width,
            s.height,
            if (fps - fps.round()).abs() < 1e-9 {
                format!("{fps:.0} fps")
            } else {
                format!("{fps:.3} fps")
            },
            times.len() as f64 / fps
        ))
    }
    /// Only the delivery cards (the quick export dialog).
    pub fn quick(&mut self, ui: &mut egui::Ui, doc: &Composition) {
        let s = self
            .settings
            .get_or_insert_with(|| Settings::for_document(doc));
        delivery_cards(ui, s, doc);
    }
    /// The settings as they stand, when they can be exported (the Export page's button).
    pub fn request(&mut self, doc: &Composition) -> Option<Settings> {
        let s = self
            .settings
            .get_or_insert_with(|| Settings::for_document(doc));
        s.times(doc).is_ok().then(|| s.clone())
    }
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

/// A ready-made delivery: name, detail line, size, format, quality and what it is for.
type Delivery = (
    &'static str,
    String,
    (u32, u32),
    Codec,
    Quality,
    &'static str,
);
/// Even dimensions, as video needs.
fn even(v: u32) -> u32 {
    (v / 2 * 2).clamp(2, 8192)
}
/// A flag kept while the dialog is open (whether a row shows its manual fields).
fn flag(ui: &egui::Ui, name: &'static str) -> bool {
    ui.data(|d| d.get_temp(egui::Id::new(name)))
        .unwrap_or(false)
}
fn set_flag(ui: &egui::Ui, name: &'static str, on: bool) {
    ui.data_mut(|d| d.insert_temp(egui::Id::new(name), on));
}
/// Ready-made deliveries as cards: one click sets the format, quality and size.
fn delivery_cards(ui: &mut egui::Ui, s: &mut Settings, doc: &Composition) {
    use crate::design::{format_card, heading};
    let own = (even(doc.width), even(doc.height));
    let half = (even(doc.width / 2), even(doc.height / 2));
    // Ready-made deliveries: one click sets the format, quality and size.
    heading(ui, "DELIVERY");
    let deliveries: [Delivery; 7] = [
        (
            "Composition",
            format!("{} × {}", own.0, own.1),
            own,
            Codec::H264,
            Quality::High,
            "The composition's own size in H.264, for most uses",
        ),
        (
            "Web 1080p",
            "1920 × 1080".into(),
            (1920, 1080),
            Codec::H264,
            Quality::High,
            "Full HD for video sites",
        ),
        (
            "Web 4K",
            "3840 × 2160".into(),
            (3840, 2160),
            Codec::H264,
            Quality::High,
            "Ultra HD for video sites",
        ),
        (
            "Vertical",
            "1080 × 1920".into(),
            (1080, 1920),
            Codec::H264,
            Quality::High,
            "Upright video for phones",
        ),
        (
            "Square",
            "1080 × 1080".into(),
            (1080, 1080),
            Codec::H264,
            Quality::High,
            "Square video for feeds",
        ),
        (
            "Master",
            "HEVC 10-bit".into(),
            own,
            Codec::Hevc10,
            Quality::Master,
            "The best quality at the composition's size, for archiving or further editing; large files",
        ),
        (
            "Draft",
            format!("{} × {}", half.0, half.1),
            half,
            Codec::H264,
            Quality::Draft,
            "Half size and fast, to check timing",
        ),
    ];
    ui.horizontal_wrapped(|ui| {
        // Only the first card that matches is lit: when the composition is itself Full
        // HD, "Composition" and "Web 1080p" are the same delivery.
        let mut lit = false;
        for (name, detail, size, codec, quality, hint) in deliveries {
            let chosen =
                !lit && (s.width, s.height) == size && s.codec == codec && s.quality == quality;
            lit |= chosen;
            if format_card(
                ui,
                name,
                &detail,
                Some([size.0 as f32, size.1 as f32]),
                chosen,
            )
            .on_hover_text(hint)
            .clicked()
            {
                (s.width, s.height) = size;
                s.codec = codec;
                s.quality = quality;
                set_flag(ui, "video-custom-size", false);
            }
        }
    });
}
/// Everything beyond the delivery cards: format, quality, size, frame rate, range,
/// sound and the rest.
fn details(ui: &mut egui::Ui, s: &mut Settings, doc: &Composition) {
    use crate::design::{chip, heading};
    let own = (even(doc.width), even(doc.height));
    heading(ui, "FORMAT AND QUALITY");
    ui.horizontal_wrapped(|ui| {
        for (codec, label, hint) in [
            (Codec::H264, "H.264", "Plays everywhere"),
            (
                Codec::Hevc10,
                "HEVC 10-bit",
                "Smaller files and smoother gradients; needs a recent player",
            ),
        ] {
            if chip(ui, label, s.codec == codec)
                .on_hover_text(hint)
                .clicked()
            {
                s.codec = codec;
            }
        }
        ui.add_space(14.0);
        for (quality, label, hint) in [
            (Quality::Draft, "Draft", "Fast, small, visibly compressed"),
            (Quality::High, "High", "Recommended"),
            (Quality::Master, "Master", "Nearly lossless; large files"),
        ] {
            if chip(ui, label, s.quality == quality)
                .on_hover_text(hint)
                .clicked()
            {
                s.quality = quality;
            }
        }
    });
    heading(ui, "SIZE");
    let custom_size = flag(ui, "video-custom-size");
    ui.horizontal_wrapped(|ui| {
        for (label, size) in [
            ("Composition", own),
            ("720p", (1280, 720)),
            ("1080p", (1920, 1080)),
            ("4K", (3840, 2160)),
        ] {
            if chip(ui, label, !custom_size && (s.width, s.height) == size).clicked() {
                (s.width, s.height) = size;
                set_flag(ui, "video-custom-size", false);
            }
        }
        if chip(ui, "Custom", custom_size).clicked() {
            set_flag(ui, "video-custom-size", true);
        }
        if custom_size {
            ui.add(
                egui::DragValue::new(&mut s.width)
                    .range(2..=8192)
                    .speed(2.)
                    .suffix(" px"),
            );
            ui.label("×");
            ui.add(
                egui::DragValue::new(&mut s.height)
                    .range(2..=8192)
                    .speed(2.)
                    .suffix(" px"),
            );
        }
    });
    heading(ui, "FRAME RATE");
    ui.horizontal_wrapped(|ui| {
        let same = |a: Time, b: Time| a.checked_sub(b).is_ok_and(|delta| delta.ticks == 0);
        if chip(ui, "Composition", same(s.frame_rate, doc.frame_rate)).clicked() {
            s.frame_rate = doc.frame_rate;
        }
        for (n, d, label) in [
            (24, 1, "24"),
            (25, 1, "25"),
            (30000, 1001, "29.97"),
            (30, 1, "30"),
            (50, 1, "50"),
            (60000, 1001, "59.94"),
            (60, 1, "60"),
        ] {
            let value = Time::new(n, d).unwrap();
            // The composition's own rate is shown once, as Composition.
            if same(value, doc.frame_rate) {
                continue;
            }
            if chip(ui, label, same(s.frame_rate, value)).clicked() {
                s.frame_rate = value;
            }
        }
    });
    heading(ui, "RANGE");
    let last = i64::from(doc.frame_count) - 1;
    let whole = s.start == 0 && s.end == last;
    let custom_range = flag(ui, "video-custom-range") || !whole;
    ui.horizontal_wrapped(|ui| {
        if chip(ui, "Whole composition", !custom_range).clicked() {
            s.start = 0;
            s.end = last;
            set_flag(ui, "video-custom-range", false);
        }
        if chip(ui, "Part of it", custom_range).clicked() {
            set_flag(ui, "video-custom-range", true);
        }
        if custom_range {
            ui.label("Frames");
            ui.add(egui::DragValue::new(&mut s.start).range(0..=last));
            ui.label("to");
            ui.add(egui::DragValue::new(&mut s.end).range(0..=last));
        }
    });
    heading(ui, "SOUND");
    let mut include = !s.audio.mute;
    if ui
        .checkbox(&mut include, "Include the footage's sound")
        .on_hover_text(
            "Mix the sound of every clip feeding the output, at its timeline position and trims.",
        )
        .changed()
    {
        s.audio.mute = !include;
    }
    ui.horizontal(|ui| {
        ui.label("Music");
        let mut text = s
            .audio
            .music
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        if ui
            .add(
                egui::TextEdit::singleline(&mut text)
                    .hint_text("Optional audio file path")
                    .desired_width(330.),
            )
            .changed()
        {
            s.audio.music =
                (!text.trim().is_empty()).then(|| std::path::PathBuf::from(text.trim()));
        }
        ui.add_enabled(
            s.audio.music.is_some(),
            egui::DragValue::new(&mut s.audio.music_gain_db)
                .range(-60.0..=12.0)
                .speed(0.2)
                .suffix(" dB"),
        );
    });
    heading(ui, "MORE");
    ui.horizontal(|ui| {
        ui.label("Background");
        ui.color_edit_button_rgb(&mut s.matte)
            .on_hover_text("MP4 has no transparency: clear areas are filled with this colour. For alpha or scene-linear HDR, use Export image sequence.");
        ui.add_space(14.0);
        ui.checkbox(&mut s.allow_software, "Allow software encoding")
            .on_hover_text("Encoding uses the graphics card (NVENC or VA-API). Tick this to fall back to the processor when that is unavailable.");
    });
}
fn settings_panel(ui: &mut egui::Ui, s: &mut Settings, doc: &Composition) -> Option<Settings> {
    ui.set_width(622.0_f32.min(ui.ctx().content_rect().width() - 40.0));
    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
    delivery_cards(ui, s, doc);
    details(ui, s, doc);
    ui.add_space(6.0);
    ui.separator();
    let valid = s.times(doc);
    let mut export = false;
    ui.horizontal(|ui| {
        match &valid {
            Ok(times) => {
                let fps = s.frame_rate.seconds().unwrap_or(1.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{} × {}  ·  {}  ·  {:.1} s  ·  {} frames",
                        s.width,
                        s.height,
                        if (fps - fps.round()).abs() < 1e-9 {
                            format!("{fps:.0} fps")
                        } else {
                            format!("{fps:.3} fps")
                        },
                        times.len() as f64 / fps,
                        times.len()
                    ))
                    .color(ui.visuals().weak_text_color()),
                );
            }
            Err(e) => {
                ui.colored_label(egui::Color32::LIGHT_RED, e.to_string());
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            export = ui
                .add_enabled_ui(valid.is_ok(), |ui| {
                    crate::design::primary(ui, "Choose where to save")
                })
                .inner
                .clicked();
        });
    });
    ui.weak("The picture is fitted without stretching. Existing files are never replaced.");
    export.then(|| s.clone())
}
