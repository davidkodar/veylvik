//! Small, pre-rendered loops keep browsing independent of the composition renderer.
use eframe::egui;
use std::collections::BTreeMap;

pub struct Gallery {
    textures: BTreeMap<(&'static str, bool), egui::TextureHandle>,
    playing: bool,
    before: bool,
    frame: usize,
    last_time: Option<f64>,
    elapsed: f64,
}
impl Default for Gallery {
    fn default() -> Self {
        Self {
            textures: Default::default(),
            playing: true,
            before: false,
            frame: 24,
            last_time: None,
            elapsed: 2.0,
        }
    }
}
fn bytes(id: &str, before: bool) -> &'static [u8] {
    match (id, before) {
        ("neon_outline", false) => include_bytes!("../assets/showcase/neon_outline-after.png"),
        ("neon_outline", true) => include_bytes!("../assets/showcase/neon_outline-before.png"),
        ("prismatic_glow", false) => include_bytes!("../assets/showcase/prismatic_glow-after.png"),
        ("prismatic_glow", true) => include_bytes!("../assets/showcase/prismatic_glow-before.png"),
        ("liquid_shimmer", false) => include_bytes!("../assets/showcase/liquid_shimmer-after.png"),
        ("liquid_shimmer", true) => include_bytes!("../assets/showcase/liquid_shimmer-before.png"),
        _ => unreachable!("only built-in gallery entries"),
    }
}
impl Gallery {
    fn tick(&mut self, ui: &egui::Ui) {
        let now = ui.input(|i| i.time);
        if self.playing {
            if let Some(last) = self.last_time {
                self.elapsed += (now - last).clamp(0.0, 0.25);
            }
            self.frame = ((self.elapsed * 12.0) as usize) % 48;
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(83));
        }
        self.last_time = Some(now);
    }
    fn image(
        &mut self,
        ui: &mut egui::Ui,
        id: &'static str,
        before: bool,
        width: f32,
    ) -> egui::Response {
        self.tick(ui);
        // Retain at most the three card loops and one selected comparison.
        self.textures
            .retain(|(key, is_before), _| !is_before || *key == id);
        let texture = self.textures.entry((id, before)).or_insert_with(|| {
            let image = image::load_from_memory(bytes(id, before))
                .expect("validated bundled preview")
                .to_rgba8();
            ui.ctx().load_texture(
                format!("{id}-{before}"),
                egui::ColorImage::from_rgba_unmultiplied(
                    [image.width() as usize, image.height() as usize],
                    image.as_raw(),
                ),
                egui::TextureOptions::LINEAR,
            )
        });
        let x = (self.frame % 8) as f32 / 8.0;
        let y = (self.frame / 8) as f32 / 6.0;
        ui.add(
            egui::Image::new((texture.id(), egui::vec2(width, width * 9.0 / 16.0)))
                .uv(egui::Rect::from_min_max(
                    egui::pos2(x, y),
                    egui::pos2(x + 1.0 / 8.0, y + 1.0 / 6.0),
                ))
                .sense(egui::Sense::click()),
        )
    }
    pub fn card(&mut self, ui: &mut egui::Ui, id: &'static str) -> egui::Response {
        self.image(ui, id, false, 160.0)
    }
    pub fn detail(&mut self, ui: &mut egui::Ui, id: &'static str) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.before, true, "Before");
            ui.selectable_value(&mut self.before, false, "After");
            if ui
                .button(if self.playing { "Pause" } else { "Play" })
                .clicked()
            {
                self.playing = !self.playing;
            }
            ui.weak("Rendered preview · 12 fps");
        });
        self.image(ui, id, self.before, ui.available_width().min(560.0));
        if ui
            .add(egui::Slider::new(&mut self.frame, 0..=47).text("Frame"))
            .changed()
        {
            self.playing = false;
            self.elapsed = self.frame as f64 / 12.0;
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn bundled_loops_have_complete_matching_frame_grids() {
        for entry in veylvik_document::presets::builtin::ALL {
            for before in [false, true] {
                let image = image::load_from_memory(super::bytes(entry.id, before)).unwrap();
                assert_eq!((image.width(), image.height()), (2560, 1080));
            }
        }
    }
}
