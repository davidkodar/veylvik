//! Collect an editable project and its disk-backed image assets into a new folder.
use crate::Result;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
use veylvik_document::Composition;
#[cfg(test)]
use veylvik_document::Operation;

const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct Collected {
    pub project: PathBuf,
    pub assets: usize,
    pub bytes: u64,
}

/// Copy all image sources, including disconnected nodes, without modifying the
/// original document or files. Canonical aliases share one copy. Images are read
/// from disk, not a running editor's cache. The destination must not exist.
/// Each file is bounded to 64 MiB and validated with the normal 128 MiB decode
/// limit; the collection is bounded to 1 GiB of encoded image bytes. The project
/// file is published last. An interrupted process may leave an incomplete folder.
pub fn collect(document: &Composition, base: &Path, directory: &Path) -> Result<Collected> {
    collect_with_limit(document, base, directory, MAX_TOTAL_BYTES)
}

struct Pending {
    directory: PathBuf,
    files: Vec<PathBuf>,
    complete: bool,
}
impl Drop for Pending {
    fn drop(&mut self) {
        if !self.complete {
            for path in &self.files {
                let _ = std::fs::remove_file(path);
            }
            let _ = std::fs::remove_dir(self.directory.join("assets"));
            let _ = std::fs::remove_dir(&self.directory);
        }
    }
}

