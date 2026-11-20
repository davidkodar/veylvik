//! Bounded, lazy image-sequence sources shared by Studio, CLI and MCP.
use crate::Result;
use std::{
    collections::VecDeque,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use veylvik_document::Time;
use veylvik_render::{FrameSource, Image, ImageInputs};

#[derive(Clone)]
pub enum Media {
    Still(Arc<Image>),
    Sequence(Arc<Sequence>),
    Video(Arc<crate::video::Video>),
    /// Frames someone else decodes: a host's own media.
    Source(Arc<dyn FrameSource>),
}
impl From<Arc<Image>> for Media {
    fn from(image: Arc<Image>) -> Self {
        Self::Still(image)
    }
}
impl Media {
    pub fn info(&self) -> Option<veylvik_render::SourceInfo> {
        match self {
            Self::Still(i) => Some(veylvik_render::SourceInfo {
                width: i.width(),
                height: i.height(),
                duration: None,
                frame_rate: None,
                frames: None,
                description: "Still image".into(),
            }),
            Self::Sequence(s) => s.info(),
            Self::Video(v) => v.info(),
            Self::Source(s) => s.info(),
        }
    }

    pub fn byte_size(&self) -> u64 {
        match self {
            Self::Still(i) => i.byte_size(),
            Self::Sequence(s) => s.byte_budget(),
            Self::Video(v) => v.byte_budget(),
            // Held by whoever decodes it, not by us.
            Self::Source(_) => 0,
        }
    }
    pub fn insert(self, inputs: &mut ImageInputs, key: impl Into<String>) {
        match self {
            Self::Still(i) => inputs.insert(key, i),
            Self::Sequence(s) => inputs.insert_source(key, s),
            Self::Video(v) => inputs.insert_source(key, v),
            Self::Source(s) => inputs.insert_source(key, s),
        }
    }
}
pub fn is_sequence(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("veyseq"))
        || path.file_name().is_some_and(|n| n == "sequence.json")
}
pub fn load(path: &Path, budget: u64) -> Result<Media> {
    load_layer(path, None, budget)
}
/// The media of a source key (`veylvik_document::Operation::source_key`) relative to `base`.
pub fn load_key(base: &Path, key: &str, budget: u64) -> Result<Media> {
    let (path, layer) = veylvik_document::split_source_key(key);
    load_layer(&base.join(path), layer, budget)
}
/// The OpenEXR layers of a still or of a sequence's first frame; empty for other media.
pub fn layers(path: &Path) -> Result<Vec<crate::Layer>> {
    if crate::is_exr(path) {
        return crate::exr_layers(path);
    }
    if is_sequence(path) {
        let (_, files, _) = Sequence::manifest(path)?;
        if let Some(first) = files.first().filter(|f| crate::is_exr(f)) {
            return crate::exr_layers(first);
        }
    }
    Ok(Vec::new())
}
/// As `load`; `layer` reads one layer of multi-layer OpenEXR stills or sequences.
pub fn load_layer(path: &Path, layer: Option<&str>, budget: u64) -> Result<Media> {
    if layer.is_some() && !(is_sequence(path) || crate::is_exr(path)) {
        return Err("only OpenEXR stills and sequences have layers".into());
    }
    if crate::video::is_video(path) {
        Ok(Media::Video(Arc::new(crate::video::Video::open(
            path, budget,
        )?)))
    } else if is_sequence(path) {
        Ok(Media::Sequence(Arc::new(Sequence::open_layer(
            path, layer, budget,
        )?)))
    } else if crate::lut::is_lut(path) {
        Ok(Media::Still(Arc::new(crate::lut::load(path)?)))
    } else {
        Ok(Media::Still(Arc::new(crate::load_image_layer(
            path, layer, budget,
        )?)))
    }
}

