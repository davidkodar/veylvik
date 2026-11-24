//! What a node's controls ask of the editor (`nodes::Request`), done in one place once the inspector is drawn.
use crate::{Studio, eyedropper, nodes::Request, scene_focus};
use eframe::egui;
use veylvik_document::Operation;

impl Studio {
    /// Does `request`. True when the inspector should stop drawing this frame (another
    /// node is selected, or the graph changed under it). `dirty`: the controls changed
    /// the draft this frame.
    pub(crate) fn inspector_request(
        &mut self,
        request: Request,
        ctx: &egui::Context,
        dirty: bool,
    ) -> bool {
        match request {
            Request::ReplaceImage => self.replace_image(),
            Request::ChooseSplats => self.choose_splats(),
            Request::ChooseModel => self.choose_model(),
            Request::BakeFlock(true) => self.bake_flock(ctx),
            Request::BakeFlock(false) => self.unbake_flock(),
            Request::Select(id) => {
                if self.can_change_selection() {
                    self.project.selected = id;
                    self.project.preview = None;
                    self.refresh_draft();
                    return true;
                }
            }
            Request::SurroundLights { warm } => {
                self.surround_lights(warm, dirty);
                return true;
            }
            Request::MeasureGrain => self.measure_grain(ctx),
            Request::PickFocus => scene_focus::set_picking(ctx, true),
            Request::MatchShot => self.match_shot(ctx),
            Request::ShareLights(true) => self.share_material_lights(),
            Request::ShareLights(false) => {
                // Keep the look: the surface takes over the rig's current lights.
                let rig = self
                    .draft
                    .inputs
                    .remove("lights")
                    .and_then(|id| self.project.document.snapshot().nodes.get(&id))
                    .and_then(|n| match &n.operation {
                        Operation::LightRig(rig) => Some(rig.studio.clone()),
                        _ => None,
                    });
                if let (Some(studio), Operation::Material(m)) = (rig, &mut self.draft.operation) {
                    m.studio = studio;
                }
                self.draft_dirty = true;
            }
            Request::ReflectShot(true) => {
                // The backdrop the material sits on becomes its surroundings.
                let doc = self.project.document.snapshot();
                match veylvik_document::presets::background::automatic(doc)
                    .filter(|id| *id != self.draft.id)
                {
                    Some(backdrop) => {
                        self.draft.inputs.insert("environment".into(), backdrop);
                        self.draft_dirty = true;
                    }
                    None => self.fail(
                        "No single backdrop found. Pick the shot in the Environment input below.",
                    ),
                }
            }
            Request::ReflectShot(false) => {
                self.draft.inputs.remove("environment");
                self.draft_dirty = true;
            }
            Request::SaveMaterial => {
                if let Operation::Material(material) = &self.draft.operation {
                    let label = self.draft.label.trim_start_matches("Text · ").to_owned();
                    self.materials.save(&label, material);
                }
            }
            Request::Pick(field) => {
                // A second press on the active field cancels, like Esc.
                let active = self
                    .picking
                    .as_ref()
                    .is_some_and(|p| p.node == self.draft.id && p.field == field);
                self.picking = (!active).then(|| eyedropper::Picking::new(self.draft.id, field));
                self.tell(if active {
                    "Eyedropper cancelled"
                } else {
                    "Eyedropper: click or drag in the viewer to sample a colour · Esc cancels"
                });
            }
        }
        false
    }
    /// Adds surround lighting to the selected Render scene, once the inspector is clean.
    fn surround_lights(&mut self, warm: bool, dirty: bool) {
        if self.draft_dirty || dirty {
            self.status = "Apply or revert the inspector before adding lights".into();
            return;
        }
        match veylvik_presets::surround_lights(
            self.project.document.snapshot(),
            self.draft.id,
            warm,
        ) {
            Ok(rig) => match self.project.apply(&rig.commands) {
                Ok(_) => {
                    self.refresh_draft();
                    self.request_preview();
                    self.status = "Added surround lighting".into();
                }
                Err(e) => self.status = e.to_string(),
            },
            Err(e) => self.status = e.to_string(),
        }
    }
}
