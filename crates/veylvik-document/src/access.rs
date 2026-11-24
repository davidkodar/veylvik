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
    /// An OpenFX plugin bundle: native code, so it may also come from the system's
    /// plugin folders (see `plugin_folders`), and from nowhere else outside the folder.
    pub plugin: bool,
}
/// The folders OpenFX plugins are installed in: `OFX_PLUGIN_PATH` first, then the
/// standard Linux places. A plugin from one of them is the user's own installation.
pub fn plugin_folders() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::env::var_os("OFX_PLUGIN_PATH")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default();
    out.push("/usr/OFX/Plugins".into());
    out.push("/usr/local/OFX/Plugins".into());
    out
}
/// The file `named` is, if it may be read: a plugin installed in one of the plugin
/// folders always may; everything else as `resolve` says.
pub fn resolve_named(base: &Path, named: &Named, reach: &Reach) -> Result<PathBuf, String> {
    resolve_among(base, named, reach, &plugin_folders())
}
fn resolve_among(
    base: &Path,
    named: &Named,
    reach: &Reach,
    folders: &[PathBuf],
) -> Result<PathBuf, String> {
    if named.plugin && installed(&named.name, folders) {
        return Ok(PathBuf::from(&named.name));
    }
    resolve(base, &named.name, reach).map_err(|why| {
        if named.plugin {
            format!("{why}; a plugin may also come from the OpenFX plugin folders")
        } else {
            why
        }
    })
}
/// Whether `bundle` lies inside one of `folders` (links followed).
fn installed(bundle: &str, folders: &[PathBuf]) -> bool {
    let Ok(real) = Path::new(bundle).canonicalize() else {
        return false;
    };
    folders
        .iter()
        .filter_map(|folder| folder.canonicalize().ok())
        .any(|folder| real.starts_with(folder))
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
/// Every file `composition` names, in node order. This is the one list of what a
/// composition reads: confinement, the untrusted-project warning and collecting a
/// project all go by it (a test checks every file setting of every node type is here).
pub fn named(composition: &Composition) -> Vec<Named> {
    let mut names = Vec::new();
    let mut add = |node: Option<NodeId>, label: &str, name: &str, plugin: bool| {
        let name = name.trim();
        if !name.is_empty() && !name.starts_with("ocio://") && !name.starts_with("builtin:") {
            names.push(Named {
                node,
                label: label.to_owned(),
                name: name.to_owned(),
                plugin,
            });
        }
    };
    for transform in [&composition.ocio.view, &composition.ocio.output]
        .into_iter()
        .flatten()
    {
        add(None, "Colour view", &transform.config, false);
    }
    for (id, node) in &composition.nodes {
        let (id, label) = (Some(*id), node.label.as_str());
        match &node.operation {
            Operation::Footage(footage) => {
                add(id, label, crate::split_source_key(&footage.asset).0, false);
                add(id, label, &footage.fallback, false);
            }
            Operation::Sound(sound) => add(id, label, &sound.file, false),
            Operation::Model3d(model) => add(id, label, &model.model, false),
            Operation::Ocio(ocio) => add(id, label, &ocio.transform.config, false),
            Operation::GaussianSplats(splats) => add(id, label, &splats.file, false),
            Operation::Particles(particles) => add(id, label, &particles.bake, false),
            Operation::OfxEffect(effect) => add(id, label, &effect.bundle, true),
            other => {
                if let Some(asset) = other.asset() {
                    add(id, label, asset, false);
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
        if let Err(why) = resolve_named(base, &named, reach) {
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
            resolve_named(base, &named, &Reach::Within(Vec::new()))
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
    #[test]
    fn every_file_setting_of_every_node_type_is_on_the_list() {
        use crate::registry::{ParameterKind, TYPES};
        for kind in TYPES {
            for parameter in kind.parameters {
                if !matches!(
                    parameter.kind,
                    ParameterKind::FilePath | ParameterKind::Asset
                ) {
                    continue;
                }
                let name = format!("media/{}-{}.file", kind.type_id, parameter.name);
                let mut op = kind.default_operation();
                op[parameter.name] = serde_json::json!(name);
                let operation: Operation = serde_json::from_value(op).unwrap();
                let mut composition =
                    crate::blank(4, 4, crate::Time { ticks: 24, rate: 1 }).unwrap();
                let node = crate::Node::new("Probe", operation, &[]);
                composition.nodes.insert(node.id, node);
                assert!(
                    named(&composition).iter().any(|n| n.name == name),
                    "{} {} is not on the list of files a composition reads",
                    kind.type_id,
                    parameter.name
                );
            }
        }
    }
    #[test]
    fn a_plugin_comes_from_the_plugin_folders_or_from_inside_the_project() {
        let root = std::env::temp_dir().join(format!("veylvik-plugins-{}", NodeId::new_v4()));
        let (base, folder, elsewhere) = (
            root.join("project"),
            root.join("plugins"),
            root.join("downloads"),
        );
        for dir in [&base, &folder, &elsewhere] {
            std::fs::create_dir_all(dir.join("Fx.ofx.bundle")).unwrap();
        }
        let plugin = |name: String| Named {
            node: None,
            label: "Effect".into(),
            name,
            plugin: true,
        };
        let within = Reach::Within(Vec::new());
        let folders = [folder.clone()];
        let installed = plugin(folder.join("Fx.ofx.bundle").to_string_lossy().into_owned());
        assert!(resolve_among(&base, &installed, &within, &folders).is_ok());
        assert!(resolve_among(&base, &plugin("Fx.ofx.bundle".into()), &within, &folders).is_ok());
        // A bundle anywhere else is native code from outside: refused unless trusted.
        let stray = plugin(
            elsewhere
                .join("Fx.ofx.bundle")
                .to_string_lossy()
                .into_owned(),
        );
        let why = resolve_among(&base, &stray, &within, &folders).unwrap_err();
        assert!(why.contains("plugin folders"), "{why}");
        assert!(resolve_among(&base, &stray, &Reach::Anywhere, &folders).is_ok());
        // A plain file is never let through for being in a plugin folder.
        let file = Named {
            plugin: false,
            ..installed
        };
        assert!(resolve_among(&base, &file, &within, &folders).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
