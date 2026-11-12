use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use veylvik_document::{Command, Composition, Document, Node, NodeId, Operation, Time};
use veylvik_render::{ImageInputs, Plan};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// GUI state owns the same transactional document used by headless clients.
pub struct Project {
    pub session: NodeId,
    pub recovery_origin: Option<NodeId>,
    pub document: Document,
    pub path: Option<PathBuf>,
    pub base: PathBuf,
    pub selected: NodeId,
    pub preview: Option<NodeId>,
    pub inputs: ImageInputs,
    asset_errors: std::collections::BTreeMap<String, String>,
    pub frame: i64,
    saved_revision: Option<u64>,
    stamp: Option<veylvik_io::FileStamp>,
    compiled: Option<(u64, NodeId, Arc<Plan>)>,
}
impl Project {
    pub fn open(path: Option<&Path>) -> Result<Self> {
        let (composition, base, path, stamp) = if let Some(path) = path {
            let (composition, stamp) = veylvik_io::FileStamp::load(path)?;
            let path = stamp.path().to_path_buf();
            (
                composition,
                veylvik_io::document_base(&path)?,
                Some(path),
                Some(stamp),
            )
        } else {
            (
                veylvik_document::motion_demo(),
                std::env::current_dir()?,
                None,
                None,
            )
        };
        let selected = composition.output;
        let mut project = Self {
            session: NodeId::new_v4(),
            recovery_origin: None,
            document: Document::new(composition)?,
            saved_revision: path.as_ref().map(|_| 0),
            compiled: None,
            stamp,
            base,
            path,
            selected,
            preview: None,
            inputs: ImageInputs::default(),
            asset_errors: Default::default(),
            frame: 0,
        };
        project.load_available_inputs();
        Ok(project)
    }
    pub fn blank(width: u32, height: u32, frame_rate: Time, frame_count: u32) -> Result<Self> {
        let mut composition = veylvik_document::blank(width, height, frame_rate)?;
        composition.frame_count = frame_count;
        let selected = composition.output;
        Ok(Self {
            session: NodeId::new_v4(),
            recovery_origin: None,
            document: Document::new(composition)?,
            path: None,
            base: std::env::current_dir()?,
            selected,
            preview: None,
            inputs: ImageInputs::default(),
            asset_errors: Default::default(),
            frame: 0,
            saved_revision: None,
            stamp: None,
            compiled: None,
        })
    }
    pub fn recover(store: &veylvik_io::recovery::Store, id: NodeId) -> Result<Self> {
        let composition = store.read(id)?;
        let selected = composition.output;
        let mut project = Self {
            session: NodeId::new_v4(),
            recovery_origin: Some(id),
            document: Document::new(composition)?,
            path: None,
            base: std::env::current_dir()?,
            selected,
            preview: None,
            inputs: ImageInputs::default(),
            asset_errors: Default::default(),
            frame: 0,
            saved_revision: None,
            stamp: None,
            compiled: None,
        };
        project.load_available_inputs();
        Ok(project)
    }
    pub fn dirty(&self) -> bool {
        self.saved_revision != Some(self.document.revision())
    }
    pub fn time(&self) -> Result<Time> {
        Ok(Time::at_frame(
            self.frame,
            self.document.snapshot().frame_rate,
        )?)
    }

