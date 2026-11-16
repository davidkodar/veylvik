//! The user's saved materials and library favourites, bounded and validated on every read.
use crate::Result;
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Path, PathBuf},
};
use veylvik_document::{NodeId, presets::material::Saved};

pub const MAX_MATERIAL_BYTES: u64 = 1024 * 1024;
pub const MAX_MATERIALS: usize = 512;
pub const MAX_FAVOURITES: usize = 1024;
pub const EXTENSION: &str = "veymat";

#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub saved: Saved,
}
impl Entry {
    /// Stable library key: the file name, which never changes on rename.
    pub fn key(&self) -> String {
        format!(
            "saved:{}",
            self.path.file_stem().unwrap_or_default().to_string_lossy()
        )
    }
}
#[derive(Debug, Default)]
pub struct Listing {
    pub entries: Vec<Entry>,
    pub warnings: Vec<String>,
}

pub fn directory() -> Result<PathBuf> {
    Ok(crate::presets::directory()?.with_file_name("materials"))
}

pub fn load(path: &Path) -> Result<Saved> {
    let mut data = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_MATERIAL_BYTES + 1)
        .read_to_end(&mut data)?;
    if data.len() as u64 > MAX_MATERIAL_BYTES {
        return Err("Saved material exceeds 1 MiB".into());
    }
    let saved: Saved = serde_json::from_slice(&data)?;
    saved.validate()?;
    Ok(saved)
}

fn encode(saved: &Saved) -> Result<Vec<u8>> {
    saved.validate()?;
    let data = serde_json::to_vec_pretty(saved)?;
    if data.len() as u64 > MAX_MATERIAL_BYTES {
        return Err("Saved material exceeds 1 MiB".into());
    }
    Ok(data)
}

/// Save as a new file; names may repeat, files never collide.
pub fn add(directory: &Path, saved: &Saved) -> Result<PathBuf> {
    let data = encode(saved)?;
    std::fs::create_dir_all(directory)?;
    let path = directory.join(format!("{}.{EXTENSION}", NodeId::new_v4()));
    veylvik_document::atomic_create(&path, &data)?;
    Ok(path)
}

fn owned(directory: &Path, path: &Path) -> Result<()> {
    if path.parent() != Some(directory) || path.extension().is_none_or(|e| e != EXTENSION) {
        return Err("Only saved materials in the library folder can be changed".into());
    }
    Ok(())
}

/// Replace a saved material in place (rename or update), atomically.
pub fn replace(directory: &Path, path: &Path, saved: &Saved) -> Result<()> {
    owned(directory, path)?;
    let data = encode(saved)?;
    veylvik_document::atomic_write(path, &data)?;
    Ok(())
}

pub fn remove(directory: &Path, path: &Path) -> Result<()> {
    owned(directory, path)?;
    std::fs::remove_file(path)?;
    Ok(())
}

/// Every readable saved material, sorted by name; unreadable files become warnings.
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
        let path = entry.path();
        if path.extension().is_some_and(|e| e == EXTENSION) && entry.file_type()?.is_file() {
            if paths.len() == MAX_MATERIALS {
                listing
                    .warnings
                    .push(format!("Showing at most {MAX_MATERIALS} saved materials."));
                break;
            }
            paths.push(path);
        }
    }
    paths.sort();
    for path in paths {
        match load(&path) {
            Ok(saved) => listing.entries.push(Entry { path, saved }),
            Err(e) => listing.warnings.push(format!("{}: {e}", path.display())),
        }
    }
    listing
        .entries
        .sort_by_cached_key(|e| e.saved.label.to_lowercase());
    Ok(listing)
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Favourites {
    schema: u32,
    keys: BTreeSet<String>,
}

fn favourites_path(directory: &Path) -> PathBuf {
    directory.join("favourites.json")
}

/// Library keys the user starred: `builtin:<id>` or a saved entry's key.
pub fn favourites(directory: &Path) -> Result<BTreeSet<String>> {
    let path = favourites_path(directory);
    let mut data = Vec::new();
    match std::fs::File::open(&path) {
        Ok(file) => file.take(MAX_MATERIAL_BYTES + 1).read_to_end(&mut data)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(e) => return Err(e.into()),
    };
    if data.len() as u64 > MAX_MATERIAL_BYTES {
        return Err("Material favourites exceed 1 MiB".into());
    }
    let favourites: Favourites = serde_json::from_slice(&data)?;
    if favourites.schema != 1 || favourites.keys.len() > MAX_FAVOURITES {
        return Err("Unsupported or oversized material favourites".into());
    }
    Ok(favourites.keys)
}

pub fn set_favourites(directory: &Path, keys: &BTreeSet<String>) -> Result<()> {
    if keys.len() > MAX_FAVOURITES || keys.iter().any(|k| k.len() > 256) {
        return Err("Too many material favourites".into());
    }
    std::fs::create_dir_all(directory)?;
    let data = serde_json::to_vec_pretty(&Favourites {
        schema: 1,
        keys: keys.clone(),
    })?;
    veylvik_document::atomic_write(&favourites_path(directory), &data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::presets::material::Source;

    #[test]
    fn saved_materials_add_list_rename_and_remove() {
        let directory =
            std::env::temp_dir().join(format!("veylvik-materials-{}", NodeId::new_v4()));
        let mut material = Source::Builtin("marble").material().unwrap();
        material.colours = Some(veylvik_document::optical::palette_track([
            [0.02; 3],
            [0.8, 0.55, 0.15],
            [0.; 3],
        ]));
        let saved = Saved::new("Black marble", &material).unwrap();
        let path = add(&directory, &saved).unwrap();
        add(&directory, &Saved::new("Aqua leather", &material).unwrap()).unwrap();
        std::fs::write(directory.join("broken.veymat"), b"{").unwrap();
        let listing = list(&directory).unwrap();
        assert_eq!(listing.entries.len(), 2);
        assert_eq!(listing.entries[0].saved.label, "Aqua leather");
        assert_eq!(listing.warnings.len(), 1);

        let renamed = Saved::new("Night marble", &material).unwrap();
        replace(&directory, &path, &renamed).unwrap();
        assert_eq!(load(&path).unwrap().label, "Night marble");
        assert!(replace(&directory, Path::new("/etc/passwd"), &renamed).is_err());

        let key = list(&directory).unwrap().entries[1].key();
        assert!(key.starts_with("saved:"));
        let keys = BTreeSet::from([key, "builtin:gold_text".to_owned()]);
        set_favourites(&directory, &keys).unwrap();
        assert_eq!(favourites(&directory).unwrap(), keys);

        remove(&directory, &path).unwrap();
        assert!(remove(&directory, &directory.join("favourites.json")).is_err());
        assert_eq!(list(&directory).unwrap().entries.len(), 1);
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
