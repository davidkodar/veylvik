//! Inspector controls for keyers.
use super::{Inspect, InspectCx};
use crate::animation;
use eframe::egui;

impl Inspect for veylvik_document::nodes::ChromaKey {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.horizontal(|ui| {
            // Typical scene-linear colours of lit chroma screens; the key needs mainly the
            // dominant channel right, so these work for most footage.
            for (label, color) in [
                ("Green screen", [0.08, 0.45, 0.12, 1.0]),
                ("Blue screen", [0.05, 0.12, 0.45, 1.0]),
            ] {
                if ui.button(label).clicked() {
                    self.key_color = color;
                    cx.dirty = true;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Key colour (linear RGB)");
            crate::eyedropper::button(ui, cx, "key_color");
        });
        cx.dirty |= animation::color(ui, &mut self.key_color);
        ui.weak("Pixels leaning towards the key colour's dominant channel become transparent. Raise Tolerance to remove more of an unevenly lit screen, Softness for gentler edges, and use Despill against coloured fringes. Check the matte with View → Show alpha.");
    }
}
impl Inspect for veylvik_document::nodes::LumaKey {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        ui.weak("Transparent at or below Low, opaque at or above High. Set Low above High to key out bright areas instead, such as a white backdrop.");
    }
}
impl Inspect for veylvik_document::nodes::RefineMatte {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        ui.weak("Connect the footage to Image and a coarse subject mask to Matte. Raise Radius when the matte misses hair or edges by more than a few pixels; raise Detail to follow finer features. Decontaminate removes the old background's colour from the edges. Check the result with View → Show alpha.");
    }
}
