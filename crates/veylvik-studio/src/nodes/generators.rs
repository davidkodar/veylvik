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
