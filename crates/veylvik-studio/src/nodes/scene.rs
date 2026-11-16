//! Inspector controls for 3D scene nodes: kinds, keyable placement and colour.
use super::{Inspect, InspectCx};
use eframe::egui;
use veylvik_document::nodes::*;

/// Position, rotation and scale rows, each keyable as a whole vector.
fn placement(ui: &mut egui::Ui, placement: &mut Placement, cx: &InspectCx<'_>) -> bool {
    let mut changed = false;
    ui.weak("Offsets from where it naturally sits (pixels); rotation in degrees, X then Y then Z.");
    changed |= crate::animation::vector_track(
        ui,
        "Position",
        ["X", "Y", "Z"],
        &mut placement.position,
        -1.0e6..=1.0e6,
        cx.time,
    );
    changed |= crate::animation::vector_track(
        ui,
        "Rotation",
        ["X", "Y", "Z"],
        &mut placement.rotation,
        -1.0e5..=1.0e5,
        cx.time,
    );
    changed |= crate::animation::vector_track(
        ui,
        "Scale",
        ["X", "Y", "Z"],
        &mut placement.scale,
        -1000.0..=1000.0,
        cx.time,
    );
    changed
}
fn kind<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut T,
    choices: &[(T, &str)],
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        let current = choices
            .iter()
            .find(|(c, _)| c == value)
            .map_or("", |(_, l)| *l);
        egui::ComboBox::from_id_salt(label)
            .selected_text(current)
            .show_ui(ui, |ui| {
                for (choice, name) in choices {
                    changed |= ui.selectable_value(value, *choice, *name).changed();
                }
            });
    });
    changed
}
impl Inspect for Shape3d {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        use ShapeKind::*;
        cx.dirty |= kind(
            ui,
            "Shape",
            &mut self.kind,
            &[
                (Cube, "Cube"),
                (Sphere, "Sphere"),
                (Plane, "Plane"),
                (Cylinder, "Cylinder"),
                (Cone, "Cone"),
                (Torus, "Torus"),
            ],
        );
        cx.dirty |= crate::animation::colour_track(ui, "Colour", &mut self.colour, cx.time);
        cx.dirty |= placement(ui, &mut self.placement, cx);
    }
}
impl Inspect for Text3d {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.weak("Extrudes the Text or Path on its Outline input; Depth sets how deep.");
        cx.dirty |= crate::animation::colour_track(ui, "Colour", &mut self.colour, cx.time);
        cx.dirty |= placement(ui, &mut self.placement, cx);
    }
}
impl Inspect for ImageCard {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.weak("Untouched, the card covers the frame and renders like the 2D image.");
        cx.dirty |= placement(ui, &mut self.placement, cx);
    }
}
impl Inspect for Transform3d {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.weak("Moves, turns and scales the whole scene, cameras and lights included, about the frame centre.");
        cx.dirty |= placement(ui, &mut self.placement, cx);
    }
}
impl Inspect for MergeScenes {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        ui.weak("Combines up to eight scenes; depth decides what is in front.");
    }
}
impl Inspect for Camera3d {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.weak("Untouched, the camera sees exactly the 2D frame at depth 0. Move and turn it from there.");
        cx.dirty |= placement(ui, &mut self.placement, cx);
    }
}
impl Inspect for Light3d {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        use LightKind::*;
        cx.dirty |= kind(
            ui,
            "Light",
            &mut self.kind,
            &[(Point, "Point"), (Spot, "Spot"), (Sun, "Sun")],
        );
        cx.dirty |= crate::animation::colour_track(ui, "Colour", &mut self.colour, cx.time);
        cx.dirty |= placement(ui, &mut self.placement, cx);
    }
}
impl Inspect for RenderScene {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        ui.weak("Films the scene on its Scene input. Camera 1 is the first camera in the scene; 0 is the home view that matches 2D. The 3D renderer arrives in the next update.");
    }
}
