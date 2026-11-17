//! Inspector controls for filters and transforms.
#[allow(unused_imports)]
use super::{Inspect, InspectCx};
#[allow(unused_imports)]
use crate::{animation, media_controls, shape_animation};
use eframe::egui;

impl Inspect for veylvik_document::nodes::Blur {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.horizontal(|ui| {
            ui.label("Filter");
            egui::ComboBox::from_id_salt("blur-filter")
                .selected_text(self.filter.label())
                .show_ui(ui, |ui| {
                    for choice in veylvik_document::nodes::BlurFilter::ALL {
                        cx.dirty |= ui
                            .selectable_value(&mut self.filter, choice, choice.label())
                            .changed();
                    }
                });
        });
        ui.label("Gaussian is smooth and round; box is fastest but squares bright points.");
    }
}
impl Inspect for veylvik_document::nodes::LensBlur {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        ui.label("Round bokeh: lights brighter than Threshold bloom into discs.");
        ui.label("Animate Radius for a rack focus.");
    }
}
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

impl Inspect for veylvik_document::nodes::ContactShadow {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        ui.weak("Merge behind the layer so it sits on the surface: darkest at the edge, softer within Reach. Connect a material to Lighting to lean away from its lights; otherwise Angle sets the direction.");
    }
}
impl Inspect for veylvik_document::nodes::Shadow {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        cx.dirty |= crate::text_effects::shadow_controls(ui, self);
    }
}
impl Inspect for veylvik_document::nodes::LensDistortion {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.horizontal(|ui| {
            cx.dirty |= ui
                .selectable_value(&mut self.redistort, false, "Remove the lens")
                .on_hover_text("Straighten footage: lines bent by the lens come out straight")
                .changed();
            cx.dirty |= ui
                .selectable_value(&mut self.redistort, true, "Put the lens on")
                .on_hover_text(
                    "Bend 3D or graphics made in a straight frame the way the footage's lens does",
                )
                .changed();
        });
        ui.weak("Negative k1 is barrel distortion (wide lenses), positive is pincushion; k2 shapes the corners. Solve camera measures both and adds this node after Render scene.");
    }
}
impl Inspect for veylvik_document::nodes::Bloom {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        ui.weak("Bright light spreads as glow over its surroundings. Put it after the merge so titles light the background too; raise Threshold to keep it to the hottest highlights.");
    }
}
impl Inspect for veylvik_document::nodes::Shader {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        cx.dirty |= crate::shader_editor::show(ui, cx.node_id, self);
    }
}
