mod animation;
mod control;
mod sequence;
mod text_outline;
mod vector_editor;
use animation::scalar;
mod display;
mod files;
mod graph;
mod project;
use display::{Display, DisplayFrame, Paint};
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
    let project = Project::open(path.as_deref())?;
    let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    setup.instance_descriptor.backends = wgpu::Backends::VULKAN;
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
            let renderer = Arc::new(Renderer::from_device(
                state.device.clone(),
                state.queue.clone(),
                state.adapter.get_info(),
            )?);
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
            let mut app = Studio {
                control,
                project,
                renderer,
                scheduler: Some(scheduler),
                display,
                displayed: None,
                displayed_ticket: None,
                wanted: None,
                draft,
                draft_dirty: false,
                graph: graph::Graph::default(),
                status: String::new(),
                error: false,
                playing: false,
                playback: Instant::now(),
                playback_frame: 0,
                end_frame: 240,
                files: Tasks::new(cc.egui_ctx.clone()),
                export_controls: sequence::Controls::default(),
                outline_controls: text_outline::Controls::default(),
                confirm: None,
                after_save: None,
                allow_close: false,
                help: false,
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
    Close,
}
struct Studio {
    control: Option<veylvik_control::ipc::Server>,
    project: Project,
    renderer: Arc<Renderer>,
    scheduler: Option<GpuPreviewScheduler>,
    display: Display,
    displayed: Option<Arc<DisplayFrame>>,
    displayed_ticket: Option<PreviewTicket>,
    wanted: Option<PreviewTicket>,
    draft: Node,
    draft_dirty: bool,
    graph: graph::Graph,
    status: String,
    error: bool,
    playing: bool,
    playback: Instant,
    playback_frame: i64,
    end_frame: i64,
    files: Tasks,
    export_controls: sequence::Controls,
    outline_controls: text_outline::Controls,
    confirm: Option<Leave>,
    after_save: Option<Leave>,
    allow_close: bool,
    help: bool,
    theme_checked: Instant,
    accent: egui::Color32,
}
impl Studio {
    fn fail(&mut self, error: impl ToString) {
        self.status = error.to_string();
        self.error = true;
    }
    fn refresh_draft(&mut self) {
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
    fn request_preview(&mut self) {
        self.wanted = None;
        let result = (|| {
            let scheduler = self
                .scheduler
                .as_ref()
                .ok_or("Preview worker unavailable")?;
            scheduler.cancel()?;
            Ok::<_, Box<dyn std::error::Error>>(scheduler.submit(
                self.project.document.revision(),
                self.project.plan()?,
                RenderRequest::full(self.project.time()?),
                self.project.inputs.clone(),
            )?)
        })();
        match result {
            Ok(ticket) => {
                self.wanted = Some(ticket);
                self.status = "Rendering…".into();
                self.error = false;
            }
            Err(e) => self.fail(e),
        }
    }
    fn apply_draft(&mut self) -> bool {
        if !self.draft_dirty {
            return true;
        }
        match self.project.apply(&[Command::Put {
            node: Box::new(self.draft.clone()),
        }]) {
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
        if self.outline_controls.open {
            self.fail("Finish or close the text outline dialog first.");
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
    fn outline_text(&mut self, mut request: veylvik_io::text::OutlineRequest, ctx: &egui::Context) {
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
        let position = self.graph.insertion_position(ctx);
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
            node.position = Some(position);
            Ok(Completed::Outlined(Box::new(node)))
        });
        self.outline_controls.pending = result.is_ok();
        if result.is_err() {
            self.outline_controls.open = true;
        }
        self.file_started(result);
    }
    fn import_image(&mut self) {
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
            let image = veylvik_io::load_png(&path, budget).map_err(|e| e.to_string())?;
            Ok(Completed::Imported {
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
            Ok(Completed::Opened(project)) => {
                self.outline_controls = text_outline::Controls::default();
                self.export_controls = sequence::Controls::default();
                self.project = *project;
                self.graph = graph::Graph::default();
                self.displayed = None;
                self.displayed_ticket = None;
                self.refresh_draft();
                self.request_preview();
            }
            Ok(Completed::Imported { path, image }) => {
                match self
                    .project
                    .import(&path, image, self.graph.insertion_position(ctx))
                {
                    Ok(()) => {
                        self.refresh_draft();
                        self.request_preview();
                    }
                    Err(e) => self.fail(e),
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
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(egui::RichText::new("VEYLVIK").color(self.accent));
            if self.control.is_some() {
                ui.weak("MCP")
                    .on_hover_text("Local live control is enabled for this editor session.");
            }
            ui.separator();
            ui.label(
                self.project
                    .path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or("Untitled".into()),
            );
            if self.project.dirty() || self.draft_dirty {
                ui.label("•");
            }
            if ui.button("Open…").clicked() {
                self.leave(Leave::Open, ui.ctx());
            }
            if ui.button("Import…").clicked() {
                self.import_image();
            }
            if ui
                .button("Export…")
                .on_hover_text(
                    "Export PNG (SDR) or OpenEXR (linear HDR), chosen by filename extension",
                )
                .clicked()
            {
                self.export_frame();
            }
            if ui.button("Sequence…").clicked() {
                self.export_controls.open = true;
            }
            if ui.button("Save").clicked() {
                self.save_project(false);
            }
            if ui.button("Save as…").clicked() {
                self.save_project(true);
            }
            ui.separator();
            if ui
                .add_enabled(
                    self.project.document.history().undo_steps > 0,
                    egui::Button::new("Undo"),
                )
                .clicked()
            {
                self.history(false);
            }
            if ui
                .add_enabled(
                    self.project.document.history().redo_steps > 0,
                    egui::Button::new("Redo"),
                )
                .clicked()
            {
                self.history(true);
            }
            ui.menu_button("Add node", |ui| {
                if ui.button("Outline text…").clicked() && self.can_change_selection() {
                    self.playing = false;
                    self.outline_controls.open = true;
                    ui.close();
                }
                ui.separator();
                for kind in veylvik_document::registry::TYPES {
                    if kind.type_id == "image" {
                        continue;
                    }
                    let label = kind.label;
                    let mut operation: Operation = serde_json::from_value(kind.default_operation())
                        .expect("validated registry defaults");
                    let doc = self.project.document.snapshot();
                    match &mut operation {
                        Operation::Ellipse {
                            x,
                            y,
                            radius_x,
                            radius_y,
                            ..
                        } => {
                            *x = (doc.width as f32 * 0.5).into();
                            *y = (doc.height as f32 * 0.5).into();
                            let radius = doc.width.min(doc.height) as f32 * 0.25;
                            *radius_x = radius.into();
                            *radius_y = radius.into();
                        }
                        Operation::Affine {
                            pivot_x, pivot_y, ..
                        } => {
                            *pivot_x = (doc.width as f32 * 0.5).into();
                            *pivot_y = (doc.height as f32 * 0.5).into();
                        }
                        _ => {}
                    }
                    if ui.button(label).clicked() && self.can_change_selection() {
                        match self
                            .project
                            .add_at(operation, Some(self.graph.insertion_position(ui.ctx())))
                        {
                            Ok(_) => {
                                self.refresh_draft();
                                self.request_preview();
                            }
                            Err(e) => self.fail(e),
                        }
                        ui.close();
                    }
                }
            });
            if ui.button("?").clicked() {
                self.help = !self.help;
            }
        });
    }
    fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.heading("Inspector");
        ui.label(self.draft.operation.descriptor().label);
        ui.add_space(8.0);
        self.draft_dirty |= ui.text_edit_singleline(&mut self.draft.label).changed();
        ui.separator();
        let time = self
            .project
            .time()
            .unwrap_or(veylvik_document::Time { ticks: 0, rate: 1 });
        match &mut self.draft.operation {
            Operation::Path { geometry } => {
                self.draft_dirty |= vector_editor::edit(ui, geometry);
            }
            Operation::Solid { color } => {
                ui.label("Linear RGB and alpha");
                for (i, label) in ["Red", "Green", "Blue", "Alpha"].iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(*label);
                        let mut value = egui::DragValue::new(&mut color[i]).speed(0.01);
                        if i == 3 {
                            value = value.range(0.0..=1.0);
                        }
                        self.draft_dirty |= ui.add(value).changed();
                    });
                }
            }
            Operation::Noise { seed, .. } => {
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
            Operation::Merge {} => {
                ui.label("Foreground over background");
            }
            Operation::Mask {} => {
                ui.label("Image multiplied by mask alpha");
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
            }
        }
        let kind = self.draft.operation.descriptor();
        for parameter in kind.parameters {
            if let Some(value) = self.draft.operation.scalar_mut(parameter.name) {
                let label = format!("{} ({})", parameter.name.replace('_', " "), parameter.unit);
                self.draft_dirty |= scalar(
                    ui,
                    &label,
                    value,
                    parameter.min.unwrap()..=parameter.max.unwrap(),
                    time,
                );
            }
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
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.draft_dirty, egui::Button::new("Apply"))
                .clicked()
            {
                self.apply_draft();
            }
            if ui
                .add_enabled(self.draft_dirty, egui::Button::new("Revert"))
                .clicked()
            {
                self.refresh_draft();
            }
        });
        ui.separator();
        if ui.button("Preview selected").clicked() {
            self.project.preview = Some(self.project.selected);
            self.request_preview();
        }
        if ui.button("View composition output").clicked() {
            self.project.preview = None;
            self.request_preview();
        }
        if ui.button("Set as output").clicked() && self.apply_draft() {
            match self.project.apply(&[Command::Output {
                id: self.project.selected,
            }]) {
                Ok(()) => {
                    self.project.preview = None;
                    self.request_preview();
                }
                Err(e) => self.fail(e),
            }
        }
        if ui.button("Delete node").clicked() && self.can_change_selection() {
            match self.project.apply(&[Command::Remove {
                id: self.project.selected,
            }]) {
                Ok(()) => {
                    self.refresh_draft();
                    self.request_preview();
                }
                Err(e) => self.fail(e),
            }
        }
        ui.add_space(15.0);
        ui.weak("Parameter changes become one undo step when applied.");
    }
    fn timeline(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .button(if self.playing { "Pause" } else { "Play" })
                .clicked()
            {
                self.toggle_play();
            }
            let changed = ui
                .add(egui::Slider::new(&mut self.project.frame, 0..=self.end_frame).text("Frame"))
                .changed();
            if changed {
                self.playing = false;
                self.request_preview();
            }
            ui.label("End");
            ui.add(egui::DragValue::new(&mut self.end_frame).range(1..=100000));
            let fps = self.project.document.snapshot().frame_rate;
            ui.weak(format!("{} / {} fps", fps.ticks, fps.rate));
        });
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
                } => {
                    if self.can_change_selection() {
                        match self.project.connect(source, target, port) {
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
        if self.help {
            egui::Window::new("Keyboard shortcuts")
                .open(&mut self.help)
                .show(ctx, |ui| {
                    for (key, action) in [
                        ("Space", "Play / pause"),
                        ("Ctrl+O", "Open composition"),
                        ("Ctrl+I", "Import PNG image"),
                        ("Ctrl+E", "Export current preview"),
                        ("Ctrl+Shift+E", "Export PNG sequence"),
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
        for _ in 0..8 {
            let Some(request) = self.control.as_ref().and_then(|server| server.try_recv()) else {
                break;
            };
            let result =
                control::call(self, &request.name, &request.arguments).map_err(|e| e.to_string());
            request.respond(result);
        }
        ctx.request_repaint_after(Duration::from_secs(2));
        if let Some(scheduler) = &self.scheduler {
            match scheduler.try_take() {
                Ok(Some(result)) if self.wanted == Some(result.ticket) => match result.frame {
                    Ok(frame) => {
                        self.displayed = Some(self.display.frame(&self.renderer, frame));
                        self.displayed_ticket = Some(result.ticket);
                        if !self.error && !self.files.busy() {
                            self.status = format!(
                                "{:.1} ms · {}",
                                result.elapsed.as_secs_f64() * 1000.0,
                                self.renderer.adapter.name
                            );
                        }
                    }
                    Err(e) => self.fail(e),
                },
                Err(e) => self.fail(e),
                _ => {}
            }
        }
        if self.playing {
            let fps = self.project.document.snapshot().frame_rate;
            let elapsed =
                (self.playback.elapsed().as_secs_f64() * fps.ticks as f64 / fps.rate as f64) as i64;
            let frame = (self.playback_frame + elapsed) % (self.end_frame + 1);
            if frame != self.project.frame {
                self.project.frame = frame;
                self.request_preview();
            }
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        if !self.files.busy()
            && !self.outline_controls.open
            && self.confirm.is_none()
            && !ctx.egui_wants_keyboard_input()
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
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::O)) {
                self.leave(Leave::Open, &ctx);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::I)) {
                self.import_image();
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
            !self.files.busy() && !self.outline_controls.open && self.confirm.is_none(),
            |ui| {
                egui::Panel::top("toolbar").show_inside(ui, |ui| self.toolbar(ui));
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
                        self.timeline(ui);
                        ui.separator();
                        self.graph(ui);
                    });
                egui::Panel::right("inspector")
                    .default_size(280.0)
                    .min_size(250.0)
                    .resizable(true)
                    .show_inside(ui, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| self.inspector(ui));
                    });
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading("Viewer");
                        ui.weak(if self.project.preview.is_some() {
                            "Selected node preview"
                        } else {
                            "Composition output"
                        });
                    });
                    if self.displayed.is_some() && self.displayed_ticket != self.wanted {
                        ui.weak(if self.error {
                            "Showing previous frame · preview failed"
                        } else {
                            "Showing previous frame · rendering"
                        });
                    }
                    if let Some(frame) = &self.displayed {
                        let available = ui.available_size();
                        let aspect =
                            frame.frame.region().width as f32 / frame.frame.region().height as f32;
                        let width = available.x.min(available.y * aspect).max(1.0);
                        let size = egui::vec2(width, width / aspect);
                        let rect = egui::Rect::from_center_size(
                            ui.available_rect_before_wrap().center(),
                            size,
                        );
                        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                            rect,
                            Paint(frame.clone()),
                        ));
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
        self.dialogs(&ctx);
        if let Some(request) = self
            .outline_controls
            .show(&ctx, !self.files.busy() && self.confirm.is_none())
        {
            self.outline_text(request, &ctx);
        }
        if let Some(settings) = self.export_controls.show(
            &ctx,
            self.project.document.snapshot().frame_rate,
            !self.files.busy() && !self.outline_controls.open && self.confirm.is_none(),
        ) {
            self.export_sequence(settings, &ctx);
        }
    }
    fn on_exit(&mut self) {
        self.control.take();
        self.scheduler.take();
        self.displayed.take();
        let _ = self.renderer.device().poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(30)),
        });
    }
}
