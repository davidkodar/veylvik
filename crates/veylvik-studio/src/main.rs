mod display;
mod project;
use display::{Display, DisplayFrame, Paint};
use eframe::{egui, egui_wgpu};
use project::Project;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use veylvik_document::{Command, Node, NodeId, Operation, Scalar};
use veylvik_render::{GpuPreviewScheduler, PreviewTicket, RenderRequest, Renderer};

fn main() {
    if let Err(error) = run() {
        eprintln!("Veylvik: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() > 1 {
        return Err("usage: veylvik-studio [FILE]".into());
    }
    let path = args.first().map(PathBuf::from);
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
            let mut app = Studio {
                project,
                renderer,
                scheduler: Some(scheduler),
                display,
                displayed: None,
                displayed_ticket: None,
                wanted: None,
                draft,
                draft_dirty: false,
                positions: BTreeMap::new(),
                status: String::new(),
                error: false,
                playing: false,
                playback: Instant::now(),
                playback_frame: 0,
                end_frame: 240,
                save_dialog: false,
                save_path: String::new(),
                confirm_close: false,
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
struct Studio {
    project: Project,
    renderer: Arc<Renderer>,
    scheduler: Option<GpuPreviewScheduler>,
    display: Display,
    displayed: Option<Arc<DisplayFrame>>,
    displayed_ticket: Option<PreviewTicket>,
    wanted: Option<PreviewTicket>,
    draft: Node,
    draft_dirty: bool,
    positions: BTreeMap<NodeId, egui::Pos2>,
    status: String,
    error: bool,
    playing: bool,
    playback: Instant,
    playback_frame: i64,
    end_frame: i64,
    save_dialog: bool,
    save_path: String,
    confirm_close: bool,
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
            node: self.draft.clone(),
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
        if !self.apply_draft() {
            return;
        }
        if save_as || self.project.path.is_none() {
            self.save_path = self
                .project
                .path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| {
                    self.project
                        .base
                        .join("untitled.veylvik")
                        .display()
                        .to_string()
                });
            self.save_dialog = true;
        } else {
            let path = self.project.path.clone().unwrap();
            self.save_to(path);
        }
    }
    fn save_to(&mut self, path: PathBuf) {
        match self.project.save(&path) {
            Ok(()) => {
                self.status = format!("Saved {}", path.display());
                self.error = false;
                self.save_dialog = false;
            }
            Err(e) => self.fail(e),
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
            if ui.button("Save").clicked() {
                self.save_project(false);
            }
            if ui.button("Save as…").clicked() {
                self.save_project(true);
            }
            ui.separator();
            if ui.button("Undo").clicked() {
                self.history(false);
            }
            if ui.button("Redo").clicked() {
                self.history(true);
            }
            ui.menu_button("Add node", |ui| {
                for (label, operation) in [
                    (
                        "Solid",
                        Operation::Solid {
                            color: [0.2, 0.4, 1.0, 1.0],
                        },
                    ),
                    (
                        "Noise",
                        Operation::Noise {
                            seed: 42,
                            scale: 12.0.into(),
                        },
                    ),
                    ("Exposure", Operation::Exposure { stops: 0.0.into() }),
                    (
                        "Transform",
                        Operation::Transform {
                            x: 0.0.into(),
                            y: 0.0.into(),
                        },
                    ),
                    ("Blur", Operation::Blur { radius: 4.0.into() }),
                    ("Merge", Operation::Merge),
                ] {
                    if ui.button(label).clicked() && self.can_change_selection() {
                        match self.project.add(operation) {
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
            Operation::Noise { seed, scale } => {
                ui.horizontal(|ui| {
                    ui.label("Seed");
                    self.draft_dirty |= ui.add(egui::DragValue::new(seed)).changed();
                });
                self.draft_dirty |= scalar(
                    ui,
                    "Cell size",
                    scale,
                    bounds(&veylvik_document::registry::NOISE, "scale"),
                    time,
                );
            }
            Operation::Exposure { stops } => {
                self.draft_dirty |= scalar(
                    ui,
                    "Exposure (stops)",
                    stops,
                    bounds(&veylvik_document::registry::EXPOSURE, "stops"),
                    time,
                )
            }
            Operation::Transform { x, y } => {
                self.draft_dirty |= scalar(
                    ui,
                    "X (pixels)",
                    x,
                    bounds(&veylvik_document::registry::TRANSFORM, "x"),
                    time,
                );
                self.draft_dirty |= scalar(
                    ui,
                    "Y (pixels)",
                    y,
                    bounds(&veylvik_document::registry::TRANSFORM, "y"),
                    time,
                );
            }
            Operation::Blur { radius } => {
                self.draft_dirty |= scalar(
                    ui,
                    "Radius (pixels)",
                    radius,
                    bounds(&veylvik_document::registry::BLUR, "radius"),
                    time,
                )
            }
            Operation::Merge => {
                ui.label("Foreground over background");
            }
            Operation::Image { asset } => {
                ui.label("Source image");
                ui.label(asset.as_str());
                ui.label("Use the import command to create image compositions.");
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
        ui.horizontal(|ui| {
            ui.heading("Nodes");
            ui.weak("Select a node to inspect it · drag to arrange · scroll to pan");
        });
        let doc = self.project.document.snapshot();
        let nodes: Vec<_> = doc
            .validate()
            .unwrap_or_default()
            .iter()
            .map(|id| doc.nodes[id].clone())
            .collect();
        let output = doc.output;
        let mut depths = BTreeMap::new();
        let mut rows = BTreeMap::<usize, usize>::new();
        for node in &nodes {
            let depth = node
                .inputs
                .values()
                .map(|id| depths[id] + 1)
                .max()
                .unwrap_or(0);
            depths.insert(node.id, depth);
            let row = rows.entry(depth).or_default();
            self.positions.entry(node.id).or_insert(egui::pos2(
                25.0 + depth as f32 * 195.0,
                20.0 + *row as f32 * 90.0,
            ));
            *row += 1;
        }
        egui::ScrollArea::both().show(ui, |ui| {
            let size = self
                .positions
                .values()
                .fold(egui::vec2(800.0, 170.0), |size, pos| {
                    size.max(pos.to_vec2() + egui::vec2(210.0, 90.0))
                });
            let (canvas, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            let origin = canvas.min.to_vec2();
            for node in &nodes {
                for source in node.inputs.values() {
                    let from = self.positions[source] + origin + egui::vec2(160.0, 30.0);
                    let to = self.positions[&node.id] + origin + egui::vec2(0.0, 30.0);
                    ui.painter()
                        .add(egui::epaint::CubicBezierShape::from_points_stroke(
                            [
                                from,
                                from + egui::vec2(45.0, 0.0),
                                to - egui::vec2(45.0, 0.0),
                                to,
                            ],
                            false,
                            egui::Color32::TRANSPARENT,
                            egui::Stroke::new(1.5_f32, egui::Color32::from_gray(90)),
                        ));
                }
            }
            for node in &nodes {
                let rect = egui::Rect::from_min_size(
                    self.positions[&node.id] + origin,
                    egui::vec2(160.0, 60.0),
                );
                let response =
                    ui.interact(rect, ui.id().with(node.id), egui::Sense::click_and_drag());
                if response.dragged() {
                    let pos = self.positions.get_mut(&node.id).unwrap();
                    *pos += response.drag_delta();
                    pos.x = pos.x.max(0.0);
                    pos.y = pos.y.max(0.0);
                }
                if response.clicked() && self.can_change_selection() {
                    self.project.selected = node.id;
                    self.refresh_draft();
                }
                let selected = node.id == self.project.selected;
                ui.painter()
                    .rect_filled(rect, 0.0, egui::Color32::from_rgb(36, 41, 50));
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(
                        if selected { 2.0_f32 } else { 1.0_f32 },
                        if selected {
                            self.accent
                        } else {
                            egui::Color32::from_gray(65)
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    rect.min + egui::vec2(10.0, 15.0),
                    egui::Align2::LEFT_CENTER,
                    &node.label,
                    egui::FontId::proportional(13.0),
                    egui::Color32::WHITE,
                );
                let subtitle = if node.id == output {
                    "Composition output"
                } else {
                    node.operation.descriptor().label
                };
                ui.painter().text(
                    rect.min + egui::vec2(10.0, 41.0),
                    egui::Align2::LEFT_CENTER,
                    subtitle,
                    egui::FontId::proportional(11.0),
                    egui::Color32::from_gray(160),
                );
            }
        });
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.save_dialog {
            egui::Window::new("Save composition")
                .collapsible(false)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.label("Destination path");
                    ui.text_edit_singleline(&mut self.save_path);
                    ui.weak("An existing composition at this path will be replaced.");
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            self.save_to(PathBuf::from(&self.save_path));
                        }
                        if ui.button("Cancel").clicked() {
                            self.save_dialog = false;
                        }
                    });
                });
        }
        if self.confirm_close {
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("Save your work before closing, or discard it.");
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            self.save_project(false);
                            self.confirm_close = false;
                        }
                        if ui.button("Discard and close").clicked() {
                            self.allow_close = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui.button("Keep editing").clicked() {
                            self.confirm_close = false;
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
fn bounds(
    kind: &veylvik_document::registry::NodeType,
    name: &str,
) -> std::ops::RangeInclusive<f32> {
    let parameter = kind
        .parameters
        .iter()
        .find(|parameter| parameter.name == name)
        .unwrap();
    parameter.min.unwrap()..=parameter.max.unwrap()
}
fn scalar(
    ui: &mut egui::Ui,
    label: &str,
    scalar: &mut Scalar,
    range: std::ops::RangeInclusive<f32>,
    time: veylvik_document::Time,
) -> bool {
    ui.label(label);
    let mut changed = match scalar {
        Scalar::Constant { value } => ui
            .add(egui::DragValue::new(value).speed(0.1).range(range))
            .changed(),
        Scalar::Linear { keys } => {
            let mut changed = false;
            ui.weak(format!(
                "{} keyframes · {:.3} now",
                keys.len(),
                Scalar::Linear { keys: keys.clone() }
                    .at(time)
                    .unwrap_or(0.0)
            ));
            egui::CollapsingHeader::new(format!("Edit {label} keys")).show(ui, |ui| {
                for (index, key) in keys.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.horizontal(|ui| {
                            changed |= ui.add(egui::DragValue::new(&mut key.time.ticks)).changed();
                            ui.label("/");
                            changed |= ui
                                .add(egui::DragValue::new(&mut key.time.rate).range(1..=u32::MAX))
                                .changed();
                            ui.label("s");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut key.value)
                                        .speed(0.1)
                                        .range(range.clone()),
                                )
                                .changed();
                        });
                    });
                }
            });
            changed
        }
    };
    if ui.small_button("Add key at playhead").clicked() {
        let value = scalar.at(time).unwrap_or(0.0);
        match scalar {
            Scalar::Constant { .. } => {
                *scalar = Scalar::Linear {
                    keys: vec![veylvik_document::Key { time, value }],
                };
                changed = true;
            }
            Scalar::Linear { keys } => {
                if !keys
                    .iter()
                    .any(|key| key.time.compare(time).is_ok_and(|order| order.is_eq()))
                {
                    keys.push(veylvik_document::Key { time, value });
                    keys.sort_by(|a, b| {
                        a.time.compare(b.time).unwrap_or(std::cmp::Ordering::Equal)
                    });
                    changed = true;
                }
            }
        }
    }
    changed
}
impl eframe::App for Studio {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.theme(&ctx);
        ctx.request_repaint_after(Duration::from_secs(2));
        if let Some(scheduler) = &self.scheduler {
            match scheduler.try_take() {
                Ok(Some(result)) if self.wanted == Some(result.ticket) => match result.frame {
                    Ok(frame) => {
                        self.displayed = Some(self.display.frame(&self.renderer, frame));
                        self.displayed_ticket = Some(result.ticket);
                        self.status = format!(
                            "{:.1} ms · {}",
                            result.elapsed.as_secs_f64() * 1000.0,
                            self.renderer.adapter.name
                        );
                        self.error = false;
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
        if !ctx.egui_wants_keyboard_input() {
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
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && (self.project.dirty() || self.draft_dirty)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        egui::Panel::top("toolbar").show_inside(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show_inside(ui, |ui| {
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
                let aspect = frame.frame.width() as f32 / frame.frame.height() as f32;
                let width = available.x.min(available.y * aspect).max(1.0);
                let size = egui::vec2(width, width / aspect);
                let rect =
                    egui::Rect::from_center_size(ui.available_rect_before_wrap().center(), size);
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
        self.dialogs(&ctx);
    }
    fn on_exit(&mut self) {
        self.scheduler.take();
        self.displayed.take();
        let _ = self.renderer.device().poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(30)),
        });
    }
}
