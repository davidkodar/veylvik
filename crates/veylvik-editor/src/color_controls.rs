//! Project view/output transforms are edited as one undoable transaction.
use eframe::egui;
use veylvik_document::{NodeId, color_management::OcioViews};
#[derive(Clone)]
struct Draft {
    settings: OcioViews,
    output_tab: bool,
}
fn id(session: NodeId) -> egui::Id {
    egui::Id::new(("project-ocio", session))
}
pub fn open(ctx: &egui::Context, session: NodeId, settings: &OcioViews) {
    ctx.data_mut(|data| {
        data.insert_temp(
            id(session),
            Draft {
                settings: settings.clone(),
                output_tab: false,
            },
        )
    });
}
pub fn show(
    ctx: &egui::Context,
    session: NodeId,
    inputs: &veylvik_render::ImageInputs,
) -> Option<OcioViews> {
    let mut draft = ctx.data_mut(|data| data.get_temp::<Draft>(id(session)))?;
    let mut open = true;
    let mut result = None;
    let mut close = false;
    egui::Window::new("Colour management").id(id(session)).open(&mut open).default_width(430.)
        .default_pos(ctx.content_rect().center()-egui::vec2(225.,300.)).resizable(false).show(ctx,|ui| {
        ui.horizontal(|ui| {ui.selectable_value(&mut draft.output_tab,false,"Preview");ui.selectable_value(&mut draft.output_tab,true,"Output");});
        ui.separator();
        if draft.output_tab {
            ui.weak("Output transforms affect exported pixels, including EXR. Leave off for the unmodified scene-linear result. Output is independent of the preview.");
            if ui.add_enabled(draft.settings.view.is_some(),egui::Button::new("Copy preview settings")).clicked() {draft.settings.output=draft.settings.view.clone();}
        } else {ui.weak("The preview transform only changes the viewer. It never changes the composition's pixels or the output transform.");}
        let transform=if draft.output_tab {&mut draft.settings.output}else{&mut draft.settings.view};
        let mut enabled=transform.is_some();
        if ui.checkbox(&mut enabled,"Use OpenColorIO").changed() {*transform=enabled.then(Default::default);}
        if let Some(transform)=transform {
            egui::ScrollArea::vertical().max_height(550.).show(ui,|ui| {crate::nodes::ocio_controls(ui,transform,inputs);});
            ui.weak("Use linear Rec.709 for source and output. Working space is set separately.");
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Apply").clicked(){result=Some(draft.settings.clone());close=true;}
            if ui.button("Cancel").clicked(){close=true;}
        });
    });
    ctx.data_mut(|data| {
        if open && !close {
            data.insert_temp(id(session), draft);
        } else {
            data.remove::<Draft>(id(session));
        }
    });
    result
}
