//! Inspector controls for procedural material nodes.
#[allow(unused_imports)]
use super::{Inspect, InspectCx};
#[allow(unused_imports)]
use crate::{animation, media_controls, shape_animation};
use eframe::egui;

impl Inspect for veylvik_document::nodes::Material {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Material { kind, .. } = self;
        {
            egui::ComboBox::from_id_salt("Material-kind")
                .selected_text(kind.label())
                .show_ui(ui, |ui| {
                    for choice in veylvik_document::MaterialKind::ALL {
                        cx.dirty |= ui.selectable_value(kind, choice, choice.label()).changed();
                    }
                });
        }
    }
}
impl Inspect for veylvik_document::nodes::Patina {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Patina { kind, seed, .. } = self;
        {
            egui::ComboBox::from_id_salt("patina-kind")
                .selected_text(kind.label())
                .show_ui(ui, |ui| {
                    for choice in veylvik_document::PatinaKind::ALL {
                        cx.dirty |= ui.selectable_value(kind, choice, choice.label()).changed();
                    }
                });
            cx.dirty |= ui.add(egui::DragValue::new(seed).prefix("Seed ")).changed();
        }
    }
}
impl Inspect for veylvik_document::nodes::MetallicShine {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        {
            ui.label("Apply to isolated text or a logo before adding a background.");
            ui.label("Animate X/Y to sweep; alpha stays unchanged.");
        }
    }
}
impl Inspect for veylvik_document::nodes::SunGlints {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::SunGlints { seed, .. } = self;
        {
            cx.dirty |= ui.add(egui::DragValue::new(seed).prefix("Seed ")).changed();
        }
    }
}
impl Inspect for veylvik_document::nodes::Glass {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        {
            ui.label(
                "Use isolated shape alpha and the scene behind it. Roughness makes frosted glass.",
            );
        }
    }
}
impl Inspect for veylvik_document::nodes::SurfaceReflection {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::SurfaceReflection { kind, .. } = self;
        {
            egui::ComboBox::from_id_salt("reflection-kind")
                .selected_text(kind.label())
                .show_ui(ui, |ui| {
                    for choice in veylvik_document::ReflectionKind::ALL {
                        cx.dirty |= ui.selectable_value(kind, choice, choice.label()).changed();
                    }
                });
            ui.label("Connect any image or animated branch to Reflection.");
        }
    }
}
