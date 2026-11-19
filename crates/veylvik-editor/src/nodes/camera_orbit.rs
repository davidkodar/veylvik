//! Compact procedural orbit controls; disabling preserves both orbit and manual keys.
use super::InspectCx;
use eframe::egui;
use veylvik_document::nodes::{Camera3d, CameraOrbit};
pub(super) fn controls(camera: &mut Camera3d, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
    ui.separator();
    let mut enabled = camera.orbit.is_some_and(|o| o.enabled);
    if ui.checkbox(&mut enabled, "Orbit camera").on_hover_text("Fly around the Aim at object, or the frame centre. Disable to return to the existing manual position and rotation keys.").changed() {
        camera.orbit.get_or_insert_with(CameraOrbit::default).enabled = enabled;
        cx.dirty = true;
    }
    if !enabled {
        return;
    }
    let orbit = camera.orbit.as_mut().unwrap();
    ui.weak("Target: Aim at above. Motion is continuous, including between frames.");
    let defaults = CameraOrbit::default();
    row(
        ui,
        "Radius",
        &mut orbit.radius,
        1.0..=1e6,
        defaults.radius,
        " px",
        cx,
    );
    row(
        ui,
        "Height",
        &mut orbit.height,
        -1e6..=1e6,
        defaults.height,
        " px",
        cx,
    );
    row(
        ui,
        "Start",
        &mut orbit.start,
        0.0..=1e6,
        defaults.start,
        " s",
        cx,
    );
    row(
        ui,
        "Duration",
        &mut orbit.duration,
        0.001..=1e6,
        defaults.duration,
        " s",
        cx,
    );
    row(
        ui,
        "Start angle",
        &mut orbit.start_angle,
        -36000.0..=36000.0,
        0.,
        " deg",
        cx,
    );
    row(
        ui,
        "Sweep",
        &mut orbit.sweep,
        0.0..=36000.0,
        360.,
        " deg",
        cx,
    );
    cx.dirty |= ui
        .checkbox(&mut orbit.clockwise, "Clockwise")
        .on_hover_text("Reverse the orbit direction")
        .changed();
    cx.dirty |= ui
        .checkbox(&mut orbit.ease, "Ease in and out")
        .on_hover_text("Smooth acceleration and deceleration; disable for constant angular speed")
        .changed();
    ui.weak("Manual position and rotation are preserved while orbit takes control. Increase radius if the title leaves the frame.");
}
fn row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    default: f32,
    suffix: &str,
    cx: &mut InspectCx<'_>,
) {
    ui.push_id(label, |ui| {
        ui.horizontal(|ui| {
            ui.add_sized([82., 24.], egui::Label::new(label));
            cx.dirty |= ui
                .add_sized(
                    [100., 24.],
                    egui::DragValue::new(value)
                        .range(range)
                        .speed(if suffix == " s" { 0.05 } else { 1. })
                        .suffix(suffix),
                )
                .on_hover_text(format!("Adjust {label}; double-click to type a value"))
                .changed();
            if ui
                .add(crate::icons::Button::new(
                    crate::icons::Icon::Reset,
                    format!("Reset {label}"),
                ))
                .clicked()
            {
                *value = default;
                cx.dirty = true;
            }
        });
    });
}