/// .veyseq: {"schema":1,"frame_rate":{"ticks":24,"rate":1},"files":["shot0001.png",...]}
/// Also reads Veylvik's completed sequence.json export manifests.
pub struct Sequence {
    files: Vec<PathBuf>,
    stamps: Vec<(u64, std::time::SystemTime)>,
    rate: Time,
    size: (u32, u32),
    /// Owner id of this sequence's frames in `host_cache`.
    owner: u64,
    /// Frames this sequence holds in the shared host cache, oldest first (at most 2).
    cache: Mutex<VecDeque<usize>>,
    /// Largest single frame the caller accepted when opening.
    frame_limit: u64,
    /// The OpenEXR layer read from every frame; None for the main image.
    layer: Option<String>,
}
impl Sequence {
    pub fn open(path: &Path, budget: u64) -> Result<Self> {
        Self::open_layer(path, None, budget)
    }
    pub fn open_layer(path: &Path, layer: Option<&str>, budget: u64) -> Result<Self> {
        let (rate, files, stamps) = Self::manifest(path)?;
        let first = Arc::new(crate::load_image_layer(
            &files[0],
            layer,
            budget.min(crate::MAX_IMAGE_BYTES),
        )?);
        let size = (first.width(), first.height());
        let owner = crate::host_cache::owner();
        let sequence = Self {
            files,
            stamps,
            rate,
            size,
            owner,
            cache: Mutex::new(VecDeque::from([0])),
            frame_limit: budget.min(crate::MAX_IMAGE_BYTES),
            layer: layer.map(str::to_owned),
        };
        crate::host_cache::put(sequence.key(0), first.clone(), first.byte_size());
        Ok(sequence)
    }
    /// The frame rate, frame files and their stamps listed by a manifest.
    #[allow(clippy::type_complexity)]
    fn manifest(path: &Path) -> Result<(Time, Vec<PathBuf>, Vec<(u64, std::time::SystemTime)>)> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 1024 * 1024 {
            return Err("Sequence manifest exceeds 1 MiB".into());
        }
        let data: serde_json::Value = serde_json::from_slice(&bytes)?;
        if data["schema"] != 1 {
            return Err("Unsupported sequence schema".into());
        }
        let (rate, names): (Time, Vec<String>) = if let Some(files) = data["files"].as_array() {
            (
                serde_json::from_value(data["frame_rate"].clone())?,
                files
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(str::to_owned)
                            .ok_or("Sequence filenames must be strings")
                    })
                    .collect::<std::result::Result<_, _>>()?,
            )
        } else {
            let range: veylvik_document::sequence::FrameRange =
                serde_json::from_value(data["frames"].clone())?;
            let count = range.count()?;
            let extension = match data["format"].as_str() {
                Some("png") => "png",
                Some("exr") => "exr",
                _ => return Err("Unsupported sequence format".into()),
            };
            if data["status"] != "complete"
                || data["count"] != count
                || data["completed"] != count
                || data["file_start"] != 0
                || data["file_pattern"] != format!("frame-%06d.{extension}")
            {
                return Err("Only completed Veylvik sequence exports can be imported".into());
            }
            (
                range.frame_rate,
                (0..count)
                    .map(|i| format!("frame-{i:06}.{extension}"))
                    .collect(),
            )
        };
        if names.is_empty()
            || names.len() > 10_000
            || rate.ticks <= 0
            || rate.rate == 0
            || rate.seconds()? > 1000.0
        {
            return Err(
                "Sequence needs 1..10000 frames and a positive frame rate up to 1000 fps".into(),
            );
        }
        let base = path
            .canonicalize()?
            .parent()
            .ok_or("Sequence needs a parent directory")?
            .to_owned();
        let mut files = Vec::new();
        let mut stamps = Vec::new();
        for name in names {
            let name = Path::new(&name);
            if name.is_absolute()
                || name.components().any(|c| {
                    !matches!(
                        c,
                        std::path::Component::Normal(_) | std::path::Component::CurDir
                    )
                })
            {
                return Err(
                    "Sequence frames must use relative paths inside the sequence directory".into(),
                );
            }
            let file = base.join(name).canonicalize()?;
            if !file.starts_with(&base) {
                return Err("Sequence frame escapes its directory".into());
            }
            let meta = std::fs::metadata(&file)?;
            if !meta.is_file() || meta.len() > 256 * 1024 * 1024 {
                return Err("Sequence frame must be a regular file of at most 256 MiB".into());
            }
            stamps.push((meta.len(), meta.modified()?));
            files.push(file);
        }
        Ok((rate, files, stamps))
    }
    fn key(&self, index: usize) -> crate::host_cache::Key {
        crate::host_cache::Key {
            owner: self.owner,
            kind: 0,
            index,
            variant: 0,
        }
    }
    fn index(&self, time: Time) -> Result<usize> {
        time.seconds()?;
        let value = (i128::from(time.ticks) * i128::from(self.rate.ticks))
            .div_euclid(i128::from(time.rate) * i128::from(self.rate.rate));
        Ok(value.clamp(0, self.files.len() as i128 - 1) as usize)
    }
}
impl Drop for Sequence {
    fn drop(&mut self) {
        crate::host_cache::forget(self.owner, None);
    }
}
impl FrameSource for Sequence {
    // Every frame carries a name made of its file as it stands on disk.
    fn steady(&self) -> bool {
        true
    }
    fn cache_dependencies(&self) -> Option<Vec<PathBuf>> {
        Some(self.files.clone())
    }
    fn info(&self) -> Option<veylvik_render::SourceInfo> {
        Some(veylvik_render::SourceInfo {
            width: self.size.0,
            height: self.size.1,
            duration: Time::at_frame(self.files.len() as i64, self.rate).ok(),
            frame_rate: Some(self.rate),
            frames: Some(self.files.len() as u32),
            description: "Image sequence".into(),
        })
    }
    /// Frames live in the shared `host_cache`, bounded there for all sources together.
    fn byte_budget(&self) -> u64 {
        0
    }
    fn frame(&self, time: Time) -> veylvik_render::Result<Arc<Image>> {
        let work = || -> Result<Arc<Image>> {
            let index = self.index(time)?;
            let mut cache = self
                .cache
                .lock()
                .map_err(|_| "Sequence cache unavailable")?;
            let meta = std::fs::metadata(&self.files[index])?;
            if (meta.len(), meta.modified()?) != self.stamps[index] {
                return Err(
                    "Sequence frame changed on disk; reload sources before continuing".into(),
                );
            }
            if cache.contains(&index)
                && let Some(image) = crate::host_cache::get::<Image>(self.key(index))
            {
                return Ok(image);
            }
            let name = {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                (&self.files[index], self.stamps[index], &self.layer).hash(&mut hasher);
                hasher.finish()
            };
            let image = Arc::new(
                crate::load_image_layer(
                    &self.files[index],
                    self.layer.as_deref(),
                    self.frame_limit,
                )?
                .named(name),
            );
            let after = std::fs::metadata(&self.files[index])?;
            if (after.len(), after.modified()?) != self.stamps[index] {
                return Err("Sequence frame changed during decoding; reload sources".into());
            }
            if (image.width(), image.height()) != self.size {
                return Err("All sequence frames must have identical dimensions".into());
            }
            // At most two frames per sequence; the shared cache bounds all sources together.
            cache.retain(|n| *n != index);
            while cache.len() >= 2 {
                if let Some(old) = cache.pop_front() {
                    crate::host_cache::remove(self.key(old));
                }
            }
            crate::host_cache::put(self.key(index), image.clone(), image.byte_size());
            cache.push_back(index);
            Ok(image)
        };
        work().map_err(|e| veylvik_render::Error::Message(e.to_string()))
    }
}

