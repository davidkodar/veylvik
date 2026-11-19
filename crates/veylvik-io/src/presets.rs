//! Bounded preset persistence and a user-owned effect library.
use crate::Result;
use std::{
    io::Read,
    path::{Path, PathBuf},
};
use veylvik_document::{NodeId, presets::Preset};
pub const MAX_PRESET_BYTES: usize = 16 * 1024 * 1024;
#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub label: String,
    pub nodes: usize,
    pub inputs: usize,
}
#[derive(Debug, Default)]
pub struct Listing {
    pub entries: Vec<Entry>,
    pub warnings: Vec<String>,
}
pub fn directory() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| Path::new(v).is_absolute())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or("No user data directory is available")?;
    Ok(base.join("veylvik/effects"))
}
pub fn load(path: &Path) -> Result<Preset> {
    let mut data = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_PRESET_BYTES as u64 + 1)
        .read_to_end(&mut data)?;
    if data.len() > MAX_PRESET_BYTES {
        return Err("Effect preset exceeds 16 MiB".into());
    }
    let preset: Preset = serde_json::from_slice(&data)?;
    preset.validate()?;
    // A sound the effect brings is small and of a known kind, or the effect is refused.
    let beside = path.parent().unwrap_or(Path::new("."));
    for cue in &preset.sounds {
        crate::sounds::check_effect_sound(beside, &cue.file)?;
    }
    Ok(preset)
}
pub fn save_new(path: &Path, preset: &Preset) -> Result<()> {
    preset.validate()?;
    let data = serde_json::to_vec_pretty(preset)?;
    if data.len() > MAX_PRESET_BYTES {
        return Err("Effect preset exceeds 16 MiB".into());
    }
    veylvik_document::atomic_create(path, &data)?;
    Ok(())
}
pub fn add(directory: &Path, preset: &Preset) -> Result<PathBuf> {
    preset.validate()?;
    std::fs::create_dir_all(directory)?;
    let path = directory.join(format!("{}.veyfx", NodeId::new_v4()));
    save_new(&path, preset)?;
    Ok(path)
}
pub fn list(directory: &Path) -> Result<Listing> {
    let mut listing = Listing::default();
    let files = match std::fs::read_dir(directory) {
        Ok(files) => files,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(listing),
        Err(e) => return Err(e.into()),
    };
    let mut paths = Vec::new();
    for entry in files {
        let entry = entry?;
        if entry.path().extension().is_some_and(|e| e == "veyfx") && entry.file_type()?.is_file() {
            paths.push(entry.path());
            if paths.len() >= 256 {
                listing
                    .warnings
                    .push("Showing at most 256 saved effects.".into());
                break;
            }
        }
    }
    paths.sort();
    let mut budget = 64 * 1024 * 1024u64;
    for path in paths {
        let size = std::fs::metadata(&path)?.len();
        if size > MAX_PRESET_BYTES as u64 {
            listing
                .warnings
                .push(format!("{} exceeds 16 MiB", path.display()));
            continue;
        }
        if size > budget {
            listing
                .warnings
                .push("Library scan reached its 64 MiB limit.".into());
            break;
        }
        budget -= size;
        match load(&path) {
            Ok(preset) => listing.entries.push(Entry {
                path,
                label: preset.group.label,
                nodes: preset.nodes.len(),
                inputs: preset.group.inputs.len(),
            }),
            Err(e) => listing.warnings.push(format!("{}: {e}", path.display())),
        }
    }
    listing
        .entries
        .sort_by_cached_key(|e| e.label.to_lowercase());
    Ok(listing)
}
