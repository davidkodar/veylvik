//! Inspector controls for filters and transforms.
#[allow(unused_imports)]
use super::{Inspect, InspectCx};
#[allow(unused_imports)]
use crate::{animation, media_controls, shape_animation};
use eframe::egui;

impl Inspect for veylvik_document::nodes::Blur {}
impl Inspect for veylvik_document::nodes::Morphology {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        {
            ui.label("Positive radius grows alpha; negative shrinks it.");
            ui.label("Square neighborhood, transparent borders, white mask output.");
        }
    }
}
impl Inspect for veylvik_document::nodes::Displace {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        {
            ui.label("Map red drives X, green drives Y.");
            ui.label("Mid-gray is neutral; alpha controls strength.");
        }
    }
}
impl Inspect for veylvik_document::nodes::Transform {}
impl Inspect for veylvik_document::nodes::Affine {}

impl Inspect for veylvik_document::nodes::Shadow {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        cx.dirty |= ui.checkbox(&mut self.follow_lighting, "Follow scene lighting")
            .on_hover_text("Project shadows using enabled area lights from the material connected to Lighting.").changed();
        if self.follow_lighting {
            cx.dirty |= crate::light_guides::source_selector(ui, &mut self.light);
            ui.weak("Enable area lights on the connected material. Environment reflections alone do not cast shadows.").on_hover_text("Distance lifts the object above a flat receiving plane. X/Y and softness remain fine adjustments.");
        }
    }
}
