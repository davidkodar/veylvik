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
        if let Some(outline) = cx.outline
            && ui
                .button("Edit the words…")
                .on_hover_text("Select the Text or Path node this 3D text is made from, to change its words, font and size")
                .clicked()
        {
            cx.select = Some(outline);
        }
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
        cx.dirty |= letters(ui, &mut self.letters);
    }
}
/// The per-letter animation of a 3D text.
fn letters(ui: &mut egui::Ui, l: &mut veylvik_document::nodes::Letters) -> bool {
    use veylvik_document::{nodes::LetterMotion::*, text::TextUnit};
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_sized([88., 22.], egui::Label::new("Letters"));
        let name = |m| match m {
            None => "Stay together",
            Fly => "Fly in",
            Drop => "Drop in",
            Turn => "Turn in",
            Grow => "Grow",
        };
        egui::ComboBox::from_id_salt("letters-motion")
            .selected_text(name(l.motion))
            .show_ui(ui, |ui| {
                for motion in veylvik_document::nodes::LetterMotion::ALL {
                    changed |= ui.selectable_value(&mut l.motion, motion, name(motion)).changed();
                }
            })
            .response
            .on_hover_text("Bring the text in piece by piece in 3D. Needs a Text node on Outline; a Path stays whole");
        if l.motion != None {
            let unit = |u| match u {
                TextUnit::Character => "By letter",
                TextUnit::Word => "By word",
                TextUnit::Line => "By line",
                TextUnit::Whole => "All at once",
            };
            egui::ComboBox::from_id_salt("letters-unit")
                .selected_text(unit(l.unit))
                .show_ui(ui, |ui| {
                    for u in [TextUnit::Character, TextUnit::Word, TextUnit::Line, TextUnit::Whole] {
                        changed |= ui.selectable_value(&mut l.unit, u, unit(u)).changed();
                    }
                });
        }
    });
    if l.motion == None {
        return changed;
    }
    let mut row = |ui: &mut egui::Ui,
                   label: &str,
                   value: &mut f32,
                   range: std::ops::RangeInclusive<f32>,
                   suffix: &str,
                   hint: &str| {
        ui.horizontal(|ui| {
            ui.add_sized([88., 22.], egui::Label::new(label));
            changed |= ui
                .add(
                    egui::DragValue::new(value)
                        .range(range)
                        .speed(0.01)
                        .suffix(suffix),
                )
                .on_hover_text(hint)
                .changed();
        });
    };
    row(
        ui,
        "Starts at",
        &mut l.start,
        -86400.0..=86400.0,
        " s",
        "When the first piece starts to arrive, in seconds on the timeline",
    );
    row(
        ui,
        "Takes",
        &mut l.duration,
        0.001..=3600.0,
        " s",
        "How long each piece takes to arrive",
    );
    row(
        ui,
        "Apart",
        &mut l.stagger,
        0.0..=60.0,
        " s",
        "How long after the piece before it each piece starts",
    );
    if matches!(l.motion, Fly | Drop) {
        row(
            ui,
            "From",
            &mut l.distance,
            -100000.0..=100000.0,
            " px",
            "How far away the pieces start: towards the viewer for Fly in, above for Drop in; negative for the other side",
        );
    }
    ui.horizontal(|ui| {
        ui.add_sized([88., 22.], egui::Label::new("Leaves"));
        let mut leaves = l.exit.is_some();
        if ui
            .checkbox(&mut leaves, "")
            .on_hover_text("Send the pieces away again, the way they came")
            .changed()
        {
            l.exit = leaves.then_some(l.start + l.duration + 3.0);
            changed = true;
        }
        if let Some(at) = &mut l.exit {
            changed |= ui
                .add(
                    egui::DragValue::new(at)
                        .range(-86400.0..=86400.0)
                        .speed(0.01)
                        .suffix(" s"),
                )
                .changed();
        }
    });
    changed |= ui.checkbox(&mut l.ease, "Ease in and out").changed();
    changed
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
/// A row choosing a 3D object to aim at, or none.
fn aim_row(
    ui: &mut egui::Ui,
    aim_at: &mut Option<veylvik_document::NodeId>,
    cx: &mut InspectCx<'_>,
    what: &str,
) {
    ui.horizontal(|ui| {
        ui.add_sized([88., 22.], egui::Label::new("Aim at"));
        let shown = match *aim_at {
            None => "Its own rotation".to_string(),
            Some(id) => cx
                .objects
                .iter()
                .find(|(o, _)| *o == id)
                .map_or("Missing object".into(), |(_, label)| label.clone()),
        };
        egui::ComboBox::from_id_salt("aim_at")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                cx.dirty |= ui
                    .selectable_value(aim_at, None, "Its own rotation")
                    .on_hover_text("Turn it with the rotation above")
                    .changed();
                for (id, label) in cx.objects {
                    cx.dirty |= ui.selectable_value(aim_at, Some(*id), label).changed();
                }
            })
            .response
            .on_hover_text(format!(
                "Keep the {what} pointed at this object wherever both move; the rotation above is then not used"
            ));
    });
}
impl Inspect for Camera3d {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.weak("Untouched, the camera sees exactly the 2D frame at depth 0. Move and turn it from there.");
        cx.dirty |= placement(ui, &mut self.placement, cx);
        aim_row(ui, &mut self.aim_at, cx, "camera");
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
            &[
                (Point, "Point"),
                (Spot, "Spot"),
                (Sun, "Sun"),
                (Area, "Area"),
            ],
        );
        cx.dirty |= ui
            .checkbox(&mut self.shadows, "Shadows")
            .on_hover_text("Cast shadows from this light (up to four lights per scene); Softness blurs their edges")
            .changed();
        cx.dirty |= crate::animation::colour_track(ui, "Colour", &mut self.colour, cx.time);
        cx.dirty |= placement(ui, &mut self.placement, cx);
        aim_row(ui, &mut self.aim_at, cx, "light");
    }
}
impl Inspect for veylvik_document::nodes::MatchGrain {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.weak("Adds grain like the plate's to a clean image, such as 3D over footage. Connect the footage to Plate, then measure.");
        if ui
            .add_enabled(cx.plated, egui::Button::new("Measure from plate"))
            .on_hover_text("Measure the plate's grain at this frame: its strength in shadows, midtones and highlights, colour and size")
            .clicked()
        {
            cx.measure_grain = true;
        }
    }
}
impl Inspect for veylvik_document::nodes::Model3d {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let name = std::path::Path::new(&self.model)
            .file_name()
            .map_or("No model chosen".to_string(), |n| {
                n.to_string_lossy().into_owned()
            });
        ui.horizontal(|ui| {
            ui.label(name).on_hover_text(&self.model);
            if ui
                .button("Choose model file…")
                .on_hover_text("A glTF 2.0 model: .glb, or .gltf with its files")
                .clicked()
            {
                cx.choose_model = true;
            }
        });
        cx.dirty |= look(ui, &mut self.look, cx);
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
