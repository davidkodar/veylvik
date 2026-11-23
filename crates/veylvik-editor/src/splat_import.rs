//! Choosing a Gaussian splat file for a Gaussian splats node: picked with the desktop portal, read off the UI thread, then the node takes its path as one undo step.
use crate::{Studio, files, files::Completed};
use std::path::PathBuf;
use std::sync::Arc;
use veylvik_document::{Command, NodeId, Operation};

/// Splats read for a node, to apply on the UI thread.
pub struct Chosen {
    session: NodeId,
    revision: u64,
    node: NodeId,
    path: PathBuf,
    splats: Arc<veylvik_splat::Splats>,
}
impl Studio {
    /// Ask for a splat file for the selected Gaussian splats node and read it.
    pub(crate) fn choose_splats(&mut self) {
        if self.files.busy() || !self.apply_draft() {
            return;
        }
        let node = self.project.selected;
        let (folder, session, revision) = (
            self.project.base.clone(),
            self.project.session,
            self.project.document.revision(),
        );
        let result = self.files.start(move || {
            let Some(path) = files::select(files::Choice::Splats, &folder, "")? else {
                return Ok(Completed::Cancelled);
            };
            let path = path.canonicalize().map_err(|e| e.to_string())?;
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            let splats = veylvik_splat::parse(&bytes)?;
            Ok(Completed::SplatsChosen(Box::new(Chosen {
                session,
                revision,
                node,
                path,
                splats: Arc::new(splats),
            })))
        });
        self.file_started(result);
    }
    pub(crate) fn splats_chosen(&mut self, chosen: Chosen) {
        if chosen.session != self.project.session
            || chosen.revision != self.project.document.revision()
        {
            return self.fail("The project changed while reading the splats; choose them again.");
        }
        let Some(path) = chosen.path.to_str().map(str::to_owned) else {
            return self.fail("The file's path is not UTF-8");
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
        let Operation::GaussianSplats(g) = &mut node.operation else {
            return;
        };
        g.file = path.clone();
        if node.label == veylvik_document::nodes::GAUSSIAN_SPLATS.label
            && let Some(stem) = chosen.path.file_stem()
        {
            node.label = stem.to_string_lossy().into_owned();
        }
        let (count, degree) = (chosen.splats.count, chosen.splats.degree);
        self.project.allow_added(std::path::Path::new(&path));
        self.project.inputs.insert_splats(path, chosen.splats);
        match self.project.apply(&[Command::Put {
            node: Box::new(node),
        }]) {
            Ok(()) => {
                self.status =
                    format!("{count} splats read (view-dependent colour to degree {degree})");
                self.error = false;
                self.refresh_draft();
                self.request_preview();
            }
            Err(e) => self.fail(e),
        }
    }
}
