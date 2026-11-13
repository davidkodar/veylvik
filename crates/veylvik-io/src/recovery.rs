//! Local recovery copies are separate from project files and retain absolute asset paths.
use crate::Result;
use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};
use veylvik_document::{Composition, NodeId};
#[derive(Clone)]
pub struct Store {
    directory: PathBuf,
}
#[derive(Clone)]
pub struct Entry {
    pub id: NodeId,
    pub modified: SystemTime,
}
/// Held for the lifetime of a running editor session. The OS releases it on crash.
pub struct Lease {
    _file: std::fs::File,
    path: PathBuf,
}
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
impl Store {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    fn path(&self, id: NodeId) -> PathBuf {
        self.directory.join(format!("{id}.veylvik"))
    }
    pub fn claim(&self, id: NodeId) -> std::io::Result<Lease> {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.directory)?;
        let path = self.directory.join(format!("{id}.lock"));
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)?;
        file.try_lock().map_err(std::io::Error::other)?;
        Ok(Lease { _file: file, path })
    }
    /// Active sessions are unavailable for recovery, including in other processes.
    pub fn available(&self, id: NodeId) -> std::io::Result<bool> {
        let path = self.directory.join(format!("{id}.lock"));
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(error),
        };
        match file.try_lock() {
            Ok(()) => Ok(true),
            Err(std::fs::TryLockError::WouldBlock) => Ok(false),
            Err(std::fs::TryLockError::Error(error)) => Err(error),
        }
    }
    /// Atomically replace only this session's recovery copy, never its source project.
    pub fn write(&self, id: NodeId, composition: &Composition, base: &Path) -> Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        let mut copy = composition.clone();
        for node in copy.nodes.values_mut() {
            if let Some(asset) = node.operation.asset_mut() {
                let path = Path::new(asset);
                if path.is_relative() {
                    let absolute = base.join(path);
                    if !absolute.is_absolute() {
                        return Err("recovery asset base must be absolute".into());
                    }
                    *asset = absolute
                        .to_str()
                        .ok_or("recovery image path is not UTF-8")?
                        .to_owned();
                }
            }
        }
        copy.validate()?;
        let bytes = serde_json::to_vec_pretty(&copy)?;
        if bytes.len() > veylvik_document::limits::MAX_DOCUMENT_BYTES {
            return Err("recovery composition exceeds 16 MiB".into());
        }
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.directory)?;
        if !self.path(id).exists() && self.list()?.len() >= 256 {
            return Err(
                "recovery directory already contains 256 copies; archive older copies first".into(),
            );
        }
        // Existing records from another session are never replaced by a new UUID.
        veylvik_document::atomic_write_with(&self.path(id), |file| {
            use std::{io::Write, os::unix::fs::PermissionsExt};
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            file.write_all(&bytes)
        })?;
        Ok(())
    }
    pub fn read(&self, id: NodeId) -> Result<Composition> {
        if !self.available(id)? {
            return Err("recovery copy belongs to a running editor".into());
        }
        let path = self.path(id);
        if !std::fs::symlink_metadata(&path)?.is_file() {
            return Err("recovery copy must be a regular file".into());
        }
        let composition = Composition::load(&path)?;
        if composition.nodes.values().any(|node| {
            node.operation
                .asset()
                .is_some_and(|asset| !Path::new(asset).is_absolute())
        }) {
            return Err("recovery image paths must be absolute".into());
        }
        Ok(composition)
    }
    pub fn remove(&self, id: NodeId) -> Result<()> {
        match std::fs::remove_file(self.path(id)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if self.available(id)? {
            match std::fs::remove_file(self.directory.join(format!("{id}.lock"))) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        match std::fs::File::open(&self.directory) {
            Ok(directory) => directory.sync_all()?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }
    /// Show newest records first. Unrelated files and symlinks are ignored.
    pub fn list(&self) -> Result<Vec<Entry>> {
        let directory = match std::fs::read_dir(&self.directory) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut entries = Vec::new();
        for item in directory {
            let item = item?;
            let path = item.path();
            if path.extension().is_none_or(|ext| ext != "veylvik") || !item.file_type()?.is_file() {
                continue;
            }
            let Some(id) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.parse().ok())
            else {
                continue;
            };
            if !self.available(id)? {
                continue;
            }
            entries.push(Entry {
                id,
                modified: item.metadata()?.modified()?,
            });
            if entries.len() > 256 {
                return Err(
                    "recovery directory has more than 256 copies; archive older copies first"
                        .into(),
                );
            }
        }
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.modified));
        Ok(entries)
    }
}
