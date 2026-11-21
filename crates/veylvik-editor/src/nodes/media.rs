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
                ui.colored_label(egui::Color32::LIGHT_RED, reason.as_str());
            }
            cx.replace_image = ui.button("Replace source…").clicked();
        }
    }
}
impl Inspect for veylvik_document::nodes::Footage {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let key = veylvik_document::Operation::Footage(self.clone())
            .source_key()
            .map(|k| k.into_owned())
            .unwrap_or_default();
        let veylvik_document::nodes::Footage {
            asset,
            timing,
            placement,
            colour,
            layer,
            ..
        } = self;
        {
            ui.label(asset.as_str());
            let path =
                std::path::PathBuf::from(cx.inputs.resolve_config(asset).unwrap_or_default());
            if crate::exr_layers::choose(ui, &path, layer) {
                cx.dirty = true;
                cx.text_changed = true;
            }
            let changed =
                media_controls::edit(ui, timing, placement, cx.inputs.info(&key), cx.frame_rate);
            cx.dirty |= changed;
            cx.text_changed |= changed;
            if input_colour(ui, colour) {
                cx.dirty = true;
                cx.text_changed = true;
            }
            if let Some(reason) = cx.unavailable.get(asset.as_str()) {
                ui.colored_label(egui::Color32::LIGHT_RED, reason.as_str());
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

impl Inspect for veylvik_document::nodes::OpticalFlow {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        use veylvik_document::nodes::FlowQuality;
        ui.weak("Smooth slow motion between source frames. Uncertain regions use the nearest frame; audio is unchanged.");
        ui.label("Source frame rate");
        ui.horizontal(|ui| {
            cx.dirty |= ui
                .add(egui::DragValue::new(&mut self.source_rate.ticks).range(1..=1_000_000))
                .on_hover_text("Original clip frame-rate numerator")
                .changed();
            ui.label("/");
            cx.dirty |= ui
                .add(egui::DragValue::new(&mut self.source_rate.rate).range(1..=1_000_000))
                .on_hover_text("Original clip frame-rate denominator; e.g. 24000 / 1001")
                .changed();
            if ui
                .button("Use composition")
                .on_hover_text("Use the project frame rate as the source cadence")
                .clicked()
            {
                self.source_rate = cx.frame_rate;
                cx.dirty = true;
            }
        });
        ui.label("Analysis quality");
        egui::ComboBox::from_id_salt("flow-quality")
            .selected_text(match self.quality {
                FlowQuality::Fast => "Fast",
                FlowQuality::Balanced => "Balanced",
                FlowQuality::Fine => "Fine",
            })
            .show_ui(ui, |ui| {
                for (value, label) in [
                    (FlowQuality::Fast, "Fast"),
                    (FlowQuality::Balanced, "Balanced"),
                    (FlowQuality::Fine, "Fine"),
                ] {
                    cx.dirty |= ui
                        .selectable_value(&mut self.quality, value, label)
                        .changed();
                }
            });
        cx.dirty|=ui.checkbox(&mut self.diagnostic,"Show confidence").on_hover_text("Green is reliable; red uses source-frame fallback. This changes the node output, including export.").changed();
        ui.weak("Preview uses reduced analysis resolution. Output keeps the full project resolution. Increase confidence to reject more uncertain motion.");
    }
}
impl Inspect for veylvik_document::nodes::Sound {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        use veylvik_document::Time;
        ui.weak("Plays on the timeline and in video exports. It makes no picture and connects to nothing.");
        ui.horizontal(|ui| {
            ui.add_sized([88., 22.], egui::Label::new("File"));
            let changed = ui
                .add(egui::TextEdit::singleline(&mut self.file).desired_width(f32::INFINITY))
                .on_hover_text("The audio file: wav, mp3, flac, ogg, m4a, or a video with sound. Import media adds one for you")
                .changed();
            cx.dirty |= changed;
            cx.text_changed |= changed;
        });
        let rate = cx.frame_rate;
        let fps = rate.ticks as f64 / f64::from(rate.rate.max(1));
        let frames = |time: Time| time.seconds().map_or(0, |s| (s * fps).round() as i64);
        let row = |ui: &mut egui::Ui, label: &str, time: &mut Time, hint: &str| -> bool {
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.add_sized([88., 22.], egui::Label::new(label));
                let mut frame = frames(*time);
                if ui
                    .add(
                        egui::DragValue::new(&mut frame)
                            .range(0..=1_000_000)
                            .suffix(" f"),
                    )
                    .on_hover_text(hint)
                    .changed()
                    && let Ok(at) = Time::at_frame(frame, rate)
                {
                    *time = at;
                    changed = true;
                }
            });
            changed
        };
        row(
            ui,
            "Starts at",
            &mut self.timing.start,
            "The frame where the sound begins on the timeline",
        );
        row(
            ui,
            "Skip",
            &mut self.timing.source_in,
            "How much of the file's beginning to leave out, in frames",
        );
        let mut trimmed = self.timing.source_out.is_some();
        if ui
            .checkbox(&mut trimmed, "Stop early")
            .on_hover_text("End the sound before the file ends")
            .changed()
        {
            self.timing.source_out = trimmed
                .then(|| Time::at_frame(frames(self.timing.source_in) + 60, rate).ok())
                .flatten();
            cx.dirty = true;
        }
        if let Some(out) = &mut self.timing.source_out {
            row(
                ui,
                "Stop at",
                out,
                "Where in the file the sound stops, in frames from the file's beginning",
            );
        }
    }
}
