//! Durable export snapshots. One independent worker renders jobs while the editor stays usable.
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Instant, SystemTime},
};
use veylvik_document::{Composition, NodeId, Time, sequence::FrameRange};
use veylvik_render::{Plan, RenderRequest, Renderer};
type Result<T> = std::result::Result<T, String>;
const MAX_JOBS: usize = 64;
const MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub enum Output {
    Frame {
        time: Time,
        output: NodeId,
    },
    Sequence {
        frames: FrameRange,
        format: veylvik_io::FrameFormat,
        output: NodeId,
    },
    Video(Box<veylvik_io::video_export::Settings>),
}
impl Output {
    fn label(&self) -> &'static str {
        match self {
            Self::Frame { .. } => "Still frame",
            Self::Sequence { .. } => "Image sequence",
            Self::Video(_) => "Video",
        }
    }
    fn node(&self, doc: &Composition) -> NodeId {
        match self {
            Self::Frame { output, .. } | Self::Sequence { output, .. } => *output,
            Self::Video(_) => doc.output,
        }
    }
    pub fn choice(&self) -> crate::files::Choice {
        match self {
            Self::Frame { .. } => crate::files::Choice::Export,
            Self::Sequence { .. } => crate::files::Choice::Folder,
            Self::Video(_) => crate::files::Choice::Video,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum State {
    Pending,
    Rendering,
    Complete,
    Cancelled,
    Failed,
    Interrupted,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Dependency {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}
impl Dependency {
    fn read(path: PathBuf) -> Result<Self> {
        let path = path
            .canonicalize()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let m = std::fs::metadata(&path).map_err(|e| e.to_string())?;
        Ok(Self {
            path,
            bytes: m.len(),
            modified: m.modified().map_err(|e| e.to_string())?,
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: NodeId,
    pub name: String,
    pub document: Composition,
    pub base: PathBuf,
    pub path: PathBuf,
    pub output: Output,
    state: State,
    detail: String,
    elapsed: f64,
    dependencies: Vec<Dependency>,
}
impl Job {
    /// Runs on the file worker, including potentially large sequence dependency enumeration.
    pub fn prepare(
        name: String,
        document: Composition,
        base: PathBuf,
        path: PathBuf,
        output: Output,
    ) -> Result<Self> {
        if matches!(output, Output::Frame { .. }) {
            veylvik_io::FrameFormat::from_path(&path).map_err(|e| e.to_string())?;
        }
        document.validate().map_err(|e| e.to_string())?;
        if path.exists() {
            return Err(
                "Choose a new output name; queued renders never replace existing files or folders."
                    .into(),
            );
        }
        veylvik_io::protect_inputs(&document, &base, &path).map_err(|e| e.to_string())?;
        let mut paths = std::collections::BTreeSet::new();
        for node in document.nodes.values() {
            if let Some(asset) = node.operation.asset() {
                let path = base.join(asset);
                paths.insert(path.clone());
                if veylvik_io::footage::is_sequence(&path) {
                    paths.extend(
                        veylvik_io::footage::dependencies(&path).map_err(|e| e.to_string())?,
                    );
                }
            }
        }
        for transform in document.ocio_transforms() {
            if transform.config.starts_with("ocio://") {
                continue;
            }
            let path = base.join(&transform.config);
            let config =
                veylvik_color::Config::load(&path.to_string_lossy()).map_err(|e| e.to_string())?;
            let processor = config
                .processor(
                    &transform.source,
                    &transform.destination,
                    &transform.display,
                    &transform.view,
                )
                .map_err(|e| e.to_string())?;
            paths.extend(
                processor
                    .files()
                    .iter()
                    .map(|p| path.parent().unwrap_or(&base).join(p)),
            );
            paths.insert(path);
        }
        if let Output::Video(s) = &output {
            s.times(&document).map_err(|e| e.to_string())?;
            if let Some(music) = &s.audio.music {
                paths.insert(base.join(music));
            }
        }
        let dependencies = paths
            .into_iter()
            .map(Dependency::read)
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            id: NodeId::new_v4(),
            name,
            document,
            base,
            path,
            output,
            state: State::Pending,
            detail: "Snapshot ready".into(),
            elapsed: 0.,
            dependencies,
        })
    }
    pub fn retry_to(&mut self, path: PathBuf) -> Result<()> {
        self.path = path;
        self.verify()?;
        self.id = NodeId::new_v4();
        self.state = State::Pending;
        self.detail = "Snapshot ready".into();
        self.elapsed = 0.;
        Ok(())
    }
    fn verify(&self) -> Result<()> {
        for dep in &self.dependencies {
            if Dependency::read(dep.path.clone())? != *dep {
                return Err(format!(
                    "Source changed since this job was queued: {}. Add a new job from the composition.",
                    dep.path.display()
                ));
            }
        }
        if self.path.exists() {
            return Err(
                "Output already exists. Retry to a new destination; existing output is preserved."
                    .into(),
            );
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
struct Saved {
    version: u32,
    jobs: Vec<Job>,
}
#[derive(Default)]
struct Progress {
    completed: u32,
    total: u32,
    phase: String,
}
struct Active {
    id: NodeId,
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<Progress>>,
    started: Instant,
    receiver: mpsc::Receiver<Result<bool>>,
    thread: std::thread::JoinHandle<()>,
}
pub enum Action {
    Frame,
    Sequence,
    Video,
    Retry(Box<Job>),
}
pub struct Queue {
    pub open: bool,
    jobs: Vec<Job>,
    active: Option<Active>,
    running: bool,
    path: PathBuf,
    _lock: Option<std::fs::File>,
    pub error: Option<String>,
}
impl Queue {
    pub fn new() -> Self {
        let store = crate::recovery::default_store();
        let path = store.map(|s| s.directory().parent().unwrap().join("render-queue.json"));
        match path.and_then(Self::load) {
            Ok(q) => q,
            Err(e) => Self {
                open: false,
                jobs: vec![],
                active: None,
                running: false,
                path: PathBuf::new(),
                _lock: None,
                error: Some(e),
            },
        }
    }
    fn load(path: PathBuf) -> Result<Self> {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path.parent().unwrap())
            .map_err(|e| e.to_string())?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(path.with_extension("lock"))
            .map_err(|e| e.to_string())?;
        lock.try_lock().map_err(|_| {
            "Render queue is owned by another Veylvik window. Use that window's queue.".to_owned()
        })?;
        let jobs = if path.exists() {
            if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > MAX_BYTES as u64 {
                return Err("Saved render queue is too large".into());
            }
            let mut saved: Saved =
                serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| format!("Cannot read render queue: {e}"))?;
            if saved.version != 1 || saved.jobs.len() > MAX_JOBS {
                return Err("Unsupported render queue".into());
            }
            for job in &mut saved.jobs {
                if job.state == State::Rendering {
                    job.state = State::Interrupted;
                    job.detail =
                        "Interrupted when Veylvik closed. Retry explicitly to a new destination."
                            .into();
                }
            }
            saved.jobs
        } else {
            vec![]
        };
        Ok(Self {
            open: false,
            jobs,
            active: None,
            running: false,
            path,
            _lock: Some(lock),
            error: None,
        })
    }
    fn persist(&mut self) -> bool {
        let result = (|| {
            if self._lock.is_none() {
                return Err("Render queue is unavailable".to_owned());
            }
            let bytes = serde_json::to_vec(&Saved {
                version: 1,
                jobs: self.jobs.clone(),
            })
            .map_err(|e| e.to_string())?;
            if bytes.len() > MAX_BYTES {
                return Err("Render queue exceeds 64 MiB; remove old jobs first".into());
            }
            veylvik_document::atomic_write(&self.path, &bytes).map_err(|e| e.to_string())
        })();
        if let Err(e) = result {
            self.running = false;
            self.error = Some(e);
            false
        } else {
            self.error = None;
            true
        }
    }
    pub fn add(&mut self, job: Job) -> Result<()> {
        self.open = true;
        if self._lock.is_none() {
            return Err(self.error.clone().unwrap_or_default());
        }
        if self.jobs.len() >= MAX_JOBS {
            return Err("Queue holds 64 jobs; remove finished jobs first".into());
        }
        if self
            .jobs
            .iter()
            .any(|j| j.path == job.path && matches!(j.state, State::Pending | State::Rendering))
        {
            return Err("This destination is already queued. Choose a different name.".into());
        }
        self.jobs.push(job);
        if !self.persist() {
            self.jobs.pop();
            return Err(self.error.clone().unwrap_or_default());
        }
        Ok(())
    }
    pub fn will_start(&self) -> bool {
        self.running && self.active.is_none() && self.pending() > 0
    }
    pub fn pause(&mut self) {
        self.running = false;
    }
    pub fn pending(&self) -> usize {
        self.jobs
            .iter()
            .filter(|j| j.state == State::Pending)
            .count()
    }
    pub fn busy(&self) -> bool {
        self.active.is_some()
    }
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"running":self.running,"active":self.active.as_ref().map(|a|a.id),"jobs":self.jobs.iter().map(|j|serde_json::json!({"id":j.id,"name":j.name,"path":j.path,"state":j.state,"detail":j.detail,"elapsed_seconds":j.elapsed})).collect::<Vec<_>>()})
    }
    /// Returns true when starting a job so the host can relinquish its playback cache.
    pub fn tick(&mut self, renderer: &Arc<Renderer>, ctx: &egui::Context) -> bool {
        if let Some(active) = &self.active {
            let result = match active.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Render worker disconnected".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                let active = self.active.take().unwrap();
                let _ = active.thread.join();
                if let Some(job) = self.jobs.iter_mut().find(|j| j.id == active.id) {
                    job.elapsed = active.started.elapsed().as_secs_f64();
                    let (state, detail) = match result {
                        Ok(true) => (
                            State::Complete,
                            active
                                .progress
                                .lock()
                                .map(|p| format!("Completed · {}", p.phase))
                                .unwrap_or_else(|_| "Completed".into()),
                        ),
                        Ok(false) => (
                            State::Cancelled,
                            "Cancelled. Partial sequence frames are preserved.".into(),
                        ),
                        Err(e) => (State::Failed, e),
                    };
                    job.state = state;
                    job.detail = detail;
                }
                self.persist();
                ctx.request_repaint();
                return false;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        if !self.running || self.active.is_some() {
            return false;
        }
        let Some(index) = self.jobs.iter().position(|j| j.state == State::Pending) else {
            self.running = false;
            return false;
        };
        self.jobs[index].state = State::Rendering;
        if !self.persist() {
            self.jobs[index].state = State::Pending;
            return false;
        }
        let job = self.jobs[index].clone();
        let id = job.id;
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new(Progress {
            phase: "Preparing sources".into(),
            ..Default::default()
        }));
        let (sender, receiver) = mpsc::channel();
        let (r, c, p, context) = (
            renderer.clone(),
            cancel.clone(),
            progress.clone(),
            ctx.clone(),
        );
        match std::thread::Builder::new()
            .name("veylvik-render-queue".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    render(&job, &r, &c, &p)
                }))
                .unwrap_or_else(|_| Err("Render worker failed unexpectedly".into()));
                let _ = sender.send(result);
                context.request_repaint();
            }) {
            Ok(thread) => {
                self.active = Some(Active {
                    id,
                    cancel,
                    progress,
                    started: Instant::now(),
                    receiver,
                    thread,
                })
            }
            Err(e) => {
                self.jobs[index].state = State::Failed;
                self.jobs[index].detail = e.to_string();
                self.persist();
            }
        }
        ctx.request_repaint();
        true
    }
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Action> {
        let mut open = self.open;
        let mut action = None;
        let mut changed = false;
        let mut reorder = None;
        let mut remove = None;
        egui::Window::new("Render queue").open(&mut open).default_width(670.).default_pos(egui::pos2(280.,100.)).min_width(460.).collapsible(false).show(ctx, |ui| {
            ui.label("Queue a snapshot, then keep editing. Jobs render one at a time.");
            ui.horizontal(|ui| {
                if ui.button("Add video…").clicked() { action = Some(Action::Video); }
                if ui.button("Add sequence…").clicked() { action = Some(Action::Sequence); }
                if ui.button("Add current frame…").clicked() { action = Some(Action::Frame); }
            });
            ui.horizontal(|ui| {
                if ui.add_enabled(self._lock.is_some() && !self.running && self.jobs.iter().any(|j|j.state == State::Pending), egui::Button::new("Start queue")).clicked() { self.running = true; }
                if ui.add_enabled(self.running, egui::Button::new("Pause after current")).clicked() { self.running = false; }
                if ui.button("Clear completed").clicked() { self.jobs.retain(|j| j.state != State::Complete); changed = true; }
                ui.label(if self.running { "Running" } else if self.busy() { "Finishing current job" } else { "Paused" });
            });
            if let Some(error) = &self.error { ui.colored_label(egui::Color32::LIGHT_RED, error); }
            ui.separator();
            if self.jobs.is_empty() { ui.weak("No jobs yet. Add a video, image sequence or still frame above."); }
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height()-260.).clamp(120.,420.)).auto_shrink([false, true]).show(ui, |ui| {
                let len = self.jobs.len();
                for (index, job) in self.jobs.iter_mut().enumerate() {
                    ui.push_id(job.id, |ui| {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| { ui.strong(&job.name); ui.weak(job.output.label());
                                let color = match job.state { State::Complete => egui::Color32::from_rgb(83,190,156), State::Failed | State::Interrupted => egui::Color32::from_rgb(232,155,93), State::Rendering => ui.visuals().selection.bg_fill, _ => ui.visuals().weak_text_color() };
                                ui.colored_label(color,format!("{:?}",job.state)); });
                            ui.add(egui::Label::new(egui::RichText::new(job.path.to_string_lossy()).small()).truncate()).on_hover_text(job.path.to_string_lossy());
                            let range = match &job.output {
                                Output::Frame { time, .. } => format!("Time {:.3} s", time.seconds().unwrap_or(0.)),
                                Output::Sequence { frames, format, .. } => format!("Frames {}–{} · {}", frames.start, frames.end, format.label()),
                                Output::Video(s) => format!("Frames {}–{} · {} × {} · {}", s.start, s.end, s.width, s.height, s.codec.label()),
                            };
                            ui.weak(range);
                            if let Some(active) = self.active.as_ref().filter(|a|a.id == job.id) {
                                if let Ok(p) = active.progress.lock() {
                                    ui.add(egui::ProgressBar::new(p.completed as f32 / p.total.max(1) as f32).text(format!("{} / {} frames · {:.1} s · {}", p.completed, p.total, active.started.elapsed().as_secs_f64(), p.phase)));
                                }
                                if ui.add_enabled(!active.cancel.load(Ordering::Relaxed), egui::Button::new("Cancel job")).on_hover_text("Stop at the next safe render or encoder boundary").clicked() { active.cancel.store(true, Ordering::Relaxed); }
                            } else {
                                ui.add(egui::Label::new(format!("{} · {:.1} s", job.detail, job.elapsed)).wrap());
                                ui.horizontal(|ui| {
                                    if job.state == State::Pending {
                                        if ui.add_enabled(index > 0, egui::Button::new("Move earlier")).clicked() { reorder = Some((index,index-1)); }
                                        if ui.add_enabled(index+1 < len, egui::Button::new("Move later")).clicked() { reorder = Some((index,index+1)); }
                                        if ui.button("Cancel").clicked() { job.state = State::Cancelled; job.detail = "Cancelled before rendering".into(); changed = true; }
                                    } else if ui.button("Retry to…").on_hover_text("Keep the same snapshot and choose a new output destination").clicked() { action = Some(Action::Retry(Box::new(job.clone()))); }
                                    if ui.button("Remove").clicked() { remove = Some(index); }
                                });
                            }
                        });
                    });
                }
            });
            ui.add(egui::Label::new(egui::RichText::new("Saved locally. Reopening starts paused. External media must remain unchanged; completed files are never overwritten.").weak()).wrap());
        });
        self.open = open;
        if let Some((a, b)) = reorder {
            self.jobs.swap(a, b);
            changed = true;
        }
        if let Some(index) = remove {
            self.jobs.remove(index);
            changed = true;
        }
        if changed {
            self.persist();
        }
        action
    }
}
impl Drop for Queue {
    fn drop(&mut self) {
        if let Some(active) = &self.active {
            active.cancel.store(true, Ordering::Relaxed);
        }
    }
}
fn render(
    job: &Job,
    renderer: &Renderer,
    cancel: &AtomicBool,
    progress: &Mutex<Progress>,
) -> Result<bool> {
    job.verify()?;
    veylvik_io::protect_inputs(&job.document, &job.base, &job.path).map_err(|e| e.to_string())?;
    if cancel.load(Ordering::Relaxed) {
        return Ok(false);
    }
    let plan = Plan::compile_output(&job.document, job.output.node(&job.document))
        .map_err(|e| e.to_string())?;
    let inputs = veylvik_io::load_inputs(&plan, &job.base, renderer.budget_bytes)
        .map_err(|e| e.to_string())?;
    let update = |completed, total, phase: String| {
        if let Ok(mut p) = progress.lock() {
            *p = Progress {
                completed,
                total,
                phase,
            };
        }
        !cancel.load(Ordering::Relaxed)
    };
    match &job.output {
        Output::Frame { time, .. } => {
            if !update(0, 1, "Rendering".into()) {
                return Ok(false);
            }
            let frame = renderer
                .render_gpu_export(&plan, RenderRequest::full(*time), &inputs)
                .map_err(|e| e.to_string())?;
            let frame = renderer.read_frame(&frame).map_err(|e| e.to_string())?;
            if cancel.load(Ordering::Relaxed) {
                return Ok(false);
            }
            let pixels = veylvik_io::export::sdr_pixels(
                &job.path,
                &frame.pixels,
                job.document.output_display(),
            );
            veylvik_io::export::save_frame_new(
                &job.path,
                frame.region.width,
                frame.region.height,
                &pixels,
            )
            .map_err(|e| e.to_string())?;
            update(
                1,
                1,
                format!("{} × {}", frame.region.width, frame.region.height),
            );
            Ok(true)
        }
        Output::Sequence {
            frames,
            format,
            output,
        } => {
            let value = veylvik_io::sequence::render_sequence_with_inputs(
                renderer,
                &job.document,
                &inputs,
                &job.path,
                veylvik_io::sequence::SequenceRequest {
                    frames: *frames,
                    format: *format,
                    output: *output,
                    region: None,
                },
                |done, total| update(done, total, "Rendering sequence".into()),
            )
            .map_err(|e| e.to_string())?;
            let complete = value["manifest"]["status"] == "complete";
            if complete {
                let count = frames.count().map_err(|e| e.to_string())?;
                update(count, count, format!("{} sequence", format.label()));
            }
            Ok(complete)
        }
        Output::Video(settings) => {
            let result = veylvik_io::video_export::render(
                renderer,
                &job.document,
                &inputs,
                &job.path,
                settings,
                |p| update(p.completed, p.total, format!("{} · {}", p.phase, p.backend)),
            )
            .map_err(|e| e.to_string())?;
            if !result.cancelled {
                update(
                    result.frames,
                    result.frames,
                    format!(
                        "{} · {}",
                        result.backend,
                        if result.host_staging {
                            "RAM staging"
                        } else if result.experimental_interop {
                            "GPU transfer · experimental DMA-BUF"
                        } else {
                            "GPU transfer"
                        }
                    ),
                );
            }
            Ok(!result.cancelled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp() -> PathBuf {
        let p = std::env::temp_dir().join(format!("veylvik-queue-test-{}", NodeId::new_v4()));
        std::fs::create_dir(&p).unwrap();
        p
    }
    fn fixture(base: &std::path::Path, name: &str) -> Job {
        let doc = veylvik_document::blank(32, 32, Time::new(24, 1).unwrap()).unwrap();
        let output = Output::Frame {
            time: Time::new(0, 1).unwrap(),
            output: doc.output,
        };
        Job::prepare(
            name.into(),
            doc,
            base.to_path_buf(),
            base.join(format!("{name}.png")),
            output,
        )
        .unwrap()
    }
    #[test]
    fn durable_snapshots_order_interruption_and_exclusive_ownership() {
        let dir = temp();
        let path = dir.join("queue.json");
        let mut queue = Queue::load(path.clone()).unwrap();
        assert!(Queue::load(path.clone()).is_err());
        let mut original = fixture(&dir, "first");
        queue.add(original.clone()).unwrap();
        original.document.width = 64;
        queue.add(fixture(&dir, "second")).unwrap();
        queue.jobs.swap(0, 1);
        queue.jobs[1].state = State::Rendering;
        assert!(queue.persist());
        drop(queue);
        let queue = Queue::load(path).unwrap();
        assert!(!queue.running);
        assert_eq!(queue.jobs[0].name, "second");
        assert_eq!(queue.jobs[1].document.width, 32);
        assert_eq!(queue.jobs[1].state, State::Interrupted);
        drop(queue);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn dependencies_destinations_and_retry_preserve_old_outputs() {
        let dir = temp();
        let source = dir.join("source.bin");
        std::fs::write(&source, b"old").unwrap();
        let mut job = fixture(&dir, "one");
        job.dependencies
            .push(Dependency::read(source.clone()).unwrap());
        assert!(job.verify().is_ok());
        std::fs::write(&source, b"changed").unwrap();
        assert!(job.verify().unwrap_err().contains("Source changed"));
        assert!(job.retry_to(dir.join("retry.png")).is_err());
        job.dependencies.clear();
        std::fs::write(&job.path, b"keep").unwrap();
        assert!(job.verify().is_err());
        let previous = job.path.clone();
        job.retry_to(dir.join("new.png")).unwrap();
        assert_eq!(std::fs::read(previous).unwrap(), b"keep");
        let mut q = Queue::load(dir.join("queue.json")).unwrap();
        q.add(job.clone()).unwrap();
        assert!(q.add(job).is_err());
        drop(q);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn gpu_queue_snapshot_frame_sequence_cancel_and_edit_independence() {
        let dir = temp();
        let renderer = Arc::new(Renderer::new().unwrap());
        let ctx = egui::Context::default();
        let mut queue = Queue::load(dir.join("queue.json")).unwrap();
        let mut job = fixture(&dir, "frame");
        job.document
            .nodes
            .get_mut(&job.document.output)
            .unwrap()
            .operation = veylvik_document::Operation::Solid(veylvik_document::nodes::Solid {
            color: [1., 0., 0., 1.],
        });
        queue.add(job.clone()).unwrap();
        // Editing the live composition after enqueue must not alter the captured red image.
        job.document
            .nodes
            .get_mut(&job.document.output)
            .unwrap()
            .operation = veylvik_document::Operation::Solid(veylvik_document::nodes::Solid {
            color: [0., 1., 0., 1.],
        });
        queue.running = true;
        let deadline = Instant::now() + std::time::Duration::from_secs(30);
        while queue.running || queue.busy() {
            queue.tick(&renderer, &ctx);
            assert!(Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(
            queue.jobs[0].state,
            State::Complete,
            "{}",
            queue.jobs[0].detail
        );
        let image = image::open(dir.join("frame.png")).unwrap().to_rgba8();
        assert!(image.get_pixel(0, 0)[0] > 200 && image.get_pixel(0, 0)[1] < 5);
        job.path = dir.join("frames");
        job.output = Output::Sequence {
            frames: FrameRange::new(0, 2, job.document.frame_rate).unwrap(),
            format: veylvik_io::FrameFormat::Png,
            output: job.document.output,
        };
        let progress = Mutex::new(Progress::default());
        assert!(render(&job, &renderer, &AtomicBool::new(false), &progress).unwrap());
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(job.path.join("sequence.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["completed"], 3);
        job.path = dir.join("cancelled");
        assert!(!render(&job, &renderer, &AtomicBool::new(true), &progress).unwrap());
        assert!(!job.path.exists());
        job.id = NodeId::new_v4();
        job.path = dir.join("partial");
        job.output = Output::Sequence {
            frames: FrameRange::new(0, 9999, job.document.frame_rate).unwrap(),
            format: veylvik_io::FrameFormat::Png,
            output: job.document.output,
        };
        queue.add(job).unwrap();
        queue.add(fixture(&dir, "waiting")).unwrap();
        queue.running = true;
        queue.tick(&renderer, &ctx);
        let deadline = Instant::now() + std::time::Duration::from_secs(30);
        while queue
            .active
            .as_ref()
            .unwrap()
            .progress
            .lock()
            .unwrap()
            .completed
            < 2
        {
            assert!(Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        queue.pause();
        queue
            .active
            .as_ref()
            .unwrap()
            .cancel
            .store(true, Ordering::Relaxed);
        while queue.busy() {
            queue.tick(&renderer, &ctx);
            assert!(Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(queue.jobs[1].state, State::Cancelled);
        assert_eq!(queue.jobs[2].state, State::Pending);
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("partial/sequence.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["status"], "cancelled");
        assert!(manifest["completed"].as_u64().unwrap() >= 2);
        drop(queue);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
