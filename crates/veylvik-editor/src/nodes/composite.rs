//! Inspector controls for layering, blending and mask nodes.
#[allow(unused_imports)]
use super::{Inspect, InspectCx};
#[allow(unused_imports)]
use crate::{animation, media_controls, shape_animation};
use eframe::egui;

impl Inspect for veylvik_document::nodes::Merge {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        {
            ui.label("Foreground over background");
        }
    }
}
impl Inspect for veylvik_document::nodes::Blend {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Blend { mode, .. } = self;
        {
            egui::ComboBox::from_id_salt("blend-mode")
                .selected_text(mode.label())
                .show_ui(ui, |ui| {
                    for choice in veylvik_document::BlendMode::ALL {
                        cx.dirty |= ui.selectable_value(mode, choice, choice.label()).changed();
                    }
                });
        }
    }
}
impl Inspect for veylvik_document::nodes::Mask {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        {
            ui.label("Image multiplied by mask alpha");
        }
    }
}
impl Inspect for veylvik_document::nodes::CombineMask {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::CombineMask { mode } = self;
        {
            egui::ComboBox::from_id_salt("mask-mode")
                .selected_text(mode.label())
                .show_ui(ui, |ui| {
                    for choice in veylvik_document::MaskMode::ALL {
                        cx.dirty |= ui.selectable_value(mode, choice, choice.label()).changed();
                    }
                });
            ui.label("Subtract removes other from mask; RGB is ignored.");
        }
    }
}
impl Inspect for veylvik_document::nodes::ChannelMask {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::ChannelMask { channel } = self;
        {
            egui::ComboBox::from_id_salt("mask-channel")
                .selected_text(channel.label())
                .show_ui(ui, |ui| {
                    for choice in veylvik_document::MaskChannel::ALL {
                        cx.dirty |= ui
                            .selectable_value(channel, choice, choice.label())
                            .changed();
                    }
                });
            ui.label("Convert image content to mask coverage.");
        }
    }
}
impl Inspect for veylvik_document::nodes::SurfaceBlend {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        {
            ui.weak("Lighting follows the surface; Detail transfers texture. Wear reveals dark cracks. All amounts can be animated.");
        }
    }
}
impl Inspect for veylvik_document::nodes::Fade {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        use veylvik_document::Time;
        let rate = cx.frame_rate;
        let fps = rate.seconds().unwrap_or(24.);
        let to_frames = |t: Time| (t.seconds().unwrap_or(0.) * fps).round() as i64;
        let mut changed = false;
        // Returns whether the time changed.
        let frames = |ui: &mut egui::Ui, label: &str, time: &mut Time, hint: &str| -> bool {
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label(label);
                let mut value = to_frames(*time);
                if ui
                    .add(
                        egui::DragValue::new(&mut value)
                            .range(0..=1_000_000)
                            .suffix(" f"),
                    )
                    .on_hover_text(hint)
                    .changed()
                    && let Ok(next) = Time::at_frame(value, rate)
                {
                    *time = next;
                    changed = true;
                }
            });
            changed
        };
        let timing = &mut self.timing;
        frames(
            ui,
            "Start",
            &mut timing.start,
            "First frame where the layer appears",
        );
        let mut until_end = timing.end.is_none();
        if ui.checkbox(&mut until_end, "Until the end").changed() {
            timing.end = if until_end {
                None
            } else {
                Time::at_frame(to_frames(timing.start) + (fps * 2.).round() as i64, rate).ok()
            };
            changed = true;
        }
        if let Some(end) = &mut timing.end {
            changed |= frames(ui, "End", end, "First frame where the layer is gone");
        }
        frames(
            ui,
            "Fade in",
            &mut timing.fade_in,
            "Frames over which the layer ramps up from its start",
        );
        if timing.end.is_some() {
            frames(
                ui,
                "Fade out",
                &mut timing.fade_out,
                "Frames over which the layer ramps down before its end",
            );
        }
        if let Err(error) = timing.validate() {
            ui.colored_label(egui::Color32::LIGHT_RED, error.to_string());
        }
        cx.dirty |= changed;
        ui.weak("Opacity multiplies the fade and can be animated. A layer faded fully out is not rendered.");
    }
}
