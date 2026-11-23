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
impl Inspect for veylvik_document::nodes::Rectangle {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.label("Fill (linear RGB and opacity)");
        cx.dirty |= animation::color(ui, &mut self.color);
    }
}

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
        ui.weak("Connect Emitter to text, a shape or footage and particles are born on it; Target and they fly onto that image; Field and Obstacle to steer and bounce them; Sprite to draw a picture as each particle; Scene to film them with a 3D camera. Depth spreads them towards and away from the viewer. Put a Bloom after it for glow.");
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
        ui.horizontal_wrapped(|ui| {
            cx.dirty |= ui
                .checkbox(&mut self.points, "Points")
                .on_hover_text("In a 3D scene: the same size on screen at any distance, as stars are points however near. Unticked, particles grow as they come close")
                .changed();
            cx.dirty |= ui
                .checkbox(&mut self.in_focus, "Always in focus")
                .on_hover_text("In a 3D scene: the camera's depth of field does not blur these particles")
                .changed();
        });
        // The colours: at birth, at the end of the life, and of a burst.
        egui::CollapsingHeader::new("Colours")
            .id_salt("particle-colours")
            .show(ui, |ui| {
                ui.weak("Linear RGBA; values above 1 glow");
                ui.label("At birth");
                cx.dirty |= animation::color(ui, &mut self.colour);
                ui.label("At the end of the life");
                cx.dirty |= animation::color(ui, &mut self.end_colour);
                ui.label("Burst");
                cx.dirty |= animation::color(ui, &mut self.burst_colour);
            });
        let obstacle: bool = ui
            .data(|d| d.get_temp(egui::Id::new("particles-obstacle")))
            .unwrap_or(false);
        if obstacle {
            cx.dirty |= ui
                .checkbox(&mut self.burst_on_impact, "Burst on the obstacle")
                .on_hover_text("A particle ends where it first touches the obstacle and its burst is thrown back off it, for splashes and sparks off a surface. Unticked, bursts come at the end of each life")
                .changed();
        }
        // Only an emitter image can move out from under its particles.
        let boxed: bool = ui
            .data(|d| d.get_temp(egui::Id::new("particles-box")))
            .unwrap_or(true);
        if !boxed {
            let name = |moments: u32| match moments {
                0 => "Off",
                1..=4 => "4 moments",
                5..=9 => "9 moments",
                _ => "16 moments",
            };
            ui.horizontal(|ui| {
                ui.label("Follow a moving emitter");
                egui::ComboBox::from_id_salt("particle-history")
                    .selected_text(name(self.history))
                    .show_ui(ui, |ui| {
                        for moments in [0, 4, 9, 16] {
                            if ui
                                .selectable_label(name(self.history) == name(moments), name(moments))
                                .clicked()
                            {
                                self.history = moments;
                                cx.dirty = true;
                            }
                        }
                    })
                    .response
                    .on_hover_text("Particles are born where the emitter was when they were born, so moving text or footage leaves them behind. The emitter is rendered that many extra times for every frame, at a half, a third or a quarter of its size; more moments give a smoother trail");
            });
        }
    }
}
