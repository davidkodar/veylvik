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
/// The line while a preview render holds the viewer up: seconds so far, and the last
/// render's time as the likely whole when it was long enough to be worth waiting on.
fn rendering_message(seconds: f64, last_ms: Option<f64>) -> String {
    match last_ms {
        Some(ms) if ms >= 1000. => {
            format!(
                "Rendering preview… {seconds:.1} s of about {:.1} s",
                ms / 1000.
            )
        }
        _ => format!("Rendering preview… {seconds:.1} s"),
    }
}
impl Studio {
    pub(super) fn status_bar(&mut self, ui: &mut egui::Ui) {
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
                ui.add_sized([112.,22.],egui::Label::new(egui::RichText::new(label).color(if self.control.is_some(){if ui.visuals().dark_mode {egui::Color32::from_rgb(111,197,174)}else{egui::Color32::from_rgb(25,100,75)}}else{ui.visuals().weak_text_color()})).truncate()).on_hover_text(tip);
                if self.render_queue.busy() || self.render_queue.pending() > 0 {
                    let label = if self.render_queue.busy() { "Queue: rendering".to_owned() } else { format!("Queue: {} pending", self.render_queue.pending()) };
                    if ui.add_sized([140.,22.],egui::Button::new(label)).on_hover_text("Open render queue, progress and cancellation").clicked() { self.render_queue.open = true; }
                }
                if wide {
                    ui.separator();
                    let doc=self.project.document.snapshot();
                    let fps=doc.frame_rate.ticks as f64/doc.frame_rate.rate as f64;
                    ui.add_sized([180.,22.],egui::Label::new(format!("{} × {} · {:.2} fps",doc.width,doc.height,fps)).truncate()).on_hover_text("Composition resolution and frame rate");
                }
                // Only a render that holds the viewer up for a noticeable time is named, so playback
                // and quick scrubs (one render per frame) keep a steady message instead of flickering.
                const SLOW:std::time::Duration=std::time::Duration::from_millis(250);
                let slow=self.preview_pending&&self.preview_started.is_some_and(|t|t.elapsed()>=SLOW);
                if self.preview_pending&&!slow {ui.ctx().request_repaint_after(SLOW);}
                let rendering=slow.then(||{
                    // Long renders (a path traced scene) count up, against how long the last one took.
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                    rendering_message(self.preview_started.map_or(0.,|t|t.elapsed().as_secs_f64()),self.last_render_ms)
                });
                let message=if self.files.busy(){"File operation in progress…"}else if let Some(r)=&rendering{r}else{&self.status};
                // The line shows the latest message; a click opens the ones before it.
                let unseen = self.log.unseen;
                let shown = if unseen > 0 && !self.error { format!("{message}  ·  {unseen} earlier error(s)") } else { message.to_owned() };
                let colour = if self.error || unseen > 0 { egui::Color32::LIGHT_RED } else { ui.visuals().weak_text_color() };
                if ui.add_sized([ui.available_width().max(0.),22.],egui::Label::new(egui::RichText::new(shown).color(colour)).truncate().sense(egui::Sense::click())).on_hover_text(format!("{message}\nClick to see earlier messages")).clicked() {
                    self.log.messages = true;
                }
            });
        });
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn a_long_render_counts_up_against_the_last_ones_time() {
        assert_eq!(
            super::rendering_message(1.25, Some(3093.0)),
            "Rendering preview… 1.2 s of about 3.1 s"
        );
        assert_eq!(
            super::rendering_message(0.4, Some(40.0)),
            "Rendering preview… 0.4 s"
        );
        assert_eq!(
            super::rendering_message(0.4, None),
            "Rendering preview… 0.4 s"
        );
    }
    #[test]
    fn render_time_keeps_precision_and_uses_seconds_for_long_renders() {
        assert_eq!(super::timing(Some(1.2)), "   1.20 ms");
        assert_eq!(super::timing(Some(123.456)), " 123.46 ms");
        assert_eq!(super::timing(Some(12345.)), "  12.35 s");
    }
}
