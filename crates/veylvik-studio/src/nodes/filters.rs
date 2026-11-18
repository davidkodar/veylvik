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
        cx.dirty |= ui
            .checkbox(&mut self.fill, "Fill the frame")
            .on_hover_text("Enlarge the picture just enough that the bend leaves no empty rim. Leave it off when the picture must line up with footage, as after a camera solve")
            .changed();
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
impl Inspect for veylvik_document::nodes::GridWarp {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.weak("Drag the points in the viewer: the image under a point moves with it. View this node (Selected node) to see the mesh on the picture.");
        let (mut columns, mut rows) = (self.columns, self.rows);
        let sides = veylvik_document::nodes::GRID_SIDE;
        ui.horizontal(|ui| {
            ui.label("Points");
            ui.add(
                egui::DragValue::new(&mut columns)
                    .range(sides.clone())
                    .suffix(" across"),
            );
            ui.add(egui::DragValue::new(&mut rows).range(sides).suffix(" down"))
                .on_hover_text(
                    "A finer mesh allows smaller, more local moves; the warp so far is kept",
                );
        });
        if (columns, rows) != (self.columns, self.rows) {
            *self = self.resized(columns, rows);
            cx.dirty = true;
        }
        if ui
            .add_enabled(
                self.points.iter().any(|p| *p != [0.0; 2]),
                egui::Button::new("Reset the mesh"),
            )
            .on_hover_text("Put every point back, leaving the image as it was")
            .clicked()
        {
            self.points.fill([0.0; 2]);
            cx.dirty = true;
        }
    }
}
impl Inspect for veylvik_document::nodes::ShapeWarp {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.weak("Put the start shape round the part to move, then shape the end shape where it should go. The picture inside follows; Reach is how far the surroundings stretch.");
        let mut end = crate::handles::editing_end_shape(ui.ctx());
        ui.horizontal(|ui| {
            ui.label("Edit in the viewer");
            let was = end;
            ui.selectable_value(&mut end, false, "Start shape");
            ui.selectable_value(&mut end, true, "End shape");
            if end != was {
                crate::handles::edit_end_shape(ui.ctx(), end);
            }
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.to.keys.is_empty()
                        && self.from.keys.is_empty()
                        && self.to.commands != self.from.commands,
                    egui::Button::new("End shape like the start"),
                )
                .on_hover_text(
                    "Make the end shape a copy of the start shape, leaving the picture as it was",
                )
                .clicked()
            {
                self.to = self.from.clone();
                cx.dirty = true;
            }
        });
    }
}
