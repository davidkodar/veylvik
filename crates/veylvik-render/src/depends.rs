//! The files a plan's pictures depend on by path alone (a colour transform's configuration and lookup tables, imported 3D models, and what the sources say they read), and the names a picture goes by once those are counted: the one rule any host that keeps pictures (the preview's playback store, an editor hosting Veylvik) uses, so a picture is never mistaken for one made from other files.
use crate::{ImageInputs, Plan, RenderRequest, Renderer, Result, error};
use std::path::PathBuf;
use std::time::SystemTime;

/// One file as it was: its path, size and when it was last changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStamp {
    pub path: PathBuf,
    pub size: u64,
    pub modified: SystemTime,
}
impl FileStamp {
    fn of(path: PathBuf) -> Result<Self> {
        let meta =
            std::fs::metadata(&path).map_err(|e| error(format!("{}: {e}", path.display())))?;
        Ok(Self {
            size: meta.len(),
            modified: meta.modified().map_err(error)?,
            path,
        })
    }
}
/// The files a plan's pictures depend on, as they were when looked at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dependencies(Vec<FileStamp>);
impl Dependencies {
    /// Looks at every file `plan` reads by path alone. An error when one cannot be
    /// looked at (gone, say): nothing made from it can be named.
    pub fn of(plan: &Plan, inputs: &ImageInputs) -> Result<Self> {
        let mut paths = inputs.playback_dependencies().unwrap_or_default();
        for transform in plan.ocio_transforms() {
            let config_path = inputs.resolve_config(&transform.config)?;
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
        paths.extend(plan.required_models().into_iter().map(PathBuf::from));
        paths.sort();
        paths.dedup();
        Ok(Self(
            paths
                .into_iter()
                .map(FileStamp::of)
                .collect::<Result<_>>()?,
        ))
    }
    pub fn stamps(&self) -> &[FileStamp] {
        &self.0
    }
    /// Whether a file has changed or gone since it was looked at.
    pub fn changed(&self) -> bool {
        self.0
            .iter()
            .any(|stamp| FileStamp::of(stamp.path.clone()).ok().as_ref() != Some(stamp))
    }
}
impl Renderer {
    /// The names a picture goes by in a store that keeps pictures: within this session
    /// (its identity, with the viewer's colour transform), and in later sessions (its
    /// lasting identity with the files it depends on, when every input has a lasting
    /// name). `dependencies` are `Dependencies::of(plan, inputs)`, looked at once for
    /// the plan rather than for every picture.
    pub fn names(
        &self,
        plan: &Plan,
        request: RenderRequest,
        inputs: &ImageInputs,
        dependencies: &Dependencies,
    ) -> Result<(u64, Option<u64>)> {
        use std::hash::{Hash, Hasher};
        let (identity, lasting) = self.identities(plan, request, inputs)?;
        let view = format!("{:?}", plan.ocio_transforms());
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        identity.hash(&mut hasher);
        view.hash(&mut hasher);
        let lasting = lasting.map(|lasting| {
            let mut text = view;
            for stamp in dependencies.stamps() {
                text.push_str(&format!("{stamp:?}"));
            }
            crate::fnv(text.as_bytes(), lasting)
        });
        Ok((hasher.finish(), lasting))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_file_that_changes_or_goes_is_noticed() {
        let path = std::env::temp_dir().join(format!(
            "veylvik-depends-{}",
            veylvik_document::NodeId::new_v4()
        ));
        std::fs::write(&path, b"a").unwrap();
        let deps = Dependencies(vec![FileStamp::of(path.clone()).unwrap()]);
        assert!(!deps.changed());
        std::fs::write(&path, b"longer").unwrap();
        assert!(deps.changed());
        std::fs::remove_file(&path).unwrap();
        assert!(deps.changed());
    }
}
