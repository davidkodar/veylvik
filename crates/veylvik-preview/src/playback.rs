//! Bounded, disposable playback cache. All file I/O and transfers run on the preview worker.
use crate::{Result, error};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant, SystemTime},
};
use veylvik_document::Time;
use veylvik_render::{GpuFrame, ImageInputs, Plan, Renderer};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub divisor: u32,
    pub gpu_bytes: u64,
    pub disk_bytes: u64,
    pub directory: Option<PathBuf>,
    /// Memory for played pictures kept as the screen shows them, packed; none keeps none.
    pub picture_memory_bytes: u64,
    /// Disk for the same, in `directory`, once memory is full.
    pub picture_disk_bytes: u64,
    /// Memory the machine must still have available: played pictures give way (to disk,
    /// or let go) before the rest of the machine goes short, whatever their own limit.
    pub memory_reserve_bytes: u64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            divisor: 1,
            gpu_bytes: 512 * 1024 * 1024,
            disk_bytes: 0,
            directory: None,
            picture_memory_bytes: 0,
            picture_disk_bytes: 0,
            memory_reserve_bytes: 4 << 30,
        }
    }
}
#[derive(Clone, Default, serde::Serialize)]
pub struct Status {
    pub revision: u64,
    pub output: veylvik_document::NodeId,
    pub size: [u32; 2],
    pub gpu_frames: Vec<Time>,
    pub disk_frames: Vec<Time>,
    pub gpu_bytes: u64,
    pub disk_bytes: u64,
    pub hits: u64,
    pub warning: Option<String>,
    /// Moments whose picture is kept as the screen shows it, for the look last played with.
    pub ready_frames: Vec<Time>,
    pub picture_memory_bytes: u64,
    pub picture_disk_bytes: u64,
    /// The store has no room for more prepared pictures.
    pub full: bool,
    /// Pictures let go to stay within the limits, since the start: a host that shows
    /// what is kept sees here that something went.
    pub pictures_dropped: u64,
    /// The composition's sources cannot be kept at all (a source that does not say which
    /// files to watch): nothing is prepared, and playing is live.
    pub cannot_keep: bool,
}
#[derive(Clone, Default)]
pub struct Control(
    Arc<Mutex<(Settings, u64, Status)>>,
    Arc<Mutex<VecDeque<Arc<GpuFrame>>>>,
    Arc<Mutex<Option<crate::screen::Look>>>,
    /// Asked to delete the lasting pictures on disk too, at the next clear.
    Arc<std::sync::atomic::AtomicBool>,
);
impl Control {
    /// While playing, with the viewer's look: frames may be answered with pictures kept
    /// as the screen shows them, and new frames are kept that way. `None` when not
    /// playing: every frame is then the renderer's own, for reading values off it.
    pub fn set_playing(&self, look: Option<crate::screen::Look>) {
        if let Ok(mut playing) = self.2.lock() {
            *playing = look;
        }
    }
    pub fn playing(&self) -> Option<crate::screen::Look> {
        self.2.lock().ok().and_then(|playing| *playing)
    }
    pub fn configure(&self, mut settings: Settings) {
        settings.divisor = match settings.divisor {
            2 => 2,
            4 => 4,
            _ => 1,
        };
        settings.gpu_bytes = settings.gpu_bytes.min(8192 * 1024 * 1024);
        settings.disk_bytes = settings.disk_bytes.min(65536 * 1024 * 1024);
        if let Ok(mut s) = self.0.lock()
            && s.0 != settings
        {
            s.0 = settings;
            s.1 = s.1.wrapping_add(1);
            s.2 = Status::default();
            if let Ok(mut frames) = self.1.lock() {
                frames.clear();
            }
        }
    }
    pub fn set_resolution(&self, divisor: u32) {
        let mut settings = self.0.lock().map(|v| v.0.clone()).unwrap_or_default();
        settings.divisor = match divisor {
            2 => 2,
            4 => 4,
            _ => 1,
        };
        self.configure(settings);
    }
    pub fn clear(&self) {
        // Asked for by the artist: the pictures kept between sessions go as well.
        self.3.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut s) = self.0.lock() {
            s.1 = s.1.wrapping_add(1);
            s.2 = Status::default();
            if let Ok(mut frames) = self.1.lock() {
                frames.clear();
            }
        }
    }
    pub fn status(&self) -> Status {
        self.0.lock().map(|s| s.2.clone()).unwrap_or_default()
    }
}
struct Entry {
    /// Every moment known to show this picture under the plan in use.
    times: Vec<Time>,
    /// What the picture is made of ([`Store::name`]); none when that cannot be told, and
    /// then it belongs to the one moment and the one plan it was made for.
    name: Option<u64>,
    gpu: Option<Weak<GpuFrame>>,
    disk: Option<(PathBuf, u64)>,
    width: u32,
    height: u32,
    flow: Option<veylvik_render::flow::FlowReport>,
}
type Stamp = (PathBuf, u64, SystemTime);
fn stamps(paths: impl IntoIterator<Item = PathBuf>) -> Result<Vec<Stamp>> {
    paths
        .into_iter()
        .map(|p| {
            let m = std::fs::metadata(&p).map_err(error)?;
            let modified = m.modified().map_err(error)?;
            Ok((p, m.len(), modified))
        })
        .collect()
}
fn ocio_paths(plan: &Plan, inputs: &ImageInputs, paths: &mut Vec<PathBuf>) -> Result<()> {
    for transform in plan.ocio_transforms() {
        let config_path = inputs.resolve_config(&transform.config)?;
        if config_path.starts_with("ocio://") {
            continue;
        }
        paths.push(PathBuf::from(&config_path));
        let config = veylvik_color::Config::load(&config_path).map_err(error)?;
        let processor = config
            .processor(
                &transform.source,
                &transform.destination,
                &transform.display,
                &transform.view,
            )
            .map_err(error)?;
        let base = std::path::Path::new(&config_path)
            .parent()
            .unwrap_or(std::path::Path::new("."));
        paths.extend(processor.files().iter().map(|p| base.join(p)));
    }
    Ok(())
}
pub(crate) struct Store {
    control: Control,
    settings: Settings,
    generation: u64,
    scope: Option<(Arc<Plan>, ImageInputs)>,
    dependencies: Vec<Stamp>,
    checked: Instant,
    entries: VecDeque<Entry>,
    folder: Option<PathBuf>,
    serial: u64,
    hits: u64,
    warning: Option<String>,
    eligible: bool,
    pictures: crate::pictures::Pictures,
    screen: Option<crate::screen::Screen>,
    look: Option<crate::screen::Look>,
    /// Packing runs beside the worker: a frame is not held up while its picture is packed.
    packer: Option<Packer>,
    /// Pictures handed to the packer and not back yet.
    packing: Vec<Moment>,
    available: (Instant, Option<u64>),
    /// The folder of pictures that last between sessions of this build, once looked in.
    lasting: Option<PathBuf>,
    /// A picture made to be kept found no room: preparing more would only push out
    /// what is kept, so it rests until the store is cleared or its limits change.
    full: bool,
}
/// What tells one build from another: a picture's lasting name means the same picture
/// only as long as the program that made it is the same.
fn build_stamp() -> u64 {
    let stamp = std::env::current_exe().ok().and_then(|path| {
        let meta = std::fs::metadata(&path).ok()?;
        let modified = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        Some(veylvik_render::fnv(
            path.to_string_lossy().as_bytes(),
            meta.len() ^ (modified.as_nanos() as u64).rotate_left(21),
        ))
    });
    stamp.unwrap_or(0)
}
/// `MemAvailable` of this machine in bytes.
fn machine_memory_available() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = text
        .lines()
        .find_map(|line| line.strip_prefix("MemAvailable:"))?;
    let kib: u64 = line.trim().trim_end_matches("kB").trim().parse().ok()?;
    Some(kib * 1024)
}
type Moment = (Time, Option<u64>, crate::screen::Look);
/// The file to write the packed picture to, and its lasting name when it lasts
/// between sessions; none when it is kept in memory only.
type Lasting = Option<(Option<u64>, PathBuf)>;
/// The last field: whether older pictures may give way for this one.
type Packed = (Moment, [u32; 2], Result<Vec<u8>>, Lasting, bool);
/// The same before packing: the picture as the screen packs it.
type Unpacked = (Moment, [u32; 2], Vec<u8>, Lasting, bool);
/// Write a packed picture to its lasting file, whole or not at all.
fn written(packed: &[u8], (lasting, path): (Option<u64>, PathBuf)) -> Lasting {
    let partial = path.with_extension("partial");
    let done = std::fs::write(&partial, packed).and_then(|()| std::fs::rename(&partial, &path));
    if done.is_err() {
        let _ = std::fs::remove_file(&partial);
        return None;
    }
    Some((lasting, path))
}
fn alike(a: &Moment, b: &Moment) -> bool {
    a.1 == b.1
        && a.2 == b.2
        && i128::from(a.0.ticks) * i128::from(b.0.rate)
            == i128::from(b.0.ticks) * i128::from(a.0.rate)
}
struct Packer {
    work: std::sync::mpsc::SyncSender<Unpacked>,
    done: std::sync::mpsc::Receiver<Packed>,
}
impl Packer {
    fn start() -> Option<Self> {
        // A few pictures may wait; beyond that the worker packs its own, so memory
        // held in pictures not packed yet stays small.
        let (work, jobs) = std::sync::mpsc::sync_channel::<Unpacked>(3);
        let (finished, done) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("veylvik-pictures".into())
            .spawn(move || {
                for (moment, size, picture, lasting, evict) in jobs {
                    let packed = crate::pictures::pack(&picture);
                    // A lasting picture goes to its file here, beside the worker.
                    let lasting = match (&packed, lasting) {
                        (Ok(packed), Some(file)) => written(packed, file),
                        _ => None,
                    };
                    if finished
                        .send((moment, size, packed, lasting, evict))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .ok()?;
        Some(Self { work, done })
    }
}
impl Store {
    pub fn new(control: Control) -> Self {
        Self {
            control,
            settings: Settings::default(),
            generation: u64::MAX,
            scope: None,
            dependencies: vec![],
            checked: Instant::now(),
            entries: VecDeque::new(),
            folder: None,
            serial: 0,
            hits: 0,
            warning: None,
            eligible: true,
            pictures: Default::default(),
            screen: None,
            look: None,
            packer: None,
            packing: Vec::new(),
            available: (Instant::now(), machine_memory_available()),
            lasting: None,
            full: false,
        }
    }
    fn clear_entries(&mut self) {
        // Pictures still being packed belong to what is being cleared.
        self.packer = None;
        self.packing.clear();
        self.full = false;
        if self
            .control
            .3
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            self.pictures.wipe();
        } else {
            self.pictures.clear();
        }
        self.entries.clear();
        if let Ok(mut frames) = self.control.1.lock() {
            frames.clear();
        }
        if let Some(path) = self.folder.take() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
    pub fn divisor(&self) -> u32 {
        self.settings.divisor
    }
    pub fn release_gpu(&mut self) {
        if let Ok(mut frames) = self.control.1.lock() {
            frames.clear();
        }
        for e in &mut self.entries {
            e.gpu = None;
        }
    }
    pub fn prepare(&mut self, plan: &Arc<Plan>, inputs: &ImageInputs) -> Result<()> {
        let (settings, generation) = self
            .control
            .0
            .lock()
            .map(|s| (s.0.clone(), s.1))
            .map_err(|_| error("Playback cache unavailable"))?;
        let changed = self.generation != generation
            || self
                .scope
                .as_ref()
                .is_none_or(|(p, i)| !Arc::ptr_eq(p, plan) || !i.same_sources(inputs));
        let stale = self.checked.elapsed() >= Duration::from_secs(2)
            && stamps(self.dependencies.iter().map(|s| s.0.clone()))
                .is_ok_and(|s| s != self.dependencies);
        let missing = self.checked.elapsed() >= Duration::from_secs(2)
            && self.dependencies.iter().any(|s| !s.0.is_file());
        // Another plan over the same sources and settings: a picture known by what it is
        // made of is the same picture under any plan that makes it, so it stays; only
        // which moments show it has to be found again.
        let edited = !(stale || missing)
            && self.generation == generation
            && self
                .scope
                .as_ref()
                .is_some_and(|(p, i)| !Arc::ptr_eq(p, plan) && i.same_sources(inputs));
        if changed || stale || missing {
            if edited {
                self.collect(true);
                self.pictures.edited();
                self.entries.retain(|e| e.name.is_some());
                for e in &mut self.entries {
                    e.times.clear();
                }
                let kept: Vec<_> = self.entries.iter().filter_map(|e| e.gpu.clone()).collect();
                if let Ok(mut frames) = self.control.1.lock() {
                    frames.retain(|f| kept.iter().any(|w| Weak::ptr_eq(&Arc::downgrade(f), w)));
                }
            } else {
                self.clear_entries();
            }
            self.settings = settings;
            self.generation = generation;
            self.warning = None;
            self.hits = 0;
            self.scope = None;
            let deps = inputs.playback_dependencies();
            self.eligible = deps.is_some();
            let mut paths = deps.unwrap_or_default();
            ocio_paths(plan, inputs, &mut paths)?;
            paths.sort();
            paths.dedup();
            self.dependencies = stamps(paths)?;
            self.scope = Some((plan.clone(), inputs.clone()));
            if !self.eligible {
                self.warning = Some("Source does not support playback caching".into());
            }
        }
        if changed || self.checked.elapsed() >= Duration::from_secs(2) {
            self.checked = Instant::now();
        }
        Ok(())
    }
    fn gpu_bytes(&self) -> u64 {
        self.entries
            .iter()
            .filter_map(|e| e.gpu.as_ref().and_then(Weak::upgrade))
            .map(|f| f.byte_size())
            .sum()
    }
    fn disk_bytes(&self) -> u64 {
        self.entries
            .iter()
            .filter_map(|e| e.disk.as_ref())
            .map(|d| d.1)
            .sum()
    }
    fn evict_gpu(&mut self) -> bool {
        if let Some(e) = self.entries.iter_mut().find(|e| e.gpu.is_some()) {
            if let Some(weak) = e.gpu.take()
                && let Ok(mut frames) = self.control.1.lock()
            {
                frames.retain(|f| !Weak::ptr_eq(&Arc::downgrade(f), &weak));
            }

            true
        } else {
            false
        }
    }
    pub fn make_room(&mut self, renderer: &Renderer, needed: u64) -> Result<()> {
        while renderer.resource_stats()?.held_bytes.saturating_add(needed) > renderer.budget_bytes
            && self.evict_gpu()
        {}
        self.entries
            .retain(|e| e.gpu.as_ref().is_some_and(|w| w.strong_count() > 0) || e.disk.is_some());
        Ok(())
    }
    /// What the picture for `request` is made of, when that can be told: the renderer's
    /// identity for it, with what is done to it afterwards for the viewer. Plans made of
    /// other moments, and sources that make a new frame every time, cannot be told yet.
    pub fn name(
        &self,
        renderer: &Renderer,
        plan: &Plan,
        request: veylvik_render::RenderRequest,
        inputs: &ImageInputs,
    ) -> (Option<u64>, Option<u64>) {
        use std::hash::{Hash, Hasher};
        if !inputs.steady(plan) {
            return (None, None);
        }
        let Ok((identity, lasting)) = renderer.identities(plan, request, inputs) else {
            return (None, None);
        };
        let view = format!("{:?}", plan.ocio_transforms());
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        identity.hash(&mut hasher);
        view.hash(&mut hasher);
        // Between sessions a picture also depends on files the renderer reads by path
        // alone: 3D models, and the files of the viewer's colour transform.
        let lasting = lasting.and_then(|lasting| {
            let mut text = view.clone();
            for stamp in &self.dependencies {
                text.push_str(&format!("{stamp:?}"));
            }
            for model in plan.required_models() {
                let meta = std::fs::metadata(model).ok()?;
                text.push_str(&format!("{model}{}{:?}", meta.len(), meta.modified().ok()?));
            }
            Some(veylvik_render::fnv(text.as_bytes(), lasting))
        });
        (Some(hasher.finish()), lasting)
    }
    pub fn lookup(
        &mut self,
        renderer: &Renderer,
        time: Time,
        name: Option<u64>,
    ) -> Result<Option<Arc<GpuFrame>>> {
        if !self.eligible {
            return Ok(None);
        }
        let same = |t: &Time| {
            i128::from(t.ticks) * i128::from(time.rate)
                == i128::from(time.ticks) * i128::from(t.rate)
        };
        let Some(index) = self.entries.iter().position(|e| match name {
            Some(_) => e.name == name,
            None => e.name.is_none() && e.times.iter().any(same),
        }) else {
            return Ok(None);
        };
        let mut entry = self.entries.remove(index).unwrap();
        let result = if let Some(frame) = entry.gpu.as_ref().and_then(Weak::upgrade) {
            Ok(frame)
        } else {
            self.read_disk(renderer, &entry)
        };
        match result {
            Ok(frame) => {
                self.hits += 1;
                self.retain_gpu(&frame);
                entry.gpu = Some(Arc::downgrade(&frame));
                if !entry.times.iter().any(same) && entry.times.len() < 4096 {
                    entry.times.push(time);
                }
                self.entries.push_back(entry);
                self.trim_gpu(renderer);
                Ok(Some(frame))
            }
            Err(e) => {
                if let Some((p, _)) = entry.disk {
                    let _ = std::fs::remove_file(p);
                }
                self.warning = Some(format!("Cache miss after disk error: {e}"));
                Ok(None)
            }
        }
    }
    fn read_disk(&mut self, renderer: &Renderer, entry: &Entry) -> Result<Arc<GpuFrame>> {
        let (path, _) = entry
            .disk
            .as_ref()
            .ok_or_else(|| error("Missing cache file"))?;
        let expected = u64::from(entry.width) * u64::from(entry.height) * 16;
        self.make_room(renderer, expected)?;
        let file = std::fs::File::open(path).map_err(error)?;
        let mut bytes = Vec::new();
        flate2::read::ZlibDecoder::new(file)
            .take(expected + 1)
            .read_to_end(&mut bytes)
            .map_err(error)?;
        if bytes.len() as u64 != expected {
            return Err(error("Invalid playback cache size"));
        }
        renderer.compute(|scratch| {
            let buffer = scratch.buffer(
                expected,
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                "Playback cache upload",
            )?;
            scratch.queue().write_buffer(&buffer, 0, &bytes);
            let mut frame = scratch.frame(&buffer, entry.width, entry.height)?;
            frame.set_flow_report(entry.flow);
            Ok(Arc::new(frame))
        })
    }
    fn retain_gpu(&self, frame: &Arc<GpuFrame>) {
        if let Ok(state) = self.control.0.lock()
            && state.1 == self.generation
            && let Ok(mut frames) = self.control.1.lock()
        {
            frames.retain(|f| !Arc::ptr_eq(f, frame));
            frames.push_back(frame.clone());
        }
    }
    fn trim_gpu(&mut self, renderer: &Renderer) {
        let limit = self.settings.gpu_bytes.min(renderer.budget_bytes / 2);
        while self.gpu_bytes() > limit && self.evict_gpu() {}
        self.entries
            .retain(|e| e.gpu.as_ref().is_some_and(|w| w.strong_count() > 0) || e.disk.is_some());
    }
    pub fn insert(
        &mut self,
        renderer: &Renderer,
        time: Time,
        name: Option<u64>,
        frame: Arc<GpuFrame>,
        cancelled: &mut dyn FnMut() -> bool,
    ) {
        if !self.eligible || cancelled() {
            return;
        }
        let mut entry = Entry {
            times: vec![time],
            name,
            width: frame.width(),
            height: frame.height(),
            flow: frame.flow_report(),
            gpu: Some(Arc::downgrade(&frame)),
            disk: None,
        };
        if frame.byte_size().saturating_add(65536) <= self.settings.disk_bytes
            && self.settings.directory.is_some()
            && !cancelled()
        {
            while self
                .disk_bytes()
                .saturating_add(frame.byte_size())
                .saturating_add(65536)
                > self.settings.disk_bytes
            {
                if let Some(e) = self.entries.iter_mut().find(|e| e.disk.is_some()) {
                    if let Some((p, _)) = e.disk.take() {
                        let _ = std::fs::remove_file(p);
                    }
                } else {
                    break;
                }
            }
            match self.write_disk(renderer, &frame) {
                Ok(d) => entry.disk = Some(d),
                Err(e) => self.warning = Some(format!("Disk cache unavailable: {e}")),
            }
        }
        self.retain_gpu(&frame);
        self.entries.push_back(entry);
        self.trim_gpu(renderer);
        while self.disk_bytes() > self.settings.disk_bytes {
            if let Some(e) = self.entries.iter_mut().find(|e| e.disk.is_some()) {
                if let Some((path, _)) = e.disk.take() {
                    let _ = std::fs::remove_file(path);
                }
            } else {
                break;
            }
        }
        while self.entries.len() > 4096 {
            if let Some(e) = self.entries.pop_front() {
                if let Some(weak) = e.gpu
                    && let Ok(mut frames) = self.control.1.lock()
                {
                    frames.retain(|f| !Weak::ptr_eq(&Arc::downgrade(f), &weak));
                }
                if let Some((p, _)) = e.disk {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
        self.entries
            .retain(|e| e.gpu.as_ref().is_some_and(|w| w.strong_count() > 0) || e.disk.is_some());
    }
    /// Whether a picture made to be kept found no room (see `full`).
    pub fn full(&self) -> bool {
        self.full
    }
    /// Whether this moment's picture is kept for `look` (the moment is noted if so).
    pub fn has_picture(
        &mut self,
        time: Time,
        (name, lasting): (Option<u64>, Option<u64>),
        look: crate::screen::Look,
    ) -> bool {
        self.look = Some(look);
        self.lasting_folder();
        self.pictures.take_up(time, name, lasting, look);
        // One still being packed counts: it is on its way.
        let moment = (time, name, look);
        let packing = self.packing.iter().any(|m| match name {
            Some(_) => m.1 == name && m.2 == look,
            None => alike(m, &moment),
        });
        self.eligible && (packing || self.pictures.has(time, name, look))
    }
    /// The machine's available memory, read at most twice a second; none when unknown.
    fn available_memory(&mut self) -> Option<u64> {
        if self.available.0.elapsed() >= Duration::from_millis(500) {
            self.available = (Instant::now(), machine_memory_available());
        }
        self.available.1
    }
    fn picture_limits(&mut self) -> crate::pictures::Limits {
        // Never more than leaves the reserve free: what is held already, plus what the
        // machine has beyond its reserve.
        let fair = self.available_memory().map_or(u64::MAX, |available| {
            self.pictures
                .memory_bytes()
                .saturating_add(available.saturating_sub(self.settings.memory_reserve_bytes))
        });
        crate::pictures::Limits {
            memory: self.settings.picture_memory_bytes.min(fair),
            disk: if self.settings.directory.is_some() {
                self.settings.picture_disk_bytes
            } else {
                0
            },
        }
    }
    /// This moment's picture as the screen shows it with `look`, as a frame to draw with
    /// a neutral look.
    pub fn picture(
        &mut self,
        renderer: &Renderer,
        time: Time,
        (name, lasting): (Option<u64>, Option<u64>),
        look: crate::screen::Look,
    ) -> Result<Option<Arc<GpuFrame>>> {
        self.look = Some(look);
        if !self.eligible {
            return Ok(None);
        }
        self.collect(false);
        self.lasting_folder();
        self.pictures.take_up(time, name, lasting, look);
        let Some((picture, [width, height])) = self.pictures.get(time, name, look) else {
            return Ok(None);
        };
        self.make_room(renderer, u64::from(width) * u64::from(height) * 20)?;
        let screen = self
            .screen
            .get_or_insert_with(|| crate::screen::Screen::new(renderer.device()));
        let frame = screen.unpack(renderer, &picture, width, height)?;
        self.hits += 1;
        Ok(Some(Arc::new(frame)))
    }
    /// Take every picture still being packed into the store (waiting for the packer): at
    /// the end of a list to prepare, when nobody waits for the worker.
    pub fn settle(&mut self) {
        self.collect(true);
    }
    /// Keep `frame` as the screen shows it with `look`, unless it is kept already.
    pub fn keep_picture(
        &mut self,
        renderer: &Renderer,
        time: Time,
        (name, lasting): (Option<u64>, Option<u64>),
        look: crate::screen::Look,
        frame: &GpuFrame,
        evict: bool,
    ) {
        self.look = Some(look);
        let limits = self.picture_limits();
        self.collect(false);
        self.pictures.take_up(time, name, lasting, look);
        let moment = (time, name, look);
        let waiting = self.packing.iter().any(|m| match name {
            Some(_) => m.1 == name && m.2 == look,
            None => alike(m, &moment),
        });
        if !self.eligible
            || limits.memory + limits.disk == 0
            || waiting
            || self.pictures.has(time, name, look)
        {
            return;
        }
        // Packing and writing happen beside the worker, never on it: the worker waiting
        // on a busy disk froze playback for seconds. When the packer is behind, this
        // picture is not kept; it is kept another time it is played or prepared.
        let kept: Result<()> = (|| {
            if self.packer.is_none() {
                self.packer = Packer::start();
            }
            // A frame made only to be kept may wait for the packer: nobody is watching it.
            // One that is being played never waits.
            if !evict && self.packing.len() >= 3 {
                self.collect(true);
            }
            if self.packer.is_none() || self.packing.len() >= 3 {
                return Ok(());
            }
            let size = [frame.width(), frame.height()];
            // Its file: in the lasting folder when it means the same in any session, else
            // in this session's folder, when there is a disk tier at all.
            let file = match lasting.and_then(|lasting| Some((lasting, self.lasting_folder()?))) {
                Some((lasting, folder)) => Some((
                    Some(lasting),
                    crate::pictures::lasting_file(&folder, lasting, look, size),
                )),
                None if limits.disk > 0 => {
                    let folder = self.session_folder()?;
                    self.serial = self.serial.wrapping_add(1);
                    Some((None, folder.join(format!("{}.picture.zst", self.serial))))
                }
                None => None,
            };
            self.make_room(renderer, u64::from(size[0]) * u64::from(size[1]) * 8)?;
            let screen = self
                .screen
                .get_or_insert_with(|| crate::screen::Screen::new(renderer.device()));
            let picture = screen.pack(renderer, frame, look)?;
            if self.packer.as_ref().is_some_and(|packer| {
                packer
                    .work
                    .try_send((moment, size, picture, file, evict))
                    .is_ok()
            }) {
                self.packing.push(moment);
            }
            Ok(())
        })();
        if let Err(e) = kept {
            self.warning = Some(format!("Played pictures are not kept: {e}"));
        }
    }
    /// Take in the pictures the packer has finished; `all` waits for those still with it.
    fn collect(&mut self, all: bool) {
        let limits = self.picture_limits();
        loop {
            let Some(packer) = &self.packer else {
                self.packing.clear();
                return;
            };
            let next = if all && !self.packing.is_empty() {
                packer.done.recv_timeout(Duration::from_secs(5)).ok()
            } else {
                packer.done.try_recv().ok()
            };
            let Some((moment, size, packed, lasting, evict)) = next else {
                if all {
                    self.packing.clear();
                }
                return;
            };
            self.packing.retain(|m| !alike(m, &moment));
            let kept = packed.and_then(|packed| {
                self.pictures
                    .put_within(moment, size, packed, lasting, limits, None, evict)
            });
            match kept {
                Ok(true) => {}
                Ok(false) => self.full = true,
                Err(e) => self.warning = Some(format!("Played pictures are not kept: {e}")),
            }
        }
    }
    /// The folder of pictures that last between sessions of this build, looked in the
    /// first time; folders of other builds are removed, their names mean nothing here.
    fn lasting_folder(&mut self) -> Option<PathBuf> {
        if self.settings.picture_disk_bytes == 0 {
            return None;
        }
        if self.lasting.is_none() {
            let root = self.settings.directory.clone()?;
            let folder = root.join(format!("kept-{:016x}", build_stamp()));
            std::fs::create_dir_all(&folder).ok()?;
            if let Ok(entries) = std::fs::read_dir(&root) {
                for entry in entries.take(256).flatten() {
                    if entry.file_name().to_string_lossy().starts_with("kept-")
                        && entry.path() != folder
                        && entry.file_type().is_ok_and(|t| t.is_dir())
                    {
                        let _ = std::fs::remove_dir_all(entry.path());
                    }
                }
            }
            self.pictures.look_in(&folder);
            self.lasting = Some(folder);
        }
        self.lasting.clone()
    }
    /// This session's folder in the cache directory, made when first needed.
    fn session_folder(&mut self) -> Result<PathBuf> {
        if self.folder.is_none() {
            let root = self
                .settings
                .directory
                .clone()
                .ok_or_else(|| error("No cache folder"))?;
            let root = &root;
            std::fs::create_dir_all(root).map_err(error)?;
            // Remove only our recognized session directories whose process no longer exists.
            if let Ok(entries) = std::fs::read_dir(root) {
                for entry in entries.take(256).flatten() {
                    if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                        continue;
                    }
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if let Some(rest) = name.strip_prefix("session-")
                        && let Some((pid, id)) = rest.split_once('-')
                        && pid.parse::<u32>().is_ok()
                        && veylvik_document::NodeId::parse_str(id).is_ok()
                        && !std::path::Path::new("/proc").join(pid).exists()
                    {
                        let _ = std::fs::remove_dir_all(entry.path());
                    }
                }
            }
            let path = root.join(format!(
                "session-{}-{}",
                std::process::id(),
                veylvik_document::NodeId::new_v4()
            ));
            std::fs::create_dir(&path).map_err(error)?;
            self.folder = Some(path);
        }
        self.folder.clone().ok_or_else(|| error("No cache folder"))
    }
    fn write_disk(&mut self, renderer: &Renderer, frame: &GpuFrame) -> Result<(PathBuf, u64)> {
        self.session_folder()?;
        self.make_room(renderer, frame.byte_size().saturating_mul(2))?;
        let pixels = renderer.read_frame(frame)?;
        self.serial = self.serial.wrapping_add(1);
        let path = self
            .folder
            .as_ref()
            .unwrap()
            .join(format!("{}.rgba.z", self.serial));
        let result = (|| {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(error)?;
            let mut encoder = flate2::write::ZlibEncoder::new(file, flate2::Compression::fast());
            encoder
                .write_all(bytemuck::cast_slice(&pixels.pixels))
                .map_err(error)?;
            let file = encoder.finish().map_err(error)?;
            let size = file.metadata().map_err(error)?.len();
            Ok((path.clone(), size))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(path);
        }
        result
    }
    pub fn publish(&mut self, revision: u64) {
        self.collect(false);
        // The machine may have got tighter since the last picture was kept.
        let limits = self.picture_limits();
        if self.pictures.memory_bytes() > limits.memory {
            let _ = self.pictures.fit(limits, None);
        }
        if let Ok(mut c) = self.control.0.lock()
            && c.1 == self.generation
        {
            c.2 = Status {
                revision,
                output: self
                    .scope
                    .as_ref()
                    .map(|s| s.0.output_node())
                    .unwrap_or_default(),
                size: self
                    .scope
                    .as_ref()
                    .map(|s| {
                        [
                            s.0.size()[0].div_ceil(self.divisor()),
                            s.0.size()[1].div_ceil(self.divisor()),
                        ]
                    })
                    .unwrap_or_default(),
                gpu_frames: self
                    .entries
                    .iter()
                    .filter(|e| e.gpu.as_ref().is_some_and(|w| w.strong_count() > 0))
                    .flat_map(|e| e.times.iter().copied())
                    .collect(),
                disk_frames: self
                    .entries
                    .iter()
                    .filter(|e| e.disk.is_some())
                    .flat_map(|e| e.times.iter().copied())
                    .collect(),
                gpu_bytes: self.gpu_bytes(),
                disk_bytes: self.disk_bytes(),
                hits: self.hits,
                warning: self.warning.clone(),
                ready_frames: self
                    .look
                    .map(|look| self.pictures.times(look))
                    .unwrap_or_default(),
                picture_memory_bytes: self.pictures.memory_bytes(),
                picture_disk_bytes: self.pictures.disk_bytes(),
                full: self.full,
                pictures_dropped: self.pictures.dropped,
                cannot_keep: !self.eligible,
            };
        }
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        self.clear_entries();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan() -> Arc<Plan> {
        Arc::new(
            Plan::compile(&veylvik_document::blank(32, 16, Time::new(24, 1).unwrap()).unwrap())
                .unwrap(),
        )
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn disk_roundtrip_corruption_clear_and_gpu_limits() {
        let renderer = Renderer::new().unwrap();
        let inputs = ImageInputs::default();
        let plan = plan();
        let root = std::env::temp_dir().join(format!(
            "veylvik-cache-test-{}",
            veylvik_document::NodeId::new_v4()
        ));
        let control = Control::default();
        control.configure(Settings {
            divisor: 1,
            gpu_bytes: 0,
            disk_bytes: 1024 * 1024,
            directory: Some(root.clone()),
            ..Default::default()
        });
        let mut store = Store::new(control.clone());
        store.prepare(&plan, &inputs).unwrap();
        let time = Time::new(0, 1).unwrap();
        let frame = Arc::new(
            renderer
                .render_gpu(&plan, veylvik_render::RenderRequest::full(time), &inputs)
                .unwrap(),
        );
        let expected = renderer.read_frame(&frame).unwrap().pixels;
        store.insert(&renderer, time, None, frame.clone(), &mut || false);
        drop(frame);
        store.publish(7);
        assert_eq!(control.status().gpu_bytes, 0);
        assert_eq!(control.status().disk_frames.len(), 1);
        let restored = store.lookup(&renderer, time, None).unwrap().unwrap();
        assert_eq!(renderer.read_frame(&restored).unwrap().pixels, expected);
        drop(restored);
        let path = store.entries[0].disk.as_ref().unwrap().0.clone();
        std::fs::write(path, b"broken").unwrap();
        assert!(store.lookup(&renderer, time, None).unwrap().is_none());
        assert!(store.warning.is_some());
        control.configure(Settings {
            divisor: 1,
            gpu_bytes: 32 * 16 * 16 * 2,
            disk_bytes: 0,
            directory: None,
            ..Default::default()
        });
        store.prepare(&plan, &inputs).unwrap();
        for n in 0..6 {
            let t = Time::new(n, 24).unwrap();
            let frame = Arc::new(
                renderer
                    .render_gpu(&plan, veylvik_render::RenderRequest::full(t), &inputs)
                    .unwrap(),
            );
            store.insert(&renderer, t, None, frame, &mut || false);
        }
        store.publish(8);
        assert_eq!(control.status().gpu_frames.len(), 2);
        assert_eq!(control.status().gpu_bytes, 32 * 16 * 16 * 2);
        let held = renderer.resource_stats().unwrap().held_bytes;
        control.clear();
        assert!(renderer.resource_stats().unwrap().held_bytes < held);
        store.prepare(&plan, &inputs).unwrap();
        assert!(store.entries.is_empty());
        drop(store);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    #[ignore = "requires Vulkan"]
    fn a_picture_is_kept_through_an_edit_that_does_not_change_it() {
        use veylvik_document::{Node, Operation, registry};
        let renderer = Renderer::new().unwrap();
        let inputs = ImageInputs::default();
        // A still picture: a solid colour, graded.
        let composition = |gain: f32, extra: bool| {
            let mut doc = veylvik_document::blank(32, 16, Time::new(24, 1).unwrap()).unwrap();
            doc.frame_count = 48;
            let of = |kind: &str| {
                let mut value = registry::TYPES
                    .iter()
                    .find(|t| t.type_id == kind)
                    .unwrap()
                    .default_operation();
                if kind == "exposure" {
                    let name = registry::TYPES
                        .iter()
                        .find(|t| t.type_id == kind)
                        .unwrap()
                        .parameters[0]
                        .name;
                    value[name] = serde_json::json!({"kind": "constant", "value": gain});
                }
                serde_json::from_value::<Operation>(value).unwrap()
            };
            let solid = Node::new("solid", of("solid"), &[]);
            let grade = Node::new("grade", of("exposure"), &[("image", solid.id)]);
            doc.output = grade.id;
            if extra {
                // A node nothing reads: an edit that changes no picture.
                let aside = Node::new("aside", of("noise"), &[]);
                doc.nodes.insert(aside.id, aside);
            }
            doc.nodes.insert(solid.id, solid);
            doc.nodes.insert(grade.id, grade);
            Arc::new(Plan::compile(&doc).unwrap())
        };
        let control = Control::default();
        control.configure(Settings {
            divisor: 1,
            gpu_bytes: 1 << 20,
            disk_bytes: 0,
            directory: None,
            ..Default::default()
        });
        let mut store = Store::new(control.clone());
        let at = |frame: i64| veylvik_render::RenderRequest::full(Time::new(frame, 24).unwrap());
        let first = composition(1.0, false);
        store.prepare(&first, &inputs).unwrap();
        let name = store.name(&renderer, &first, at(0), &inputs).0;
        assert!(name.is_some());
        let frame = Arc::new(renderer.render_gpu(&first, at(0), &inputs).unwrap());
        store.insert(&renderer, at(0).time, name, frame.clone(), &mut || false);
        // A still picture is one picture at every moment.
        let later = store.name(&renderer, &first, at(30), &inputs).0;
        assert_eq!(later, name);
        let found = store
            .lookup(&renderer, at(30).time, later)
            .unwrap()
            .unwrap();
        assert!(Arc::ptr_eq(&found, &frame));
        store.publish(1);
        assert_eq!(control.status().gpu_frames.len(), 2);
        // An edit elsewhere makes another plan; the picture is still the picture.
        let edited = composition(1.0, true);
        store.prepare(&edited, &inputs).unwrap();
        let name = store.name(&renderer, &edited, at(0), &inputs).0;
        let found = store.lookup(&renderer, at(0).time, name).unwrap();
        assert!(
            found.is_some_and(|f| Arc::ptr_eq(&f, &frame)),
            "an edit that changed nothing threw the picture away"
        );
        // Only the moment just asked for is known to show it under the new plan.
        store.publish(2);
        assert_eq!(control.status().gpu_frames.len(), 1);
        // An edit that changes the picture is not answered with the old one.
        let regraded = composition(2.0, true);
        store.prepare(&regraded, &inputs).unwrap();
        let name = store.name(&renderer, &regraded, at(0), &inputs).0;
        assert!(store.lookup(&renderer, at(0).time, name).unwrap().is_none());
    }
    #[test]
    fn changed_sources_and_dependencies_invalidate_scope() {
        struct Source {
            image: Arc<veylvik_render::Image>,
            path: PathBuf,
        }
        impl veylvik_render::FrameSource for Source {
            fn frame(&self, _: Time) -> Result<Arc<veylvik_render::Image>> {
                Ok(self.image.clone())
            }
            fn byte_budget(&self) -> u64 {
                16
            }
            fn cache_dependencies(&self) -> Option<Vec<PathBuf>> {
                Some(vec![self.path.clone()])
            }
        }
        let path = std::env::temp_dir().join(format!(
            "veylvik-stamp-{}",
            veylvik_document::NodeId::new_v4()
        ));
        std::fs::write(&path, b"a").unwrap();
        let mut inputs = ImageInputs::default();
        inputs.insert_source(
            "test",
            Arc::new(Source {
                image: Arc::new(veylvik_render::Image::new(1, 1, vec![[0.; 4]]).unwrap()),
                path: path.clone(),
            }),
        );
        let mut store = Store::new(Control::default());
        let plan = plan();
        store.prepare(&plan, &inputs).unwrap();
        store.hits = 42;
        store.prepare(&plan, &inputs.clone()).unwrap();
        assert_eq!(store.hits, 42);
        std::fs::write(&path, b"changed").unwrap();
        store.checked = Instant::now() - Duration::from_secs(3);
        store.prepare(&plan, &inputs).unwrap();
        assert_eq!(store.hits, 0);
        assert_eq!(store.dependencies[0].1, 7);
        store.hits = 17;
        store.prepare(&self::plan(), &inputs).unwrap();
        assert_eq!(store.hits, 0);
        std::fs::remove_file(path).unwrap();
        store.checked = Instant::now() - Duration::from_secs(3);
        assert!(store.prepare(&plan, &inputs).is_err());
    }
}
