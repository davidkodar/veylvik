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
            colour,
        } = self;
        {
            ui.label(asset.as_str());
            let changed =
                media_controls::edit(ui, timing, placement, cx.inputs.info(asset), cx.frame_rate);
            cx.dirty |= changed;
            cx.text_changed |= changed;
            if input_colour(ui, colour) {
                cx.dirty = true;
                cx.text_changed = true;
            }
            if let Some(reason) = cx.unavailable.get(asset.as_str()) {
                ui.colored_label(egui::Color32::LIGHT_RED, *reason);
            }
            cx.replace_image = ui.button("Replace source…").clicked();
        }
    }
}

/// Input colour for video footage: the file's tags by default, or an explicit camera curve and
/// gamut. Picking a curve also picks its usual gamut. Returns true when changed.
fn input_colour(ui: &mut egui::Ui, colour: &mut veylvik_document::media::InputColour) -> bool {
    use veylvik_document::media::{Curve, Gamut};
    let mut changed = false;
    egui::Grid::new("footage-input-colour")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Input colour");
            egui::ComboBox::from_id_salt("footage-curve")
                .selected_text(colour.curve.label())
                .show_ui(ui, |ui| {
                    for curve in Curve::ALL {
                        if ui
                            .selectable_label(colour.curve == curve, curve.label())
                            .clicked()
                            && colour.curve != curve
                        {
                            colour.curve = curve;
                            colour.gamut = curve.usual_gamut();
                            changed = true;
                        }
                    }
                });
            ui.end_row();
            ui.label("Gamut");
            egui::ComboBox::from_id_salt("footage-gamut")
                .selected_text(colour.gamut.label())
                .show_ui(ui, |ui| {
                    for gamut in Gamut::ALL {
                        changed |= ui
                            .selectable_value(&mut colour.gamut, gamut, gamut.label())
                            .changed();
                    }
                });
            ui.end_row();
        });
    ui.weak("Video only. Auto follows the file's colour tags. Camera log footage is usually untagged: choose its curve (Apple Log, S-Log3, LogC, V-Log) to get scene-linear light with 18% grey at 0.18.");
    changed
}

impl Inspect for veylvik_document::nodes::MotionBlur {
    fn inspect(&mut self, ui: &mut egui::Ui, _: &mut InspectCx<'_>) {
        ui.weak("Place after animated transforms or warps, before merging the background.");
        ui.weak("Preview uses Preview samples; export uses Final samples. Footage may already have motion blur.");
    }
}
impl Inspect for veylvik_document::nodes::TimeRemap {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        ui.horizontal(|ui| {
            if ui
                .button("Normal")
                .on_hover_text("Restore original source timing")
                .clicked()
            {
                self.rate = 1.0.into();
                self.source_time = 0.0.into();
                cx.dirty = true;
            }
            if ui
                .button("Freeze here")
                .on_hover_text("Hold the currently displayed source time")
                .clicked()
                && let (Ok(rate), Ok(offset), Ok(time)) = (
                    self.rate.at(cx.time),
                    self.source_time.at(cx.time),
                    cx.time.seconds(),
                )
            {
                self.source_time = ((time * f64::from(rate) + f64::from(offset))
                    .clamp(-86400.0, 86400.0) as f32)
                    .into();
                self.rate = 0.0.into();
                cx.dirty = true;
            }
        });
        ui.weak("Source seconds = timeline seconds × rate + source time. Negative rate reverses. For speed ramps, set rate to zero and keyframe Source time.");
        ui.weak("Only this image branch is retimed. Audio is unchanged; footage keeps its trim and transparent boundaries.");
    }
}
