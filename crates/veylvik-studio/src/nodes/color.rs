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

impl Inspect for veylvik_document::nodes::Ocio {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        cx.dirty |= ocio_controls(ui, &mut self.transform, cx.inputs);
    }
}
pub(crate) fn ocio_controls(
    ui: &mut egui::Ui,
    t: &mut veylvik_document::color_management::OcioTransform,
    inputs: &veylvik_render::ImageInputs,
) -> bool {
    let mut changed = false;

    ui.label("OpenColorIO configuration");
    changed |= ui
        .text_edit_singleline(&mut t.config)
        .on_hover_text("OCIO URI or configuration file relative to the project")
        .changed();
    let path = inputs.resolve_config(&t.config);
    // Config menus are only rebuilt when their path changes, not on mouse movement.
    let id = ui.make_persistent_id(("ocio-menus", &path));
    type Menus = std::result::Result<(Vec<String>, Vec<(String, Vec<String>)>), String>;
    let menus: Menus = ui.ctx().data_mut(|data| {
        data.get_temp::<Menus>(id).unwrap_or_else(|| {
            let result = (|| {
                let c = veylvik_color::Config::load(&path)?;
                let displays = c
                    .displays()?
                    .into_iter()
                    .map(|d| {
                        let views = c.views(&d)?;
                        Ok((d, views))
                    })
                    .collect::<std::result::Result<Vec<_>, String>>()?;
                Ok((c.spaces()?, displays))
            })();
            data.insert_temp(id, result.clone());
            result
        })
    });
    if ui.button("Reload configuration").clicked() {
        ui.ctx().data_mut(|data| data.remove::<Menus>(id));
        changed = true;
    }
    match menus {
        Ok((spaces, displays)) => {
            fn choice(
                ui: &mut egui::Ui,
                label: &str,
                value: &mut String,
                options: impl IntoIterator<Item = String>,
            ) -> bool {
                let mut changed = false;
                egui::ComboBox::from_id_salt(label)
                    .selected_text(value.as_str())
                    .width(210.)
                    .show_ui(ui, |ui| {
                        for option in options {
                            changed |= ui.selectable_value(value, option.clone(), option).changed();
                        }
                    });
                changed
            }
            ui.label("Source space");
            changed |= choice(ui, "ocio-source", &mut t.source, spaces.clone());
            let mut view_mode = !t.display.is_empty();
            if ui
                .checkbox(&mut view_mode, "Display / view transform")
                .changed()
            {
                changed = true;
                if view_mode {
                    ui.label("Linear output space");
                    changed |= choice(ui, "ocio-linear-output", &mut t.destination, spaces.clone());
                    if let Some((d, v)) = displays.first() {
                        t.display = d.clone();
                        t.view = v.first().cloned().unwrap_or_default();
                    }
                } else {
                    t.display.clear();
                    t.view.clear();
                }
            }
            if view_mode {
                ui.label("Linear output space");
                changed |= choice(ui, "ocio-linear-output", &mut t.destination, spaces.clone());
                ui.label("Display");
                if choice(
                    ui,
                    "ocio-display",
                    &mut t.display,
                    displays.iter().map(|d| d.0.clone()),
                ) {
                    changed = true;
                    t.view = displays
                        .iter()
                        .find(|d| d.0 == t.display)
                        .and_then(|d| d.1.first())
                        .cloned()
                        .unwrap_or_default();
                }
                ui.label("View");
                changed |= choice(
                    ui,
                    "ocio-view",
                    &mut t.view,
                    displays
                        .iter()
                        .find(|d| d.0 == t.display)
                        .map(|d| d.1.clone())
                        .unwrap_or_default(),
                );
            } else {
                ui.label("Destination space");
                changed |= choice(ui, "ocio-destination", &mut t.destination, spaces);
            }
        }
        Err(error) => {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
    }
    ui.weak("Keep custom configurations together with their LUT files.").on_hover_text("Source space describes the incoming pixels. ACEScg is a working space; AgX is a display view supplied by its configuration. Display views are converted back to the selected linear output space before viewer encoding.");
    changed
}