/// Discover consecutive numbered frames from the selected first frame. Publish a
/// small adjacent manifest without modifying any frame or replacing an existing file.
pub fn import_numbered(first: &Path, rate: Time, budget: u64) -> Result<(PathBuf, Media)> {
    if rate.ticks <= 0 || rate.rate == 0 || rate.seconds()? > 1000.0 {
        return Err("Invalid sequence frame rate".into());
    }
    let first = first.canonicalize()?;
    let extension = first
        .extension()
        .and_then(|s| s.to_str())
        .ok_or("Choose a numbered PNG or EXR frame")?;
    if !["png", "exr"]
        .iter()
        .any(|e| extension.eq_ignore_ascii_case(e))
    {
        return Err("Choose a PNG or EXR frame".into());
    }
    let stem = first
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("Frame filename is not UTF-8")?;
    let digits = stem.bytes().rev().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 12 {
        return Err("Frame filenames must end with a number, for example shot0001.png".into());
    }
    let prefix = &stem[..stem.len() - digits];
    let start: u64 = stem[stem.len() - digits..].parse()?;
    let parent = first.parent().ok_or("Missing sequence directory")?;
    let mut files = Vec::new();
    for i in 0..=10_000 {
        let name = format!("{prefix}{:0digits$}.{extension}", start + i);
        if !parent.join(&name).is_file() {
            break;
        }
        if i == 10_000 {
            return Err("Sequence exceeds 10000 frames; split the shot first".into());
        }
        files.push(name);
    }
    if files.len() < 2 {
        return Err("No following numbered frame was found".into());
    }
    crate::load_image(&first, budget.min(crate::MAX_IMAGE_BYTES))?;
    let bytes = serde_json::to_vec_pretty(
        &serde_json::json!({"schema":1,"frame_rate":rate,"files":files}),
    )?;
    let manifest = first.with_extension("veyseq");
    if manifest.exists() {
        if std::fs::metadata(&manifest)?.len() > 1024 * 1024 || std::fs::read(&manifest)? != bytes {
            return Err("A different sequence manifest already exists beside this frame; import that manifest or choose another starting frame".into());
        }
    } else {
        veylvik_document::atomic_create(&manifest, &bytes)?;
    }
    match load(&manifest, budget) {
        Ok(media) => Ok((manifest, media)),
        Err(e) => Err(e),
    }
}

