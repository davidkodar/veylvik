//! Portable, verified copies of editable compositions, with streamed media copying.
use crate::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
use veylvik_document::Composition;
#[cfg(test)]
use veylvik_document::Operation;
const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
const MAX_FILES: usize = 100_000;
pub const MANIFEST: &str = "collection.json";
#[derive(Debug, Serialize)]
pub struct Collected {
    pub project: PathBuf,
    pub assets: usize,
    pub bytes: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Resource {
    pub path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub files: Vec<Resource>,
    pub notes: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct Progress {
    pub files: usize,
    pub total_files: usize,
    pub bytes: u64,
    pub total_bytes: u64,
    pub phase: String,
}
struct Item {
    relative: PathBuf,
    source: Option<PathBuf>,
    data: Vec<u8>,
    bytes: u64,
    modified: Option<std::time::SystemTime>,
}
struct Plan {
    document: Composition,
    items: Vec<Item>,
    aliases: BTreeMap<PathBuf, String>,
    bytes: u64,
    limit: u64,
}
impl Plan {
    fn generated(&mut self, relative: PathBuf, data: Vec<u8>) -> Result<()> {
        self.reserve(data.len() as u64)?;
        self.items.push(Item {
            relative,
            bytes: data.len() as u64,
            data,
            source: None,
            modified: None,
        });
        Ok(())
    }
    fn reserve(&mut self, bytes: u64) -> Result<()> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or("collection size overflow")?;
        if self.bytes > self.limit || self.items.len() >= MAX_FILES {
            return Err("Collection exceeds the 1 TiB / 100,000 file limit".into());
        }
        Ok(())
    }
    fn source(&mut self, source: &Path) -> Result<String> {
        let filename = source
            .file_name()
            .ok_or("Missing filename")?
            .to_string_lossy()
            .into_owned();
        let extensionless = source.extension().is_none();
        let source = source
            .canonicalize()
            .map_err(|e| format!("Cannot collect {}: {e}", source.display()))?;
        if let Some(relative) = self.aliases.get(&source) {
            return Ok(relative.clone());
        }
        let meta = std::fs::metadata(&source)?;
        if !meta.is_file() {
            return Err(format!("Not a regular file: {}", source.display()).into());
        }
        self.reserve(meta.len())?;
        // Keep original names for recognition, prefixing a stable per-collection identity.
        let mut relative = format!("assets/{:05}-{filename}", self.items.len() + 1);
        if extensionless {
            relative.push_str(".png");
        }
        self.items.push(Item {
            relative: relative.clone().into(),
            source: Some(source.clone()),
            bytes: meta.len(),
            modified: Some(meta.modified()?),
            data: vec![],
        });
        self.aliases.insert(source, relative.clone());
        Ok(relative)
    }
}
fn plan(
    document: &Composition,
    base: &Path,
    limit: u64,
    progress: &mut impl FnMut(Progress) -> bool,
) -> Result<Plan> {
    document.validate()?;
    document.encoded_size()?;
    let mut plan = Plan {
        document: document.clone(),
        items: vec![],
        aliases: BTreeMap::new(),
        bytes: 0,
        limit,
    };
    let mut assets = BTreeMap::new();
    for node in document.nodes.values() {
        if let Some(asset) = node.operation.asset() {
            if assets.contains_key(asset) {
                continue;
            }
            if !progress(Progress {
                files: 0,
                total_files: plan.items.len(),
                bytes: 0,
                total_bytes: plan.bytes,
                phase: format!("Checking {asset}"),
            }) {
                return Err("Collection cancelled".into());
            }
            let path = base.join(asset);
            // Validate sources using the same decoding rules as the editor (including LUTs).
            let media = crate::footage::load(
                &path,
                crate::runtime::source_budget().min(128 * 1024 * 1024),
            )?;
            let relative = if crate::footage::is_sequence(&path) {
                let rate = media
                    .info()
                    .and_then(|i| i.frame_rate)
                    .ok_or("Sequence has no frame rate")?;
                let mut files = vec![];
                for source in crate::footage::dependencies(&path)? {
                    files.push(plan.source(&source)?);
                }
                let name = format!("sequence-{:05}.veyseq", plan.items.len() + 1);
                plan.generated(
                    name.clone().into(),
                    serde_json::to_vec_pretty(
                        &serde_json::json!({"schema":1,"frame_rate":rate,"files":files}),
                    )?,
                )?;
                name
            } else {
                plan.source(&path)?
            };
            assets.insert(asset.to_owned(), relative);
        }
    }
    for node in plan.document.nodes.values_mut() {
        if let Some(asset) = node.operation.asset_mut() {
            *asset = assets[asset].clone();
        }
    }
    // Preserve the exact used conversion, including analytic transforms and resolved LUTs.
    // The studio config catalogue is not required on the destination machine.
    let mut converted = vec![];
    for (index, transform) in document.ocio_transforms().enumerate() {
        if transform.config.starts_with("ocio://") {
            converted.push(transform.clone());
            continue;
        }
        let path = base.join(&transform.config);
        let config = veylvik_color::Config::load(&path.to_string_lossy())?;
        let processor = config.processor(
            &transform.source,
            &transform.destination,
            &transform.display,
            &transform.view,
        )?;
        let (config, ctf) = processor.portable()?;
        let folder = format!("colour/{index:04}");
        plan.generated(format!("{folder}/config.ocio").into(), config.into_bytes())?;
        plan.generated(format!("{folder}/transform.ctf").into(), ctf.into_bytes())?;
        let mut next = transform.clone();
        next.config = format!("{folder}/config.ocio");
        next.source = "Input".into();
        next.destination = "Output".into();
        next.display.clear();
        next.view.clear();
        converted.push(next);
    }
    for (target, value) in plan.document.ocio_transforms_mut().zip(converted) {
        *target = value;
    }
    plan.document.validate()?;
    plan.document.encoded_size()?;
    Ok(plan)
}
/// Copy without modifying the original. Existing destinations are always refused.
pub fn collect(document: &Composition, base: &Path, directory: &Path) -> Result<Collected> {
    collect_with_progress(document, base, directory, |_| true)
}
pub fn collect_with_progress(
    document: &Composition,
    base: &Path,
    directory: &Path,
    progress: impl FnMut(Progress) -> bool,
) -> Result<Collected> {
    run(document, base, directory, MAX_TOTAL_BYTES, progress)
}
#[cfg(test)]
fn collect_with_limit(
    document: &Composition,
    base: &Path,
    directory: &Path,
    limit: u64,
) -> Result<Collected> {
    run(document, base, directory, limit, |_| true)
}
struct Pending {
    directory: PathBuf,
    complete: bool,
}
impl Drop for Pending {
    fn drop(&mut self) {
        // Only the newly reserved directory is owned by this operation; no pre-existing
        // destination is ever installed here. remove_dir_all does not follow symlinks.
        if !self.complete {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }
}
fn run(
    document: &Composition,
    base: &Path,
    directory: &Path,
    limit: u64,
    mut progress: impl FnMut(Progress) -> bool,
) -> Result<Collected> {
    if directory.try_exists()? {
        return Err("Collection destination already exists. Choose a new folder name.".into());
    }
    let plan = plan(document, base, limit, &mut progress)?;
    std::fs::create_dir(directory)?;
    let mut pending = Pending {
        directory: directory.canonicalize()?,
        complete: false,
    };
    let directory = &pending.directory;
    let mut resources = vec![];
    let mut total = 0;
    for item in &plan.items {
        let destination = directory.join(&item.relative);
        std::fs::create_dir_all(destination.parent().unwrap())?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)?;
        let mut hash = Sha256::new();
        if let Some(source) = &item.source {
            let mut input = File::open(source)?;
            let meta = input.metadata()?;
            if !meta.is_file()
                || meta.len() != item.bytes
                || Some(meta.modified()?) != item.modified
            {
                return Err(format!("Source changed: {}", source.display()).into());
            }
            let mut copied = 0;
            let mut buffer = vec![0u8; 1024 * 1024];
            loop {
                if !progress(Progress {
                    files: resources.len(),
                    total_files: plan.items.len(),
                    bytes: total,
                    total_bytes: plan.bytes,
                    phase: format!("Copying {}", source.file_name().unwrap().to_string_lossy()),
                }) {
                    return Err("Collection cancelled".into());
                }
                let n = input.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                copied += n as u64;
                if copied > item.bytes {
                    return Err("Source grew during collection".into());
                }
                output.write_all(&buffer[..n])?;
                hash.update(&buffer[..n]);
                total += n as u64;
            }
            let meta = input.metadata()?;
            if copied != item.bytes
                || meta.len() != item.bytes
                || Some(meta.modified()?) != item.modified
            {
                return Err("Source changed during collection".into());
            }
        } else {
            output.write_all(&item.data)?;
            hash.update(&item.data);
            total += item.bytes;
        }
        output.sync_all()?;
        resources.push(Resource {
            path: item.relative.clone(),
            bytes: item.bytes,
            sha256: format!("{:x}", hash.finalize()),
        });
    }
    // Reopen only the copied files. Never let validation fall back to originals.
    for node in plan.document.nodes.values() {
        if let Some(asset) = node.operation.asset() {
            crate::footage::load(
                &directory.join(asset),
                crate::runtime::source_budget().min(128 * 1024 * 1024),
            )?;
        }
    }
    for transform in plan.document.ocio_transforms() {
        let path = if transform.config.starts_with("ocio://") {
            transform.config.clone()
        } else {
            directory
                .join(&transform.config)
                .to_string_lossy()
                .into_owned()
        };
        let config = veylvik_color::Config::load(&path)?;
        config.processor(
            &transform.source,
            &transform.destination,
            &transform.display,
            &transform.view,
        )?;
    }
    let project = directory.join("project.veylvik");
    let bytes = serde_json::to_vec_pretty(&plan.document)?;
    veylvik_document::atomic_create(&project, &bytes)?;
    resources.push(Resource {
        path: "project.veylvik".into(),
        bytes: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
    });
    let manifest=Manifest {schema:1,files:resources,notes:vec!["Fonts are bundled or embedded in project.veylvik.".into(),"Custom OCIO conversions are lossless resolved CTF snapshots; the original config catalogue is not included. Builtin OCIO configurations retain their pinned identities.".into()]};
    veylvik_document::atomic_create(
        &directory.join(MANIFEST),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    verify_with_progress(directory, |files, bytes| {
        progress(Progress {
            files,
            total_files: manifest.files.len(),
            bytes,
            total_bytes: total,
            phase: "Verifying SHA-256".into(),
        })
    })?;
    File::open(directory)?.sync_all()?;
    pending.complete = true;
    Ok(Collected {
        project,
        assets: plan.items.len(),
        bytes: total,
    })
}
/// Resolve untrusted manifest entries strictly inside the collection (including symlinks).
pub fn contained(directory: &Path, relative: &Path) -> Result<PathBuf> {
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Collection paths must be relative and cannot contain traversal".into());
    }
    let root = directory.canonicalize()?;
    let path = root.join(relative).canonicalize()?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err("Collection resource resolves outside its folder".into());
    }
    Ok(path)
}
pub fn manifest(directory: &Path) -> Result<Manifest> {
    let mut data = vec![];
    File::open(directory.join(MANIFEST))?
        .take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut data)?;
    if data.len() > 32 * 1024 * 1024 {
        return Err("Collection manifest exceeds 32 MiB".into());
    }
    let manifest: Manifest = serde_json::from_slice(&data)?;
    if manifest.schema != 1 || manifest.files.len() > MAX_FILES + 1 {
        return Err("Unsupported collection manifest".into());
    }
    Ok(manifest)
}
pub fn verify(directory: &Path) -> Result<usize> {
    verify_with_progress(directory, |_, _| true)
}
pub fn verify_with_progress(
    directory: &Path,
    mut progress: impl FnMut(usize, u64) -> bool,
) -> Result<usize> {
    let manifest = manifest(directory)?;
    let mut total = 0;
    for (index, item) in manifest.files.iter().enumerate() {
        let path = contained(directory, &item.path)?;
        let mut file = File::open(&path)?;
        if file.metadata()?.len() != item.bytes {
            return Err(format!("Size mismatch: {}", item.path.display()).into());
        }
        let mut hash = Sha256::new();
        let mut buffer = vec![0; 1024 * 1024];
        let mut size = 0;
        loop {
            if !progress(index, total) {
                return Err("Verification cancelled".into());
            }
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            size += n as u64;
            total += n as u64;
            if size > item.bytes {
                return Err("File changed during verification".into());
            }
            hash.update(&buffer[..n]);
        }
        if size != item.bytes || format!("{:x}", hash.finalize()) != item.sha256 {
            return Err(format!("SHA-256 mismatch: {}", item.path.display()).into());
        }
    }
    let root = directory.canonicalize()?;
    let listed = manifest
        .files
        .iter()
        .map(|r| contained(&root, &r.path))
        .collect::<Result<std::collections::BTreeSet<_>>>()?;
    let project = contained(&root, Path::new("project.veylvik"))?;
    if !listed.contains(&project) {
        return Err("Manifest does not include project.veylvik".into());
    }
    let doc = Composition::load(&project)?;
    let check = |path: &Path| -> Result<()> {
        let path = path.canonicalize()?;
        if !path.starts_with(&root) || !listed.contains(&path) {
            return Err(format!("Uncollected dependency: {}", path.display()).into());
        }
        Ok(())
    };
    for node in doc.nodes.values() {
        if let Some(asset) = node.operation.asset() {
            let path = contained(&root, Path::new(asset))?;
            check(&path)?;
            if crate::footage::is_sequence(&path) {
                for frame in crate::footage::dependencies(&path)? {
                    check(&frame)?;
                }
            }
        }
    }
    for transform in doc.ocio_transforms() {
        if transform.config.starts_with("ocio://") {
            continue;
        }
        let path = contained(&root, Path::new(&transform.config))?;
        check(&path)?;
        let config = veylvik_color::Config::load(&path.to_string_lossy())?;
        let processor = config.processor(
            &transform.source,
            &transform.destination,
            &transform.display,
            &transform.view,
        )?;
        for file in processor.files() {
            check(&path.parent().unwrap().join(file))?;
        }
    }
    Ok(manifest.files.len())
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
            Operation::Image(veylvik_document::nodes::Image {
                asset: asset.into(),
            }),
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
            Operation::Image(veylvik_document::nodes::Image { asset }) => asset.clone(),
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
    #[test]
    fn sequence_collection_is_independent_and_detects_damage_and_escape() {
        let temp = Temp::new();
        let source = temp.0.join("original");
        std::fs::create_dir(&source).unwrap();
        for n in 0..3 {
            crate::save_png(
                &source.join(format!("{n}.png")),
                1,
                1,
                &[[n as f32, 0., 0., 1.]],
            )
            .unwrap();
        }
        std::fs::write(
            source.join("clip.veyseq"),
            r#"{"schema":1,"frame_rate":{"ticks":24,"rate":1},"files":["0.png","1.png","2.png"]}"#,
        )
        .unwrap();
        let mut doc = veylvik_document::demo();
        let id = image(&mut doc, "clip.veyseq");
        let destination = temp.0.join("copy");
        let report = collect(&doc, &source, &destination).unwrap();
        assert_eq!(report.assets, 4);
        assert_eq!(verify(&destination).unwrap(), 5);
        let moved = temp.0.join("moved");
        std::fs::rename(destination, &moved).unwrap();
        std::fs::remove_dir_all(source).unwrap();
        let doc = Composition::load(&moved.join("project.veylvik")).unwrap();
        let asset = doc.nodes[&id].operation.asset().unwrap();
        let media = crate::footage::load(&moved.join(asset), crate::MAX_IMAGE_BYTES).unwrap();
        assert_eq!(media.info().unwrap().frames, Some(3));
        assert_eq!(verify(&moved).unwrap(), 5);
        let manifest = manifest(&moved).unwrap();
        let first = moved.join(&manifest.files[0].path);
        std::fs::write(&first, b"corrupt").unwrap();
        assert!(verify(&moved).is_err());
        let outside = temp.0.join("outside");
        std::fs::write(&outside, b"x").unwrap();
        std::fs::remove_file(&first).unwrap();
        std::os::unix::fs::symlink(&outside, &first).unwrap();
        assert!(contained(&moved, &manifest.files[0].path).is_err());
        assert!(contained(&moved, Path::new("../outside")).is_err());
    }
    #[test]
    fn custom_colour_is_lossless_and_independent_of_original_config_and_luts() {
        let temp = Temp::new();
        let source = temp.0.join("colour");
        std::fs::create_dir(&source).unwrap();
        let config =
            veylvik_color::Config::load("ocio://studio-config-v4.0.0_aces-v2.0_ocio-v2.5").unwrap();
        let processor = config
            .processor("Linear Rec.709 (sRGB)", "ACEScg", "", "")
            .unwrap();
        let sample = [2.5, 0.35, 0.04, 0.6];
        let expected = processor.reference(sample).unwrap();
        let (config, ctf) = processor.portable().unwrap();
        std::fs::write(source.join("config.ocio"), config).unwrap();
        std::fs::write(source.join("transform.ctf"), ctf).unwrap();
        let mut doc = veylvik_document::demo();
        doc.ocio.output = Some(veylvik_document::color_management::OcioTransform {
            config: "config.ocio".into(),
            source: "Input".into(),
            destination: "Output".into(),
            display: String::new(),
            view: String::new(),
        });
        let folder = temp.0.join("collected");
        let report = collect(&doc, &source, &folder).unwrap();
        std::fs::remove_dir_all(source).unwrap();
        let doc = Composition::load(&report.project).unwrap();
        let transform = doc.ocio.output.unwrap();
        let config =
            veylvik_color::Config::load(folder.join(transform.config).to_str().unwrap()).unwrap();
        let actual = config
            .processor(&transform.source, &transform.destination, "", "")
            .unwrap()
            .reference(sample)
            .unwrap();
        for (a, b) in actual.into_iter().zip(expected) {
            assert!((a - b).abs() < 0.00001, "{actual:?} vs {expected:?}");
        }
        assert!(verify(&folder).is_ok());
    }
    #[test]
    fn cancelled_streaming_copy_cleans_only_its_new_destination() {
        let temp = Temp::new();
        crate::save_png(&temp.0.join("source.png"), 1, 1, &[[1.; 4]]).unwrap();
        let mut doc = veylvik_document::demo();
        image(&mut doc, "source.png");
        let destination = temp.0.join("cancelled");
        assert!(
            collect_with_progress(&doc, &temp.0, &destination, |p| !p
                .phase
                .starts_with("Copying"))
            .is_err()
        );
        assert!(!destination.exists());
        assert!(temp.0.join("source.png").is_file());
    }
}
