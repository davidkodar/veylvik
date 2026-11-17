//! Inspector controls for generator nodes.
#[allow(unused_imports)]
use super::{Inspect, InspectCx};
#[allow(unused_imports)]
use crate::{animation, media_controls, shape_animation};
use eframe::egui;

impl Inspect for veylvik_document::nodes::Solid {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Solid { color } = self;
        {
            ui.horizontal(|ui| {
                ui.label("Linear RGB and alpha");
                crate::eyedropper::button(ui, cx, "color");
            });
            cx.dirty |= animation::color(ui, color);
        }
    }
}
impl Inspect for veylvik_document::nodes::Gradient {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Gradient { mode, .. } = self;
        {
            egui::ComboBox::from_id_salt("gradient-mode")
                .selected_text(mode.label())
                .show_ui(ui, |ui| {
                    for choice in veylvik_document::GradientMode::ALL {
                        cx.dirty |= ui.selectable_value(mode, choice, choice.label()).changed();
                    }
                });
            ui.label("Use Colorize for colors or Channel mask for coverage.");
        }
    }
}
impl Inspect for veylvik_document::nodes::Noise {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Noise { seed, .. } = self;
        {
            ui.horizontal(|ui| {
                ui.label("Seed");
                cx.dirty |= ui.add(egui::DragValue::new(seed)).changed();
            });
        }
    }
}
impl Inspect for veylvik_document::nodes::FractalNoise {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::FractalNoise { seed, .. } = self;
        {
            ui.horizontal(|ui| {
                ui.label("Seed");
                cx.dirty |= ui.add(egui::DragValue::new(seed)).changed();
            });
        }
    }
}
impl Inspect for veylvik_document::nodes::Fire {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Fire { seed, .. } = self;
        {
            ui.horizontal(|ui| {
                ui.label("Seed");
                cx.dirty |= ui.add(egui::DragValue::new(seed)).changed();
            });
        }
    }
}
impl Inspect for veylvik_document::nodes::Atmosphere {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Atmosphere { kind, seed, .. } = self;
        {
            cx.dirty |= ui.add(egui::DragValue::new(seed).prefix("Seed ")).changed();
            egui::ComboBox::from_id_salt("Atmosphere-kind")
                .selected_text(kind.label())
                .show_ui(ui, |ui| {
                    for choice in veylvik_document::AtmosphereKind::ALL {
                        cx.dirty |= ui.selectable_value(kind, choice, choice.label()).changed();
                    }
                });
        }
    }
}
impl Inspect for veylvik_document::nodes::Ellipse {}

impl Inspect for veylvik_document::nodes::Dust {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.horizontal(|ui| {
            ui.label("Seed");
            cx.dirty |= ui.add(egui::DragValue::new(&mut self.seed)).changed();
        });
    }
}

impl Inspect for veylvik_document::nodes::Drip {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.horizontal(|ui| {
            ui.label("Seed");
            cx.dirty |= ui.add(egui::DragValue::new(&mut self.seed)).changed();
        });
    }
}
impl Inspect for veylvik_document::nodes::Particles {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.weak("Connect Emitter to text, a shape or footage and particles are born on it; Target and they fly onto that image; Field and Obstacle to steer and bounce them; Sprite to draw a picture as each particle. Put a Bloom after it for glow.");
        // The frame's size, left here by the inspector for looks that fill the frame.
        let [width, height]: [u32; 2] = ui
            .data(|d| d.get_temp(egui::Id::new("particles-frame")))
            .unwrap_or([1920, 1080]);
        ui.menu_button("Start from a look", |ui| {
            for look in &veylvik_presets::particles::LOOKS {
                if ui.button(look.name).on_hover_text(look.hint).clicked() {
                    look.apply(self, width, height);
                    cx.dirty = true;
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text(
            "Set every setting for a ready-made effect, scaled to this frame; then adjust",
        );
    }
}
