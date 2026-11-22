use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use veylvik_document::{Command, Composition, Document, Node, NodeId, Operation, Time};
use veylvik_render::{ImageInputs, Plan};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// GUI state owns the same transactional document used by headless clients.
pub struct Project {
    /// Where the composition's files may be read from: anywhere for the artist's own
    /// work, only its own folder for a composition from someone else or from a host.
    pub reach: veylvik_document::access::Reach,
    /// What this project names outside its folder and was not read, when it was opened
    /// confined: for asking the artist.
    pub outside: Vec<(veylvik_document::access::Named, String)>,
    /// Media keys a host answered for: they have no file here to read sound from.
    host_media: std::collections::BTreeSet<String>,
    /// The host asked for media before anyone else: a composition rendered for a host
    /// with no editor open carries its own.
    pub host: Option<std::sync::Arc<dyn crate::host::Host>>,
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
/// A host's footage in the composition (see [`Project::host_footage`]).
#[derive(Clone)]
pub struct HostFootage {
    pub key: String,
    pub timing: (f64, f64, Option<f64>),
    pub length: Option<f64>,
    pub host: std::sync::Arc<dyn crate::host::Host>,
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
        // A project file opened for the first time that reads files elsewhere is read
        // only inside its folder until the artist allows the rest.
        let outside = match &path {
            Some(file) if !crate::trust::is_trusted(file) => {
                veylvik_document::access::outside(&composition, &base)
            }
            _ => Vec::new(),
        };
        let selected = composition.output;
        let mut project = Self {
            reach: Default::default(),
            outside: Vec::new(),
            host_media: Default::default(),
            host: None,
            session: NodeId::new_v4(),
            recovery_origin: None,
            document: Document::new(composition)?,
            // The bundled demo is a clean starting point, even without a save path.
            saved_revision: Some(0),
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
        if !outside.is_empty() {
            project.reach = veylvik_document::access::Reach::Within(Vec::new());
            project.outside = outside;
        }
        project.load_available_inputs();
        Ok(project)
    }
    /// A composition a host owns: nothing here knows a file for it, and its relative
    /// paths are in `assets`.
    pub fn hosted(composition: veylvik_document::Composition, assets: PathBuf) -> Result<Self> {
        Self::hosted_by(composition, assets, None)
    }
    /// As [`Project::hosted`], asking `host` for media first.
    pub fn hosted_by(
        composition: veylvik_document::Composition,
        assets: PathBuf,
        host: Option<std::sync::Arc<dyn crate::host::Host>>,
    ) -> Result<Self> {
        let selected = composition.output;
        let mut project = Self {
            reach: Default::default(),
            outside: Vec::new(),
            host_media: Default::default(),
            host,
            session: NodeId::new_v4(),
            recovery_origin: None,
            document: Document::new(composition)?,
            saved_revision: Some(0),
            compiled: None,
            stamp: None,
            base: assets,
            path: None,
            selected,
            preview: None,
            inputs: ImageInputs::default(),
            asset_errors: Default::default(),
            frame: 0,
        };
        // A host's composition comes from a project that may have been shared: its
        // files are read only from its own folder.
        project.reach = veylvik_document::access::Reach::Within(Vec::new());
        project.load_available_inputs();
        Ok(project)
    }
    pub fn example(id: &str) -> Result<Self> {
        let scene = veylvik_document::presets::showcase::build(id)?;
        let mut project = Self::blank(960, 540, Time::new(24, 1)?, 96)?;
        let label = veylvik_document::presets::builtin::ALL
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.label);
        project.selected = scene
            .composition
            .groups
            .values()
            .find(|group| Some(group.label.as_str()) == label)
            .map_or(scene.composition.output, |group| group.output);
        project.frame = 48;
        project.document = Document::new(scene.composition)?;
        // Like the startup demo, viewing or playing a bundled example is clean.
        project.saved_revision = Some(0);
        Ok(project)
    }
    pub fn blank(width: u32, height: u32, frame_rate: Time, frame_count: u32) -> Result<Self> {
        let mut composition = veylvik_document::blank(width, height, frame_rate)?;
        composition.frame_count = frame_count;
        let selected = composition.output;
        Ok(Self {
            reach: Default::default(),
            outside: Vec::new(),
            host_media: Default::default(),
            host: None,
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
            reach: Default::default(),
            outside: Vec::new(),
            host_media: Default::default(),
            host: None,
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
        self.inputs.set_base_directory(self.base.clone());
        self.inputs.set_reach(self.reach.clone());
        let mut remaining = veylvik_io::runtime::source_budget();
        for node in self.document.snapshot().nodes.values() {
            if let Some(key) = node.operation.source_key()
                && !self.inputs.contains_key(&key)
                && !self.asset_errors.contains_key(&*key)
            {
                // Media the host keeps is asked of the host, never looked for on disk.
                let theirs = self
                    .host
                    .as_ref()
                    .and_then(|host| host.media(&key))
                    .or_else(|| crate::files::host_media(&key));
                if let Some(source) = theirs {
                    self.host_media.insert(key.to_string());
                    self.inputs.insert_source(key.into_owned(), source);
                    continue;
                }
                // A key nobody here knows: the file the composition names beside it.
                let aside = match &node.operation {
                    veylvik_document::Operation::Footage(footage)
                        if !footage.fallback.is_empty() =>
                    {
                        Some(veylvik_document::access::resolve(
                            &self.base,
                            &footage.fallback,
                            &self.reach,
                        ))
                    }
                    _ => None,
                };
                // Read only what the composition may read (see `reach`).
                let (file, _) = veylvik_document::split_source_key(&key);
                if let Err(refused) =
                    veylvik_document::access::resolve(&self.base, file, &self.reach)
                {
                    let refused = match aside {
                        Some(Ok(_)) => None,
                        _ => Some(refused),
                    };
                    if let Some(refused) = refused {
                        self.asset_errors.insert(key.into_owned(), refused);
                        continue;
                    }
                }
                let aside: Option<PathBuf> = aside.and_then(|file| file.ok());
                let budget = remaining.min(128 * 1024 * 1024);
                let named =
                    veylvik_document::access::resolve(&self.base, file, &self.reach).is_ok();
                match (if named {
                    veylvik_io::footage::load_key(&self.base, &key, budget)
                } else {
                    Err("outside the composition's folder".into())
                })
                .or_else(|error| match &aside {
                    Some(file) => veylvik_io::footage::load(file, budget),
                    None => Err(error),
                }) {
                    Ok(image) => {
                        remaining -= image.byte_size();
                        image.insert(&mut self.inputs, key.into_owned());
                    }
                    Err(error) => {
                        self.asset_errors
                            .insert(key.into_owned(), error.to_string());
                    }
                }
            }
            // 3D models load with the project, by absolute path.
            if let veylvik_document::Operation::Model3d(m) = &node.operation
                && !m.model.is_empty()
                && self.inputs.model(&m.model).is_none()
                && !self.asset_errors.contains_key(m.model.as_str())
            {
                let file = veylvik_document::access::resolve(&self.base, &m.model, &self.reach);
                match file.and_then(|file| veylvik_io::model::load(&file)) {
                    Ok(model) => self.inputs.insert_model(m.model.clone(), Arc::new(model)),
                    Err(error) => {
                        self.asset_errors.insert(m.model.clone(), error);
                    }
                }
            }
        }
    }
    /// Footage a host gives, for the preview to play the host's own sound of: its key,
    /// its timing (start, source in, source out in seconds) and the media's length when
    /// known, and the host that answers for it.
    pub fn host_footage(&self) -> Vec<HostFootage> {
        let Some(host) = self.host.clone().or_else(crate::files::host) else {
            return Vec::new();
        };
        let doc = self.document.snapshot();
        doc.nodes
            .values()
            .filter(|node| !node.bypass)
            .filter_map(|node| {
                let Operation::Footage(footage) = &node.operation else {
                    return None;
                };
                let key = node.operation.source_key()?.into_owned();
                if !self.host_media.contains(&key) {
                    return None;
                }
                let timing = (
                    footage.timing.start.seconds().ok()?,
                    footage.timing.source_in.seconds().ok()?,
                    match footage.timing.source_out {
                        Some(out) => Some(out.seconds().ok()?),
                        None => None,
                    },
                );
                let length = self
                    .inputs
                    .info(&key)
                    .and_then(|info| info.duration)
                    .and_then(|d| d.seconds().ok());
                Some(HostFootage {
                    key,
                    timing,
                    length,
                    host: host.clone(),
                })
            })
            .collect()
    }
    /// The composition as the sound mix should read it: footage a host gives has no file
    /// here, so it is left out until hosts give sound too.
    pub fn sound_composition(&self) -> veylvik_document::Composition {
        let mut composition = self.document.snapshot().clone();
        composition.nodes.retain(|_, node| {
            node.operation
                .source_key()
                .is_none_or(|key| !self.host_media.contains(&*key))
        });
        composition
    }
    /// A file the artist added themselves may be read wherever it is, also in a project
    /// that is confined to its folder.
    pub fn allow_added(&mut self, path: &Path) {
        if let veylvik_document::access::Reach::Within(also) = &mut self.reach {
            also.push(path.to_string_lossy().into_owned());
            self.inputs.set_reach(self.reach.clone());
        }
    }
    /// Sources that failed to load, by asset path, with the reason.
    pub fn unavailable_assets(&self) -> std::collections::BTreeMap<String, String> {
        self.document
            .snapshot()
            .nodes
            .values()
            .filter_map(|node| {
                let key = node.operation.source_key()?;
                if !self.inputs.contains_key(&key) {
                    Some((
                        node.operation.asset().unwrap_or_default().to_owned(),
                        self.asset_errors
                            .get(&*key)
                            .map(String::as_str)
                            .unwrap_or("Source is not loaded; reload or replace it.")
                            .to_owned(),
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
    /// Unlink an optional input; required ports always stay connected.
    pub fn disconnect(&mut self, target: NodeId, port: &str) -> Result<bool> {
        let mut node = self
            .document
            .snapshot()
            .nodes
            .get(&target)
            .ok_or("target node does not exist")?
            .clone();
        if !node.operation.descriptor().is_optional(port) {
            return Err("only optional inputs can be disconnected".into());
        }
        if node.inputs.remove(port).is_none() {
            return Ok(false);
        }
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
        mut operation: Operation,
        position: Option<veylvik_document::NodePosition>,
    ) -> Result<NodeId> {
        if let Operation::OpticalFlow(flow) = &mut operation {
            let doc = self.document.snapshot();
            let mut pending = vec![self.selected];
            let mut visited = std::collections::BTreeSet::new();
            let mut rates = Vec::new();
            while let Some(id) = pending.pop() {
                if !visited.insert(id) {
                    continue;
                }
                if let Some(node) = doc.nodes.get(&id) {
                    if let Operation::Footage(clip) = &node.operation
                        && let Some(rate) = self
                            .inputs
                            .info(&clip.asset)
                            .and_then(|info| info.frame_rate)
                    {
                        rates.push(rate);
                    }
                    pending.extend(node.inputs.values().copied());
                }
            }
            flow.source_rate = rates
                .first()
                .copied()
                .filter(|first| {
                    rates.iter().all(|rate| {
                        i128::from(rate.ticks) * i128::from(first.rate)
                            == i128::from(first.ticks) * i128::from(rate.rate)
                    })
                })
                .unwrap_or(doc.frame_rate);
        }
        // Optional ports start unconnected.
        let ports: Vec<_> = operation
            .descriptor()
            .required()
            .map(|port| (port, self.selected))
            .collect();
        let mut node = Node::new(operation.descriptor().label, operation.clone(), &ports);
        node.position = position;
        let id = node.id;
        let put = Command::Put {
            node: Box::new(node),
        };
        // A node with one input goes in line after the selected node: whatever read the
        // selection now reads the new node, so it shows in the picture at once. Where
        // that cannot be done (inside an effect group, say) it is added beside it.
        let source = self.selected;
        let doc = self.document.snapshot();
        let in_line: Vec<Command> = if ports.len() == 1
            && operation.descriptor().output == veylvik_document::registry::PortKind::Image
        {
            let mut commands: Vec<Command> = doc
                .nodes
                .values()
                .flat_map(|consumer| {
                    consumer
                        .inputs
                        .iter()
                        .filter(|(_, from)| **from == source)
                        .map(|(port, _)| Command::Connect {
                            id: consumer.id,
                            input: port.clone(),
                            source: Some(id),
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            if doc.output == source {
                commands.push(Command::Output { id });
            }
            commands
        } else {
            Vec::new()
        };
        let spliced =
            !in_line.is_empty() && self.apply(&[vec![put.clone()], in_line].concat()).is_ok();
        if !spliced {
            self.apply(&[put])?;
        }
        self.selected = id;
        // In the picture: keep looking at the composition. Beside it: look at the node.
        self.preview = if spliced && self.feeds_output(id) {
            None
        } else {
            Some(id)
        };
        Ok(id)
    }
    /// Whether `id` is the composition's output or somewhere upstream of it.
    fn feeds_output(&self, id: NodeId) -> bool {
        let doc = self.document.snapshot();
        let mut pending = vec![doc.output];
        let mut seen = std::collections::BTreeSet::new();
        while let Some(node) = pending.pop() {
            if node == id {
                return true;
            }
            if seen.insert(node)
                && let Some(node) = doc.nodes.get(&node)
            {
                pending.extend(node.inputs.values().copied());
            }
        }
        false
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
        let commands = effect.into_grouped_commands(self.document.snapshot())?;
        self.apply(&commands)?;
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
        image: impl Into<veylvik_io::footage::Media>,
    ) -> Result<()> {
        let image = image.into();
        let temporal = !matches!(&image, veylvik_io::footage::Media::Still(_));
        let mut node = self
            .document
            .snapshot()
            .nodes
            .get(&id)
            .ok_or("Image node no longer exists")?
            .clone();
        let Some(asset) = node.operation.asset() else {
            return Err("Selected node is not an image".into());
        };
        let next = path.to_str().ok_or("image path is not UTF-8")?.to_owned();
        let promote = temporal
            && matches!(
                node.operation,
                Operation::Image(veylvik_document::nodes::Image { .. })
            );
        if asset != next || promote {
            if promote {
                node.operation = Operation::Footage(veylvik_document::nodes::Footage {
                    asset: next.clone(),
                    timing: Default::default(),
                    placement: Default::default(),
                    colour: Default::default(),
                    layer: None,
                    fallback: String::new(),
                });
            } else {
                *node.operation.asset_mut().unwrap() = next.clone();
            }
            self.apply(&[Command::Put {
                node: Box::new(node),
            }])?;
        }
        // Retain other loaded sources so undo can restore the previous image.
        image.insert(&mut self.inputs, next);
        Ok(())
    }
    pub fn is_empty(&self) -> bool {
        let doc = self.document.snapshot();
        doc.nodes.len() == 1
            && doc.groups.is_empty()
            && matches!(doc.nodes[&doc.output].operation, Operation::Solid(veylvik_document::nodes::Solid { color }) if color == [0.0; 4])
    }
    /// Commands that put new media `id` into the picture. Where the composition still
    /// stands on its empty starting background, the media takes that background's place,
    /// under everything already made; otherwise it is laid over the present output.
    fn layer_commands(&self, id: NodeId) -> Vec<Command> {
        let doc = self.document.snapshot();
        let blank = doc.nodes.values().find(|node| {
            node.id != doc.output
                && node.inputs.is_empty()
                && matches!(node.operation, Operation::Solid(veylvik_document::nodes::Solid { color }) if color == [0.0; 4])
        });
        if let Some(blank) = blank {
            let mut commands: Vec<Command> = doc
                .nodes
                .values()
                .flat_map(|consumer| {
                    consumer
                        .inputs
                        .iter()
                        .filter(|(_, from)| **from == blank.id)
                        .map(|(port, _)| Command::Connect {
                            id: consumer.id,
                            input: port.clone(),
                            source: Some(id),
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            if !commands.is_empty() {
                commands.push(Command::Remove { id: blank.id });
                return commands;
            }
        }
        let merge = Node::new(
            "Media over scene",
            Operation::Merge(veylvik_document::nodes::Merge {}),
            &[("background", doc.output), ("foreground", id)],
        );
        let output = merge.id;
        vec![
            Command::Put {
                node: Box::new(merge),
            },
            Command::Output { id: output },
        ]
    }
    pub fn import(
        &mut self,
        path: &Path,
        image: impl Into<veylvik_io::footage::Media>,
        position: veylvik_document::NodePosition,
        settings: Option<crate::composition::Settings>,
    ) -> Result<()> {
        let image = image.into();
        let asset = path.to_str().ok_or("image path is not UTF-8")?.to_owned();
        let empty = self.is_empty();
        let previous_output = self.document.snapshot().output;
        // Media a host keeps is called what the host calls it, not by its key.
        let named = crate::files::host_label(&asset);
        let mut node = Node::new(
            named
                .as_deref()
                .or_else(|| path.file_name().and_then(|s| s.to_str()))
                .unwrap_or("Media"),
            Operation::Footage(veylvik_document::nodes::Footage {
                asset: asset.clone(),
                timing: Default::default(),
                placement: veylvik_document::media::Placement::Fit,
                colour: Default::default(),
                layer: None,
                // The host's media keeps its own file beside its key, for a reader
                // that does not know the host.
                fallback: crate::files::host_file(&asset)
                    .and_then(|file| file.to_str().map(str::to_owned))
                    .unwrap_or_default(),
            }),
            &[],
        );
        node.position = Some(position);
        let id = node.id;
        let mut commands = settings
            .map(|s| s.commands(self.document.snapshot()))
            .transpose()?
            .unwrap_or_default();
        commands.push(Command::Put {
            node: Box::new(node),
        });
        if empty {
            commands.push(Command::Output { id });
            commands.push(Command::Remove {
                id: previous_output,
            });
        } else {
            commands.extend(self.layer_commands(id));
        }
        self.apply(&commands)?;
        self.selected = id;
        // It is in the picture now: keep looking at the composition.
        self.preview = None;
        image.insert(&mut self.inputs, asset);
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
    inputs.set_base_directory(base);
    let mut seen = std::collections::BTreeSet::new();
    let budget = veylvik_io::runtime::source_budget();
    for node in composition.nodes.values() {
        if let Some(key) = node.operation.source_key()
            && seen.insert(key.clone())
        {
            // A file the composition may not read stays unavailable (see `Project::reach`).
            let (file, _) = veylvik_document::split_source_key(&key);
            if veylvik_document::access::resolve(base, file, inputs.reach()).is_err() {
                continue;
            }
            let replaced = inputs.source_budget(&key);
            let remaining = budget.saturating_sub(inputs.byte_size().saturating_sub(replaced));
            let image = veylvik_io::footage::load_key(base, &key, remaining.min(128 * 1024 * 1024))
                .map_err(|error| format!("Image {}: {error}", key.replace('\0', " layer ")))?;
            image.insert(&mut inputs, key.into_owned());
            // Aliased keys may still retain the replaced allocation.
            if inputs.byte_size() > budget {
                return Err(format!(
                    "Reloaded and retained images exceed the {} MiB host image budget",
                    budget / (1024 * 1024)
                )
                .into());
            }
        }
    }
    Ok(inputs)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matching_import_is_atomic_and_undo_restores_blank_canvas() {
        let mut project = Project::blank(1920, 1080, Time::new(24, 1).unwrap(), 96).unwrap();
        let before = serde_json::to_value(project.document.snapshot()).unwrap();
        let image = Arc::new(veylvik_render::Image::new(8, 4, vec![[1.0; 4]; 32]).unwrap());
        let mut settings = crate::composition::Settings::current(project.document.snapshot());
        settings.width = 8;
        settings.height = 4;
        project
            .import(
                Path::new("plate.png"),
                image.clone(),
                veylvik_document::NodePosition { x: 0.0, y: 0.0 },
                Some(settings),
            )
            .unwrap();
        let id = project.selected;
        assert_eq!(project.document.snapshot().width, 8);
        assert_eq!(project.document.snapshot().output, id);
        assert_eq!(project.document.history().undo_steps, 1);
        assert!(matches!(
            project.document.snapshot().nodes[&id].operation,
            Operation::Footage(veylvik_document::nodes::Footage {
                placement: veylvik_document::media::Placement::Fit,
                ..
            })
        ));
        project.undo(false).unwrap();
        assert_eq!(
            serde_json::to_value(project.document.snapshot()).unwrap(),
            before
        );
        project.undo(true).unwrap();
        assert_eq!(project.document.snapshot().output, id);
        let before = serde_json::to_value(project.document.snapshot()).unwrap();
        settings.width = 0;
        assert!(
            project
                .import(
                    Path::new("bad.png"),
                    image,
                    veylvik_document::NodePosition { x: 0.0, y: 0.0 },
                    Some(settings)
                )
                .is_err()
        );
        assert_eq!(
            serde_json::to_value(project.document.snapshot()).unwrap(),
            before
        );
        assert!(!project.inputs.contains_key("bad.png"));
    }
    /// A composition from someone else naming files outside its folder, every way there
    /// is: an absolute path, `..`, a link inside the folder that leads out, the fallback
    /// beside a host key, a 3D model and a colour configuration.
    fn hostile(root: &Path) -> (PathBuf, Composition) {
        let folder = root.join("shared");
        std::fs::create_dir_all(&folder).unwrap();
        let secret = root.join("secret.png");
        veylvik_io::save_frame(&secret, 1, 1, &[[1.0, 0.0, 0.0, 1.0]]).unwrap();
        veylvik_io::save_frame(&folder.join("inside.png"), 1, 1, &[[0.0, 1.0, 0.0, 1.0]]).unwrap();
        std::os::unix::fs::symlink(root, folder.join("away")).unwrap();
        let mut doc = veylvik_document::blank(1, 1, Time::new(24, 1).unwrap()).unwrap();
        let image = |asset: &str| {
            Operation::Image(veylvik_document::nodes::Image {
                asset: asset.into(),
            })
        };
        doc.nodes.get_mut(&doc.output).unwrap().operation = image("inside.png");
        // A node of `kind` with its default settings, then the hostile ones put in.
        let made = |kind: &str, set: serde_json::Value| -> Operation {
            let mut value = veylvik_document::registry::TYPES
                .iter()
                .find(|t| t.type_id == kind)
                .unwrap()
                .default_operation();
            for (key, field) in set.as_object().unwrap() {
                value[key] = field.clone();
            }
            serde_json::from_value(value).unwrap()
        };
        let fallback = made(
            "footage",
            serde_json::json!({"asset": "host-key-nobody-knows", "fallback": secret}),
        );
        let model = made("model_3d", serde_json::json!({"model": "../secret.glb"}));
        let mut colour = made("ocio", serde_json::json!({}));
        if let Operation::Ocio(ocio) = &mut colour {
            ocio.transform.config = "/etc/ocio/config.ocio".into();
        }
        for (label, operation) in [
            ("Absolute", image(&secret.to_string_lossy())),
            ("Climbing", image("../secret.png")),
            ("Through a link", image("away/secret.png")),
            ("Fallback", fallback),
            ("Model", model),
            ("Colour", colour),
        ] {
            let wired = [("image", doc.output)];
            let inputs: &[(&str, NodeId)] = if label == "Colour" { &wired } else { &[] };
            let node = Node::new(label, operation, inputs);
            doc.nodes.insert(node.id, node);
        }
        (folder, doc)
    }
    fn secret_name(root: &Path) -> String {
        root.join("secret.png").to_string_lossy().into_owned()
    }
    #[test]
    fn a_hosts_media_is_its_own_and_has_no_file_to_read_sound_from() {
        struct Theirs;
        impl veylvik_render::FrameSource for Theirs {
            fn byte_budget(&self) -> u64 {
                0
            }
            fn frame(&self, _: Time) -> veylvik_render::Result<Arc<veylvik_render::Image>> {
                Err(veylvik_render::Error::Message(
                    "pictures only on the GPU".into(),
                ))
            }
        }
        struct Host;
        impl crate::host::Host for Host {
            fn pick(
                &self,
                _: &crate::host::Pick,
            ) -> std::result::Result<Option<crate::host::Picked>, String> {
                Ok(None)
            }
            fn media(&self, key: &str) -> Option<Arc<dyn veylvik_render::FrameSource>> {
                (key == "clip").then(|| Arc::new(Theirs) as Arc<dyn veylvik_render::FrameSource>)
            }
        }
        let composition = Composition::from_host_footage(
            "clip",
            "Handheld",
            None,
            [64, 36],
            Time::new(24, 1).unwrap(),
            48,
        )
        .unwrap();
        let folder = std::env::temp_dir();
        let project =
            Project::hosted_by(composition.clone(), folder.clone(), Some(Arc::new(Host))).unwrap();
        assert!(
            project.unavailable_assets().is_empty(),
            "{:?}",
            project.unavailable_assets()
        );
        // The sound mix never looks for a file called "clip" beside the composition.
        assert!(
            !project
                .sound_composition()
                .nodes
                .values()
                .any(|node| matches!(node.operation, Operation::Footage(_)))
        );
        // Asked of nobody, the key is a missing file.
        let alone = Project::hosted_by(composition, folder, None).unwrap();
        assert!(alone.unavailable_assets().contains_key("clip"));
    }
    #[test]
    fn a_hosts_composition_reads_nothing_outside_its_folder() {
        let root = std::env::temp_dir().join(format!("veylvik-hostile-{}", NodeId::new_v4()));
        let (folder, doc) = hostile(&root);
        let project = Project::hosted_by(doc.clone(), folder.clone(), None).unwrap();
        let errors = project.unavailable_assets();
        assert!(project.inputs.contains_key("inside.png"));
        for name in ["../secret.png", "away/secret.png", "host-key-nobody-knows"] {
            assert!(!project.inputs.contains_key(name), "{name} was read");
        }
        assert!(!project.inputs.contains_key(&secret_name(&root)));
        assert!(project.inputs.model("../secret.glb").is_none());
        assert!(
            errors
                .values()
                .any(|why| why.contains("outside the composition's folder")),
            "{errors:?}"
        );
        assert!(
            project
                .inputs
                .resolve_config("/etc/ocio/config.ocio")
                .is_err()
        );
        // Exports of it are refused before anything is read.
        let error = crate::render_queue::Job::prepare(
            "hostile".into(),
            doc,
            folder.clone(),
            root.join("out.png"),
            crate::render_queue::Output::Frame {
                time: Time::new(0, 1).unwrap(),
                output: project.document.snapshot().output,
            },
            project.reach.clone(),
        )
        .err()
        .unwrap();
        assert!(error.contains("outside"), "{error}");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn a_project_file_opened_the_first_time_asks_before_reading_outside_its_folder() {
        let root = std::env::temp_dir().join(format!("veylvik-hostile-{}", NodeId::new_v4()));
        let (folder, doc) = hostile(&root);
        let path = folder.join("downloaded.veylvik");
        doc.save(&path).unwrap();
        let project = Project::open(Some(&path)).unwrap();
        assert!(project.inputs.contains_key("inside.png"));
        assert!(!project.inputs.contains_key(&secret_name(&root)));
        assert!(project.inputs.model("../secret.glb").is_none());
        let named: Vec<_> = project
            .outside
            .iter()
            .map(|(named, _)| named.label.as_str())
            .collect();
        for label in [
            "Absolute",
            "Climbing",
            "Through a link",
            "Fallback",
            "Model",
            "Colour",
        ] {
            assert!(named.contains(&label), "{label} not asked about: {named:?}");
        }
        // Allowed once, it is read in full from then on.
        crate::trust::trust(&path);
        let project = Project::open(Some(&path)).unwrap();
        assert!(project.outside.is_empty());
        assert!(project.inputs.contains_key(&secret_name(&root)));
        assert!(project.inputs.contains_key("../secret.png"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn missing_sources_do_not_prevent_opening_or_recovering_a_valid_graph() {
        let root = std::env::temp_dir().join(format!("veylvik-missing-{}", NodeId::new_v4()));
        std::fs::create_dir(&root).unwrap();
        veylvik_io::save_frame(&root.join("available.png"), 1, 1, &[[0.0, 1.0, 0.0, 1.0]]).unwrap();
        let mut doc = veylvik_document::blank(1, 1, Time::new(24, 1).unwrap()).unwrap();
        doc.nodes.get_mut(&doc.output).unwrap().operation =
            Operation::Image(veylvik_document::nodes::Image {
                asset: "missing.png".into(),
            });
        let available = Node::new(
            "Available",
            Operation::Image(veylvik_document::nodes::Image {
                asset: "available.png".into(),
            }),
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
                None,
            )
            .unwrap();
        let image = project.selected;
        let original = project.document.snapshot().nodes[&image].clone();
        let effect = project
            .add(Operation::Exposure(veylvik_document::nodes::Exposure {
                stops: 1.0.into(),
            }))
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
            Operation::Image(veylvik_document::nodes::Image {
                asset: "a.png".into(),
            }),
            &[],
        );
        first.id = NodeId::from_u128(1);
        let mut second = Node::new(
            "B",
            Operation::Image(veylvik_document::nodes::Image {
                asset: "b.exr".into(),
            }),
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
            .find(|node| {
                matches!(
                    node.operation,
                    Operation::Affine(veylvik_document::nodes::Affine { .. })
                )
            })
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
        changed.operation =
            Operation::Exposure(veylvik_document::nodes::Exposure { stops: 3.0.into() });
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
            .add(Operation::Exposure(veylvik_document::nodes::Exposure {
                stops: 1.0.into(),
            }))
            .unwrap();
        let second = project
            .add(Operation::Exposure(veylvik_document::nodes::Exposure {
                stops: 2.0.into(),
            }))
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
    fn a_new_node_goes_in_line_after_the_selection_and_invalid_connections_are_atomic() {
        let mut project = Project::open(None).unwrap();
        let output = project.document.snapshot().output;
        let added = project
            .add(Operation::Exposure(veylvik_document::nodes::Exposure {
                stops: 1.0.into(),
            }))
            .unwrap();
        // Added after the output, it reads the old output and becomes the output, so it
        // is in the picture and the viewer stays on the composition.
        assert_eq!(
            project.document.snapshot().nodes[&added].inputs["image"],
            output
        );
        assert_eq!(project.document.snapshot().output, added);
        assert_eq!(project.preview, None);
        // Added after a node in the middle, whatever read that node reads the new one.
        project.selected = output;
        let between = project
            .add(Operation::Exposure(veylvik_document::nodes::Exposure {
                stops: 0.5.into(),
            }))
            .unwrap();
        assert_eq!(
            project.document.snapshot().nodes[&added].inputs["image"],
            between
        );
        assert_eq!(
            project.document.snapshot().nodes[&between].inputs["image"],
            output
        );
        assert_eq!(project.document.snapshot().output, added);
        project.undo(false).unwrap();
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
        // One undo step takes the node and its wiring away together.
        project.undo(false).unwrap();
        assert_eq!(project.document.snapshot().output, output);
        assert!(!project.document.snapshot().nodes.contains_key(&added));
        project.undo(true).unwrap();
        assert!(project.document.snapshot().nodes.contains_key(&added));
        assert_eq!(project.document.snapshot().output, added);
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
        node.operation =
            Operation::Exposure(veylvik_document::nodes::Exposure { stops: 2.0.into() });
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

#[cfg(test)]
mod startup_tests {
    use super::*;
    #[test]
    fn examples_are_clean_until_edited() {
        for entry in veylvik_document::presets::builtin::ALL {
            let mut project = Project::example(entry.id).unwrap();
            assert!(project.path.is_none());
            project.frame = 48;
            assert!(!project.dirty());
            project
                .apply(&[Command::FrameCount { frame_count: 120 }])
                .unwrap();
            assert!(project.dirty());
        }
    }
    #[test]
    fn viewing_the_startup_demo_is_clean_but_authored_work_is_dirty() {
        let mut project = Project::open(None).unwrap();
        assert!(project.path.is_none());
        assert!(!project.dirty());
        project.frame = 48;
        project.selected = *project.document.snapshot().nodes.keys().next().unwrap();
        project.preview = Some(project.selected);
        assert!(!project.dirty());
        project
            .apply(&[Command::FrameCount { frame_count: 100 }])
            .unwrap();
        assert!(project.dirty());
        assert!(
            Project::blank(640, 360, Time::new(24, 1).unwrap(), 100)
                .unwrap()
                .dirty()
        );
    }
}
