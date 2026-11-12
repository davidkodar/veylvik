mod about;
mod animation;
mod animation_editor;
mod composition;
mod control;
mod curve_preview;
mod diagnostics;
mod editable_text;
mod recovery;
mod sequence;
mod text_outline;
mod vector_editor;
use animation::scalar;
mod display;
mod effects;
mod files;
mod graph;
mod handles;
mod key_timeline;
mod menus;
mod node_finder;
mod node_palette;
mod preset_library;
mod preview_images;
mod project;
mod showcase_gallery;
mod transform_handles;
mod viewer;
use display::{Display, DisplayFrame, Paint, ViewSettings};
use eframe::{egui, egui_wgpu};
use files::{Choice, Completed, Tasks};
use project::Project;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use veylvik_document::{Command, Node, Operation};
use veylvik_render::{GpuPreviewScheduler, PreviewTicket, RenderRequest, Renderer};

fn main() {
    if let Err(error) = run() {
        eprintln!("Veylvik: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let mut path = None;
    let mut control_directory = None;
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--control-dir" && control_directory.is_none() && index + 1 < args.len() {
            control_directory = Some(PathBuf::from(&args[index + 1]));
            index += 2;
        } else if path.is_none() && !args[index].to_string_lossy().starts_with("--") {
            path = Some(PathBuf::from(&args[index]));
            index += 1;
        } else {
            return Err("usage: veylvik-studio [FILE] [--control-dir NEW_DIRECTORY]".into());
        }
    }
    let render_limits = veylvik_io::runtime::RenderLimits::from_env()?;
    let project = Project::open(path.as_deref())?;
    let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    setup.instance_descriptor.backends = wgpu::Backends::VULKAN;
    setup.device_descriptor = Arc::new(|adapter| wgpu::DeviceDescriptor {
        label: Some("Veylvik studio"),
        required_limits: veylvik_render::device_limits(adapter),
        ..Default::default()
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Veylvik")
            .with_app_id("veylvik")
            .with_inner_size([1280.0, 850.0])
            .with_min_inner_size([800.0, 600.0]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            wgpu_setup: setup.into(),
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "Veylvik",
        options,
        Box::new(move |cc| {
            let state = cc
                .wgpu_render_state
                .as_ref()
                .ok_or("Vulkan render state unavailable")?;
            let mut renderer = Renderer::from_device(
                state.device.clone(),
                state.queue.clone(),
                state.adapter.get_info(),
            )?;
            render_limits.apply(&mut renderer);
            let renderer = Arc::new(renderer);
            let context = cc.egui_ctx.clone();
            let scheduler = GpuPreviewScheduler::with_renderer(renderer.clone(), move || {
                context.request_repaint()
            })?;
            let display = Display::new(renderer.device(), state.target_format);
            let draft = project.document.snapshot().nodes[&project.selected].clone();
            let control = if let Some(directory) = control_directory {
                let context = cc.egui_ctx.clone();
                Some(veylvik_control::ipc::Server::bind(
                    &directory,
                    control::catalog(),
                    move || context.request_repaint(),
                )?)
            } else {
                None
            };
            let recovery =
                recovery::Recovery::new(recovery::default_store()?, &project, cc.egui_ctx.clone());
            let mut app = Studio {
                control,
                preview_images: None,
                recovery,
                project,
                renderer,
                scheduler: Some(scheduler),
                display,
                view: ViewSettings::default(),
                navigation: viewer::Navigation::default(),
                displayed: None,
                displayed_ticket: None,
                wanted: None,
                preview_pending: false,
                displayed_time: None,
                draft,
                draft_dirty: false,
                text_preview_deadline: None,
                graph: graph::Graph::default(),
                effects: effects::Controls::default(),
                library: preset_library::Library::default(),
                node_palette: node_palette::Controls::default(),
                node_finder: node_finder::Controls::default(),
                key_timeline: key_timeline::Keys::default(),
                animation_editor: animation_editor::Editor::default(),
                status: String::new(),
                error: false,
                playing: false,
                playback: Instant::now(),
                playback_frame: 0,
                files: Tasks::new(cc.egui_ctx.clone()),
                export_controls: sequence::Controls::default(),
                outline_controls: text_outline::Controls::default(),
                composition_controls: composition::Controls::default(),
                confirm: None,
                after_save: None,
                allow_close: false,
                help: false,
                about: about::Controls::default(),
                statistics: false,
                theme_checked: Instant::now() - Duration::from_secs(10),
                accent: egui::Color32::from_rgb(255, 214, 10),
            };
            app.request_preview();
            Ok(Box::new(app))
        }),
    )?;
    Ok(())
}
#[derive(Clone, Copy)]
enum Leave {
    Open,
    New,
    Recover(veylvik_document::NodeId),
    Example(&'static str),
    Close,
}
struct Studio {
    recovery: recovery::Recovery,
    control: Option<veylvik_control::ipc::Server>,
    preview_images: Option<preview_images::Worker>,
    project: Project,
    renderer: Arc<Renderer>,
    scheduler: Option<GpuPreviewScheduler>,
    display: Display,
    view: ViewSettings,
    navigation: viewer::Navigation,
    displayed: Option<Arc<DisplayFrame>>,
    displayed_ticket: Option<PreviewTicket>,
    wanted: Option<PreviewTicket>,
    preview_pending: bool,
    displayed_time: Option<veylvik_document::Time>,
    draft: Node,
    draft_dirty: bool,
    text_preview_deadline: Option<Instant>,
    graph: graph::Graph,
    effects: effects::Controls,
    library: preset_library::Library,
    node_palette: node_palette::Controls,
    node_finder: node_finder::Controls,
    key_timeline: key_timeline::Keys,
    animation_editor: animation_editor::Editor,
    status: String,
    error: bool,
    playing: bool,
    playback: Instant,
    playback_frame: i64,
    files: Tasks,
    export_controls: sequence::Controls,
    outline_controls: text_outline::Controls,
    composition_controls: composition::Controls,
    confirm: Option<Leave>,
    after_save: Option<Leave>,
    allow_close: bool,
    help: bool,
    about: about::Controls,
    statistics: bool,
    theme_checked: Instant,
    accent: egui::Color32,
}
impl Studio {
    fn authoring_dialog_open(&self) -> bool {
        self.library.open
            || self.effects.open
            || self.outline_controls.open
            || self.composition_controls.open
            || self.node_palette.open
            || self.node_finder.open
    }
    fn open_project(&mut self, project: Project, ctx: &egui::Context) {
        let recovery = recovery::Recovery::new(self.recovery.store.clone(), &project, ctx.clone());
        self.recovery.finish(true);
        self.recovery = recovery;
        self.outline_controls = text_outline::Controls::default();
        self.node_palette = node_palette::Controls::default();
        self.node_finder = node_finder::Controls::default();
        self.key_timeline = key_timeline::Keys::default();
        self.animation_editor = animation_editor::Editor::default();
        self.composition_controls = composition::Controls::default();
        self.export_controls = sequence::Controls::default();
        self.project = project;
        self.graph = graph::Graph::default();
        self.effects = effects::Controls::default();
        self.library = preset_library::Library::default();
        self.navigation.fit();
        self.displayed = None;
        self.displayed_ticket = None;
        self.displayed_time = None;
        self.refresh_draft();
        self.request_preview();
    }
    fn apply_composition_settings(&mut self, settings: composition::Settings, ctx: &egui::Context) {
        if settings.new {
            match Project::blank(
                settings.width,
                settings.height,
                settings.frame_rate,
                settings.frame_count,
            ) {
                Ok(project) => self.open_project(project, ctx),
                Err(error) => {
                    self.composition_controls.open = true;
                    self.fail(error);
                }
            }
        } else {
            let doc = self.project.document.snapshot();
            if doc.width == settings.width
                && doc.height == settings.height
                && doc.frame_rate.ticks == settings.frame_rate.ticks
                && doc.frame_rate.rate == settings.frame_rate.rate
                && doc.frame_count == settings.frame_count
            {
                return;
            }
            match self.project.apply(&[
                Command::Resize {
                    width: settings.width,
                    height: settings.height,
                },
                Command::FrameRate {
                    frame_rate: settings.frame_rate,
                },
                Command::FrameCount {
                    frame_count: settings.frame_count,
                },
            ]) {
                Ok(()) => {
                    self.refresh_draft();
                    self.request_preview();
                }
                Err(error) => {
                    self.composition_controls.open = true;
                    self.fail(error);
                }
            }
        }
    }

    fn fail(&mut self, error: impl ToString) {
        self.status = error.to_string();
        self.error = true;
    }
    fn refresh_draft(&mut self) {
        self.text_preview_deadline = None;
        self.effects.edits.clear();
        self.effects.connections.clear();
        self.draft = self.project.document.snapshot().nodes[&self.project.selected].clone();
        self.draft_dirty = false;
    }
    fn can_change_selection(&mut self) -> bool {
        if self.draft_dirty {
            self.fail("Apply or revert the current parameter changes first.");
            false
        } else {
            true
        }
    }
    fn open_node_finder(&mut self) {
        if self.files.busy()
            || self.authoring_dialog_open()
            || !self.can_change_selection()
            || self.graph.dragging()
            || self.animation_editor.dragging()
        {
            return;
        }
        self.playing = false;
        self.node_finder
            .open(self.project.document.snapshot(), self.project.selected);
    }
    fn open_node_palette(&mut self, ctx: &egui::Context) {
        if self.files.busy()
            || self.authoring_dialog_open()
            || !self.can_change_selection()
            || self.graph.dragging()
            || self.animation_editor.dragging()
        {
            return;
        }
        self.playing = false;
        self.node_palette
            .open_at(self.graph.insertion_position(ctx));
    }
    fn add_from_palette(
        &mut self,
        choice: node_palette::Choice,
        position: veylvik_document::NodePosition,
    ) {
        match choice {
            node_palette::Choice::Image => self.import_image(position),
            node_palette::Choice::Text => self.outline_controls.open_at(position),
            node_palette::Choice::Effect(recipe) => {
                let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                    let input = self.project.selected;
                    let mut effect = recipe.build(self.project.document.snapshot(), input)?;
                    self.graph.place_effect(
                        self.project.document.snapshot(),
                        self.project.document.revision(),
                        input,
                        &mut effect,
                        Some(position),
                    )?;
                    self.project.insert_effect(effect)?;
                    Ok(())
                })();
                match result {
                    Ok(()) => {
                        self.refresh_draft();
                        self.request_preview();
                    }
                    Err(error) => self.fail(error),
                }
            }
            node_palette::Choice::Node(kind) => {
                let doc = self.project.document.snapshot();
                let operation = node_palette::operation(kind, doc.width, doc.height);
                let added = if matches!(operation, Operation::Text { .. }) {
                    let mut text = Node::new("Text", operation, &[]);
                    text.position = Some(position);
                    let id = text.id;
                    let mut merge = Node::new(
                        "Text over scene",
                        Operation::Merge {},
                        &[("background", doc.output), ("foreground", id)],
                    );
                    merge.position = Some(veylvik_document::NodePosition {
                        x: position.x + 240.0,
                        y: position.y,
                    });
                    let output = merge.id;
                    self.project
                        .apply(&[
                            Command::Put {
                                node: Box::new(text),
                            },
                            Command::Put {
                                node: Box::new(merge),
                            },
                            Command::Output { id: output },
                        ])
                        .map(|()| {
                            self.project.selected = id;
                            self.project.preview = None;
                            id
                        })
                } else {
                    self.project.add_at(operation, Some(position))
                };
                match added {
                    Ok(_) => {
                        self.refresh_draft();
                        self.request_preview();
                    }
                    Err(error) => self.fail(error),
                }
            }
        }
    }
    fn duplicate_selected(&mut self, ctx: &egui::Context) {
        if !self.can_change_selection() || self.graph.dragging() || self.animation_editor.dragging()
        {
            return;
        }
        self.playing = false;
        if let Some(group) = self
            .graph
            .selected_group(self.project.document.snapshot(), self.project.selected)
        {
            self.duplicate_group(group.id);
            return;
        }
        match self
            .project
            .duplicate_at(self.graph.insertion_position(ctx))
        {
            Ok(_) => {
                self.refresh_draft();
                self.request_preview();
            }
            Err(e) => self.fail(e),
        }
    }
    fn duplicate_branch(&mut self) {
        if !self.can_change_selection() || self.graph.dragging() || self.animation_editor.dragging()
        {
            return;
        }
        self.playing = false;
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let mut copy = veylvik_document::editing::copy_branch(
                self.project.document.snapshot(),
                self.project.selected,
            )?;
            self.graph.place_branch_copy(
                self.project.document.snapshot(),
                self.project.document.revision(),
                &mut copy,
            )?;
            self.project.insert_branch(copy)?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.refresh_draft();
                self.request_preview();
            }
            Err(error) => self.fail(error),
        }
    }
    fn request_preview(&mut self) {
        self.wanted = None;
        self.preview_pending = false;
        let result = (|| {
            let scheduler = self
                .scheduler
                .as_ref()
                .ok_or("Preview worker unavailable")?;
            scheduler.cancel()?;
            Ok::<_, Box<dyn std::error::Error>>(scheduler.submit(
                self.project.document.revision(),
                if self.draft_dirty && matches!(self.draft.operation, Operation::Text { .. }) {
                    let doc = self.project.document.prepare(
                        self.project.document.revision(),
                        &[Command::Put {
                            node: Box::new(self.draft.clone()),
                        }],
                    )?;
                    std::sync::Arc::new(veylvik_render::Plan::compile_output(
                        &doc,
                        self.project.preview.unwrap_or(doc.output),
                    )?)
                } else {
                    self.project.plan()?
                },
                RenderRequest::full(self.project.time()?),
                self.project.inputs.clone(),
            )?)
        })();
        match result {
            Ok(ticket) => {
                self.wanted = Some(ticket);
                self.preview_pending = true;
                self.status = "Rendering…".into();
                self.error = false;
            }
            Err(e) => {
                self.playing = false;
                self.fail(e);
            }
        }
    }
    fn apply_draft(&mut self) -> bool {
        if !self.draft_dirty {
            return true;
        }
        let commands = if self.effects.edits.is_empty() && self.effects.connections.is_empty() {
            vec![Command::Put {
                node: Box::new(self.draft.clone()),
            }]
        } else {
            self.effects
                .edits
                .iter()
                .map(|((id, control), value)| Command::SetControl {
                    id: *id,
                    control: control.clone(),
                    value: value.clone(),
                })
                .chain(
                    self.effects
                        .connections
                        .iter()
                        .map(|((id, input), source)| Command::ConnectGroup {
                            id: *id,
                            input: input.clone(),
                            source: *source,
                        }),
                )
                .collect()
        };
        match self.project.apply(&commands) {
            Ok(()) => {
                self.refresh_draft();
                self.request_preview();
                true
            }
            Err(e) => {
                self.fail(e);
                false
            }
        }
    }
    fn history(&mut self, redo: bool) {
        if self.graph.dragging() {
            self.fail("Finish the graph gesture first.");
            return;
        }
        if !self.can_change_selection() {
            return;
        }
        match self.project.undo(redo) {
            Ok(()) => {
                self.refresh_draft();
                self.request_preview();
            }
            Err(e) => self.fail(e),
        }
    }
    fn save_project(&mut self, save_as: bool) {
        if self.files.busy() || !self.apply_draft() {
            self.after_save = None;
            return;
        }
        self.playing = false;
        let snapshot = self.project.save_snapshot();
        let existing = if save_as {
            None
        } else {
            self.project.path.clone()
        };
        let folder = self
            .project
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .unwrap_or(&self.project.base)
            .to_path_buf();
        let name = self
            .project
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or("untitled.veylvik".into());
        let result = self.files.start(move || {
            let path = if let Some(path) = existing {
                path
            } else {
                let Some(path) = files::select(Choice::Save, &folder, &name)? else {
                    return Ok(Completed::Cancelled);
                };
                path
            };
            snapshot
                .save(&path)
                .map(Completed::Saved)
                .map_err(|e| e.to_string())
        });
        self.file_started(result);
    }
    fn file_started(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.status = "File operation in progress…".into();
                self.error = false;
            }
            Err(e) => {
                self.after_save = None;
                self.fail(e);
            }
        }
    }
    fn leave(&mut self, action: Leave, ctx: &egui::Context) {
        if self.authoring_dialog_open() {
            self.fail("Finish or close the current authoring dialog first.");
            return;
        }
        if self.files.busy() {
            self.fail("Finish or cancel the current file operation first.");
            return;
        }
        if self.project.dirty() || self.draft_dirty {
            self.confirm = Some(action);
        } else {
            self.leave_now(action, ctx);
        }
    }
    fn leave_now(&mut self, action: Leave, ctx: &egui::Context) {
        self.playing = false;
        match action {
            Leave::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Leave::Example(id) => match Project::example(id) {
                Ok(project) => self.open_project(project, ctx),
                Err(e) => self.fail(e),
            },
            Leave::New => self
                .composition_controls
                .begin(self.project.document.snapshot(), true),
            Leave::Recover(id) => {
                let store = self.recovery.store.clone();
                let result = self.files.start(move || {
                    Project::recover(&store, id)
                        .map(|project| Completed::Opened(Box::new(project)))
                        .map_err(|e| e.to_string())
                });
                self.file_started(result);
            }
            Leave::Open => {
                let folder = self.project.base.clone();
                let result = self.files.start(move || {
                    let Some(path) = files::select(Choice::Open, &folder, "")? else {
                        return Ok(Completed::Cancelled);
                    };
                    Project::open(Some(&path))
                        .map(|project| Completed::Opened(Box::new(project)))
                        .map_err(|e| e.to_string())
                });
                self.file_started(result);
            }
        }
    }
    fn import_text_font(&mut self) {
        if self.files.busy() || !self.apply_draft() {
            return;
        }
        let mut node = self.draft.clone();
        let session = self.project.session;
        let revision = self.project.document.revision();
        let folder = self.project.base.clone();
        let time = self
            .project
            .time()
            .unwrap_or(veylvik_document::Time { ticks: 0, rate: 1 });
        let result = self.files.start(move || {
            use std::io::Read;
            let Some(path) = files::select(Choice::Font, &folder, "")? else {
                return Ok(Completed::Cancelled);
            };
            let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
            let mut bytes = Vec::new();
            file.take(veylvik_document::text::MAX_FONT_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            let font = veylvik_document::TextFont::embedded(
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                0,
                &bytes,
            )
            .map_err(|e| e.to_string())?;
            if let Operation::Text { font: target, .. } = &mut node.operation {
                *target = font;
            }
            veylvik_text::outline(&node.operation, time).map_err(|e| e.to_string())?;
            Ok(Completed::TextFont {
                session,
                revision,
                node: Box::new(node),
            })
        });
        self.file_started(result);
    }
    fn outline_text(&mut self, mut request: veylvik_io::text::OutlineRequest) {
        if self.files.busy() || !self.apply_draft() {
            self.outline_controls.open = true;
            return;
        }
        let document = self.project.document.snapshot();
        if document.width > veylvik_render::MAX_VECTOR_DIMENSION
            || document.height > veylvik_render::MAX_VECTOR_DIMENSION
        {
            self.outline_controls.open = true;
            self.fail(
                "Text outlines currently require a canvas no larger than 4096 pixels per axis.",
            );
            return;
        }
        self.playing = false;
        let folder = self.project.base.clone();
        let position = self.outline_controls.position;
        let result = self.files.start(move || {
            let Some(path) = files::select(Choice::Font, &folder, "")? else {
                return Ok(Completed::Cancelled);
            };
            request.font = path.to_str().ok_or("Font path is not UTF-8")?.into();
            let outlined =
                veylvik_io::text::outline(&folder, &request).map_err(|e| e.to_string())?;
            let mut node = Node::new(
                &request.label(),
                Operation::Path {
                    geometry: outlined.geometry,
                },
                &[],
            );
            node.position = position;
            Ok(Completed::Outlined(Box::new(node)))
        });
        self.outline_controls.pending = result.is_ok();
        if result.is_err() {
            self.outline_controls.open = true;
        }
        self.file_started(result);
    }
    fn reload_images(&mut self) {
        if self.files.busy()
            || !self.can_change_selection()
            || self.graph.dragging()
            || self.animation_editor.dragging()
        {
            return;
        }
        self.playing = false;
        let composition = self.project.document.snapshot().clone();
        if !composition
            .nodes
            .values()
            .any(|node| matches!(node.operation, Operation::Image { .. }))
        {
            self.status = "No image sources to reload".into();
            self.error = false;
            return;
        }
        let base = self.project.base.clone();
        let session = self.project.session;
        let revision = self.project.document.revision();
        let previous = self.project.inputs.clone();
        let result = self.files.start(move || {
            let inputs = project::load_inputs(&composition, &base, &previous)
                .map_err(|error| error.to_string())?;
            Ok(Completed::Reloaded {
                session,
                revision,
                inputs,
            })
        });
        self.file_started(result);
    }
    fn replace_image(&mut self) {
        if self.files.busy()
            || !self.can_change_selection()
            || self.graph.dragging()
            || self.animation_editor.dragging()
        {
            return;
        }
        if !matches!(self.draft.operation, Operation::Image { .. }) {
            return;
        }
        self.playing = false;
        let folder = self.project.base.clone();
        let session = self.project.session;
        let revision = self.project.document.revision();
        let node = self.project.selected;
        let inputs = self.project.inputs.clone();
        let result = self.files.start(move || {
            let Some(path) = files::select(Choice::Import, &folder, "")? else {
                return Ok(Completed::Cancelled);
            };
            let path = path.canonicalize().map_err(|error| error.to_string())?;
            let asset = path.to_str().ok_or("Image path is not UTF-8")?;
            let replaced_bytes = inputs.get(asset).map_or(0, |image| image.byte_size());
            let budget = (256u64 * 1024 * 1024)
                .saturating_sub(inputs.byte_size() - replaced_bytes)
                .min(128 * 1024 * 1024);
            let image =
                Arc::new(veylvik_io::load_image(&path, budget).map_err(|error| error.to_string())?);
            Ok(Completed::Replaced {
                session,
                revision,
                node,
                path,
                image,
            })
        });
        self.file_started(result);
    }
    fn import_image(&mut self, position: veylvik_document::NodePosition) {
        if self.files.busy() || !self.apply_draft() {
            return;
        }
        self.playing = false;
        let folder = self.project.base.clone();
        let budget = (256u64 * 1024 * 1024)
            .saturating_sub(self.project.inputs.byte_size())
            .min(128 * 1024 * 1024);
        let result = self.files.start(move || {
            let Some(path) = files::select(Choice::Import, &folder, "")? else {
                return Ok(Completed::Cancelled);
            };
            let path = path.canonicalize().map_err(|e| e.to_string())?;
            let image = veylvik_io::load_image(&path, budget).map_err(|e| e.to_string())?;
            Ok(Completed::Imported {
                position,
                path,
                image: Arc::new(image),
            })
        });
        self.file_started(result);
    }
    fn export_frame(&mut self) {
        if self.files.busy() || !self.apply_draft() {
            return;
        }
        self.playing = false;
        let plan = match self.project.plan() {
            Ok(plan) => plan,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        let time = match self.project.time() {
            Ok(time) => time,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        let document = self.project.document.snapshot().clone();
        let base = self.project.base.clone();
        let inputs = self.project.inputs.clone();
        let renderer = self.renderer.clone();
        let name = format!("frame-{:06}.png", self.project.frame);
        let result = self.files.start(move || {
            let Some(path) = files::select(Choice::Export, &base, &name)? else {
                return Ok(Completed::Cancelled);
            };
            veylvik_io::FrameFormat::from_path(&path).map_err(|e| e.to_string())?;
            veylvik_io::protect_inputs(&document, &base, &path).map_err(|e| e.to_string())?;
            let gpu_frame = renderer
                .render_gpu_cached(&plan, RenderRequest::full(time), &inputs)
                .map_err(|e| e.to_string())?;
            let frame = renderer.read_frame(&gpu_frame).map_err(|e| e.to_string())?;
            veylvik_io::save_frame(
                &path,
                frame.region.width,
                frame.region.height,
                &frame.pixels,
            )
            .map_err(|e| e.to_string())?;
            Ok(Completed::Exported(path))
        });
        self.file_started(result);
    }
    fn export_sequence(&mut self, settings: sequence::Settings, ctx: &egui::Context) {
        use std::sync::atomic::Ordering;
        if self.files.busy() || !self.apply_draft() {
            return;
        }
        self.playing = false;
        let document = self.project.document.snapshot().clone();
        let inputs = self.project.inputs.clone();
        let base = self.project.base.clone();
        let renderer = self.renderer.clone();
        let request = veylvik_io::sequence::SequenceRequest {
            format: settings.format,
            frames: settings.frames,
            output: self.project.preview.unwrap_or(document.output),
            region: None,
        };
        let progress = Arc::new(sequence::Progress::default());
        progress
            .total
            .store(settings.frames.count().unwrap(), Ordering::Relaxed);
        self.export_controls.active = Some(progress.clone());
        let context = ctx.clone();
        let result = self.files.start(move || {
            let Some(parent) = files::select(Choice::Folder, &base, "")? else {
                return Ok(Completed::Cancelled);
            };
            if progress.cancel.load(Ordering::Relaxed) {
                return Ok(Completed::Cancelled);
            }
            veylvik_io::sequence::render_sequence_with_inputs(
                &renderer,
                &document,
                &inputs,
                &parent.join(settings.name),
                request,
                |completed, total| {
                    progress.completed.store(completed, Ordering::Relaxed);
                    progress.total.store(total, Ordering::Relaxed);
                    context.request_repaint();
                    !progress.cancel.load(Ordering::Relaxed)
                },
            )
            .map(Completed::Sequence)
            .map_err(|e| e.to_string())
        });
        if result.is_err() {
            self.export_controls.active = None;
        }
        self.file_started(result);
    }
    fn poll_files(&mut self, ctx: &egui::Context) {
        let Some(result) = self.files.poll() else {
            return;
        };
        self.export_controls.active = None;
        if self.outline_controls.pending {
            self.outline_controls.pending = false;
            self.outline_controls.open = !matches!(&result, Ok(Completed::Outlined(_)));
        }
        match result {
            Ok(Completed::Cancelled) => {
                self.after_save = None;
                self.status = "Cancelled".into();
                self.error = false;
            }
            Ok(Completed::TextFont {
                session,
                revision,
                node,
            }) => {
                if session != self.project.session || revision != self.project.document.revision() {
                    self.fail("Composition changed while choosing font");
                } else {
                    match self.project.apply(&[Command::Put { node }]) {
                        Ok(()) => {
                            self.refresh_draft();
                            self.request_preview();
                        }
                        Err(e) => self.fail(e),
                    }
                }
            }
            Ok(Completed::Outlined(node)) => {
                let id = node.id;
                match self.project.apply(&[Command::Put { node }]) {
                    Ok(()) => {
                        self.project.selected = id;
                        self.project.preview = Some(id);
                        self.refresh_draft();
                        self.request_preview();
                    }
                    Err(error) => {
                        self.outline_controls.open = true;
                        self.fail(error);
                    }
                }
            }
            Ok(Completed::PresetLibrary(listing)) => {
                self.library.listing = listing;
                self.status = "Effect library updated".into();
                self.error = false;
            }
            Ok(Completed::PresetLoaded(preset)) => self.loaded_preset(*preset),
            Ok(Completed::Opened(project)) => self.open_project(*project, ctx),
            Ok(Completed::Imported {
                path,
                image,
                position,
            }) => match self.project.import(&path, image, position) {
                Ok(()) => {
                    self.refresh_draft();
                    self.request_preview();
                }
                Err(e) => self.fail(e),
            },
            Ok(Completed::Reloaded {
                session,
                revision,
                inputs,
            }) => {
                if self.project.session == session && self.project.document.revision() == revision {
                    self.project.inputs = inputs;
                    self.request_preview();
                } else {
                    self.fail(
                        "Source reload became obsolete; reload again for the current project.",
                    );
                }
            }
            Ok(Completed::Replaced {
                session,
                revision,
                node,
                path,
                image,
            }) => {
                if self.project.session != session || self.project.document.revision() != revision {
                    self.fail("Image replacement became obsolete; choose the source again.");
                } else {
                    match self.project.replace_image(node, &path, image) {
                        Ok(()) => {
                            self.refresh_draft();
                            self.request_preview();
                        }
                        Err(error) => self.fail(error),
                    }
                }
            }
            Ok(Completed::Saved(saved)) => {
                self.status = format!("Saved {}", saved.stamp.path().display());
                self.error = false;
                self.project.mark_saved(saved);
                if let Some(action) = self.after_save.take() {
                    self.leave_now(action, ctx);
                }
            }
            Ok(Completed::Sequence(result)) => {
                let manifest = &result["manifest"];
                self.status = format!(
                    "Sequence {}: {} / {} frames in {}",
                    manifest["status"].as_str().unwrap_or("finished"),
                    manifest["completed"],
                    manifest["count"],
                    result["directory"].as_str().unwrap_or("export folder")
                );
                self.error = false;
            }
            Ok(Completed::Exported(path)) => {
                self.status = format!("Exported {}", path.display());
                self.error = false;
            }
            Err(e) => {
                self.after_save = None;
                self.fail(e);
            }
        }
    }
    fn theme(&mut self, ctx: &egui::Context) {
        if self.theme_checked.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.theme_checked = Instant::now();
        if let Some(home) = std::env::var_os("HOME") {
            let path = PathBuf::from(home).join(".local/state/omarchy/current/theme/colors.toml");
            if let Ok(text) = std::fs::read_to_string(path) {
                for line in text.lines() {
                    if let Some((key, value)) = line.split_once('=')
                        && key.trim() == "accent"
                        && let Ok(color) = egui::Color32::from_hex(
                            value.trim().trim_matches('"').trim_matches('\''),
                        )
                    {
                        self.accent = color;
                    }
                }
            }
        }
        let mut style = (*ctx.global_style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.selection.bg_fill = self.accent.gamma_multiply(0.25);
        style.visuals.selection.stroke.color = self.accent;
        style.visuals.widgets.active.bg_stroke.color = self.accent;
        style.visuals.panel_fill = egui::Color32::from_rgb(22, 25, 31);
        for widget in [
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
            &mut style.visuals.widgets.noninteractive,
            &mut style.visuals.widgets.open,
        ] {
            widget.corner_radius = egui::CornerRadius::ZERO;
        }
        ctx.set_global_style(style);
    }
    fn inspector_actions(&mut self, ui: &mut egui::Ui) {
        ui.heading("Inspector");
        let texts: Vec<_> =
            veylvik_document::presets::background::reachable(self.project.document.snapshot())
                .into_iter()
                .filter_map(|id| {
                    let node = &self.project.document.snapshot().nodes[&id];
                    if let Operation::Text { content, .. } = &node.operation {
                        Some((id, content.chars().take(28).collect::<String>()))
                    } else {
                        None
                    }
                })
                .collect();
        let mut select = None;
        if !texts.is_empty() {
            egui::ComboBox::from_id_salt("scene-text-selector")
                .selected_text("Edit scene text…")
                .show_ui(ui, |ui| {
                    for (id, label) in texts {
                        if ui
                            .selectable_label(
                                self.project.selected == id,
                                if label.is_empty() {
                                    "Empty text"
                                } else {
                                    &label
                                },
                            )
                            .clicked()
                        {
                            select = Some(id);
                        }
                    }
                });
        }
        if let Some(id) = select
            && self.can_change_selection()
        {
            self.project.selected = id;
            self.project.preview = None;
            if let Some(group) = self
                .project
                .document
                .snapshot()
                .groups
                .values()
                .find(|g| g.members.contains(&id))
            {
                self.graph.expand(group.id);
            }
            self.refresh_draft();
            self.request_preview();
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.draft_dirty, egui::Button::new("Apply"))
                .on_hover_text("Commit this draft as one undo step (Ctrl+Enter)")
                .clicked()
            {
                self.apply_draft();
            }
            if ui
                .add_enabled(self.draft_dirty, egui::Button::new("Revert"))
                .on_hover_text("Discard unapplied inspector changes")
                .clicked()
            {
                self.refresh_draft();
                self.request_preview();
            }
            if self.draft_dirty {
                ui.weak("Unapplied changes");
            }
        });
        ui.separator();
    }
    fn inspector(&mut self, ui: &mut egui::Ui) {
        if self.effect_inspector(ui) {
            return;
        }
        ui.label(self.draft.operation.descriptor().label);
        ui.add_space(8.0);
        self.draft_dirty |= ui.text_edit_singleline(&mut self.draft.label).changed();
        ui.separator();
        let time = self
            .project
            .time()
            .unwrap_or(veylvik_document::Time { ticks: 0, rate: 1 });
        let mut replace_image = false;
        let mut import_font = false;
        let mut text_changed = false;
        match &mut self.draft.operation {
            op @ Operation::Text { .. } => {
                let (changed, import) = editable_text::controls(ui, op, time);
                self.draft_dirty |= changed;
                text_changed |= changed;
                import_font = import;
            }
            Operation::Path { geometry } => {
                self.draft_dirty |= vector_editor::edit(ui, geometry);
            }
            Operation::Solid { color } => {
                ui.label("Linear RGB and alpha");
                self.draft_dirty |= animation::color(ui, color);
            }
            Operation::Colorize { low, high } => {
                ui.label("Map luminance to linear RGBA colors.");
                ui.push_id("low", |ui| {
                    ui.label("Shadows");
                    self.draft_dirty |= animation::color(ui, low);
                });
                ui.push_id("high", |ui| {
                    ui.label("Highlights");
                    self.draft_dirty |= animation::color(ui, high);
                });
            }
            Operation::Noise { seed, .. }
            | Operation::FractalNoise { seed, .. }
            | Operation::Fire { seed, .. } => {
                ui.horizontal(|ui| {
                    ui.label("Seed");
                    self.draft_dirty |= ui.add(egui::DragValue::new(seed)).changed();
                });
            }
            Operation::Blend { mode, .. } => {
                egui::ComboBox::from_id_salt("blend-mode")
                    .selected_text(mode.label())
                    .show_ui(ui, |ui| {
                        for choice in veylvik_document::BlendMode::ALL {
                            self.draft_dirty |=
                                ui.selectable_value(mode, choice, choice.label()).changed();
                        }
                    });
            }
            Operation::Gradient { mode, .. } => {
                egui::ComboBox::from_id_salt("gradient-mode")
                    .selected_text(mode.label())
                    .show_ui(ui, |ui| {
                        for choice in veylvik_document::GradientMode::ALL {
                            self.draft_dirty |=
                                ui.selectable_value(mode, choice, choice.label()).changed();
                        }
                    });
                ui.label("Use Colorize for colors or Channel mask for coverage.");
            }
            Operation::CombineMask { mode } => {
                egui::ComboBox::from_id_salt("mask-mode")
                    .selected_text(mode.label())
                    .show_ui(ui, |ui| {
                        for choice in veylvik_document::MaskMode::ALL {
                            self.draft_dirty |=
                                ui.selectable_value(mode, choice, choice.label()).changed();
                        }
                    });
                ui.label("Subtract removes other from mask; RGB is ignored.");
            }
            Operation::ChannelMask { channel } => {
                egui::ComboBox::from_id_salt("mask-channel")
                    .selected_text(channel.label())
                    .show_ui(ui, |ui| {
                        for choice in veylvik_document::MaskChannel::ALL {
                            self.draft_dirty |= ui
                                .selectable_value(channel, choice, choice.label())
                                .changed();
                        }
                    });
                ui.label("Convert image content to mask coverage.");
            }
            Operation::Merge {} => {
                ui.label("Foreground over background");
            }
            Operation::Mask {} => {
                ui.label("Image multiplied by mask alpha");
            }
            Operation::SurfaceReflection { kind, .. } => {
                egui::ComboBox::from_id_salt("reflection-kind")
                    .selected_text(kind.label())
                    .show_ui(ui, |ui| {
                        for choice in veylvik_document::ReflectionKind::ALL {
                            self.draft_dirty |=
                                ui.selectable_value(kind, choice, choice.label()).changed();
                        }
                    });
                ui.label("Connect any image or animated branch to Reflection.");
            }
            Operation::Glass { .. } => {
                ui.label("Use isolated shape alpha and the scene behind it. Roughness makes frosted glass.");
            }
            Operation::SunGlints { seed, .. } => {
                self.draft_dirty |= ui.add(egui::DragValue::new(seed).prefix("Seed ")).changed();
            }
            Operation::Patina { kind, seed, .. } => {
                egui::ComboBox::from_id_salt("patina-kind")
                    .selected_text(kind.label())
                    .show_ui(ui, |ui| {
                        for choice in veylvik_document::PatinaKind::ALL {
                            self.draft_dirty |=
                                ui.selectable_value(kind, choice, choice.label()).changed();
                        }
                    });
                self.draft_dirty |= ui.add(egui::DragValue::new(seed).prefix("Seed ")).changed();
            }
            Operation::Material { kind, .. } => {
                egui::ComboBox::from_id_salt("Material-kind")
                    .selected_text(kind.label())
                    .show_ui(ui, |ui| {
                        for choice in veylvik_document::MaterialKind::ALL {
                            self.draft_dirty |=
                                ui.selectable_value(kind, choice, choice.label()).changed();
                        }
                    });
            }
            Operation::Atmosphere { kind, seed, .. } => {
                self.draft_dirty |= ui.add(egui::DragValue::new(seed).prefix("Seed ")).changed();
                egui::ComboBox::from_id_salt("Atmosphere-kind")
                    .selected_text(kind.label())
                    .show_ui(ui, |ui| {
                        for choice in veylvik_document::AtmosphereKind::ALL {
                            self.draft_dirty |=
                                ui.selectable_value(kind, choice, choice.label()).changed();
                        }
                    });
            }
            Operation::MetallicShine { .. } => {
                ui.label("Apply to isolated text or a logo before adding a background.");
                ui.label("Animate X/Y to sweep; alpha stays unchanged.");
            }
            Operation::Morphology { .. } => {
                ui.label("Positive radius grows alpha; negative shrinks it.");
                ui.label("Square neighborhood, transparent borders, white mask output.");
            }
            Operation::Displace { .. } => {
                ui.label("Map red drives X, green drives Y.");
                ui.label("Mid-gray is neutral; alpha controls strength.");
            }
            Operation::Affine { .. }
            | Operation::Ellipse { .. }
            | Operation::Exposure { .. }
            | Operation::Transform { .. }
            | Operation::Blur { .. } => {}
            Operation::Image { asset } => {
                ui.label("Source image");
                ui.label(asset.as_str());
                ui.label("Images use native size and transparent padding.");
                if let Some(reason) = self.project.unavailable_assets().get(asset.as_str()) {
                    ui.colored_label(egui::Color32::LIGHT_RED, *reason);
                }
                replace_image = ui.button("Replace source…").clicked();
            }
        }
        if replace_image {
            self.replace_image();
        }
        let kind = self.draft.operation.descriptor();
        for parameter in kind.parameters {
            if matches!(self.draft.operation, Operation::Text { .. }) {
                break;
            }
            if let Some(value) = self.draft.operation.scalar_mut(parameter.name) {
                let label = format!("{} ({})", parameter.name.replace('_', " "), parameter.unit);
                let changed = scalar(
                    ui,
                    &label,
                    value,
                    parameter.min.unwrap()..=parameter.max.unwrap(),
                    time,
                );
                self.draft_dirty |= changed;
                text_changed |= changed;
            }
        }
        if text_changed && matches!(self.draft.operation, Operation::Text { .. }) {
            self.text_preview_deadline =
                Some(Instant::now() + std::time::Duration::from_millis(150));
        }
        if import_font {
            self.import_text_font();
        }
        if self.draft_dirty {
            self.playing = false;
        }
        self.text_connections(ui);
        if matches!(self.draft.operation, Operation::Text { .. })
            && self.project.preview.is_none()
            && !editable_text::handles_in_scene(self.project.document.snapshot(), self.draft.id)
        {
            ui.weak("A downstream effect moves this text. Use Preview selected for source-position handles.");
        }
        ui.separator();
        for port in self.draft.operation.inputs() {
            let selected = self.draft.inputs.get_mut(*port).unwrap();
            let label = self
                .project
                .document
                .snapshot()
                .nodes
                .get(selected)
                .map(|n| n.label.as_str())
                .unwrap_or("Missing");
            egui::ComboBox::from_id_salt(("port", port))
                .selected_text(format!("{port}: {label}"))
                .show_ui(ui, |ui| {
                    for node in self.project.document.snapshot().nodes.values() {
                        if node.id != self.draft.id {
                            self.draft_dirty |= ui
                                .selectable_value(selected, node.id, &node.label)
                                .changed();
                        }
                    }
                });
        }
        ui.separator();
        if ui.button("Preview selected").clicked() {
            self.project.preview = Some(self.project.selected);
            self.request_preview();
        }
        if ui.button("View composition output").clicked() {
            self.project.preview = None;
            self.request_preview();
        }
        if ui.button("Set as output").clicked() {
            self.set_selected_output();
        }
        if ui.button("Duplicate node").clicked() {
            self.duplicate_selected(ui.ctx());
        }
        if ui
            .button("Duplicate branch")
            .on_hover_text("Copy this node and all upstream dependencies (Ctrl+Shift+D)")
            .clicked()
        {
            self.duplicate_branch();
        }
        if ui.button("Delete node").clicked() {
            self.delete_selected();
        }
        ui.add_space(15.0);
        ui.weak("Parameter changes become one undo step when applied.");
    }
    fn set_selected_output(&mut self) {
        if !self.apply_draft() {
            return;
        }
        match self.project.apply(&[Command::Output {
            id: self.project.selected,
        }]) {
            Ok(()) => {
                self.project.preview = None;
                self.request_preview();
            }
            Err(error) => self.fail(error),
        }
    }
    fn delete_selected(&mut self) {
        if !self.can_change_selection() {
            return;
        }
        match self.project.apply(&[Command::Remove {
            id: self.project.selected,
        }]) {
            Ok(()) => {
                self.refresh_draft();
                self.request_preview();
            }
            Err(error) => self.fail(error),
        }
    }
    fn sync_key_timeline(&mut self) {
        self.key_timeline.sync(
            self.project.document.snapshot(),
            self.project.document.revision(),
            self.project.selected,
        );
    }
    fn seek_key(&mut self, forward: bool) {
        self.sync_key_timeline();
        if let Some(frame) = self.key_timeline.adjacent(self.project.frame, forward) {
            self.project.frame = frame;
            self.playing = false;
            self.request_preview();
        }
    }
    fn timeline(&mut self, ui: &mut egui::Ui) {
        self.sync_key_timeline();
        let count = self.project.document.snapshot().frame_count;
        let end_frame = (i64::from(count) - 1).max(self.project.frame);
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!self.animation_editor.dragging(), |ui| {
                ui.selectable_value(&mut self.animation_editor.visible, false, "Nodes");
                ui.selectable_value(&mut self.animation_editor.visible, true, "Animation");
            });
            if ui
                .button(if self.playing { "Pause" } else { "Play" })
                .clicked()
            {
                self.toggle_play();
            }
            let changed = ui
                .add(egui::Slider::new(&mut self.project.frame, 0..=end_frame).text("Frame"))
                .changed();
            if changed {
                self.playing = false;
                self.request_preview();
            }
            ui.label(format!("of {count} frames"));
            let fps = self.project.document.snapshot().frame_rate;
            ui.weak(format!("{} / {} fps", fps.ticks, fps.rate));
            if let Some(frame) = self.key_timeline.show(ui, self.project.frame, count) {
                self.project.frame = frame;
                self.playing = false;
                self.request_preview();
            }
        });
    }
    fn animation_workspace(&mut self, ui: &mut egui::Ui) {
        let ready = !self.draft_dirty && !self.playing;
        if !ready {
            ui.weak("Pause playback and apply or revert inspector changes to edit animation.");
        }
        let mut actions = Vec::new();
        ui.add_enabled_ui(ready, |ui| {
            actions = self.animation_editor.show(
                ui,
                self.project.document.snapshot(),
                self.project.document.revision(),
                self.project.selected,
                self.project.frame,
            );
        });
        for action in actions {
            match action {
                animation_editor::Action::Batch { revision, edits } => {
                    if revision != self.project.document.revision() {
                        self.fail("Animation changed during the edit; please try again.");
                        continue;
                    }
                    let commands: Vec<_> = edits
                        .into_iter()
                        .map(|(id, parameter, scalar)| Command::SetParameter {
                            id,
                            parameter,
                            value: serde_json::to_value(scalar).expect("validated scalar"),
                        })
                        .collect();
                    match self.project.apply(&commands) {
                        Ok(()) => {
                            self.refresh_draft();
                            self.request_preview();
                        }
                        Err(e) => self.fail(e),
                    }
                }
                animation_editor::Action::Select(id) => {
                    self.project.selected = id;
                    self.refresh_draft();
                }
                animation_editor::Action::Seek(frame) => {
                    self.project.frame = frame;
                    self.playing = false;
                    self.request_preview();
                }
                animation_editor::Action::Commit {
                    revision,
                    node,
                    parameter,
                    scalar,
                } => {
                    if revision != self.project.document.revision() {
                        self.fail("Animation changed during the edit; please try again.");
                        continue;
                    }
                    let command = Command::SetParameter {
                        id: node,
                        parameter,
                        value: serde_json::to_value(scalar).expect("finite validated scalar"),
                    };
                    match self.project.apply(&[command]) {
                        Ok(()) => {
                            self.refresh_draft();
                            self.request_preview();
                        }
                        Err(error) => self.fail(error),
                    }
                }
            }
        }
    }
    fn toggle_play(&mut self) {
        self.playing = !self.playing;
        self.playback = Instant::now();
        self.playback_frame = self.project.frame;
    }
    fn graph(&mut self, ui: &mut egui::Ui) {
        let actions = self.graph.show(
            ui,
            self.project.document.snapshot(),
            self.project.document.revision(),
            self.project.selected,
            self.accent,
        );
        for action in actions {
            match action {
                graph::Action::Group => self.begin_group(false),
                graph::Action::CloseGroup(id) => {
                    if self.can_change_selection() {
                        self.graph.collapse(id);
                        self.project.selected = self.project.document.snapshot().groups[&id].output;
                        self.refresh_draft();
                    }
                }
                graph::Action::OpenGroup(id) => {
                    if self.can_change_selection() {
                        self.graph.expand(id);
                    }
                }
                graph::Action::Find => self.open_node_finder(),
                graph::Action::Add => self.open_node_palette(ui.ctx()),
                graph::Action::Select { id, preview } => {
                    if self.can_change_selection() {
                        self.project.selected = id;
                        self.refresh_draft();
                        if preview {
                            self.project.preview = Some(id);
                            self.request_preview();
                        }
                    }
                }
                graph::Action::Connect {
                    source,
                    target,
                    port,
                    group,
                } => {
                    if self.can_change_selection() {
                        let result = if group {
                            self.project
                                .apply(&[Command::ConnectGroup {
                                    id: target,
                                    input: port,
                                    source,
                                }])
                                .map(|_| true)
                        } else {
                            self.project.connect(source, target, &port)
                        };
                        match result {
                            Ok(true) => {
                                self.refresh_draft();
                                self.request_preview();
                            }
                            Ok(false) => {}
                            Err(error) => self.fail(error),
                        }
                    }
                }
                graph::Action::Place { id, position } => self.place(&[Command::Place {
                    id,
                    position: Some(position),
                }]),
                graph::Action::Arrange(positions) => self.place(
                    &positions
                        .into_iter()
                        .map(|(id, position)| Command::Place {
                            id,
                            position: Some(position),
                        })
                        .collect::<Vec<_>>(),
                ),
            }
        }
    }
    fn place(&mut self, commands: &[Command]) {
        if let Err(e) = self.project.apply(commands) {
            self.fail(e);
            return;
        }
        // Keep an unapplied parameter draft while accepting its new layout.
        self.draft.position = self.project.document.snapshot().nodes[&self.draft.id].position;
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        self.effect_dialog(ctx);
        self.preset_library(ctx);
        if let Some(action) = self.confirm {
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("Save your work before continuing, or discard it.");
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            self.confirm = None;
                            self.after_save = Some(action);
                            self.save_project(false);
                        }
                        if ui.button("Discard changes").clicked() {
                            self.confirm = None;
                            self.leave_now(action, ctx);
                        }
                        if ui.button("Keep editing").clicked() {
                            self.confirm = None;
                        }
                    });
                });
        }
        diagnostics::show(ctx, &mut self.statistics, &self.renderer);
        self.about.show(ctx, &self.renderer, self.accent);
        if self.help {
            egui::Window::new("Keyboard shortcuts")
                .open(&mut self.help)
                .show(ctx, |ui| {
                    for (key, action) in [
                        ("Space", "Play / pause"),
                        (
                            "Alt+Left / Right",
                            "Previous / next selected-node keyed frame",
                        ),
                        ("Tab", "Search and add a node"),
                        ("Ctrl+F", "Find and preview an existing node"),
                        ("Ctrl+D", "Duplicate selected node"),
                        ("Ctrl+Shift+D", "Duplicate upstream branch"),
                        ("Ctrl+Enter", "Apply inspector draft"),
                        ("Ctrl+N", "New composition"),
                        ("Ctrl+O", "Open composition"),
                        ("Ctrl+I", "Import PNG or OpenEXR image"),
                        ("Ctrl+R", "Reload source images"),
                        ("Ctrl+E", "Export current preview"),
                        ("Ctrl+Shift+E", "Export image sequence"),
                        ("Ctrl+S", "Save"),
                        ("Ctrl+Shift+S", "Save as"),
                        ("Ctrl+Z", "Undo"),
                        ("Ctrl+Shift+Z", "Redo"),
                        ("Q", "Close (asks about unsaved changes)"),
                        ("?", "Show shortcuts"),
                    ] {
                        ui.horizontal(|ui| {
                            ui.monospace(key);
                            ui.label(action);
                        });
                    }
                });
        }
    }
}
impl eframe::App for Studio {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.theme(&ctx);
        self.poll_files(&ctx);
        if let Some(deadline) = self.text_preview_deadline {
            if Instant::now() >= deadline {
                self.text_preview_deadline = None;
                if self.draft_dirty && matches!(self.draft.operation, Operation::Text { .. }) {
                    self.request_preview();
                }
            } else {
                ctx.request_repaint_after(deadline.saturating_duration_since(Instant::now()));
            }
        }
        if let Some(error) = self.recovery.tick(&self.project) {
            self.fail(error);
        }
        for _ in 0..8 {
            let Some(request) = self.control.as_ref().and_then(|server| server.try_recv()) else {
                break;
            };
            control::dispatch(self, request);
        }
        ctx.request_repaint_after(Duration::from_secs(2));
        if let Some(scheduler) = &self.scheduler {
            match scheduler.try_take() {
                Ok(Some(result)) if self.wanted == Some(result.ticket) => {
                    self.preview_pending = false;
                    match result.frame {
                        Ok(frame) => {
                            self.displayed = Some(self.display.frame(
                                &self.renderer,
                                frame,
                                self.view,
                                viewer::ViewTransform::default(),
                            ));
                            self.displayed_ticket = Some(result.ticket);
                            self.displayed_time = Some(result.request.time);
                            if !self.error && !self.files.busy() {
                                self.status = format!(
                                    "{:.1} ms · {}",
                                    result.elapsed.as_secs_f64() * 1000.0,
                                    self.renderer.adapter.name
                                );
                            }
                        }
                        Err(e) => {
                            self.playing = false;
                            self.fail(e);
                        }
                    }
                }
                Err(e) => {
                    self.preview_pending = false;
                    self.playing = false;
                    self.fail(e);
                }
                _ => {}
            }
        }
        if self.playing {
            let fps = self.project.document.snapshot().frame_rate;
            let elapsed =
                (self.playback.elapsed().as_secs_f64() * fps.ticks as f64 / fps.rate as f64) as i64;
            let count = i64::from(self.project.document.snapshot().frame_count);
            let frame = (self.playback_frame.rem_euclid(count) + elapsed.rem_euclid(count)) % count;
            // Playback must let an active frame finish before submitting again.
            // Explicit edits and scrubbing still replace obsolete work immediately.
            if frame != self.project.frame && !self.preview_pending {
                self.project.frame = frame;
                self.request_preview();
            }
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        // Consume early, apply after widgets have processed this frame's text edits.
        let apply_inspector = !self.files.busy()
            && !self.authoring_dialog_open()
            && self.confirm.is_none()
            && !self.graph.dragging()
            && !self.animation_editor.dragging()
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Enter));
        if !self.files.busy()
            && !self.authoring_dialog_open()
            && self.confirm.is_none()
            && (!ctx.egui_wants_keyboard_input() || self.animation_editor.has_focus(&ctx))
            && !self.animation_editor.dragging()
        {
            if ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::S)
            }) {
                self.save_project(true);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::S)) {
                self.save_project(false);
            }
            if ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::Z)
            }) {
                self.history(true);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::F)) {
                self.open_node_finder();
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)) {
                self.open_node_palette(&ctx);
            }
            if ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::D)
            }) {
                self.duplicate_branch();
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::D)) {
                self.duplicate_selected(&ctx);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::N)) {
                self.leave(Leave::New, &ctx);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::O)) {
                self.leave(Leave::Open, &ctx);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::R)) {
                self.reload_images();
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::I)) {
                self.import_image(self.graph.insertion_position(&ctx));
            }
            if ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::E)
            }) {
                self.export_controls.open = true;
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::E)) {
                self.export_frame();
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Z)) {
                self.history(false);
            }
            for (key, forward) in [(egui::Key::ArrowLeft, false), (egui::Key::ArrowRight, true)] {
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, key)) {
                    self.seek_key(forward);
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
                self.toggle_play();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Q)) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if ctx.input(|i| {
                i.events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Text(s) if s == "?"))
            }) {
                self.help = !self.help;
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.leave(Leave::Close, &ctx);
        }
        self.export_controls.progress(ui);
        ui.add_enabled_ui(
            !self.files.busy() && !self.authoring_dialog_open() && self.confirm.is_none(),
            |ui| {
                egui::Panel::top("menu_bar").show_inside(ui, |ui| self.menu_bar(ui));
                egui::Panel::bottom("status").show_inside(ui, |ui| {
                    if self.files.busy() {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("File operation in progress…");
                        });
                    }
                    ui.colored_label(
                        if self.error {
                            egui::Color32::LIGHT_RED
                        } else {
                            egui::Color32::from_gray(165)
                        },
                        &self.status,
                    );
                });
                egui::Panel::bottom("graph")
                    .default_size(245.0)
                    .min_size(130.0)
                    .resizable(true)
                    .show_inside(ui, |ui| {
                        ui.add_enabled_ui(!self.animation_editor.dragging(), |ui| {
                            self.timeline(ui)
                        });
                        ui.separator();
                        if self.animation_editor.visible {
                            self.animation_workspace(ui);
                        } else {
                            self.graph(ui);
                        }
                    });
                egui::Panel::right("inspector")
                    .default_size(280.0)
                    .min_size(250.0)
                    .resizable(true)
                    .show_inside(ui, |ui| {
                        self.inspector_actions(ui);
                        egui::ScrollArea::vertical().show(ui, |ui| self.inspector(ui));
                    });
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.heading("Viewer");
                        ui.weak(if self.project.preview.is_some() {
                            "Selected node preview"
                        } else {
                            "Composition output"
                        });
                        ui.separator();
                        ui.checkbox(&mut self.view.alpha, "Alpha")
                            .on_hover_text("Inspect coverage without changing the composition");
                        ui.add_enabled_ui(!self.view.alpha, |ui| {
                            ui.label("View exposure");
                            ui.add(
                                egui::DragValue::new(&mut self.view.exposure)
                                    .speed(0.1)
                                    .range(-16.0..=16.0)
                                    .suffix(" stops"),
                            );
                        });
                        if ui.button("Fit").clicked() {
                            self.navigation.fit();
                        }
                        if ui
                            .button("100%")
                            .on_hover_text("One image pixel per physical display pixel")
                            .clicked()
                        {
                            self.navigation.actual_size();
                        }
                        if ui.button("Reset view").clicked() {
                            self.view = ViewSettings::default();
                        }
                    });
                    if let Some(frame) = self.displayed.clone() {
                        let image = egui::vec2(
                            frame.frame.region().width as f32,
                            frame.frame.region().height as f32,
                        );
                        let (canvas, response) = ui.allocate_exact_size(
                            ui.available_size().max(egui::vec2(1.0, 1.0)),
                            egui::Sense::click_and_drag(),
                        );
                        self.navigation.interact(ui, &response, canvas, image);
                        let transform =
                            self.navigation
                                .transform(canvas, image, ctx.pixels_per_point());
                        response.on_hover_text(format!(
                            "{:.0}% · scroll to zoom · drag to pan · double-click to fit",
                            self.navigation.scale(canvas, image, ctx.pixels_per_point()) * 100.0
                        ));
                        if frame.settings != self.view || frame.transform != transform {
                            self.displayed = Some(self.display.frame(
                                &self.renderer,
                                frame.frame.clone(),
                                self.view,
                                transform,
                            ));
                        }
                        ui.painter().with_clip_rect(canvas).add(
                            egui_wgpu::Callback::new_paint_callback(
                                canvas,
                                Paint(self.displayed.as_ref().unwrap().clone()),
                            ),
                        );
                        let label = format!(
                            "{} × {} · {:.0}%",
                            image.x as u32,
                            image.y as u32,
                            self.navigation.scale(canvas, image, ctx.pixels_per_point()) * 100.0
                        );
                        let painter = ui.painter().with_clip_rect(canvas);
                        let galley = painter.layout_no_wrap(
                            label,
                            egui::FontId::monospace(12.0),
                            egui::Color32::WHITE,
                        );
                        let position = canvas.right_bottom() - egui::vec2(8.0, 8.0) - galley.size();
                        painter.rect_filled(
                            egui::Rect::from_min_size(position, galley.size()).expand(3.0),
                            2.0,
                            egui::Color32::from_black_alpha(190),
                        );
                        painter.galley(position, galley, egui::Color32::WHITE);
                        // A pending frame must never change the canvas geometry.
                        // During playback, retaining the previous frame is normal.
                        if self.displayed_ticket != self.wanted && (!self.playing || self.error) {
                            viewer::paint_status(
                                ui,
                                canvas,
                                if self.error {
                                    "Showing previous frame · preview failed"
                                } else {
                                    "Showing previous frame · rendering"
                                },
                            );
                        }
                        let root = self
                            .project
                            .preview
                            .unwrap_or(self.project.document.snapshot().output);
                        if !self.playing
                            && (self.displayed_ticket == self.wanted
                                || matches!(self.draft.operation, Operation::Text { .. }))
                            && (root == self.draft.id
                                || (matches!(self.draft.operation, Operation::Text { .. })
                                    && self.project.preview.is_none()
                                    && editable_text::handles_in_scene(
                                        self.project.document.snapshot(),
                                        self.draft.id,
                                    )))
                            && let Ok(time) = self.project.time()
                        {
                            match handles::show(
                                ui,
                                canvas,
                                transform,
                                image,
                                &mut self.draft.operation,
                                time,
                            ) {
                                Ok(changed) => {
                                    self.draft_dirty |= changed;
                                    if changed
                                        && matches!(self.draft.operation, Operation::Text { .. })
                                    {
                                        self.text_preview_deadline =
                                            Some(Instant::now() + Duration::from_millis(40));
                                    }
                                }
                                Err(error) => self.fail(error),
                            }
                        }
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label(if self.error {
                                "Preview unavailable"
                            } else {
                                "Preparing preview…"
                            });
                        });
                    }
                });
            },
        );
        if apply_inspector
            && !self.files.busy()
            && !self.authoring_dialog_open()
            && self.confirm.is_none()
            && !self.graph.dragging()
            && !self.animation_editor.dragging()
            && self.apply_draft()
        {
            ctx.memory_mut(|memory| {
                if let Some(focused) = memory.focused() {
                    memory.surrender_focus(focused);
                }
            });
        }
        if let Some(id) = self.node_finder.show(&ctx) {
            if self.project.document.snapshot().nodes.contains_key(&id) {
                self.project.selected = id;
                self.project.preview = Some(id);
                if let Some(group) = self
                    .project
                    .document
                    .snapshot()
                    .groups
                    .values()
                    .find(|g| g.output == id)
                {
                    self.graph.collapse(group.id);
                }
                self.graph.focus(
                    self.project.document.snapshot(),
                    self.project.document.revision(),
                    id,
                );
                self.refresh_draft();
                self.request_preview();
            } else {
                self.fail("The selected node no longer exists");
            }
        }
        if let Some((choice, position)) = self.node_palette.show(&ctx) {
            self.add_from_palette(choice, position);
        }
        self.dialogs(&ctx);
        if let Some(settings) = self.composition_controls.show(&ctx) {
            self.apply_composition_settings(settings, &ctx);
        }
        if let Some(id) = self.recovery.show(
            &ctx,
            !self.files.busy() && self.confirm.is_none() && !self.authoring_dialog_open(),
        ) {
            self.leave(Leave::Recover(id), &ctx);
        }
        if let Some(request) = self
            .outline_controls
            .show(&ctx, !self.files.busy() && self.confirm.is_none())
        {
            self.outline_text(request);
        }
        if let Some(settings) = self.export_controls.show(
            &ctx,
            self.project.document.snapshot().frame_rate,
            self.project.document.snapshot().frame_count,
            !self.files.busy() && !self.authoring_dialog_open() && self.confirm.is_none(),
        ) {
            self.export_sequence(settings, &ctx);
        }
    }
    fn on_exit(&mut self) {
        self.recovery
            .finish(self.allow_close || (!self.project.dirty() && !self.draft_dirty));
        self.control.take();
        self.scheduler.take();
        self.preview_images.take();
        self.displayed.take();
        let _ = self.renderer.device().poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(30)),
        });
    }
}
