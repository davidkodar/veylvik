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
                ui.label("Shadows");
                cx.dirty |= animation::color(ui, low);
            });
            ui.push_id("high", |ui| {
                ui.label("Highlights");
                cx.dirty |= animation::color(ui, high);
            });
        }
    }
}
