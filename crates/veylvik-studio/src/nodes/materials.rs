//! Inspector controls for procedural material nodes.
#[allow(unused_imports)]
use super::{Inspect, InspectCx};
#[allow(unused_imports)]
use crate::{animation, media_controls, shape_animation};
use eframe::egui;

impl Inspect for veylvik_document::nodes::Material {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Material { kind, .. } = self;
        {
            egui::ComboBox::from_id_salt("Material-kind")
                .selected_text(kind.label())
                .show_ui(ui, |ui| {
                    for choice in veylvik_document::MaterialKind::ALL {
                        cx.dirty |= ui.selectable_value(kind, choice, choice.label()).changed();
                    }
                });
        }
        cx.dirty |= finish_controls(ui, self);
        cx.dirty |= lighting_controls(ui, self, cx.node_id);
    }
}
impl Inspect for veylvik_document::nodes::Patina {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::Patina { kind, seed, .. } = self;
        {
            egui::ComboBox::from_id_salt("patina-kind")
                .selected_text(kind.label())
                .show_ui(ui, |ui| {
                    for choice in veylvik_document::PatinaKind::ALL {
                        cx.dirty |= ui.selectable_value(kind, choice, choice.label()).changed();
                    }
                });
            cx.dirty |= ui.add(egui::DragValue::new(seed).prefix("Seed ")).changed();
        }
    }
}
impl Inspect for veylvik_document::nodes::MetallicShine {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        {
            ui.label("Apply to isolated text or a logo before adding a background.");
            ui.label("Animate X/Y to sweep; alpha stays unchanged.");
        }
    }
}
impl Inspect for veylvik_document::nodes::SunGlints {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::SunGlints { seed, .. } = self;
        {
            cx.dirty |= ui.add(egui::DragValue::new(seed).prefix("Seed ")).changed();
        }
    }
}
impl Inspect for veylvik_document::nodes::Glass {
    fn inspect(&mut self, ui: &mut egui::Ui, _cx: &mut InspectCx<'_>) {
        {
            ui.label(
                "Use isolated shape alpha and the scene behind it. Roughness makes frosted glass.",
            );
        }
    }
}
impl Inspect for veylvik_document::nodes::SurfaceReflection {
    fn inspect(&mut self, ui: &mut egui::Ui, cx: &mut InspectCx<'_>) {
        let veylvik_document::nodes::SurfaceReflection { kind, .. } = self;
        {
            egui::ComboBox::from_id_salt("reflection-kind")
                .selected_text(kind.label())
                .show_ui(ui, |ui| {
                    for choice in veylvik_document::ReflectionKind::ALL {
                        cx.dirty |= ui.selectable_value(kind, choice, choice.label()).changed();
                    }
                });
            ui.label("Connect any image or animated branch to Reflection.");
        }
    }
}

