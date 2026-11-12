//! Small, pre-rendered loops keep browsing independent of the composition renderer.
use eframe::egui;
use std::collections::{BTreeMap, VecDeque};

pub struct Gallery {
    textures: BTreeMap<(&'static str, bool), egui::TextureHandle>,
    recent: VecDeque<(&'static str, bool)>,
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
            recent: Default::default(),
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
        ("oak_light", false) => include_bytes!("../assets/showcase/oak_light-after.png"),
        ("oak_light", true) => include_bytes!("../assets/showcase/oak_light-before.png"),
        ("oak_dark", false) => include_bytes!("../assets/showcase/oak_dark-after.png"),
        ("oak_dark", true) => include_bytes!("../assets/showcase/oak_dark-before.png"),
        ("marble", false) => include_bytes!("../assets/showcase/marble-after.png"),
        ("marble", true) => include_bytes!("../assets/showcase/marble-before.png"),
        ("brick_light", false) => include_bytes!("../assets/showcase/brick_light-after.png"),
        ("brick_light", true) => include_bytes!("../assets/showcase/brick_light-before.png"),
        ("brick_red", false) => include_bytes!("../assets/showcase/brick_red-after.png"),
        ("brick_red", true) => include_bytes!("../assets/showcase/brick_red-before.png"),
        ("concrete", false) => include_bytes!("../assets/showcase/concrete-after.png"),
        ("concrete", true) => include_bytes!("../assets/showcase/concrete-before.png"),
        ("paper_clean", false) => include_bytes!("../assets/showcase/paper_clean-after.png"),
        ("paper_clean", true) => include_bytes!("../assets/showcase/paper_clean-before.png"),
        ("paper_aged", false) => include_bytes!("../assets/showcase/paper_aged-after.png"),
        ("paper_aged", true) => include_bytes!("../assets/showcase/paper_aged-before.png"),
        ("leather_red", false) => include_bytes!("../assets/showcase/leather_red-after.png"),
        ("leather_red", true) => include_bytes!("../assets/showcase/leather_red-before.png"),
        ("leather_brown", false) => include_bytes!("../assets/showcase/leather_brown-after.png"),
        ("leather_brown", true) => include_bytes!("../assets/showcase/leather_brown-before.png"),
        ("leather_tan", false) => include_bytes!("../assets/showcase/leather_tan-after.png"),
        ("leather_tan", true) => include_bytes!("../assets/showcase/leather_tan-before.png"),
        ("leather_dark", false) => include_bytes!("../assets/showcase/leather_dark-after.png"),
        ("leather_dark", true) => include_bytes!("../assets/showcase/leather_dark-before.png"),
        ("leather_black", false) => include_bytes!("../assets/showcase/leather_black-after.png"),
        ("leather_black", true) => include_bytes!("../assets/showcase/leather_black-before.png"),
        ("surface_reflection", false) => {
            include_bytes!("../assets/showcase/surface_reflection-after.png")
        }
        ("surface_reflection", true) => {
            include_bytes!("../assets/showcase/surface_reflection-before.png")
        }
        ("clear_glass", false) => include_bytes!("../assets/showcase/clear_glass-after.png"),
        ("clear_glass", true) => include_bytes!("../assets/showcase/clear_glass-before.png"),
        ("frosted_glass_material", false) => {
            include_bytes!("../assets/showcase/frosted_glass_material-after.png")
        }
        ("frosted_glass_material", true) => {
            include_bytes!("../assets/showcase/frosted_glass_material-before.png")
        }
        ("sun_glints", false) => include_bytes!("../assets/showcase/sun_glints-after.png"),
        ("sun_glints", true) => include_bytes!("../assets/showcase/sun_glints-before.png"),
        ("metal_patina", false) => include_bytes!("../assets/showcase/metal_patina-after.png"),
        ("metal_patina", true) => include_bytes!("../assets/showcase/metal_patina-before.png"),

        ("metallic_shine", false) => include_bytes!("../assets/showcase/metallic_shine-after.png"),
        ("metallic_shine", true) => include_bytes!("../assets/showcase/metallic_shine-before.png"),
        ("gold_text", false) => include_bytes!("../assets/showcase/gold_text-after.png"),
        ("gold_text", true) => include_bytes!("../assets/showcase/gold_text-before.png"),
        ("silver_text", false) => include_bytes!("../assets/showcase/silver_text-after.png"),
        ("silver_text", true) => include_bytes!("../assets/showcase/silver_text-before.png"),
        ("aluminium_text", false) => include_bytes!("../assets/showcase/aluminium_text-after.png"),
        ("aluminium_text", true) => include_bytes!("../assets/showcase/aluminium_text-before.png"),
        ("bronze_text", false) => include_bytes!("../assets/showcase/bronze_text-after.png"),
        ("bronze_text", true) => include_bytes!("../assets/showcase/bronze_text-before.png"),
        ("phosphor_text", false) => include_bytes!("../assets/showcase/phosphor_text-after.png"),
        ("phosphor_text", true) => include_bytes!("../assets/showcase/phosphor_text-before.png"),
        ("wood_text", false) => include_bytes!("../assets/showcase/wood_text-after.png"),
        ("wood_text", true) => include_bytes!("../assets/showcase/wood_text-before.png"),
        ("light_smoke", false) => include_bytes!("../assets/showcase/light_smoke-after.png"),
        ("light_smoke", true) => include_bytes!("../assets/showcase/light_smoke-before.png"),
        ("dense_smoke", false) => include_bytes!("../assets/showcase/dense_smoke-after.png"),
        ("dense_smoke", true) => include_bytes!("../assets/showcase/dense_smoke-before.png"),
        ("fog_layer", false) => include_bytes!("../assets/showcase/fog_layer-after.png"),
        ("fog_layer", true) => include_bytes!("../assets/showcase/fog_layer-before.png"),
        ("drifting_fog", false) => include_bytes!("../assets/showcase/drifting_fog-after.png"),
        ("drifting_fog", true) => include_bytes!("../assets/showcase/drifting_fog-before.png"),
        ("clouds", false) => include_bytes!("../assets/showcase/clouds-after.png"),
        ("clouds", true) => include_bytes!("../assets/showcase/clouds-before.png"),

        ("neon_outline", false) => include_bytes!("../assets/showcase/neon_outline-after.png"),
        ("neon_outline", true) => include_bytes!("../assets/showcase/neon_outline-before.png"),
        ("prismatic_glow", false) => include_bytes!("../assets/showcase/prismatic_glow-after.png"),
        ("prismatic_glow", true) => include_bytes!("../assets/showcase/prismatic_glow-before.png"),
        ("liquid_shimmer", false) => include_bytes!("../assets/showcase/liquid_shimmer-after.png"),
        ("liquid_shimmer", true) => include_bytes!("../assets/showcase/liquid_shimmer-before.png"),
        ("soft_bloom", false) => include_bytes!("../assets/showcase/soft_bloom-after.png"),
        ("soft_bloom", true) => include_bytes!("../assets/showcase/soft_bloom-before.png"),
        ("anamorphic_streaks", false) => {
            include_bytes!("../assets/showcase/anamorphic_streaks-after.png")
        }
        ("anamorphic_streaks", true) => {
            include_bytes!("../assets/showcase/anamorphic_streaks-before.png")
        }
        ("dream_focus", false) => include_bytes!("../assets/showcase/dream_focus-after.png"),
        ("dream_focus", true) => include_bytes!("../assets/showcase/dream_focus-before.png"),
        ("edge_engrave", false) => include_bytes!("../assets/showcase/edge_engrave-after.png"),
        ("edge_engrave", true) => include_bytes!("../assets/showcase/edge_engrave-before.png"),
        ("chromatic_echo", false) => include_bytes!("../assets/showcase/chromatic_echo-after.png"),
        ("chromatic_echo", true) => include_bytes!("../assets/showcase/chromatic_echo-before.png"),
        ("radial_echo", false) => include_bytes!("../assets/showcase/radial_echo-after.png"),
        ("radial_echo", true) => include_bytes!("../assets/showcase/radial_echo-before.png"),
        ("duotone_print", false) => include_bytes!("../assets/showcase/duotone_print-after.png"),
        ("duotone_print", true) => include_bytes!("../assets/showcase/duotone_print-before.png"),
        ("silver_relief", false) => include_bytes!("../assets/showcase/silver_relief-after.png"),
        ("silver_relief", true) => include_bytes!("../assets/showcase/silver_relief-before.png"),
        ("ink_bleed", false) => include_bytes!("../assets/showcase/ink_bleed-after.png"),
        ("ink_bleed", true) => include_bytes!("../assets/showcase/ink_bleed-before.png"),
        ("light_sweep", false) => include_bytes!("../assets/showcase/light_sweep-after.png"),
        ("light_sweep", true) => include_bytes!("../assets/showcase/light_sweep-before.png"),
        ("soft_vignette", false) => include_bytes!("../assets/showcase/soft_vignette-after.png"),
        ("soft_vignette", true) => include_bytes!("../assets/showcase/soft_vignette-before.png"),
        ("film_grain", false) => include_bytes!("../assets/showcase/film_grain-after.png"),
        ("film_grain", true) => include_bytes!("../assets/showcase/film_grain-before.png"),
        ("frosted_glass", false) => include_bytes!("../assets/showcase/frosted_glass-after.png"),
        ("frosted_glass", true) => include_bytes!("../assets/showcase/frosted_glass-before.png"),
        ("heat_haze", false) => include_bytes!("../assets/showcase/heat_haze-after.png"),
        ("heat_haze", true) => include_bytes!("../assets/showcase/heat_haze-before.png"),
        ("underwater", false) => include_bytes!("../assets/showcase/underwater-after.png"),
        ("underwater", true) => include_bytes!("../assets/showcase/underwater-before.png"),
        ("scanlines", false) => include_bytes!("../assets/showcase/scanlines-after.png"),
        ("scanlines", true) => include_bytes!("../assets/showcase/scanlines-before.png"),
        ("hologram", false) => include_bytes!("../assets/showcase/hologram-after.png"),
        ("hologram", true) => include_bytes!("../assets/showcase/hologram-before.png"),
        ("glitch_slices", false) => include_bytes!("../assets/showcase/glitch_slices-after.png"),
        ("glitch_slices", true) => include_bytes!("../assets/showcase/glitch_slices-before.png"),
        ("orbit_echo", false) => include_bytes!("../assets/showcase/orbit_echo-after.png"),
        ("orbit_echo", true) => include_bytes!("../assets/showcase/orbit_echo-before.png"),
        ("spotlight", false) => include_bytes!("../assets/showcase/spotlight-after.png"),
        ("spotlight", true) => include_bytes!("../assets/showcase/spotlight-before.png"),
        ("dust_motes", false) => include_bytes!("../assets/showcase/dust_motes-after.png"),
        ("dust_motes", true) => include_bytes!("../assets/showcase/dust_motes-before.png"),
        ("fireflies", false) => include_bytes!("../assets/showcase/fireflies-after.png"),
        ("fireflies", true) => include_bytes!("../assets/showcase/fireflies-before.png"),
        ("embers", false) => include_bytes!("../assets/showcase/embers-after.png"),
        ("embers", true) => include_bytes!("../assets/showcase/embers-before.png"),
        ("snowfall", false) => include_bytes!("../assets/showcase/snowfall-after.png"),
        ("snowfall", true) => include_bytes!("../assets/showcase/snowfall-before.png"),
        ("starfield", false) => include_bytes!("../assets/showcase/starfield-after.png"),
        ("starfield", true) => include_bytes!("../assets/showcase/starfield-before.png"),
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
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, width * 9.0 / 16.0), egui::Sense::click());
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let key = (id, before);
        self.recent.retain(|k| *k != key);
        self.recent.push_back(key);
        while self.recent.len() > 6 {
            if let Some(old) = self.recent.pop_front() {
                self.textures.remove(&old);
            }
        }
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
        ui.painter().image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(x, y), egui::pos2(x + 1.0 / 8.0, y + 1.0 / 6.0)),
            egui::Color32::WHITE,
        );
        response
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
    fn browsing_many_effects_bounds_textures_and_skips_clipped_cards() {
        use super::*;
        let ctx = egui::Context::default();
        let mut gallery = Gallery::default();
        for entry in veylvik_document::presets::builtin::ALL {
            let _ = ctx.run_ui(
                egui::RawInput {
                    max_texture_side: Some(8192),
                    ..Default::default()
                },
                |ui| {
                    gallery.card(ui, entry.id);
                },
            );
            assert!(gallery.textures.contains_key(&(entry.id, false)));
            assert!(gallery.textures.len() <= 6);
            assert_eq!(gallery.textures.len(), gallery.recent.len());
        }
        assert_eq!(gallery.textures.len(), 6);
        assert!(!gallery.textures.contains_key(&("prismatic_glow", false)));
        let _ = ctx.run_ui(
            egui::RawInput {
                max_texture_side: Some(8192),
                ..Default::default()
            },
            |ui| {
                ui.set_clip_rect(egui::Rect::NOTHING);
                gallery.card(ui, "prismatic_glow");
            },
        );
        assert!(!gallery.textures.contains_key(&("prismatic_glow", false)));
    }

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
