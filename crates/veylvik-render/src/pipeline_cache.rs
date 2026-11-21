//! Compiled GPU programs kept in a file between runs, so a driver compiles each program once per driver and card, not once per program start or build. The file is read only when whole: a header with its length and checksum.
use crate::{Renderer, Result, error};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const MAGIC: &[u8; 8] = b"VEYLPIPE";

pub(crate) struct Kept {
    pub cache: wgpu::PipelineCache,
    file: PathBuf,
    saving: AtomicBool,
}
/// The folder Veylvik keeps its compiled programs in when the host names none.
pub fn default_pipeline_folder() -> Option<PathBuf> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(cache.join("veylvik").join("pipelines"))
}
fn read(file: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(file).ok()?;
    let (magic, rest) = bytes.split_at_checked(8)?;
    let (length, rest) = rest.split_at_checked(8)?;
    let (sum, data) = rest.split_at_checked(8)?;
    let length = u64::from_le_bytes(length.try_into().ok()?);
    let sum = u64::from_le_bytes(sum.try_into().ok()?);
    (magic == MAGIC && data.len() as u64 == length && crate::fnv(data, 0) == sum)
        .then(|| data.to_vec())
}
fn write(file: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let mut bytes = Vec::with_capacity(data.len() + 24);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(data.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&crate::fnv(data, 0).to_le_bytes());
    bytes.extend_from_slice(data);
    let partial = file.with_extension("partial");
    std::fs::write(&partial, &bytes)?;
    std::fs::rename(&partial, file)
}
impl Kept {
    fn save(&self) -> Result<()> {
        if let Some(data) = self.cache.get_data() {
            write(&self.file, &data).map_err(error)?;
        }
        Ok(())
    }
}
impl Renderer {
    /// Keep compiled GPU programs in `folder` from now on, starting from what is there:
    /// call it right after making the renderer, before anything renders. False when the
    /// device cannot keep them (made without `wgpu::Features::PIPELINE_CACHE`, or not
    /// Vulkan); everything works as before then.
    pub fn use_pipeline_cache(&self, folder: &Path) -> Result<bool> {
        let Some(key) = wgpu::util::pipeline_cache_key(&self.adapter) else {
            return Ok(false);
        };
        let file = folder.join(format!("{key}.veylvik"));
        let data = read(&file);
        let Some(cache) = veylvik_interop::pipeline_cache(&self.device, data.as_deref()) else {
            return Ok(false);
        };
        *self
            .pipeline_cache
            .lock()
            .map_err(|_| error("pipeline cache poisoned"))? = Some(Arc::new(Kept {
            cache,
            file,
            saving: AtomicBool::new(false),
        }));
        Ok(true)
    }
    /// The cache compiled programs go into, for other GPU code of the host's (the 3D
    /// renderer) to use as well.
    pub fn pipeline_cache(&self) -> Option<wgpu::PipelineCache> {
        self.kept_pipelines().map(|kept| kept.cache.clone())
    }
    pub(crate) fn kept_pipelines(&self) -> Option<Arc<Kept>> {
        self.pipeline_cache.lock().ok()?.clone()
    }
    /// Write the compiled programs to their file now. Call it from a thread of the
    /// host's, not while frames are wanted: it reads them from the driver and writes.
    pub fn save_pipeline_cache(&self) -> Result<()> {
        match self.kept_pipelines() {
            Some(kept) => kept.save(),
            None => Ok(()),
        }
    }
    /// After a slow compile on a render thread: write them on a thread of their own.
    pub(crate) fn save_pipelines_later(&self) {
        let Some(kept) = self.kept_pipelines() else {
            return;
        };
        if kept.saving.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = std::thread::Builder::new()
            .name("veylvik-pipelines".into())
            .spawn(move || {
                let _ = kept.save();
                kept.saving.store(false, Ordering::Release);
            });
    }
}
