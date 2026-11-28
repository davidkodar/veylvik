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
        ui.horizontal(|ui| {
            ui.add_sized([88., 22.], egui::Label::new("Show"));
            egui::ComboBox::from_id_salt("key-show")
                .selected_text(self.show.label())
                .show_ui(ui, |ui| {
                    for view in veylvik_document::nodes::KeyView::ALL {
                        cx.dirty |= ui.selectable_value(&mut self.show, view, view.label()).changed();
                    }
                })
                .response
                .on_hover_text("Matte shows what the key keeps in white and removes in black. Set it back to Keyed picture when the matte is right");
        });
        if self.show == veylvik_document::nodes::KeyView::Matte {
            ui.label("The matte shows where the viewer is on this node: press Selected node below. Set Show back to Keyed picture when the matte is right.");
        }
        ui.weak("Pixels leaning towards the key colour's dominant channel become transparent. Raise Tolerance to remove more of an unevenly lit screen, Softness for gentler edges, and use Despill against coloured fringes. Then shape the matte while looking at it (Show: Matte): Keep darks holds dark hair, Clip black clears leftover screen, Clip white fills thin spots, Shrink and Soften treat the edge. Connect masks to Keep and Garbage for what must always stay or go.");
    }
}
impl Inspect for veylvik_document::nodes::LumaKey {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        ui.weak("Transparent at or below Low, opaque at or above High. Set Low above High to key out bright areas instead, such as a white backdrop.");
    }
}
impl Inspect for veylvik_document::nodes::RefineMatte {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        ui.weak("Connect the footage to Image and a coarse subject mask to Matte. Raise Radius when the matte misses hair or edges by more than a few pixels; raise Detail to follow finer features. Decontaminate removes the old background's colour from the edges. Check the result with Show alpha in the View menu.");
    }
}
impl Inspect for veylvik_document::nodes::Cryptomatte {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.weak("Connect a Render scene with Pass objects to Objects (and one with objects_more to Objects more, for pixels shared by more than two objects). The matte is white where the picked objects are, as much as they cover each pixel.");
        ui.horizontal(|ui| {
            let active = cx.picking == Some("objects");
            if ui
                .selectable_label(active, "Pick")
                .on_hover_text("Click an object in the viewer to add it to the matte, or click one already in it to take it out. Esc stops")
                .clicked()
            {
                cx.ask(super::Request::Pick("objects"));
            }
            ui.menu_button("Add object", |ui| {
                if cx.objects.is_empty() {
                    ui.weak("No 3D objects in this composition");
                }
                for (_, label) in cx.objects {
                    let picked = self.objects.contains(label);
                    if ui.selectable_label(picked, label).clicked() {
                        if picked {
                            self.objects.retain(|o| o != label);
                        } else {
                            self.objects.push(label.clone());
                        }
                        cx.dirty = true;
                    }
                }
            });
        });
        if self.objects.is_empty() {
            ui.weak("No objects picked yet: the matte is empty.");
        }
        let mut gone = None;
        for (k, name) in self.objects.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui
                    .small_button("Remove")
                    .on_hover_text("Take this object out of the matte")
                    .clicked()
                {
                    gone = Some(k);
                }
                ui.label(name);
                if !name.starts_with('#') && !cx.objects.iter().any(|(_, l)| l == name) {
                    ui.weak("(no object of this name here)");
                }
            });
        }
        if let Some(k) = gone {
            self.objects.remove(k);
            cx.dirty = true;
        }
    }
}
