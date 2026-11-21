//! Which files a composition may read. A composition names files (footage, images, LUTs, sounds, 3D models, colour configurations); one that came from someone else must not make Veylvik read any file the user can, so every such name goes through `resolve`, which confines it to the composition's own folder unless the composition is trusted.
use crate::{Composition, NodeId, Operation};
use std::path::{Component, Path, PathBuf};

/// Where a composition's files may be.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Reach {
    /// Anywhere: the artist's own work, or a composition they chose to trust.
    #[default]
    Anywhere,
    /// Inside the composition's folder, and also the names listed, which the artist
    /// added themselves in this session (an import of a file from elsewhere).
    Within(Vec<String>),
}
/// A file a composition names, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    /// The node that names it; none for the composition's own colour view.
    pub node: Option<NodeId>,
    /// The node's label, or what names it.
    pub label: String,
    /// The name as the composition writes it.
    pub name: String,
}
/// The file `name` is, for a composition in `base`, if it may be read: a name that
/// leaves the folder (absolute, with `..`, through a link that leads out) is refused
/// unless `reach` lets it. The answer says why when it refuses.
pub fn resolve(base: &Path, name: &str, reach: &Reach) -> Result<PathBuf, String> {
    let joined = base.join(name);
    let Reach::Within(also) = reach else {
        return Ok(joined);
    };
    if also.iter().any(|allowed| allowed == name) {
        return Ok(joined);
    }
    let path = Path::new(name);
    if name.is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Err(format!(
            "{name} is outside the composition's folder; only files inside it are read"
        ));
    }
    // A link inside the folder may lead out: what it leads to has to be inside too.
    if joined.exists() {
        let (Ok(real), Ok(folder)) = (joined.canonicalize(), base.canonicalize()) else {
            return Err(format!("{name} could not be checked"));
        };
        if !real.starts_with(&folder) {
            return Err(format!(
                "{name} leads outside the composition's folder; only files inside it are read"
            ));
        }
    }
    Ok(joined)
}
/// Every file `composition` names, in node order.
pub fn named(composition: &Composition) -> Vec<Named> {
    let mut names = Vec::new();
    let mut add = |node: Option<NodeId>, label: &str, name: &str| {
        let name = name.trim();
        if !name.is_empty() && !name.starts_with("ocio://") && !name.starts_with("builtin:") {
            names.push(Named {
                node,
                label: label.to_owned(),
                name: name.to_owned(),
            });
        }
    };
    for transform in [&composition.ocio.view, &composition.ocio.output]
        .into_iter()
        .flatten()
    {
        add(None, "Colour view", &transform.config);
    }
    for (id, node) in &composition.nodes {
        let label = node.label.as_str();
        match &node.operation {
            Operation::Footage(footage) => {
                add(Some(*id), label, crate::split_source_key(&footage.asset).0);
                add(Some(*id), label, &footage.fallback);
            }
            Operation::Sound(sound) => add(Some(*id), label, &sound.file),
            Operation::Model3d(model) => add(Some(*id), label, &model.model),
            Operation::Ocio(ocio) => add(Some(*id), label, &ocio.transform.config),
            other => {
                if let Some(asset) = other.asset() {
                    add(Some(*id), label, asset);
                }
            }
        }
    }
    names
}
/// Every file `composition` names may be read under `reach`; otherwise the first
/// refused, named by its node, with why.
pub fn check(composition: &Composition, base: &Path, reach: &Reach) -> Result<(), String> {
    for named in named(composition) {
        if let Err(why) = resolve(base, &named.name, reach) {
            return Err(format!("{}: {why}", named.label));
        }
    }
    Ok(())
}
/// The names in `composition` that confinement to `base` would refuse, with why.
pub fn outside(composition: &Composition, base: &Path) -> Vec<(Named, String)> {
    named(composition)
        .into_iter()
        .filter_map(|named| {
            resolve(base, &named.name, &Reach::Within(Vec::new()))
                .err()
                .map(|why| (named, why))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_that_leave_the_folder_are_refused_and_others_are_not() {
        let base = std::env::temp_dir().join(format!("veylvik-access-{}", NodeId::new_v4()));
        let inside = base.join("media");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::write(inside.join("clip.png"), b"x").unwrap();
        let within = Reach::Within(Vec::new());
        assert!(resolve(&base, "media/clip.png", &within).is_ok());
        assert!(resolve(&base, "./media/clip.png", &within).is_ok());
        assert!(resolve(&base, "media/not-there.png", &within).is_ok());
        for hostile in [
            "/etc/passwd",
            "../outside.png",
            "media/../../outside.png",
            "",
        ] {
            assert!(
                resolve(&base, hostile, &within).is_err(),
                "{hostile:?} was let through"
            );
        }
        // A link inside the folder that leads out of it.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc", inside.join("away")).unwrap();
            assert!(resolve(&base, "media/away/hostname", &within).is_err());
        }
        // What the artist added this session, and trusted compositions, go anywhere.
        assert!(
            resolve(
                &base,
                "/etc/passwd",
                &Reach::Within(vec!["/etc/passwd".into()])
            )
            .is_ok()
        );
        assert!(resolve(&base, "/etc/passwd", &Reach::Anywhere).is_ok());
        std::fs::remove_dir_all(base).unwrap();
    }
}
