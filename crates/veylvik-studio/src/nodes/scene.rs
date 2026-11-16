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
/// The surface's library look, or a pointer to the library when it is plain.
fn look(
    ui: &mut egui::Ui,
    look: &mut Option<Box<veylvik_document::nodes::Material>>,
    cx: &InspectCx<'_>,
) -> bool {
    let mut changed = false;
    match look {
        Some(material) => {
            ui.horizontal(|ui| {
                ui.label(format!("Look: {}", material.kind.label()));
                if ui
                    .small_button("Plain surface")
                    .on_hover_text("Remove the look and shade with colour, roughness and metallic")
                    .clicked()
                {
                    changed = true;
                }
            });
            if changed {
                *look = None;
                return true;
            }
            changed |= super::look_slider(ui, material);
            changed |= super::colour_controls(ui, material, cx.time);
        }
        None => {
            ui.weak(
                "Pick a material in Library > Materials and Apply to selection to give it a look.",
            );
        }
    }
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
        cx.dirty |= ui
            .checkbox(&mut self.shadow_catcher, "Shadow catcher")
            .on_hover_text("Invisible except for the shadows falling on it: lay a plane on the ground of your footage so 3D objects shadow the real floor")
            .changed();
        cx.dirty |= look(ui, &mut self.look, cx);
        if self.look.is_none() {
            cx.dirty |= crate::animation::colour_track(ui, "Colour", &mut self.colour, cx.time);
        }
        cx.dirty |= placement(ui, &mut self.placement, cx);
    }
}
impl Inspect for Text3d {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.weak("Extrudes the Text or Path on its Outline input; Depth sets how deep.");
        cx.dirty |= ui
            .checkbox(&mut self.round_bevel, "Round bevel")
            .on_hover_text(
                "Rounded edges catch soft highlights; untick for a crisp 45-degree chamfer",
            )
            .changed();
        cx.dirty |= look(ui, &mut self.look, cx);
        if self.look.is_none() {
            cx.dirty |= crate::animation::colour_track(ui, "Colour", &mut self.colour, cx.time);
        }
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
        ui.horizontal(|ui| {
            ui.add_sized([88., 22.], egui::Label::new("Focus on"));
            let name = |id: Option<veylvik_document::NodeId>| match id {
                None => "Focus distance".to_string(),
                Some(id) => cx
                    .objects
                    .iter()
                    .find(|(o, _)| *o == id)
                    .map_or("Missing object".into(), |(_, label)| label.clone()),
            };
            egui::ComboBox::from_id_salt("focus_on")
                .selected_text(name(self.focus_on))
                .show_ui(ui, |ui| {
                    cx.dirty |= ui
                        .selectable_value(&mut self.focus_on, None, "Focus distance")
                        .on_hover_text("Focus at Focus distance; 0 focuses on whatever is at the frame centre")
                        .changed();
                    for (id, label) in cx.objects {
                        cx.dirty |= ui
                            .selectable_value(&mut self.focus_on, Some(*id), label)
                            .changed();
                    }
                })
                .response
                .on_hover_text("Keep this object sharp wherever it and the camera move (depth of field needs an F-stop above 0)");
            if ui
                .button("Pick in viewer")
                .on_hover_text("Click an object in the viewer to focus on it")
                .clicked()
            {
                cx.pick_focus = true;
            }
        });
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
        cx.dirty |= ui
            .checkbox(&mut self.shadows, "Shadows")
            .on_hover_text("Cast shadows from this light (up to four lights per scene); Softness blurs their edges")
            .changed();
        cx.dirty |= crate::animation::colour_track(ui, "Colour", &mut self.colour, cx.time);
        cx.dirty |= placement(ui, &mut self.placement, cx);
    }
}
impl Inspect for PointCloud {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        ui.label(format!("{} points", self.points.len()));
        ui.weak("Shown as dots in the 3D viewer. Select a 3D object and click a dot to put the object there.");
    }
}
impl Inspect for Environment3d {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.weak("Connect a panorama (an HDR or EXR sky, any image or footage) to light and reflect in the scene; without one the studio is used.");
        cx.dirty |= ui
            .checkbox(&mut self.background, "Show as backdrop")
            .on_hover_text("Also fill the sky behind the objects; Blur softens it")
            .changed();
    }
}
impl Inspect for RenderScene {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        ui.weak("Films the scene on its Scene input. Camera 1 is the first camera in the scene; 0 is the home view that matches 2D. Occlusion shades the environment light where surfaces meet, so objects sit on the floor (0 = off).");
    }
}
