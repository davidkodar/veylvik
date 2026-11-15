//! Conservative folder search: filenames suggest; verified content can select.
//! Like Klipp's relinker, ties never silently choose a file. Legacy projects have
//! no saved content identity, so their name-only suggestions require selection.
use crate::{Result, collect};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug)]
pub struct Candidate {
    pub path: PathBuf,
    pub verified: bool,
}
#[derive(Clone, Debug)]
pub struct Proposal {
    pub asset: String,
    pub candidates: Vec<Candidate>,
    pub selected: Option<usize>,
}
/// No symlink-directory traversal, at most 100,000 entries and 32 directory levels.
pub fn scan(
    base: &Path,
    assets: &[String],
    root: &Path,
    mut progress: impl FnMut(usize) -> bool,
) -> Result<Vec<Proposal>> {
    let manifest = collect::manifest(base).ok();
    let expected: BTreeMap<_, _> = manifest
        .as_ref()
        .map(|m| {
            m.files
                .iter()
                .map(|r| (r.path.to_string_lossy().into_owned(), r))
                .collect()
        })
        .unwrap_or_default();
    let mut rows: Vec<_> = assets
        .iter()
        .map(|asset| Proposal {
            asset: asset.clone(),
            candidates: vec![],
            selected: None,
        })
        .collect();
    let root = root.canonicalize()?;
    let mut stack = vec![(root, 0)];
    let mut count = 0;
    while let Some((directory, depth)) = stack.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            count += 1;
            if count > 100_000 {
                return Err("Search exceeds 100,000 entries; choose a narrower folder".into());
            }
            if !progress(count) {
                return Err("Relink search cancelled".into());
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                if depth < 32 && !entry.file_name().to_string_lossy().starts_with('.') {
                    stack.push((entry.path(), depth + 1));
                }
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            for row in &mut rows {
                if Path::new(&row.asset).file_name() != Some(entry.file_name().as_os_str()) {
                    continue;
                }
                if row.candidates.len() >= 64 {
                    return Err("Too many same-name candidates; choose a narrower folder".into());
                }
                let path = entry.path();
                let verified = if let Some(expected) = expected.get(&row.asset) {
                    let mut file = std::fs::File::open(&path)?;
                    if file.metadata()?.len() != expected.bytes {
                        false
                    } else {
                        let mut hash = Sha256::new();
                        let mut buffer = vec![0; 1024 * 1024];
                        let mut bytes = 0;
                        loop {
                            if !progress(count) {
                                return Err("Relink search cancelled".into());
                            }
                            let n = file.read(&mut buffer)?;
                            if n == 0 {
                                break;
                            }
                            bytes += n as u64;
                            if bytes > expected.bytes {
                                break;
                            }
                            hash.update(&buffer[..n]);
                        }
                        bytes == expected.bytes
                            && format!("{:x}", hash.finalize()) == expected.sha256
                    }
                } else {
                    false
                };
                row.candidates.push(Candidate { path, verified });
            }
        }
    }
    for row in &mut rows {
        row.candidates
            .sort_by(|a, b| b.verified.cmp(&a.verified).then(a.path.cmp(&b.path)));
        if row.candidates.iter().filter(|c| c.verified).count() == 1 {
            row.selected = Some(0);
        }
    }
    Ok(rows)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_names_require_choice_and_content_evidence_is_unique() {
        let root = std::env::temp_dir().join(format!(
            "veylvik-relink-{}",
            veylvik_document::NodeId::new_v4()
        ));
        for folder in ["a", "b"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
            std::fs::write(root.join(folder).join("shot.png"), folder).unwrap();
        }
        let rows = scan(&root, &["shot.png".into()], &root, |_| true).unwrap();
        assert_eq!(rows[0].candidates.len(), 2);
        assert_eq!(rows[0].selected, None);
        let m = collect::Manifest {
            schema: 1,
            notes: vec![],
            files: vec![collect::Resource {
                path: "shot.png".into(),
                bytes: 1,
                sha256: format!("{:x}", Sha256::digest(b"a")),
            }],
        };
        std::fs::write(
            root.join(collect::MANIFEST),
            serde_json::to_vec(&m).unwrap(),
        )
        .unwrap();
        let rows = scan(&root, &["shot.png".into()], &root, |_| true).unwrap();
        assert_eq!(rows[0].selected, Some(0));
        assert_eq!(rows[0].candidates[0].path, root.join("a/shot.png"));
        assert!(scan(&root, &["shot.png".into()], &root, |_| false).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
