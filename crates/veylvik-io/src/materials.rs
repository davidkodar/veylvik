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

/// Every material an agent can apply: the family surfaces, the catalogue's whole
/// looks and the user's saved materials, with favourites marked. Saved ids are
/// `saved:<file stem>`, as in the library.
pub fn library(directory: &Path) -> Result<serde_json::Value> {
    use veylvik_document::presets::{builtin, catalogue, material};
    let favourites = favourites(directory).unwrap_or_default();
    let mut materials = Vec::new();
    for entry in builtin::ALL
        .iter()
        .filter(|e| builtin::is_material(e.id) && catalogue::get(e.id).is_none())
    {
        materials.push(serde_json::json!({"id":entry.id,"label":entry.label,"category":material::category(entry.id),"source":"family","description":entry.description,"favourite":favourites.contains(&format!("builtin:{}",entry.id))}));
    }
    for entry in &catalogue::ENTRIES {
        materials.push(serde_json::json!({"id":entry.id,"label":entry.label,"category":entry.category,"source":"catalogue","description":entry.description,"favourite":favourites.contains(&format!("builtin:{}",entry.id))}));
    }
    let listing = list(directory)?;
    for entry in &listing.entries {
        materials.push(serde_json::json!({"id":entry.key(),"label":entry.saved.label,"category":"My materials","source":"saved","favourite":favourites.contains(&entry.key())}));
    }
    Ok(serde_json::json!({"materials":materials,"warnings":listing.warnings}))
}

/// Commands that put material `id` on `target`, exactly like the library: a Text
/// node gets (or restyles) its Text surface, a Text surface is restyled in place,
/// and 3D text or a 3D shape gets it as its look. A family keeps the target's
/// finish and lighting; a catalogue look or a saved material brings its whole look.
pub fn apply(
    doc: &veylvik_document::Composition,
    target: NodeId,
    id: &str,
    directory: &Path,
) -> Result<Vec<veylvik_document::Command>> {
    use veylvik_document::presets::{builtin, material::Source};
    let listing;
    let source = if let Some(stem) = id.strip_prefix("saved:") {
        listing = list(directory)?;
        let entry = listing
            .entries
            .iter()
            .find(|e| e.key() == format!("saved:{stem}"))
            .ok_or_else(|| format!("no saved material {id}"))?;
        Source::Saved(&entry.saved)
    } else if builtin::is_material(id) {
        Source::Builtin(id)
    } else {
        return Err(format!("unknown material {id}; list them with materials").into());
    };
    Ok(veylvik_document::presets::material::apply_commands(
        doc, target, source,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::presets::material::Source;

    #[test]
    fn agents_list_every_material_and_apply_it_to_text_or_a_surface() {
        use veylvik_document::{Command, Document, Node, Operation, Time, blank};
        let directory =
            std::env::temp_dir().join(format!("veylvik-materials-{}", NodeId::new_v4()));
        let mut navy = Source::Builtin("leather_brown").material().unwrap();
        navy.roughness = 0.61.into();
        let path = add(&directory, &Saved::new("Navy leather", &navy).unwrap()).unwrap();
        let library = library(&directory).unwrap();
        let materials = library["materials"].as_array().unwrap();
        let find = |id: &str| materials.iter().find(|m| m["id"] == id).cloned();
        assert_eq!(find("gold_text").unwrap()["source"], "family");
        assert_eq!(find("chrome").unwrap()["source"], "catalogue");
        let saved = format!("saved:{}", path.file_stem().unwrap().to_string_lossy());
        assert_eq!(find(&saved).unwrap()["label"], "Navy leather");
        assert!(
            find("liquid_shimmer").is_none(),
            "effects are not materials"
        );

        // A bare title gets a surface; applying again restyles that surface.
        let mut scene = blank(64, 32, Time::new(24, 1).unwrap()).unwrap();
        let text: Operation =
            serde_json::from_value(veylvik_document::registry::TEXT.default_operation()).unwrap();
        let title = Node::new("Title", text, &[]);
        let id = title.id;
        scene.nodes.insert(id, title);
        scene.output = id;
        let mut document = Document::new(scene).unwrap();
        let commands = apply(document.snapshot(), id, "chrome", &directory).unwrap();
        let revision = document.apply(0, &commands).unwrap();
        let surface = veylvik_document::presets::text_style::current(document.snapshot(), id)
            .unwrap()
            .unwrap();
        let commands = apply(document.snapshot(), surface, &saved, &directory).unwrap();
        assert!(matches!(commands[..], [Command::Put { .. }]));
        document.apply(revision, &commands).unwrap();
        let Operation::Material(m) = &document.snapshot().nodes[&surface].operation else {
            panic!()
        };
        assert_eq!(m.kind, navy.kind);
        assert_eq!(
            serde_json::to_value(&m.roughness).unwrap(),
            serde_json::to_value(&navy.roughness).unwrap(),
            "a saved material brings its whole look"
        );
        for bad in ["liquid_shimmer", "saved:missing", "nope"] {
            assert!(apply(document.snapshot(), id, bad, &directory).is_err());
        }
        let output = document.snapshot().output;
        if output != id && output != surface {
            assert!(apply(document.snapshot(), output, "chrome", &directory).is_err());
        }
        std::fs::remove_dir_all(&directory).unwrap();
    }
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
