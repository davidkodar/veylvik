use eframe::egui;
use veylvik_io::text::{Alignment, Direction, OutlineRequest};
pub struct Controls {
    pub open: bool,
    pub pending: bool,
    pub position: Option<veylvik_document::NodePosition>,
    text: String,
    size: f32,
    origin: [f32; 2],
    rtl: bool,
    language: String,
    font_index: u32,
    alignment: Alignment,
    line_height: f32,
    /// Showing the manual fields for size and line spacing.
    custom: [bool; 2],
}
impl Default for Controls {
    fn default() -> Self {
        Self {
            open: false,
            pending: false,
            position: None,
            text: "Veylvik".into(),
            size: 96.0,
            origin: [40.0, 120.0],
            rtl: false,
            language: String::new(),
            font_index: 0,
            alignment: Alignment::Left,
            line_height: 120.0,
            custom: [false; 2],
        }
    }
}
impl Controls {
    pub fn open_at(&mut self, position: veylvik_document::NodePosition) {
        self.position = Some(position);
        self.open = true;
    }
    pub fn show(&mut self, ctx: &egui::Context, enabled: bool) -> Option<OutlineRequest> {
        let mut request = None;
        let mut open = self.open;
        if open {
            egui::Window::new("Outline text")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center())
                .show(ctx, |ui| {
                    use crate::design::{chip, heading};
                    ui.set_width(622.0_f32.min(ctx.content_rect().width() - 40.0));
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                    ui.add_enabled_ui(enabled, |ui| {
                        heading(ui, "WORDS");
                        ui.add(
                            egui::TextEdit::multiline(&mut self.text)
                                .desired_rows(3)
                                .desired_width(f32::INFINITY)
                                .char_limit(4096),
                        );
                        // Line spacing is a share of the size, so it follows a new size.
                        let spacing = self.line_height / self.size.max(1.0);
                        heading(ui, "SIZE");
                        ui.horizontal_wrapped(|ui| {
                            for (label, size) in [
                                ("Small", 48.0),
                                ("Medium", 96.0),
                                ("Large", 160.0),
                                ("Huge", 280.0),
                            ] {
                                if chip(ui, label, !self.custom[0] && self.size == size)
                                    .on_hover_text(format!("{size} pixels"))
                                    .clicked()
                                {
                                    self.size = size;
                                    self.line_height = size * spacing;
                                    self.custom[0] = false;
                                }
                            }
                            if chip(ui, "Custom", self.custom[0]).clicked() {
                                self.custom[0] = true;
                            }
                            if self.custom[0]
                                && ui
                                    .add(
                                        egui::DragValue::new(&mut self.size)
                                            .range(1.0..=4096.0)
                                            .suffix(" px"),
                                    )
                                    .changed()
                            {
                                self.line_height = self.size * spacing;
                            }
                        });
                        heading(ui, "ALIGNMENT AND LINE SPACING");
                        ui.horizontal_wrapped(|ui| {
                            for (label, value) in [
                                ("Left", Alignment::Left),
                                ("Centre", Alignment::Center),
                                ("Right", Alignment::Right),
                            ] {
                                if chip(ui, label, self.alignment == value).clicked() {
                                    self.alignment = value;
                                }
                            }
                            ui.add_space(14.0);
                            for (label, factor) in
                                [("Tight", 1.0_f32), ("Normal", 1.25), ("Loose", 1.6)]
                            {
                                let chosen = !self.custom[1] && (spacing - factor).abs() < 0.005;
                                if chip(ui, label, chosen)
                                    .on_hover_text(format!("{factor} times the size between lines"))
                                    .clicked()
                                {
                                    self.line_height = self.size * factor;
                                    self.custom[1] = false;
                                }
                            }
                            if chip(ui, "Custom", self.custom[1]).clicked() {
                                self.custom[1] = true;
                            }
                            if self.custom[1] {
                                ui.add(
                                    egui::DragValue::new(&mut self.line_height)
                                        .range(1.0..=4096.0)
                                        .suffix(" px"),
                                );
                            }
                        });
                        ui.collapsing("More", |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Position of the first line");
                                for value in &mut self.origin {
                                    ui.add(
                                        egui::DragValue::new(value)
                                            .range(-32768.0..=32768.0)
                                            .suffix(" px"),
                                    );
                                }
                            });
                            ui.checkbox(&mut self.rtl, "Right-to-left writing");
                            ui.horizontal(|ui| {
                                ui.label("Language tag");
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.language)
                                        .desired_width(120.0)
                                        .hint_text("Optional"),
                                );
                                ui.add_space(12.0);
                                ui.label("Face in a font collection");
                                ui.add(egui::DragValue::new(&mut self.font_index));
                            });
                        });
                        ui.add_space(6.0);
                        ui.separator();
                        let valid = !self.text.trim().is_empty() && self.text.len() <= 4096;
                        ui.horizontal(|ui| {
                            if valid {
                                ui.label(
                                    egui::RichText::new(
                                        "Makes editable shapes; the font is not needed afterwards",
                                    )
                                    .color(ui.visuals().weak_text_color()),
                                );
                            } else {
                                ui.colored_label(
                                    egui::Color32::LIGHT_RED,
                                    "Type some words, up to 4096 bytes.",
                                );
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_enabled_ui(valid, |ui| {
                                            crate::design::primary(ui, "Choose a font")
                                        })
                                        .inner
                                        .clicked()
                                    {
                                        request = Some(OutlineRequest {
                                            font: String::new(),
                                            text: self.text.clone(),
                                            font_index: self.font_index,
                                            font_size: self.size,
                                            origin: self.origin,
                                            alignment: self.alignment,
                                            line_height: Some(self.line_height),
                                            direction: if self.rtl {
                                                Direction::RightToLeft
                                            } else {
                                                Direction::LeftToRight
                                            },
                                            language: if self.language.trim().is_empty() {
                                                None
                                            } else {
                                                Some(self.language.trim().into())
                                            },
                                        });
                                    }
                                },
                            );
                        });
                    });
                });
        }
        self.open = open && request.is_none();
        request
    }
}
