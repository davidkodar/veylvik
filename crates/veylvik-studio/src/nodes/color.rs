//! Inspector controls for colour adjustment nodes.
#[allow(unused_imports)]
use super::{Inspect, InspectCx};
#[allow(unused_imports)]
use crate::{animation, media_controls, shape_animation};
use eframe::egui;

impl Inspect for veylvik_document::nodes::Exposure {}
impl Inspect for veylvik_document::nodes::Colorize {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Colorize { low, high } = self;
        {
            ui.label("Map luminance to linear RGBA colors.");
            ui.push_id("low", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Shadows");
                    crate::eyedropper::button(ui, cx, "low");
                });
                cx.dirty |= animation::color(ui, low);
            });
            ui.push_id("high", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Highlights");
                    crate::eyedropper::button(ui, cx, "high");
                });
                cx.dirty |= animation::color(ui, high);
            });
        }
    }
}
impl Inspect for veylvik_document::nodes::ColorCorrect {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        ui.weak("Applied in order: white balance, lift and gain, contrast around 18% grey, gamma, saturation. Every control can be animated; defaults change nothing.");
    }
}
