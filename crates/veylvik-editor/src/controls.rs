//! Published controls drawn without an editor: a host shows a composition's published settings (its own, then its groups') with the inspector's widgets, and gets each change back as commands; nothing here touches the GPU or the disk.
use eframe::egui;
use serde_json::Value;
use veylvik_document::{
    Command, Composition, Operation, Scalar, Time, groups::Control, registry::ParameterKind,
};

/// Kept by the host between frames for one panel.
#[derive(Default)]
pub struct ControlsState {
    /// A drag on one of the panel's widgets has changed something and goes on.
    gesture: bool,
}
/// What one frame of the panel did.
#[derive(Default, Debug)]
pub struct ControlsResponse {
    /// The changes made this frame, to apply to the composition in one transaction:
    /// a `SetParameter` with the setting's whole new value (its keys included).
    pub commands: Vec<Command>,
    /// A drag that changed something is still held. Changes reported while it is true
    /// belong to one gesture: a host merges them into one history step, which ends at the
    /// first frame this is false again.
    pub dragging: bool,
}

/// The kinds of setting the panel can draw; others are not offered for publishing.
pub fn drawable(kind: ParameterKind) -> bool {
    matches!(
        kind,
        ParameterKind::Scalar
            | ParameterKind::Boolean
            | ParameterKind::Color
            | ParameterKind::Colour3
            | ParameterKind::Seed
            | ParameterKind::TextContent
            | ParameterKind::FrameRate
            | ParameterKind::FlowQuality
            | ParameterKind::BlurFilter
    )
}

/// Draw `composition`'s published controls at `time` (composition time): keyed settings
/// are edited at that time, as the inspector does. Build the panel each frame from the
/// composition as it is now; apply `commands` to it when there are any.
pub fn ui(
    ui: &mut egui::Ui,
    composition: &Composition,
    time: Time,
    state: &mut ControlsState,
) -> ControlsResponse {
    let mut response = ControlsResponse::default();
    for control in veylvik_document::groups::published(composition) {
        let (Some(node), Ok(mut value)) = (
            composition.nodes.get(&control.node),
            control.value(composition),
        ) else {
            continue;
        };
        ui.push_id((control.node, &control.parameter), |ui| {
            if widget(ui, &control, &mut value, &node.operation, time) {
                response.commands.push(Command::SetParameter {
                    id: control.node,
                    parameter: control.parameter.clone(),
                    value,
                });
            }
        });
    }
    let held = ui.ctx().dragged_id().is_some();
    state.gesture = held && (state.gesture || !response.commands.is_empty());
    response.dragging = state.gesture;
    response
}

/// The inspector's widget for one control, editing `value` (the parameter's whole JSON
/// value); true when it changed. Shared with a group's exposed controls.
pub(crate) fn widget(
    ui: &mut egui::Ui,
    control: &Control,
    value: &mut Value,
    operation: &Operation,
    time: Time,
) -> bool {
    let Some(descriptor) = operation
        .descriptor()
        .parameters
        .iter()
        .find(|p| p.name == control.parameter)
    else {
        return false;
    };
    let mut changed = false;
    match descriptor.kind {
        ParameterKind::Scalar => {
            let Ok(mut scalar) = serde_json::from_value::<Scalar>(value.clone()) else {
                return false;
            };
            if control.parameter == "light"
                && matches!(operation, Operation::Shadow(_))
                && matches!(scalar, Scalar::Constant { .. })
            {
                changed = crate::light_guides::source_selector(ui, &mut scalar);
            } else {
                let [min, max] = control.range.unwrap_or([
                    descriptor.min.unwrap_or(f32::MIN),
                    descriptor.max.unwrap_or(f32::MAX),
                ]);
                let default = serde_json::from_value::<Scalar>(control.default.clone())
                    .ok()
                    .and_then(|s| s.at(time).ok());
                changed = crate::animation::scalar_with_default(
                    ui,
                    &control.label,
                    &mut scalar,
                    min..=max,
                    time,
                    default,
                );
            }
            *value = serde_json::to_value(scalar).unwrap_or_default();
        }
        ParameterKind::FrameRate => {
            ui.label(&control.label);
            let Ok(mut rate) = serde_json::from_value::<Time>(value.clone()) else {
                return false;
            };
            ui.horizontal(|ui| {
                changed |= ui
                    .add(egui::DragValue::new(&mut rate.ticks).range(1..=1_000_000))
                    .on_hover_text("Source frame-rate numerator")
                    .changed();
                ui.label("/");
                changed |= ui
                    .add(egui::DragValue::new(&mut rate.rate).range(1..=1_000_000))
                    .on_hover_text("Source frame-rate denominator; e.g. 24000 / 1001")
                    .changed();
            });
            *value = serde_json::to_value(rate).unwrap_or_default();
        }
        ParameterKind::FlowQuality => {
            use veylvik_document::nodes::FlowQuality;
            ui.label(&control.label);
            let Ok(mut quality) = serde_json::from_value::<FlowQuality>(value.clone()) else {
                return false;
            };
            egui::ComboBox::from_id_salt("flow-quality")
                .selected_text(format!("{quality:?}"))
                .show_ui(ui, |ui| {
                    for q in [FlowQuality::Fast, FlowQuality::Balanced, FlowQuality::Fine] {
                        changed |= ui
                            .selectable_value(&mut quality, q, format!("{q:?}"))
                            .changed();
                    }
                });
            *value = serde_json::to_value(quality).unwrap_or_default();
        }
        ParameterKind::BlurFilter => {
            use veylvik_document::nodes::BlurFilter;
            ui.label(&control.label);
            let mut filter: BlurFilter = serde_json::from_value(value.clone()).unwrap_or_default();
            egui::ComboBox::from_id_salt(("blur-filter", &control.label))
                .selected_text(filter.label())
                .show_ui(ui, |ui| {
                    for f in BlurFilter::ALL {
                        changed |= ui.selectable_value(&mut filter, f, f.label()).changed();
                    }
                });
            *value = serde_json::to_value(filter).unwrap_or_default();
        }
        ParameterKind::Boolean => {
            let mut enabled = value.as_bool().unwrap_or(false);
            changed = ui.checkbox(&mut enabled, &control.label).changed();
            *value = enabled.into();
        }
        ParameterKind::Color => {
            ui.label(&control.label);
            let Ok(mut color) = serde_json::from_value::<[f32; 4]>(value.clone()) else {
                return false;
            };
            changed = crate::animation::color(ui, &mut color);
            *value = serde_json::to_value(color).unwrap_or_default();
        }
        ParameterKind::Colour3 => {
            let Ok(mut track) =
                serde_json::from_value::<veylvik_document::track::Track3>(value.clone())
            else {
                return false;
            };
            changed = crate::animation::colour_track(ui, &control.label, &mut track, time);
            *value = serde_json::to_value(track).unwrap_or_default();
        }
        ParameterKind::Seed => {
            ui.label(&control.label);
            let mut seed = value.as_u64().unwrap_or(0) as u32;
            changed = ui.add(egui::DragValue::new(&mut seed)).changed();
            *value = seed.into();
        }
        ParameterKind::TextContent => {
            ui.label(&control.label);
            let mut text = value.as_str().unwrap_or_default().to_owned();
            changed = ui
                .add(
                    egui::TextEdit::multiline(&mut text)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                )
                .changed();
            *value = text.into();
        }
        _ => {
            ui.label(&control.label);
            ui.weak("Edit this setting in Veylvik.");
        }
    }
    changed
}
