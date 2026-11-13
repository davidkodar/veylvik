//! Surface appearance shortcuts leave all resulting parameters editable.
use crate::Studio;
use eframe::egui;
use veylvik_document::{Operation, surface::Look};
impl Studio {
    pub(super) fn surface_controls(&mut self, ui: &mut egui::Ui) {
        if matches!(
            self.draft.operation,
            Operation::PlanarWarp(veylvik_document::nodes::PlanarWarp {
                mode: veylvik_document::planar::WarpMode::Follow
                    | veylvik_document::planar::WarpMode::Fit
            })
        ) && ui
            .button("Blend onto tracked surface…")
            .on_hover_text("Add surface lighting and texture controls to this output layer")
            .clicked()
            && self.apply_draft()
        {
            match veylvik_document::surface::integrate(
                self.project.document.snapshot(),
                self.draft.id,
                Look::Painted,
            ) {
                Ok((id, commands)) => match self.project.apply(&commands) {
                    Ok(()) => {
                        self.project.selected = id;
                        self.project.preview = None;
                        self.refresh_draft();
                        self.request_preview();
                    }
                    Err(e) => self.fail(e),
                },
                Err(e) => self.fail(e),
            }
        }
        if matches!(
            self.draft.operation,
            Operation::SurfaceBlend(veylvik_document::nodes::SurfaceBlend { .. })
        ) {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("surface-look").selected_text("Starting look…").show_ui(ui,|ui| {
                    for look in Look::ALL {
                        if ui.selectable_label(false,look.label()).on_hover_text("Replace surface parameters with this starting look; Apply commits one undo step").clicked() {
                            self.draft.operation=look.operation();self.draft_dirty=true;
                        }
                    }
                });
                if ui.button("Neutral").on_hover_text("Reset to a normal source-over composite; Apply commits the change").clicked() {
                    self.draft.operation=Look::Painted.operation();
                    for name in ["lighting","detail","wear","distortion","softness","grain"] {
                        *self.draft.operation.scalar_mut(name).unwrap()=0.0.into();
                    }
                    self.draft_dirty=true;
                }
            });
            let background = self.draft.inputs.get("background").copied();
            let comparing = background.is_some() && self.project.preview == background;
            if ui
                .button(if comparing {
                    "Show composition"
                } else {
                    "Compare clean surface"
                })
                .clicked()
            {
                self.project.preview = if comparing { None } else { background };
                self.request_preview();
            }
        }
    }
}