pub(crate) fn lighting_controls(
    ui: &mut egui::Ui,
    material: &mut veylvik_document::nodes::Material,
    node_id: veylvik_document::NodeId,
) -> bool {
    use veylvik_document::optical::{AreaLight, MAX_STUDIO_LIGHTS, StudioRig};
    let mut changed = false;
    ui.push_id("surface-lighting",|ui| {
        ui.collapsing("Surface response",|ui| {
            let defaults=material.kind.optics();
            ui.weak(if defaults.metallic {"Conductor: coloured reflections, no diffuse colour."}else{"Dielectric: diffuse colour and neutral surface reflections."});
            let mut custom=material.optics.is_some();
            if ui.checkbox(&mut custom,"Custom optical response").changed() {
                material.optics=custom.then_some(defaults);changed=true;
            }
            if let Some(optics)=material.optics.as_mut() {
                changed|=ui.checkbox(&mut optics.metallic,"Metallic").changed();
                if optics.metallic {
                    ui.label("Reflection colour (linear RGB)");
                    changed|=ui.color_edit_button_rgb(&mut optics.reflectance).changed();
                }else{changed|=light_slider(ui,"Refractive index",&mut optics.ior,1.0..=3.0);}
            } else {ui.label(format!("Normal-incidence reflectance: {:.1}%",defaults.f0()[1]*100.));}
        });
        ui.collapsing("Studio lighting",|ui| {
            crate::light_guides::controls(ui,node_id);
            ui.weak("Area lights reflect in the surface. Position and size use scene units; rotations use degrees.");
            changed|=light_slider(ui,"Studio environment",&mut material.studio.environment,0.0..=16.0);
            changed|=light_slider(ui,"Environment rotation",&mut material.studio.environment_rotation,-360.0..=360.0);
            changed|=light_slider(ui,"Ambient",&mut material.studio.ambient,0.0..=16.0);
            let mut remove=None;
            for (index,light) in material.studio.lights.iter_mut().enumerate() {
                ui.push_id(index,|ui| {
                    ui.collapsing(crate::light_guides::label(index),|ui| {
                        crate::light_guides::select(ui,index);
                        changed|=ui.checkbox(&mut light.enabled,"Enabled").changed();
                        changed|=ui.color_edit_button_rgb(&mut light.colour).on_hover_text("Emitter colour in linear RGB").changed();
                        changed|=light_slider(ui,"Intensity",&mut light.intensity,0.0..=100.0);
                        for (label,value) in ["X","Y","Z"].into_iter().zip(&mut light.position) { changed|=light_slider(ui,label,value,-100.0..=100.0); }
                        for (label,value) in ["Rotate X","Rotate Y","Rotate Z"].into_iter().zip(&mut light.rotation) { changed|=light_slider(ui,label,value,-360.0..=360.0); }
                        for (label,value) in ["Width","Height"].into_iter().zip(&mut light.size) { changed|=light_slider(ui,label,value,0.01..=100.0); }
                        if ui.button("Remove light").clicked(){remove=Some(index);}
                    });
                });
            }
            if let Some(index)=remove{material.studio.lights.remove(index);changed=true;}
            if ui.add_enabled(material.studio.lights.len()<MAX_STUDIO_LIGHTS,egui::Button::new("Add area light")).clicked(){material.studio.lights.push(AreaLight::default());changed=true;}
            if ui.button("Reset studio lighting").on_hover_text("Restore the default photographic studio rig").clicked(){material.studio=StudioRig::default();changed=true;}
        });
    });
    changed
}
fn light_slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    ui.label(label);
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = (ui.available_width() - 75.0).clamp(30.0, 180.0);
        ui.add(egui::Slider::new(value, range))
            .on_hover_text(label)
            .changed()
    })
    .inner
}

pub(crate) fn finish_controls(
    ui: &mut egui::Ui,
    material: &mut veylvik_document::nodes::Material,
) -> bool {
    use veylvik_document::optical::{Brushing, MetalFinish};
    if !material.kind.optics().metallic {
        return false;
    }
    let mut changed = false;
    let selected = material.finish();
    ui.horizontal(|ui| {
        ui.label("Finish");
        egui::ComboBox::from_id_salt("metal-finish")
            .selected_text(selected.map_or("Custom", MetalFinish::label))
            .show_ui(ui, |ui| {
                for finish in MetalFinish::ALL {
                    if ui.selectable_label(selected == Some(finish), finish.label()).clicked() {
                        material.apply_finish(finish);
                        changed = true;
                    }
                }
            }).response.on_hover_text("Set roughness, grain and brushing together. Replaces any roughness/grain keyframes; keeps lighting, bevel and optical response.");
    });
    let mut brushed = material.is_brushed();
    if ui.checkbox(&mut brushed, "Directional brushing")
        .on_hover_text("Stretch reflections along the brushed surface. Grain controls the surface detail; roughness controls reflection softness.")
        .changed() {
        material.brushing = if brushed { Brushing::Directional } else { Brushing::Smooth };
        changed = true;
    }
    ui.weak("Fine-tune roughness and grain below. Tarnish and dirt are separate effects.");
    changed
}
