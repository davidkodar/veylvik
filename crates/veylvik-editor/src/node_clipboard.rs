//! Node clipboard operations commit through the same atomic document path as editing.
use crate::Studio;
use eframe::egui;
use veylvik_document::{NodePosition, clipboard::NodeClipboard};
impl Studio {
    pub(super) fn copy_graph_nodes(&mut self, ctx: &egui::Context) {
        if self.files.busy()
            || self.authoring_dialog_open()
            || self.graph.dragging()
            || !self.can_change_selection()
        {
            return;
        }
        let result = (|| -> Result<String, Box<dyn std::error::Error>> {
            let positions = self
                .graph
                .selected_positions(self.project.document.snapshot());
            let mut clip = NodeClipboard::capture(
                self.project.document.snapshot(),
                self.project.session,
                positions.keys().copied().collect(),
                self.project.selected,
            )?;
            clip.source_base = Some(
                self.project
                    .base
                    .to_str()
                    .ok_or("Project path is not UTF-8")?
                    .to_owned(),
            );
            for (id, position) in positions {
                clip.fragment.nodes.get_mut(&id).unwrap().position = Some(position);
            }
            Ok(clip.encode()?)
        })();
        match result {
            Ok(text) => {
                ctx.copy_text(text);
                self.status = "Copied selected nodes".into();
            }
            Err(e) => self.fail(e),
        }
    }
    pub(super) fn paste_graph_nodes(&mut self, text: &str, position: NodePosition) {
        if self.files.busy()
            || self.authoring_dialog_open()
            || self.graph.dragging()
            || !self.can_change_selection()
        {
            return;
        }
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let clip = NodeClipboard::decode(text)?;
            let mut copy = clip.paste(self.project.document.snapshot(), self.project.session)?;
            self.graph.place_paste(
                self.project.document.snapshot(),
                self.project.document.revision(),
                &mut copy,
                position,
            )?;
            let root = copy.root;
            let ids = clip.selected.iter().map(|id| copy.mapping[id]).collect();
            self.project.apply(&copy.into_commands())?;
            self.project.selected = root;
            self.project.preview = Some(root);
            self.playing = false;
            self.refresh_draft();
            self.graph.select_pasted(ids, root);
            self.request_preview();
            Ok(())
        })();
        if let Err(e) = result {
            self.fail(e);
        }
    }
}
