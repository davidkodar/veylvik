//! Inspector controls for still images and footage.
#[allow(unused_imports)]
use super::{Inspect, InspectCx};
#[allow(unused_imports)]
use crate::{animation, media_controls, shape_animation};
use eframe::egui;

impl Inspect for veylvik_document::nodes::Image {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Image { asset } = self;
        {
            ui.label("Source image / sequence");
            ui.label(asset.as_str());
            ui.label("Images use native size and transparent padding.");
            if let Some(reason) = cx.unavailable.get(asset.as_str()) {
                ui.colored_label(egui::Color32::LIGHT_RED, *reason);
            }
            cx.replace_image = ui.button("Replace source…").clicked();
        }
    }
}
impl Inspect for veylvik_document::nodes::Footage {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Footage {
            asset,
            timing,
            placement,
        } = self;
        {
            ui.label(asset.as_str());
            let changed =
                media_controls::edit(ui, timing, placement, cx.inputs.info(asset), cx.frame_rate);
            cx.dirty |= changed;
            cx.text_changed |= changed;
            if let Some(reason) = cx.unavailable.get(asset.as_str()) {
                ui.colored_label(egui::Color32::LIGHT_RED, *reason);
            }
            cx.replace_image = ui.button("Replace source…").clicked();
        }
    }
}
