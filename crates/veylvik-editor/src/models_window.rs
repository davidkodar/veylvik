//! AI models manager: install, cancel, uninstall, enable and inspect every local model. All file
//! work runs on its own thread; the window only reads cached status.
use eframe::egui;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::JoinHandle,
};
use veylvik_segment::models::{self, Model};

#[derive(Default)]
struct Shared {
    done: AtomicU64,
    total: AtomicU64,
    cancel: AtomicBool,
}
enum Action {
    Install,
    Uninstall,
    Enable(bool),
}
struct Job {
    action: Action,
    shared: Arc<Shared>,
    thread: JoinHandle<Result<(), String>>,
}
#[derive(Clone, Copy, Default)]
pub struct Status {
    pub installed: bool,
    pub enabled: bool,
}
#[derive(Default)]
pub struct ModelsWindow {
    pub open: bool,
    status: BTreeMap<&'static str, Status>,
    /// Status is re-read from disk when the window opens and after every action.
    stale: bool,
    jobs: BTreeMap<&'static str, Job>,
    details: Option<&'static str>,
    message: Option<(String, bool)>,
}
impl ModelsWindow {
    pub fn show_window(&mut self) {
        self.open = true;
        self.stale = true;
    }
    /// Cached status of a model; `None` until it has been read.
    pub fn status(&mut self, id: &str) -> Option<Status> {
        if self.status.is_empty() {
            self.stale = true;
        }
        self.status.get(id).copied()
    }
    pub fn busy(&self, id: &str) -> bool {
        self.jobs.contains_key(id)
    }
    /// Starts installing `model` (the explicit consent is the click that calls this).
    pub fn install(&mut self, model: &'static Model, ctx: &egui::Context) {
        self.start(model, Action::Install, ctx);
    }
    fn start(&mut self, model: &'static Model, action: Action, ctx: &egui::Context) {
        if self.jobs.contains_key(model.id) {
            return;
        }
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        let ctx = ctx.clone();
        let kind = match action {
            Action::Install => 0,
            Action::Uninstall => 1,
            Action::Enable(on) => 2 + u8::from(on),
        };
        let thread = std::thread::Builder::new()
            .name("veylvik-models".into())
            .spawn(move || {
                let result = match kind {
                    0 => models::install(model, &mut |done, total| {
                        worker.done.store(done, Ordering::Relaxed);
                        worker.total.store(total, Ordering::Relaxed);
                        ctx.request_repaint();
                        !worker.cancel.load(Ordering::Relaxed)
                    }),
                    1 => models::uninstall(model),
                    k => models::set_enabled(model, k == 3),
                };
                ctx.request_repaint();
                result.map_err(|e| e.to_string())
            });
        match thread {
            Ok(thread) => {
                self.jobs.insert(
                    model.id,
                    Job {
                        action,
                        shared,
                        thread,
                    },
                );
            }
            Err(e) => self.message = Some((e.to_string(), true)),
        }
    }
    /// Collects finished jobs and refreshes status; call every frame.
    pub fn poll(&mut self) {
        let finished: Vec<&'static str> = self
            .jobs
            .iter()
            .filter(|(_, job)| job.thread.is_finished())
            .map(|(id, _)| *id)
            .collect();
        for id in finished {
            let Some(job) = self.jobs.remove(id) else {
                continue;
            };
            let title = models::find(id).map_or(id, |m| m.title);
            let result = job
                .thread
                .join()
                .unwrap_or_else(|_| Err("model task failed unexpectedly".into()));
            self.message = Some(match (result, job.action) {
                (Ok(()), Action::Install) => (format!("{title} is installed and ready."), false),
                (Ok(()), Action::Uninstall) => (format!("{title} was removed."), false),
                (Ok(()), Action::Enable(true)) => (format!("{title} is switched on."), false),
                (Ok(()), Action::Enable(false)) => (format!("{title} is switched off."), false),
                (Err(e), _) => (e, true),
            });
            self.stale = true;
        }
        if self.stale {
            self.stale = false;
            for model in models::MODELS {
                self.status.insert(
                    model.id,
                    Status {
                        installed: models::installed(model).unwrap_or(false),
                        enabled: models::enabled(model).unwrap_or(true),
                    },
                );
            }
        }
    }
    pub fn show(&mut self, ctx: &egui::Context) {
        self.poll();
        if !self.open {
            return;
        }
        let mut open = true;
        let mut start = None;
        egui::Window::new("AI models")
            .open(&mut open)
            .default_width(460.0)
            .collapsible(false)

            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.weak("Models run on this computer; nothing is sent anywhere. Each download is verified before it is used.");
                ui.add_space(6.0);
                for model in models::MODELS {
                    let status = self.status.get(model.id).copied().unwrap_or_default();
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.strong(model.title);
                            let info = ui
                                .add(
                                    crate::icons::Button::new(crate::icons::Icon::Info, "About this model")
                                        .without_tooltip()
                                        .selected(self.details == Some(model.id)),
                                )
                                .on_hover_ui(|ui| summary(ui, model));
                            if info.clicked() {
                                self.details =
                                    (self.details != Some(model.id)).then_some(model.id);
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.weak(format!("{} · {}", megabytes(model.bytes()), model.licence));
                            });
                        });
                        ui.label(model.uses);
                        ui.horizontal(|ui| {
                            if let Some(job) = self.jobs.get(model.id) {
                                if matches!(job.action, Action::Install) {
                                    let (done, total) = (
                                        job.shared.done.load(Ordering::Relaxed),
                                        job.shared.total.load(Ordering::Relaxed).max(1),
                                    );
                                    ui.add(
                                        egui::ProgressBar::new(done as f32 / total as f32)
                                            .desired_width(220.0)
                                            .text(format!("{} of {}", megabytes(done), megabytes(total))),
                                    );
                                    if ui.button("Cancel").clicked() {
                                        job.shared.cancel.store(true, Ordering::Relaxed);
                                    }
                                } else {
                                    ui.spinner();
                                }
                            } else if status.installed {
                                ui.label(if status.enabled { "Installed" } else { "Installed, switched off" });
                                let mut enabled = status.enabled;
                                if ui
                                    .checkbox(&mut enabled, "Enabled")
                                    .on_hover_text("Switched-off models stay on disk but are not offered or loaded")
                                    .changed()
                                {
                                    start = Some((*model, Action::Enable(enabled)));
                                }
                                if ui.button("Uninstall").on_hover_text("Remove the model's files; you can install it again at any time").clicked() {
                                    start = Some((*model, Action::Uninstall));
                                }
                            } else if ui
                                .button(format!("Install ({})", megabytes(model.bytes())))
                                .on_hover_text(format!("Download from {} and verify every file", model.homepage))
                                .clicked()
                            {
                                start = Some((*model, Action::Install));
                            }
                        });
                        if self.details == Some(model.id) {
                            ui.separator();
                            details(ui, model);
                        }
                    });
                }
                if let Some((message, error)) = &self.message {
                    ui.add_space(4.0);
                    if *error {
                        ui.colored_label(ui.visuals().error_fg_color, message);
                    } else {
                        ui.label(message);
                    }
                }
                if let Ok(root) = models::root() {
                    ui.weak(format!("Stored in {}", root.display()));
                }
            });
        if let Some((model, action)) = start {
            self.start(model, action, ctx);
        }
        self.open = open;
    }
}
fn megabytes(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / 1e6)
}
fn summary(ui: &mut egui::Ui, model: &Model) {
    ui.set_max_width(320.0);
    ui.strong(model.title);
    ui.label(model.summary);
    ui.weak(format!(
        "{} download · {} · {} of graphics memory",
        megabytes(model.bytes()),
        model.speed,
        model.memory
    ));
    ui.weak("Click the info button for pros, cons and details.");
}
fn details(ui: &mut egui::Ui, model: &Model) {
    ui.label(model.summary);
    ui.add_space(4.0);
    ui.strong("Pros");
    for pro in model.pros {
        ui.label(format!("+ {pro}"));
    }
    ui.strong("Cons");
    for con in model.cons {
        ui.label(format!("− {con}"));
    }
    ui.add_space(4.0);
    egui::Grid::new(("model-facts", model.id))
        .num_columns(2)
        .show(ui, |ui| {
            for (label, value) in [
                ("Download", megabytes(model.bytes())),
                ("Speed", model.speed.to_owned()),
                ("Graphics memory", model.memory.to_owned()),
                ("Licence", model.licence.to_owned()),
            ] {
                ui.weak(label);
                ui.label(value);
                ui.end_row();
            }
        });
    ui.hyperlink_to("Model page", model.homepage);
}