    pub fn plan(&mut self) -> Result<Arc<Plan>> {
        let revision = self.document.revision();
        let output = self.preview.unwrap_or(self.document.snapshot().output);
        if let Some((cached_revision, cached_output, plan)) = &mut self.compiled
            && *cached_output == output
            && (*cached_revision == revision
                || plan.matches_composition(self.document.snapshot(), output)?)
        {
            *cached_revision = revision;
            return Ok(plan.clone());
        }
        let plan = Arc::new(Plan::compile_output(self.document.snapshot(), output)?);
        self.compiled = Some((revision, output, plan.clone()));
        Ok(plan)
    }
    fn load_available_inputs(&mut self) {
        let mut remaining = 256 * 1024 * 1024;
        for node in self.document.snapshot().nodes.values() {
            if let Operation::Image { asset } = &node.operation
                && !self.inputs.contains_key(asset)
                && !self.asset_errors.contains_key(asset)
            {
                match veylvik_io::load_image(
                    &self.base.join(asset),
                    remaining.min(128 * 1024 * 1024),
                ) {
                    Ok(image) => {
                        remaining -= image.byte_size();
                        self.inputs.insert(asset, Arc::new(image));
                    }
                    Err(error) => {
                        self.asset_errors.insert(asset.clone(), error.to_string());
                    }
                }
            }
        }
    }
    pub fn unavailable_assets(&self) -> std::collections::BTreeMap<&str, &str> {
        self.document
            .snapshot()
            .nodes
            .values()
            .filter_map(|node| {
                if let Operation::Image { asset } = &node.operation
                    && !self.inputs.contains_key(asset)
                {
                    Some((
                        asset.as_str(),
                        self.asset_errors
                            .get(asset)
                            .map(String::as_str)
                            .unwrap_or("Source is not loaded; reload or replace it."),
                    ))
                } else {
                    None
                }
            })
            .collect()
    }
    #[cfg(test)]
    pub fn reload_inputs(&mut self) -> Result<()> {
        self.inputs = load_inputs(self.document.snapshot(), &self.base, &self.inputs)?;
        Ok(())
    }
    pub fn apply(&mut self, commands: &[Command]) -> Result<()> {
        self.document.apply(self.document.revision(), commands)?;
        self.reconcile();
        Ok(())
    }
    pub fn connect(&mut self, source: NodeId, target: NodeId, port: &str) -> Result<bool> {
        let doc = self.document.snapshot();
        if !doc.nodes.contains_key(&source) {
            return Err("source node does not exist".into());
        }
        let mut node = doc
            .nodes
            .get(&target)
            .ok_or("target node does not exist")?
            .clone();
        if !node.operation.inputs().contains(&port) {
            return Err("unknown input port".into());
        }
        if node.inputs.get(port) == Some(&source) {
            return Ok(false);
        }
        node.inputs.insert(port.to_owned(), source);
        self.apply(&[Command::Put {
            node: Box::new(node),
        }])?;
        Ok(true)
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
    #[cfg(test)]
    pub fn add(&mut self, operation: Operation) -> Result<NodeId> {
        self.add_at(operation, None)
    }
    pub fn add_at(
        &mut self,
        operation: Operation,
        position: Option<veylvik_document::NodePosition>,
    ) -> Result<NodeId> {
        let ports: Vec<_> = operation
            .inputs()
            .iter()
            .map(|port| (*port, self.selected))
            .collect();
        let mut node = Node::new(operation.descriptor().label, operation.clone(), &ports);
        node.position = position;
        let id = node.id;
        self.apply(&[Command::Put {
            node: Box::new(node),
        }])?;
        self.selected = id;
        self.preview = Some(id);
        Ok(id)
    }
    pub fn duplicate_at(&mut self, position: veylvik_document::NodePosition) -> Result<NodeId> {
        let mut node = self.document.snapshot().nodes[&self.selected].clone();
        node.id = NodeId::new_v4();
        node.label.push_str(" copy");
        node.position = Some(position);
        let id = node.id;
        self.apply(&[Command::Put {
            node: Box::new(node),
        }])?;
        self.selected = id;
        self.preview = Some(id);
        Ok(id)
    }
    pub fn insert_branch(&mut self, copy: veylvik_document::editing::BranchCopy) -> Result<NodeId> {
        let id = copy.root;
        self.apply(&copy.into_commands())?;
        self.selected = id;
        self.preview = Some(id);
        Ok(id)
    }
    pub fn insert_effect(&mut self, effect: veylvik_document::recipes::Effect) -> Result<NodeId> {
        let id = effect.root;
        self.apply(&effect.into_commands())?;
        self.selected = id;
        self.preview = Some(id);
        Ok(id)
    }
    pub fn save_snapshot(&self) -> SaveSnapshot {
        SaveSnapshot {
            composition: self.document.snapshot().clone(),
            base: self.base.clone(),
            stamp: self.stamp.clone(),
            revision: self.document.revision(),
        }
    }
    pub fn mark_saved(&mut self, saved: Saved) {
        self.path = Some(saved.stamp.path().to_path_buf());
        self.stamp = Some(saved.stamp);
        self.saved_revision = Some(saved.revision);
    }
    pub fn replace_image(
        &mut self,
        id: NodeId,
        path: &Path,
        image: Arc<veylvik_render::Image>,
    ) -> Result<()> {
        let mut node = self
            .document
            .snapshot()
            .nodes
            .get(&id)
            .ok_or("Image node no longer exists")?
            .clone();
        let Operation::Image { asset } = &node.operation else {
            return Err("Selected node is not an image".into());
        };
        let next = path.to_str().ok_or("image path is not UTF-8")?.to_owned();
        if asset != &next {
            node.operation = Operation::Image {
                asset: next.clone(),
            };
            self.apply(&[Command::Put {
                node: Box::new(node),
            }])?;
        }
        // Retain other loaded sources so undo can restore the previous image.
        self.inputs.insert(next, image);
        Ok(())
    }
    pub fn import(
        &mut self,
        path: &Path,
        image: Arc<veylvik_render::Image>,
        position: veylvik_document::NodePosition,
    ) -> Result<()> {
        let asset = path.to_str().ok_or("image path is not UTF-8")?.to_owned();
        self.add_at(
            Operation::Image {
                asset: asset.clone(),
            },
            Some(position),
        )?;
        self.inputs.insert(asset, image);
        Ok(())
    }
}
/// Build a complete replacement snapshot before publishing any changed sources.
pub fn load_inputs(
    composition: &Composition,
    base: &Path,
    previous: &ImageInputs,
) -> Result<ImageInputs> {
    // Preserve sources retained for undo/redo; only current nodes read from disk.
    let mut inputs = previous.clone();
    let mut seen = std::collections::BTreeSet::new();
    const BUDGET: u64 = 256 * 1024 * 1024;
    for node in composition.nodes.values() {
        if let Operation::Image { asset } = &node.operation
            && seen.insert(asset)
        {
            let replaced = inputs.get(asset).map_or(0, |image| image.byte_size());
            let remaining = BUDGET.saturating_sub(inputs.byte_size().saturating_sub(replaced));
            let image = veylvik_io::load_image(&base.join(asset), remaining.min(128 * 1024 * 1024))
                .map_err(|error| format!("Image {asset}: {error}"))?;
            inputs.insert(asset, Arc::new(image));
            // Aliased keys may still retain the replaced allocation.
            if inputs.byte_size() > BUDGET {
                return Err(
                    "Reloaded and retained images exceed the 256 MiB host image budget".into(),
                );
            }
        }
    }
    Ok(inputs)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_sources_do_not_prevent_opening_or_recovering_a_valid_graph() {
        let root = std::env::temp_dir().join(format!("veylvik-missing-{}", NodeId::new_v4()));
        std::fs::create_dir(&root).unwrap();
        veylvik_io::save_frame(&root.join("available.png"), 1, 1, &[[0.0, 1.0, 0.0, 1.0]]).unwrap();
        let mut doc = veylvik_document::blank(1, 1, Time::new(24, 1).unwrap()).unwrap();
        doc.nodes.get_mut(&doc.output).unwrap().operation = Operation::Image {
            asset: "missing.png".into(),
        };
        let available = Node::new(
            "Available",
            Operation::Image {
                asset: "available.png".into(),
            },
            &[],
        );
        doc.nodes.insert(available.id, available);
        let path = root.join("project.veylvik");
        doc.save(&path).unwrap();
        let mut project = Project::open(Some(&path)).unwrap();
        assert!(!project.dirty());
        assert_eq!(project.document.revision(), 0);
        assert!(project.inputs.contains_key("available.png"));
        assert!(project.unavailable_assets().contains_key("missing.png"));
        let store = veylvik_io::recovery::Store::new(root.join("recovery"));
        let id = NodeId::new_v4();
        store.write(id, &doc, &root).unwrap();
        let recovered = Project::recover(&store, id).unwrap();
        assert!(recovered.path.is_none() && recovered.dirty());
        assert_eq!(recovered.unavailable_assets().len(), 1);
        assert_eq!(recovered.document.snapshot().nodes.len(), 2);
        let replacement =
            Arc::new(veylvik_render::Image::new(1, 1, vec![[0.0, 0.0, 1.0, 1.0]]).unwrap());
        project
            .replace_image(doc.output, Path::new("replacement.png"), replacement)
            .unwrap();
        assert!(project.unavailable_assets().is_empty());
        project.undo(false).unwrap();
        assert_eq!(project.unavailable_assets().len(), 1);
        veylvik_io::save_frame(&root.join("missing.png"), 1, 1, &[[1.0, 0.0, 0.0, 1.0]]).unwrap();
        project.reload_inputs().unwrap();
        assert!(project.unavailable_assets().is_empty());
        assert_eq!(
            project.inputs.get("missing.png").unwrap().pixels(),
            &[[1.0, 0.0, 0.0, 1.0]]
        );
        assert_eq!(
            serde_json::to_value(Composition::load(&path).unwrap()).unwrap(),
            serde_json::to_value(doc).unwrap()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn image_replacement_preserves_node_wiring_and_retains_pixels_for_undo() {
        let mut project = Project::blank(1, 1, Time::new(24, 1).unwrap(), 24).unwrap();
        let old = Arc::new(veylvik_render::Image::new(1, 1, vec![[1.0, 0.0, 0.0, 1.0]]).unwrap());
        let new = Arc::new(veylvik_render::Image::new(1, 1, vec![[0.0, 1.0, 0.0, 1.0]]).unwrap());
        project
            .import(
                Path::new("old.png"),
                old.clone(),
                veylvik_document::NodePosition { x: 17.0, y: 23.0 },
            )
            .unwrap();
        let image = project.selected;
        let original = project.document.snapshot().nodes[&image].clone();
        let effect = project
            .add(Operation::Exposure { stops: 1.0.into() })
            .unwrap();
        project.apply(&[Command::Output { id: effect }]).unwrap();
        let steps = project.document.history().undo_steps;
        project
            .replace_image(image, Path::new("new.png"), new.clone())
            .unwrap();
        let changed = &project.document.snapshot().nodes[&image];
        assert_eq!(changed.position, original.position);
        assert_eq!(changed.label, original.label);
        assert_eq!(
            project.document.snapshot().nodes[&effect].inputs["image"],
            image
        );
        assert_eq!(project.document.snapshot().output, effect);
        assert_eq!(project.document.history().undo_steps, steps + 1);
        assert!(Arc::ptr_eq(project.inputs.get("old.png").unwrap(), &old));
        assert!(Arc::ptr_eq(project.inputs.get("new.png").unwrap(), &new));
        project.undo(false).unwrap();
        assert_eq!(
            serde_json::to_value(&project.document.snapshot().nodes[&image]).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
        project.undo(true).unwrap();
        let revision = project.document.revision();
        project
            .replace_image(image, Path::new("new.png"), new.clone())
            .unwrap();
        assert_eq!(project.document.revision(), revision);
        assert!(
            project
                .replace_image(effect, Path::new("bad.png"), new)
                .is_err()
        );
        assert_eq!(project.document.revision(), revision);
        assert!(!project.inputs.contains_key("bad.png"));
    }
    #[test]
    fn source_reload_replaces_all_inputs_without_changing_history_and_failure_keeps_old_images() {
        let root = std::env::temp_dir().join(format!("veylvik-reload-{}", NodeId::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let a = root.join("a.png");
        let b = root.join("b.exr");
        veylvik_io::save_frame(&a, 1, 1, &[[1.0, 0.0, 0.0, 1.0]]).unwrap();
        veylvik_io::save_frame(&b, 1, 1, &[[0.0, 0.0, 2.0, 1.0]]).unwrap();
        let mut project = Project::blank(1, 1, Time::new(24, 1).unwrap(), 24).unwrap();
        project.base = root.clone();
        let mut first = Node::new(
            "A",
            Operation::Image {
                asset: "a.png".into(),
            },
            &[],
        );
        first.id = NodeId::from_u128(1);
        let mut second = Node::new(
            "B",
            Operation::Image {
                asset: "b.exr".into(),
            },
            &[],
        );
        second.id = NodeId::from_u128(2);
        project
            .apply(&[
                Command::Put {
                    node: Box::new(first),
                },
                Command::Put {
                    node: Box::new(second),
                },
            ])
            .unwrap();
        project.reload_inputs().unwrap();
        let before = serde_json::to_vec(project.document.snapshot()).unwrap();
        let revision = project.document.revision();
        let old_a = project.inputs.get("a.png").unwrap().clone();
        let old_b = project.inputs.get("b.exr").unwrap().clone();
        project.inputs.insert("historical.png", old_a.clone());
        veylvik_io::save_frame(&a, 1, 1, &[[0.0, 1.0, 0.0, 1.0]]).unwrap();
        std::fs::remove_file(&b).unwrap();
        assert!(
            project
                .reload_inputs()
                .unwrap_err()
                .to_string()
                .contains("b.exr")
        );
        assert!(Arc::ptr_eq(project.inputs.get("a.png").unwrap(), &old_a));
        assert!(Arc::ptr_eq(project.inputs.get("b.exr").unwrap(), &old_b));
        veylvik_io::save_frame(&b, 1, 1, &[[0.0, 0.0, 4.0, 1.0]]).unwrap();
        project.reload_inputs().unwrap();
        assert!(!Arc::ptr_eq(project.inputs.get("a.png").unwrap(), &old_a));
        assert_eq!(
            project.inputs.get("a.png").unwrap().pixels(),
            &[[0.0, 1.0, 0.0, 1.0]]
        );
        assert_eq!(
            project.inputs.get("b.exr").unwrap().pixels(),
            &[[0.0, 0.0, 4.0, 1.0]]
        );
        assert!(Arc::ptr_eq(
            project.inputs.get("historical.png").unwrap(),
            &old_a
        ));
        assert_eq!(project.document.revision(), revision);
        assert_eq!(project.document.history().undo_steps, 1);
        assert_eq!(
            serde_json::to_vec(project.document.snapshot()).unwrap(),
            before
        );
        assert_eq!(old_a.pixels(), &[[1.0, 0.0, 0.0, 1.0]]);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn duplication_preserves_connections_and_keys_with_independent_history() {
        let mut project = Project::open(None).unwrap();
        project.selected = project
            .document
            .snapshot()
            .nodes
            .values()
            .find(|node| matches!(node.operation, Operation::Affine { .. }))
            .unwrap()
            .id;
        let source = project.selected;
        let original = project.document.snapshot().nodes[&source].clone();
        let output = project.document.snapshot().output;
        let count = project.document.snapshot().nodes.len();
        let copy = project
            .duplicate_at(veylvik_document::NodePosition { x: 250.0, y: 180.0 })
            .unwrap();
        assert_ne!(copy, source);
        let duplicate = project.document.snapshot().nodes[&copy].clone();
        assert_eq!(duplicate.inputs, original.inputs);
        assert_eq!(
            serde_json::to_value(&duplicate.operation).unwrap(),
            serde_json::to_value(&original.operation).unwrap()
        );
        assert_eq!(project.document.snapshot().output, output);
        assert_eq!(project.selected, copy);
        assert_eq!(project.preview, Some(copy));
        assert_eq!(project.document.history().undo_steps, 1);
        let mut changed = duplicate.clone();
        changed.operation = Operation::Exposure { stops: 3.0.into() };
        project
            .apply(&[Command::Put {
                node: Box::new(changed),
            }])
            .unwrap();
        assert_eq!(
            serde_json::to_value(&project.document.snapshot().nodes[&source]).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
        project.undo(false).unwrap();
        project.undo(false).unwrap();
        assert_eq!(project.document.snapshot().nodes.len(), count);
        assert_eq!(project.selected, output);
        project.undo(true).unwrap();
        assert_eq!(
            serde_json::to_value(&project.document.snapshot().nodes[&copy]).unwrap(),
            serde_json::to_value(&duplicate).unwrap()
        );
        let revision = project.document.revision();
        let selected = project.selected;
        assert!(
            project
                .duplicate_at(veylvik_document::NodePosition {
                    x: f32::NAN,
                    y: 0.0
                })
                .is_err()
        );
        assert_eq!(project.document.revision(), revision);
        assert_eq!(project.selected, selected);
    }
    #[test]
    fn socket_rewiring_is_atomic_undoable_and_ignores_noops() {
        let mut project = Project::open(None).unwrap();
        let output = project.selected;
        let first = project
            .add(Operation::Exposure { stops: 1.0.into() })
            .unwrap();
        let second = project
            .add(Operation::Exposure { stops: 2.0.into() })
            .unwrap();
        let revision = project.document.revision();
        assert!(!project.connect(first, second, "image").unwrap());
        assert_eq!(project.document.revision(), revision);
        assert!(project.connect(second, first, "image").is_err());
        assert!(project.connect(first, second, "unknown").is_err());
        assert_eq!(project.document.revision(), revision);
        assert!(project.connect(output, second, "image").unwrap());
        assert_eq!(
            project.document.snapshot().nodes[&second].inputs["image"],
            output
        );
        project.undo(false).unwrap();
        assert_eq!(
            project.document.snapshot().nodes[&second].inputs["image"],
            first
        );
        project.undo(true).unwrap();
        assert_eq!(
            project.document.snapshot().nodes[&second].inputs["image"],
            output
        );
    }
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
        assert!(
            project
                .apply(&[Command::Put {
                    node: Box::new(node)
                }])
                .is_err()
        );
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
    fn authoring_changes_reuse_plans_but_render_dependencies_invalidate_them() {
        let mut project = Project::open(None).unwrap();
        let first = project.plan().unwrap();
        project.frame = 20;
        assert!(Arc::ptr_eq(&first, &project.plan().unwrap()));
        let mut node = project.document.snapshot().nodes[&project.selected].clone();
        node.label = "Changed".into();
        project
            .apply(&[Command::Put {
                node: Box::new(node),
            }])
            .unwrap();
        assert!(Arc::ptr_eq(&first, &project.plan().unwrap()));
        project
            .apply(&[Command::Place {
                id: project.selected,
                position: Some(veylvik_document::NodePosition { x: 100.0, y: 120.0 }),
            }])
            .unwrap();
        assert!(Arc::ptr_eq(&first, &project.plan().unwrap()));
        let mut node = project.document.snapshot().nodes[&project.selected].clone();
        node.operation = Operation::Exposure { stops: 2.0.into() };
        project
            .apply(&[Command::Put {
                node: Box::new(node),
            }])
            .unwrap();
        let changed = project.plan().unwrap();
        assert!(!Arc::ptr_eq(&first, &changed));
        project.undo(false).unwrap();
        assert!(!Arc::ptr_eq(&changed, &project.plan().unwrap()));
    }
}

pub struct SaveSnapshot {
    composition: Composition,
    base: PathBuf,
    stamp: Option<veylvik_io::FileStamp>,
    revision: u64,
}
pub struct Saved {
    pub stamp: veylvik_io::FileStamp,
    pub revision: u64,
}
impl SaveSnapshot {
    pub fn save(self, path: &Path) -> Result<Saved> {
        let stamp =
            veylvik_io::save_tracked(&self.composition, &self.base, path, self.stamp.as_ref())?;
        Ok(Saved {
            stamp,
            revision: self.revision,
        })
    }
}

#[cfg(test)]
mod save_tests {
    use super::*;
    #[test]
    fn completing_an_older_save_does_not_mark_newer_edits_clean() {
        let mut project = Project::open(None).unwrap();
        let snapshot = project.save_snapshot();
        project
            .apply(&[Command::Resize {
                width: 120,
                height: 80,
            }])
            .unwrap();
        let path = std::env::temp_dir().join(format!(
            "veylvik-save-snapshot-{}-{}.veylvik",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let saved = snapshot.save(&path).unwrap();
        project.mark_saved(saved);
        assert!(project.dirty());
        assert_eq!(project.document.snapshot().width, 120);
        assert_eq!(Composition::load(&path).unwrap().width, 960);
        std::fs::remove_file(path).unwrap();
    }
}