fn collect_with_limit(
    document: &Composition,
    base: &Path,
    directory: &Path,
    limit: u64,
) -> Result<Collected> {
    document.validate()?;
    document.encoded_size()?;
    let mut copy = document.clone();
    let mut sources: BTreeMap<(PathBuf, String), String> = BTreeMap::new();
    let mut planned_bytes = 0u64;
    for node in copy.nodes.values_mut() {
        let Some(asset) = node.operation.asset_mut() else {
            continue;
        };
        if crate::footage::is_sequence(Path::new(asset)) {
            return Err("Collecting image sequences is not supported yet; copy the sequence directory with its manifest".into());
        }
        let source = base
            .join(&*asset)
            .canonicalize()
            .map_err(|error| format!("cannot collect image {asset}: {error}"))?;
        // Decoder choice follows the original asset key, including extensionless PNGs.
        let extension = if crate::video::is_video(Path::new(asset)) {
            Path::new(asset)
                .extension()
                .unwrap()
                .to_string_lossy()
                .to_ascii_lowercase()
        } else if Path::new(asset)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exr"))
        {
            "exr".into()
        } else {
            "png".into()
        };
        let next = sources.len() + 1;
        let relative = match sources.entry((source.clone(), extension.clone())) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.get().clone(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let metadata = std::fs::metadata(&source)?;
                if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
                    return Err(format!(
                        "image {} must be a regular file of at most 64 MiB",
                        source.display()
                    )
                    .into());
                }
                planned_bytes = planned_bytes
                    .checked_add(metadata.len())
                    .ok_or("collection size overflow")?;
                if planned_bytes > limit {
                    return Err("collected image files exceed the collection byte limit".into());
                }
                entry
                    .insert(format!("assets/image-{next:04}.{}", extension))
                    .clone()
            }
        };
        *asset = relative;
    }
    copy.validate()?;
    copy.encoded_size()?;
    std::fs::create_dir(directory)?;
    let directory = directory.canonicalize()?;
    let mut pending = Pending {
        directory: directory.clone(),
        files: Vec::new(),
        complete: false,
    };
    std::fs::create_dir(directory.join("assets"))?;
    let mut bytes = 0u64;
    for ((source, _), relative) in &sources {
        let file = File::open(source)?;
        if !file.metadata()?.is_file() {
            return Err("image source is no longer a regular file".into());
        }
        let remaining = limit.saturating_sub(bytes).min(MAX_FILE_BYTES);
        let mut data = Vec::new();
        file.take(remaining + 1).read_to_end(&mut data)?;
        if data.len() as u64 > remaining {
            return Err("image files grew beyond the collection byte limit".into());
        }
        bytes += data.len() as u64;
        let destination = directory.join(relative);
        veylvik_document::atomic_create(&destination, &data)?;
        pending.files.push(destination.clone());
        drop(data);
        // Validate the actual copied bytes, not a second read of a changing source.
        crate::footage::load(&destination, crate::MAX_IMAGE_BYTES)?;
    }
    File::open(directory.join("assets"))?.sync_all()?;
    let project = directory.join("project.veylvik");
    veylvik_document::atomic_create(&project, &serde_json::to_vec_pretty(&copy)?)?;
    pending.files.push(project.clone());
    File::open(&directory)?.sync_all()?;
    pending.complete = true;
    Ok(Collected {
        project,
        assets: sources.len(),
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_document::{Node, NodeId};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("veylvik-collect-{}", NodeId::new_v4()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn image(doc: &mut Composition, asset: &str) -> NodeId {
        let node = Node::new(
            "Source",
            Operation::Image {
                asset: asset.into(),
            },
            &[],
        );
        let id = node.id;
        doc.nodes.insert(id, node);
        id
    }
    #[test]
    fn portable_copy_preserves_graph_and_deduplicates_aliases() {
        let temp = Temp::new();
        let source = temp.0.join("originals");
        std::fs::create_dir(&source).unwrap();
        crate::save_png(&source.join("source"), 1, 1, &[[1.0, 0.0, 0.0, 1.0]]).unwrap();
        crate::export::save_exr(&source.join("hdr.EXR"), 1, 1, &[[4.0, -0.25, 0.5, 1.0]]).unwrap();
        std::os::unix::fs::symlink(source.join("source"), source.join("alias.png")).unwrap();
        let mut doc = veylvik_document::demo();
        let png = image(&mut doc, "source");
        let alias = image(&mut doc, "alias.png");
        let exr = image(&mut doc, "hdr.EXR");
        let original = serde_json::to_value(&doc).unwrap();
        let png_bytes = std::fs::read(source.join("source")).unwrap();
        let exr_bytes = std::fs::read(source.join("hdr.EXR")).unwrap();
        let destination = temp.0.join("collected");
        let report = collect(&doc, &source, &destination).unwrap();
        assert_eq!(report.assets, 2);
        assert_eq!(report.bytes, (png_bytes.len() + exr_bytes.len()) as u64);
        assert_eq!(serde_json::to_value(&doc).unwrap(), original);
        let mut copied = Composition::load(&report.project).unwrap();
        let asset = |id| match &copied.nodes[&id].operation {
            Operation::Image { asset } => asset.clone(),
            _ => unreachable!(),
        };
        assert_eq!(asset(png), asset(alias));
        assert_eq!(
            std::fs::read(destination.join(asset(png))).unwrap(),
            png_bytes
        );
        assert_eq!(
            std::fs::read(destination.join(asset(exr))).unwrap(),
            exr_bytes
        );
        let moved = temp.0.join("moved");
        std::fs::rename(&destination, &moved).unwrap();
        std::fs::remove_dir_all(source).unwrap();
        for id in [png, alias, exr] {
            crate::load_image(&moved.join(asset(id)), crate::MAX_IMAGE_BYTES).unwrap();
        }
        for id in [png, alias, exr] {
            copied.nodes.get_mut(&id).unwrap().operation = doc.nodes[&id].operation.clone();
        }
        assert_eq!(serde_json::to_value(copied).unwrap(), original);
    }
    #[test]
    fn failures_preserve_existing_targets_and_clean_partial_collections() {
        let temp = Temp::new();
        let mut doc = veylvik_document::demo();
        let destination = temp.0.join("collection");
        let source = temp.0.join("source.png");
        crate::save_png(&source, 1, 1, &[[0.0; 4]]).unwrap();
        image(&mut doc, "source.png");
        assert!(collect_with_limit(&doc, &temp.0, &destination, 1).is_err());
        assert!(!destination.exists());
        let missing = image(&mut doc, "missing.png");
        assert!(collect(&doc, &temp.0, &destination).is_err());
        assert!(!destination.exists());
        doc.nodes.remove(&missing);
        std::fs::write(&source, b"invalid png").unwrap();
        assert!(collect(&doc, &temp.0, &destination).is_err());
        assert!(!destination.exists());
        crate::save_png(&source, 1, 1, &[[0.0; 4]]).unwrap();
        std::fs::create_dir(&destination).unwrap();
        let sentinel = destination.join("keep");
        std::fs::write(&sentinel, b"keep").unwrap();
        assert!(collect(&doc, &temp.0, &destination).is_err());
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"keep");
        std::fs::remove_file(sentinel).unwrap();
        std::fs::remove_dir(&destination).unwrap();
        std::os::unix::fs::symlink(&temp.0, &destination).unwrap();
        assert!(collect(&doc, &temp.0, &destination).is_err());
        assert!(source.is_file());
    }
}
