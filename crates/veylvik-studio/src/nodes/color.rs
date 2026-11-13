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
impl Inspect for veylvik_document::nodes::Lut {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let name = std::path::Path::new(&self.asset)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&self.asset);
        ui.horizontal(|ui| {
            ui.label(if self.asset.ends_with(".cube") {
                name
            } else {
                "No LUT chosen"
            });
            cx.replace_image = ui.button("Choose .cube…").clicked();
        });
        if let Some(reason) = cx.unavailable.get(self.asset.as_str()) {
            ui.colored_label(egui::Color32::LIGHT_RED, *reason);
        }
        egui::Grid::new("lut-spaces").num_columns(3).show(ui, |ui| {
            for (label, colour) in [("Input", &mut self.input), ("Output", &mut self.output)] {
                ui.label(label);
                cx.dirty |= space(ui, label, colour);
                ui.end_row();
            }
        });
        ui.weak("Input is what the LUT expects and Output what it produces. A camera LUT (for example S-Log3 / S-Gamut3.Cine to Rec.709) takes the camera's curve and gamut as Input; a creative look made for video uses Rec.709 for both. Mix blends with the original.");
    }
}
/// Curve and gamut menus for one side of a LUT; Auto means Rec.709. True when changed.
fn space(ui: &mut egui::Ui, id: &str, colour: &mut veylvik_document::media::InputColour) -> bool {
    use veylvik_document::media::{Curve, Gamut};
    let label = |auto: bool, text: &'static str| if auto { "Rec.709" } else { text };
    let mut changed = false;
    egui::ComboBox::from_id_salt(("lut-curve", id))
        .selected_text(label(colour.curve == Curve::Auto, colour.curve.label()))
        .show_ui(ui, |ui| {
            for curve in Curve::ALL.into_iter().skip(1) {
                if ui
                    .selectable_label(colour.curve == curve, curve.label())
                    .clicked()
                    && colour.curve != curve
                {
                    colour.curve = curve;
                    colour.gamut = match curve.usual_gamut() {
                        Gamut::Auto => Gamut::Rec709,
                        gamut => gamut,
                    };
                    changed = true;
                }
            }
        });
    egui::ComboBox::from_id_salt(("lut-gamut", id))
        .selected_text(label(colour.gamut == Gamut::Auto, colour.gamut.label()))
        .show_ui(ui, |ui| {
            for gamut in Gamut::ALL.into_iter().skip(1) {
                changed |= ui
                    .selectable_value(&mut colour.gamut, gamut, gamut.label())
                    .changed();
            }
        });
    changed
}
