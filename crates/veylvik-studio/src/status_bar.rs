//! Stable workspace telemetry; field sizes never depend on changing values.
use crate::Studio;
use eframe::egui;
fn timing(ms: Option<f64>) -> String {
    match ms {
        Some(ms) if ms >= 10_000. => format!("{:>7.2} s", ms / 1000.),
        Some(ms) => format!("{ms:>7.2} ms"),
        None => "  --.-- ms".into(),
    }
}
impl Studio {
    pub(super) fn status_bar(&self, ui: &mut egui::Ui) {
        let wide = ui.available_width() > 1100.;
        ui.horizontal(|ui| {
            ui.set_min_height(22.);
            ui.add_sized([100.,22.],egui::Label::new(egui::RichText::new(timing(self.last_render_ms)).monospace().size(12.)).truncate())
                .on_hover_text("Last completed preview render. This is render time, not playback frame rate.");
            if wide {
                ui.separator();
                ui.add_sized([205.,22.],egui::Label::new(egui::RichText::new(&self.renderer.adapter.name).weak()).truncate()).on_hover_text(&self.renderer.adapter.name);
            }
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui| {
                let (label,tip)=if let Some(server)=&self.control {
                    ("MCP ready",format!("Live control server is listening on {}.\nThis indicates availability, not an active client connection.",server.path().display()))
                }else if let Some(error)=&self.control_error {("MCP unavailable",error.clone())}else{("MCP off","Live control is disabled for this session.".into())};
                ui.add_sized([112.,22.],egui::Label::new(egui::RichText::new(label).color(if self.control.is_some(){egui::Color32::from_rgb(111,197,174)}else{ui.visuals().weak_text_color()})).truncate()).on_hover_text(tip);
                if wide {
                    ui.separator();
                    let doc=self.project.document.snapshot();
                    let fps=doc.frame_rate.ticks as f64/doc.frame_rate.rate as f64;
                    ui.add_sized([180.,22.],egui::Label::new(format!("{} × {} · {:.2} fps",doc.width,doc.height,fps)).truncate()).on_hover_text("Composition resolution and frame rate");
                }
                let message=if self.files.busy(){"File operation in progress…"}else if self.preview_pending{"Rendering preview…"}else{&self.status};
                ui.add_sized([ui.available_width().max(0.),22.],egui::Label::new(egui::RichText::new(message).color(if self.error {egui::Color32::LIGHT_RED}else{ui.visuals().weak_text_color()})).truncate()).on_hover_text(message);
            });
        });
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn render_time_keeps_precision_and_uses_seconds_for_long_renders() {
        assert_eq!(super::timing(Some(1.2)), "   1.20 ms");
        assert_eq!(super::timing(Some(123.456)), " 123.46 ms");
        assert_eq!(super::timing(Some(12345.)), "  12.35 s");
    }
}
