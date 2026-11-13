//! Inspector controls for planar tracks and warps.
#[allow(unused_imports)]
use super::{Inspect, InspectCx};
#[allow(unused_imports)]
use crate::{animation, media_controls, shape_animation};
use eframe::egui;

impl Inspect for veylvik_document::nodes::PlanarTrack {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::PlanarTrack { data } = self;
        {
            ui.label(format!("{} tracked frames", data.samples.len()));
            ui.weak("Connect this node to a Planar warp's track input. The source image passes through unchanged.");
        }
    }
}
impl Inspect for veylvik_document::nodes::PlanarWarp {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::PlanarWarp { mode } = self;
        {
            egui::ComboBox::from_id_salt("planar-warp-mode")
                .selected_text(mode.label())
                .show_ui(ui, |ui| {
                    for choice in veylvik_document::planar::WarpMode::ALL {
                        cx.dirty |= ui.selectable_value(mode, choice, choice.label()).changed();
                    }
                });
        }
    }
}
