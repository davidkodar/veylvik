use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use veylvik_document::{Command, Composition, Document, Node, NodeId, Operation, Time};
use veylvik_render::{ImageInputs, Plan};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// GUI state owns the same transactional document used by headless clients.
pub struct Project {
    pub document: Document,
    pub path: Option<PathBuf>,
    pub base: PathBuf,
    pub selected: NodeId,
    pub preview: Option<NodeId>,
    pub inputs: ImageInputs,
    pub frame: i64,
    saved_revision: Option<u64>,
    compiled: Option<(u64, NodeId, Arc<Plan>)>,
}
impl Project {
    pub fn open(path: Option<&Path>) -> Result<Self> {
        let (composition, base, path) = if let Some(path) = path {
            let path = path.canonicalize()?;
            (
                Composition::load(&path)?,
                veylvik_io::document_base(&path)?,
                Some(path),
            )
        } else {
            (veylvik_document::demo(), std::env::current_dir()?, None)
        };
        let selected = composition.output;
        let mut project = Self {
            document: Document::new(composition)?,
            saved_revision: path.as_ref().map(|_| 0),
            compiled: None,
            base,
            path,
            selected,
            preview: None,
            inputs: ImageInputs::default(),
            frame: 0,
        };
        project.reload_inputs()?;
        Ok(project)
    }
    pub fn dirty(&self) -> bool {
        self.saved_revision != Some(self.document.revision())
    }
    pub fn time(&self) -> Result<Time> {
        let fps = self.document.snapshot().frame_rate;
        Ok(Time::new(
            self.frame
                .checked_mul(i64::from(fps.rate))
                .ok_or("frame time overflow")?,
            u32::try_from(fps.ticks).map_err(|_| "frame rate exceeds timeline range")?,
        )?)
    }
    pub fn plan(&mut self) -> Result<Arc<Plan>> {
        let revision = self.document.revision();
        let output = self.preview.unwrap_or(self.document.snapshot().output);
        if let Some((cached_revision, cached_output, plan)) = &self.compiled
            && *cached_revision == revision
            && *cached_output == output
        {
            return Ok(plan.clone());
        }
        let plan = Arc::new(Plan::compile_output(self.document.snapshot(), output)?);
        self.compiled = Some((revision, output, plan.clone()));
        Ok(plan)
    }
    pub fn reload_inputs(&mut self) -> Result<()> {
        // Load every image node so switching preview roots never reads files in a paint callback.
        let mut inputs = ImageInputs::default();
        let mut remaining = 256 * 1024 * 1024;
        for node in self.document.snapshot().nodes.values() {
            if let Operation::Image { asset } = &node.operation
                && inputs.get(asset).is_none()
            {
                let image =
                    veylvik_io::load_png(&self.base.join(asset), remaining.min(128 * 1024 * 1024))?;
                remaining -= image.byte_size();
                inputs.insert(asset, Arc::new(image));
            }
        }
        self.inputs = inputs;
        Ok(())
    }
    pub fn apply(&mut self, commands: &[Command]) -> Result<()> {
        self.document.apply(self.document.revision(), commands)?;
        self.reconcile();
        Ok(())
    }
    pub fn undo(&mut self, redo: bool) -> Result<()> {
        let revision = self.document.revision();
        if redo {
            self.document.redo(revision)?;
        } else {
            self.document.undo(revision)?;
        }
        self.reconcile();
        Ok(())
    }
    fn reconcile(&mut self) {
        let doc = self.document.snapshot();
        if !doc.nodes.contains_key(&self.selected) {
            self.selected = doc.output;
        }
        if self.preview.is_some_and(|id| !doc.nodes.contains_key(&id)) {
            self.preview = None;
        }
    }
    pub fn add(&mut self, operation: Operation) -> Result<NodeId> {
        let ports: Vec<_> = operation
            .inputs()
            .iter()
            .map(|port| (*port, self.selected))
            .collect();
        let node = Node::new(operation.descriptor().label, operation.clone(), &ports);
        let id = node.id;
        self.apply(&[Command::Put { node }])?;
        self.selected = id;
        self.preview = Some(id);
        Ok(id)
    }
    pub fn save(&mut self, path: &Path) -> Result<()> {
        veylvik_io::save(self.document.snapshot(), &self.base, path)?;
        // Keep the original asset base for undo snapshots; each save rebases
        // from it without mutating the document or its edit history.
        self.path = Some(path.canonicalize()?);
        self.saved_revision = Some(self.document.revision());
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_is_not_output_and_invalid_connections_are_atomic() {
        let mut project = Project::open(None).unwrap();
        let output = project.document.snapshot().output;
        let added = project
            .add(Operation::Exposure { stops: 1.0.into() })
            .unwrap();
        assert_eq!(project.preview, Some(added));
        assert_eq!(project.document.snapshot().output, output);
        let revision = project.document.revision();
        let mut node = project.document.snapshot().nodes[&added].clone();
        node.inputs.insert("image".into(), added);
        assert!(project.apply(&[Command::Put { node }]).is_err());
        assert_eq!(project.document.revision(), revision);
        project.undo(false).unwrap();
        assert_eq!(project.selected, output);
        assert_eq!(project.preview, None);
        project.undo(true).unwrap();
        assert!(project.document.snapshot().nodes.contains_key(&added));
    }
}

#[cfg(test)]
mod plan_tests {
    use super::*;
    #[test]
    fn scrubbing_reuses_compiled_plan_but_edits_invalidate_it() {
        let mut project = Project::open(None).unwrap();
        let first = project.plan().unwrap();
        project.frame = 20;
        assert!(Arc::ptr_eq(&first, &project.plan().unwrap()));
        let mut node = project.document.snapshot().nodes[&project.selected].clone();
        node.label = "Changed".into();
        project.apply(&[Command::Put { node }]).unwrap();
        assert!(!Arc::ptr_eq(&first, &project.plan().unwrap()));
    }
}
