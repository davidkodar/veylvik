//! Guided foreground occlusion; no graph changes until the outline closes.
use eframe::egui;
use veylvik_document::{Command, NodeId, Operation, occlusion, vector::PathMask};
#[derive(Default)]
pub struct Controls {
    pub open: bool,
    source: Option<NodeId>,
    pending: Option<Pending>,
}
struct Pending {
    owner: NodeId,
    source: NodeId,
    session: NodeId,
    revision: u64,
    preview: Option<NodeId>,
    redraw: bool,
}
impl super::Studio {
    pub(super) fn occlusion_controls(&mut self, ui: &mut egui::Ui) {
        if matches!(self.draft.operation, Operation::Text { .. }) {
            if ui
                .button("Place behind object…")
                .on_hover_text(
                    "Outline a foreground object and place it over the text automatically",
                )
                .clicked()
                && self.apply_draft()
            {
                self.occlusion.open = true;
                self.occlusion.source = None;
            }
        } else if let Some(setup) = occlusion::find(self.project.document.snapshot(), self.draft.id)
        {
            ui.strong("Foreground object");
            ui.weak("The outlined object sits in front of your composition.");
            ui.horizontal(|ui| {
                if ui.button("View result").clicked() {
                    self.project.preview = None;
                    self.request_preview();
                }
                if ui.add_enabled(!matches!(&self.draft.operation,Operation::Path { geometry } if !geometry.keys.is_empty()),egui::Button::new("Redraw over source")).on_disabled_hover_text("Disable shape animation before redrawing the outline").clicked() && self.apply_draft() {
                    self.begin_occlusion(setup.source, true);
                }
            });
            if let Ok(time) = self.project.time() {
                for (id, label, range) in [
                    (setup.expansion, "Edge expansion", -256.0..=256.0),
                    (setup.softness, "Edge softness", 0.0..=256.0),
                ] {
                    let mut node = self
                        .text_edits
                        .get(&id)
                        .unwrap_or(&self.project.document.snapshot().nodes[&id])
                        .clone();
                    if let Some(radius) = node.operation.scalar_mut("radius")
                        && crate::animation::scalar(ui, label, radius, range, time)
                    {
                        self.text_edits.insert(id, node);
                        self.draft_dirty = true;
                        self.text_preview_deadline.get_or_insert_with(|| {
                            std::time::Instant::now() + std::time::Duration::from_millis(32)
                        });
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_millis(32));
                    }
                }
            }
            ui.weak("Edges use composition pixels. Apply saves; Revert cancels.");
            ui.separator();
        }
    }
    fn begin_occlusion(&mut self, source: NodeId, redraw: bool) {
        self.playing = false;
        self.occlusion.pending = Some(Pending {
            owner: self.draft.id,
            source,
            session: self.project.session,
            revision: self.project.document.revision(),
            preview: self.project.preview,
            redraw,
        });
        self.project.preview = Some(source);
        self.path_transform.active = false;
        self.path_pen.begin();
        self.request_preview();
    }
    pub(super) fn occlusion_dialog(&mut self, ctx: &egui::Context) {
        if !self.occlusion.open {
            return;
        }
        let mut open = true;
        let mut start = false;
        let mut cancel = false;
        let window = egui::Window::new("Place text behind object")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(400.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO);
        window.show(ctx, |ui| {
            ui.label("Choose the image or layer containing the foreground object.");
            ui.weak("Next, outline the object in the viewer. Your composition stays unchanged until you close the outline.");
            let doc=self.project.document.snapshot();
            let connected=occlusion::depends_on(doc,doc.output,self.draft.id);
            let independent = occlusion::independent_sources(doc, self.draft.id);
            let mut candidates:Vec<_>=doc.nodes.values().filter(|n|independent.contains(&n.id) && !matches!(n.operation,Operation::Path {..}|Operation::Text {..}|Operation::Ellipse {..}|Operation::ChannelMask {..}|Operation::CombineMask {..})).collect();
            candidates.sort_by_key(|n|(!matches!(n.operation,Operation::Image {..}),n.label.clone()));
            egui::ComboBox::from_id_salt("occlusion-source").width(340.0).selected_text(self.occlusion.source.and_then(|id|doc.nodes.get(&id)).map_or("Choose foreground image/layer…",|n|n.label.as_str())).show_ui(ui,|ui| {
                for node in &candidates {ui.selectable_value(&mut self.occlusion.source,Some(node.id),format!("{} · {}",node.label,node.operation.descriptor().label));}
            });
            if candidates.is_empty() {ui.weak("Import a foreground image first, then return here.");}
            if !connected {ui.colored_label(egui::Color32::LIGHT_RED,"Connect this text to the composition output first.");}
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                start=ui.add_enabled(connected && self.occlusion.source.is_some(),egui::Button::new("Draw foreground outline")).clicked();
                cancel=ui.button("Cancel").clicked();
            });
        });
        self.occlusion.open = open && !start && !cancel;
        if start && let Some(source) = self.occlusion.source {
            self.begin_occlusion(source, false);
        }
    }
    pub(super) fn occlusion_pending(&self) -> bool {
        self.occlusion.pending.is_some()
    }
    pub(super) fn occlusion_tick(&mut self) {
        let invalid = self.occlusion.pending.as_ref().is_some_and(|p| {
            !self.path_pen.active
                || self.project.session != p.session
                || self.project.document.revision() != p.revision
                || self.project.selected != p.owner
                || self.project.preview != Some(p.source)
        });
        if invalid && let Some(p) = self.occlusion.pending.take() {
            self.path_pen.cancel();
            if self.project.session == p.session && self.project.preview == Some(p.source) {
                self.project.preview = p.preview;
                self.request_preview();
            }
        }
    }
    pub(super) fn occlusion_canvas(
        &mut self,
        ui: &egui::Ui,
        canvas: egui::Rect,
        transform: crate::viewer::ViewTransform,
        image: egui::Vec2,
    ) {
        if self.occlusion.pending.is_none() || self.displayed_ticket != self.wanted {
            return;
        }
        let Some(geometry) = self
            .path_pen
            .show(ui, canvas, transform, image, &PathMask::default())
        else {
            return;
        };
        let pending = self.occlusion.pending.take().unwrap();
        if self.project.session != pending.session
            || self.project.document.revision() != pending.revision
        {
            self.fail("The composition changed. Start the foreground outline again.");
            return;
        }
        let result = (|| -> Result<NodeId, Box<dyn std::error::Error>> {
            if pending.redraw {
                let mut node = self.project.document.snapshot().nodes[&pending.owner].clone();
                node.operation = Operation::Path { geometry };
                self.project.apply(&[Command::Put {
                    node: Box::new(node),
                }])?;
                Ok(pending.owner)
            } else {
                let (setup, commands) = occlusion::create(
                    self.project.document.snapshot(),
                    pending.owner,
                    pending.source,
                    geometry,
                )?;
                self.project.apply(&commands)?;
                Ok(setup.outline)
            }
        })();
        match result {
            Ok(id) => {
                self.project.selected = id;
                self.project.preview = None;
                self.refresh_draft();
                self.request_preview();
            }
            Err(e) => {
                self.project.preview = pending.preview;
                self.request_preview();
                self.fail(e);
            }
        }
    }
}
