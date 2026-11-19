//! Footage metadata and exact frame-based source placement controls.
use eframe::egui;
use veylvik_document::{Time, media::Timing};
use veylvik_render::SourceInfo;
fn frames(time: Time, rate: Time) -> i64 {
    ((i128::from(time.ticks) * i128::from(rate.ticks))
        / (i128::from(time.rate) * i128::from(rate.rate)))
    .clamp(0, i64::MAX as i128) as i64
}
fn field(ui: &mut egui::Ui, label: &str, time: &mut Time, rate: Time, default: Time) -> bool {
    let mut frame = frames(*time, rate);
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        if ui
            .add(
                egui::DragValue::new(&mut frame)
                    .range(
                        0..=frames(
                            Time {
                                ticks: 86400,
                                rate: 1,
                            },
                            rate,
                        ),
                    )
                    .suffix(" fr"),
            )
            .changed()
            && let Ok(value) = Time::at_frame(frame, rate)
        {
            *time = value;
            changed = true;
        }
        if ui
            .add(crate::icons::Button::new(
                crate::icons::Icon::Reset,
                "Reset to default",
            ))
            .clicked()
        {
            *time = default;
            changed = true;
        }
    });
    changed
}
pub fn edit(
    ui: &mut egui::Ui,
    timing: &mut Timing,
    placement: &mut veylvik_document::media::Placement,
    info: Option<SourceInfo>,
    rate: Time,
) -> bool {
    if let Some(info) = &info {
        ui.strong(format!("{} × {}", info.width, info.height));
        if let Some(duration) = info.duration {
            ui.label(format!(
                "{:.3} s · {} source frames",
                duration.seconds().unwrap_or_default(),
                info.frames.unwrap_or_default()
            ));
        }
        if let Some(fps) = info.frame_rate {
            ui.label(format!(
                "{:.3} fps average",
                fps.seconds().unwrap_or_default()
            ));
        }
        ui.weak(&info.description);
    }
    ui.separator();
    let old = *placement;
    use veylvik_document::media::Placement;
    egui::ComboBox::from_label("Placement")
        .selected_text(placement.label())
        .show_ui(ui, |ui| {
            for value in [
                Placement::Fit,
                Placement::Fill,
                Placement::Original,
                Placement::Stretch,
                Placement::Legacy,
            ] {
                ui.selectable_value(placement, value, value.label());
            }
        });
    ui.weak("Fit shows the entire image; Fill crops edges. Original size keeps source pixels.");
    if info.as_ref().is_some_and(|i| i.duration.is_none()) {
        return old != *placement;
    }
    ui.strong("Clip timing");
    ui.weak("Frame numbers use the composition frame rate. Source out is exclusive.");
    let mut next = timing.clone();
    let zero = Time { ticks: 0, rate: 1 };
    let mut changed = field(ui, "Timeline start", &mut next.start, rate, zero);
    changed |= field(ui, "Source in", &mut next.source_in, rate, zero);
    let mut full = next.source_out.is_none();
    if ui.checkbox(&mut full, "Use source end").changed() {
        next.source_out = if full {
            None
        } else {
            info.as_ref()
                .and_then(|i| i.duration)
                .or(Some(Time { ticks: 1, rate: 1 }))
        };
        changed = true;
    }
    if let Some(end) = &mut next.source_out {
        let default = info
            .as_ref()
            .and_then(|i| i.duration)
            .unwrap_or(Time { ticks: 1, rate: 1 });
        changed |= field(ui, "Source out", end, rate, default);
    }
    if let Err(error) = next.validate() {
        ui.colored_label(egui::Color32::LIGHT_RED, error.to_string());
        return false;
    }
    if let Some(end) = info.as_ref().and_then(|i| i.duration)
        && next.source_in.compare(end).is_ok_and(|o| !o.is_lt())
    {
        ui.colored_label(
            egui::Color32::LIGHT_RED,
            "Source in must be before the end of the footage.",
        );
        return false;
    }
    if changed {
        *timing = next;
    }
    ui.weak("Outside this clip is transparent.");
    changed || old != *placement
}
