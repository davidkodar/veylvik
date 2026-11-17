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
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            divisor: 1,
            gpu_bytes: 512 * 1024 * 1024,
            disk_bytes: 0,
            directory: None,
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
}
#[derive(Clone, Default)]
pub struct Control(
    Arc<Mutex<(Settings, u64, Status)>>,
    Arc<Mutex<VecDeque<Arc<GpuFrame>>>>,
);
impl Control {
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
    time: Time,
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
        let config_path = inputs.resolve_config(&transform.config);
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
        }
    }
    fn clear_entries(&mut self) {
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
        if changed || stale || missing {
            self.clear_entries();
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
    pub fn lookup(&mut self, renderer: &Renderer, time: Time) -> Result<Option<Arc<GpuFrame>>> {
        if !self.eligible {
            return Ok(None);
        }
        let Some(index) = self.entries.iter().position(|e| {
            i128::from(e.time.ticks) * i128::from(time.rate)
                == i128::from(time.ticks) * i128::from(e.time.rate)
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
        frame: Arc<GpuFrame>,
        cancelled: &mut dyn FnMut() -> bool,
    ) {
        if !self.eligible || cancelled() {
            return;
        }
        let mut entry = Entry {
            time,
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
    fn write_disk(&mut self, renderer: &Renderer, frame: &GpuFrame) -> Result<(PathBuf, u64)> {
        if self.folder.is_none() {
            let root = self.settings.directory.as_ref().unwrap();
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
    pub fn publish(&self, revision: u64) {
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
                    .map(|e| e.time)
                    .collect(),
                disk_frames: self
                    .entries
                    .iter()
                    .filter(|e| e.disk.is_some())
                    .map(|e| e.time)
                    .collect(),
                gpu_bytes: self.gpu_bytes(),
                disk_bytes: self.disk_bytes(),
                hits: self.hits,
                warning: self.warning.clone(),
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
        store.insert(&renderer, time, frame.clone(), &mut || false);
        drop(frame);
        store.publish(7);
        assert_eq!(control.status().gpu_bytes, 0);
        assert_eq!(control.status().disk_frames.len(), 1);
        let restored = store.lookup(&renderer, time).unwrap().unwrap();
        assert_eq!(renderer.read_frame(&restored).unwrap().pixels, expected);
        drop(restored);
        let path = store.entries[0].disk.as_ref().unwrap().0.clone();
        std::fs::write(path, b"broken").unwrap();
        assert!(store.lookup(&renderer, time).unwrap().is_none());
        assert!(store.warning.is_some());
        control.configure(Settings {
            divisor: 1,
            gpu_bytes: 32 * 16 * 16 * 2,
            disk_bytes: 0,
            directory: None,
        });
        store.prepare(&plan, &inputs).unwrap();
        for n in 0..6 {
            let t = Time::new(n, 24).unwrap();
            let frame = Arc::new(
                renderer
                    .render_gpu(&plan, veylvik_render::RenderRequest::full(t), &inputs)
                    .unwrap(),
            );
            store.insert(&renderer, t, frame, &mut || false);
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
