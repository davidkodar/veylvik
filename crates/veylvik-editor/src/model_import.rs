//! Choosing a 3D model file for a 3D model node: picked with the desktop portal, loaded off the UI thread, then the node takes its path and proportions as one undo step.
use crate::{Studio, files, files::Completed};
use std::path::PathBuf;
use std::sync::Arc;
use veylvik_document::{Command, NodeId, Operation};

/// A loaded model for a node, to apply on the UI thread.
pub struct Chosen {
    session: NodeId,
    revision: u64,
    node: NodeId,
    path: PathBuf,
    model: Arc<veylvik_scene::Model>,
}
impl Studio {
    /// Ask for a model file for the selected 3D model node and load it.
    pub(crate) fn choose_model(&mut self) {
        if self.files.busy() || !self.apply_draft() {
            return;
        }
        let node = self.project.selected;
        if !matches!(
            self.project
                .document
                .snapshot()
                .nodes
                .get(&node)
                .map(|n| &n.operation),
            Some(Operation::Model3d(_))
        ) {
            return;
        }
        self.playing = false;
        let (folder, session, revision) = (
            self.project.base.clone(),
            self.project.session,
            self.project.document.revision(),
        );
        let result = self.files.start(move || {
            let Some(path) = files::select(files::Choice::Model, &folder, "")? else {
                return Ok(Completed::Cancelled);
            };
            let path = path.canonicalize().map_err(|e| e.to_string())?;
            let model = veylvik_io::model::load(&path)?;
            Ok(Completed::ModelChosen(Box::new(Chosen {
                session,
                revision,
                node,
                path,
                model: Arc::new(model),
            })))
        });
        self.file_started(result);
    }
    pub(crate) fn model_chosen(&mut self, chosen: Chosen) {
        if chosen.session != self.project.session
            || chosen.revision != self.project.document.revision()
        {
            return self.fail("The project changed while loading the model; choose it again.");
        }
        let Some(path) = chosen.path.to_str().map(str::to_owned) else {
            return self.fail("The model's path is not UTF-8");
        };
        let Some(mut node) = self
            .project
            .document
            .snapshot()
            .nodes
            .get(&chosen.node)
            .cloned()
        else {
            return;
        };
        let Operation::Model3d(m) = &mut node.operation else {
            return;
        };
        m.model = path.clone();
        m.extent = chosen.model.extent;
        if node.label == veylvik_document::nodes::MODEL_3D.label
            && let Some(stem) = chosen.path.file_stem()
        {
            node.label = stem.to_string_lossy().into_owned();
        }
        let parts = chosen.model.parts.len();
        self.project.allow_added(std::path::Path::new(&path));
        self.project.inputs.insert_model(path, chosen.model);
        match self.project.apply(&[Command::Put {
            node: Box::new(node),
        }]) {
            Ok(()) => {
                self.status = format!("Model loaded: {parts} parts");
                self.error = false;
                self.refresh_draft();
                self.request_preview();
            }
            Err(e) => self.fail(e),
        }
    }
}
