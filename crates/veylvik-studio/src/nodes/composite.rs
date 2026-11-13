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