/// Frame dependencies for overwrite protection, without decoding pixels.
pub fn dependencies(path: &Path) -> Result<Vec<PathBuf>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err("Sequence manifest exceeds 1 MiB".into());
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    let names: Vec<String> = if let Some(files) = value["files"].as_array() {
        files
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or("Invalid sequence filename")
            })
            .collect::<std::result::Result<_, _>>()?
    } else {
        let count = value["count"]
            .as_u64()
            .filter(|n| *n <= 10000)
            .ok_or("Invalid sequence count")?;
        let ext = match value["format"].as_str() {
            Some("png") => "png",
            Some("exr") => "exr",
            _ => return Err("Invalid sequence format".into()),
        };
        (0..count).map(|i| format!("frame-{i:06}.{ext}")).collect()
    };
    if names.len() > 10000 {
        return Err("Sequence exceeds 10000 frames".into());
    }
    let parent = path.parent().ok_or("Sequence needs parent directory")?;
    Ok(names.into_iter().map(|name| parent.join(name)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sequence_sampling_is_exact_bounded_and_detects_changes() {
        let dir = std::env::temp_dir().join(format!(
            "veylvik-footage-{}",
            veylvik_document::NodeId::new_v4()
        ));
        std::fs::create_dir(&dir).unwrap();
        for (n, pixel) in [
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0, 1.0],
        ]
        .into_iter()
        .enumerate()
        {
            crate::save_png(&dir.join(format!("{n}.png")), 1, 1, &[pixel]).unwrap();
        }
        let path = dir.join("shot.veyseq");
        std::fs::write(&path, r#"{"schema":1,"frame_rate":{"ticks":24000,"rate":1001},"files":["0.png","1.png","2.png"]}"#).unwrap();
        let sequence = Sequence::open(&path, 32).unwrap();
        assert_eq!(
            sequence.byte_budget(),
            0,
            "frames are accounted in host_cache"
        );
        let zero = sequence.frame(Time::new(-1, 1).unwrap()).unwrap();
        assert_eq!(zero.pixels()[0], [1.0, 0.0, 0.0, 1.0]);
        assert!(Arc::ptr_eq(
            &zero,
            &sequence.frame(Time::new(1000, 24000).unwrap()).unwrap()
        ));
        assert_eq!(
            sequence
                .frame(Time::new(1001, 24000).unwrap())
                .unwrap()
                .pixels()[0],
            [0.0, 1.0, 0.0, 1.0]
        );
        assert_eq!(
            sequence.frame(Time::new(100, 1).unwrap()).unwrap().pixels()[0],
            [0.0, 0.0, 1.0, 1.0]
        );
        assert!(sequence.cache.lock().unwrap().len() <= 2);
        std::fs::write(dir.join("1.png"), b"changed").unwrap();
        assert!(sequence.frame(Time::new(1001, 24000).unwrap()).is_err());
        std::fs::write(
            &path,
            r#"{"schema":1,"frame_rate":{"ticks":24,"rate":1},"files":["../other.png"]}"#,
        )
        .unwrap();
        assert!(Sequence::open(&path, 32).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
