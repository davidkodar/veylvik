//! Reading one file a composition renders from: footage (by its source key, with the file named beside a host's key as a fallback), a 3D model, Gaussian splats or a baked flock, always through file confinement (`veylvik_document::access`) and within a byte budget. The editor's and the headless loaders both read through here, so they confine and fall back alike.
use crate::{Result, flock, footage, media_error, model};
use std::path::Path;
use std::sync::Arc;
use veylvik_document::access::{self, Reach};
use veylvik_render::ImageInputs;

/// One file to read, as a composition names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wanted {
    /// Footage or an image by its source key; `fallback` is the file named beside a
    /// key only a host knows.
    Media {
        key: String,
        fallback: Option<String>,
    },
    Model(String),
    Splats(String),
    /// A baked flock: one that cannot be read is simulated instead.
    Flock(String),
}
impl Wanted {
    /// The name the inputs keep it under.
    pub fn name(&self) -> &str {
        match self {
            Self::Media { key, .. } => key,
            Self::Model(name) | Self::Splats(name) | Self::Flock(name) => name,
        }
    }
}
/// What was read, ready to go into the inputs.
pub enum Loaded {
    Media(footage::Media),
    Model(Arc<veylvik_scene::Model>),
    Splats(Arc<veylvik_splat::Splats>),
    Flock(Arc<flock::Bake>),
}
impl Loaded {
    /// Bytes it holds against the source budget.
    pub fn byte_size(&self) -> u64 {
        match self {
            Self::Media(media) => media.byte_size(),
            _ => 0,
        }
    }
    pub fn insert(self, inputs: &mut ImageInputs, name: &str) {
        match self {
            Self::Media(media) => media.insert(inputs, name),
            Self::Model(model) => inputs.insert_model(name, model),
            Self::Splats(splats) => inputs.insert_splats(name, splats),
            Self::Flock(bake) => inputs.insert_flock(name, bake),
        }
    }
}
/// Every file `plan` renders from.
pub fn wanted_by(plan: &veylvik_render::Plan) -> Vec<Wanted> {
    let mut out: Vec<Wanted> = plan
        .required_assets()
        .into_iter()
        .map(|key| Wanted::Media {
            fallback: plan.fallback(&key).map(str::to_owned),
            key: key.into_owned(),
        })
        .collect();
    out.extend(
        plan.required_models()
            .into_iter()
            .map(|m| Wanted::Model(m.into())),
    );
    out.extend(
        plan.required_splats()
            .into_iter()
            .map(|s| Wanted::Splats(s.into())),
    );
    out.extend(
        plan.required_flocks()
            .into_iter()
            .map(|f| Wanted::Flock(f.into())),
    );
    out
}
/// Every file the nodes of `composition` render from, whatever is shown now.
pub fn wanted(composition: &veylvik_document::Composition) -> Vec<Wanted> {
    use veylvik_document::Operation;
    let mut out = Vec::new();
    for node in composition.nodes.values() {
        if let Some(key) = node.operation.source_key() {
            let fallback = match &node.operation {
                Operation::Footage(footage) if !footage.fallback.is_empty() => {
                    Some(footage.fallback.clone())
                }
                _ => None,
            };
            out.push(Wanted::Media {
                key: key.into_owned(),
                fallback,
            });
        }
        match &node.operation {
            Operation::GaussianSplats(g) if !g.file.is_empty() => {
                out.push(Wanted::Splats(g.file.clone()))
            }
            Operation::Particles(p) if !p.bake.is_empty() => {
                out.push(Wanted::Flock(p.bake.clone()))
            }
            Operation::Model3d(m) if !m.model.is_empty() => {
                out.push(Wanted::Model(m.model.clone()))
            }
            _ => {}
        }
    }
    out
}
/// Reads `wanted` for a composition in `base`, only from where `reach` lets it, using
/// at most `budget` bytes. Footage whose key cannot be read (refused, missing or a key
/// only a host knows) is read from its fallback file when that may be read. A flock
/// bake that cannot be read is `Ok(None)`: it is simulated instead.
pub fn load(wanted: &Wanted, base: &Path, reach: &Reach, budget: u64) -> Result<Option<Loaded>> {
    let named = |name: &str| access::resolve(base, name, reach);
    Ok(Some(match wanted {
        Wanted::Media { key, fallback } => {
            let (file, _) = veylvik_document::split_source_key(key);
            let own = named(file).map_err(Into::into).and_then(|_| {
                footage::load_key(base, key, budget).map_err(|e| media_error(base, key, &*e).into())
            });
            match (own, fallback) {
                (Ok(media), _) => Loaded::Media(media),
                (Err(error), Some(fallback)) => match named(fallback) {
                    Ok(path) => Loaded::Media(
                        footage::load(&path, budget)
                            .map_err(|e| media_error(base, fallback, &*e))?,
                    ),
                    Err(_) => return Err(error),
                },
                (Err(error), None) => return Err(error),
            }
        }
        Wanted::Model(name) => {
            let path = named(name)?;
            Loaded::Model(Arc::new(
                model::load(&path).map_err(|e| format!("3D model {name}: {e}"))?,
            ))
        }
        Wanted::Splats(name) => {
            let path = named(name)?;
            let bytes = std::fs::read(&path).map_err(|e| format!("Gaussian splats {name}: {e}"))?;
            Loaded::Splats(Arc::new(
                veylvik_splat::parse(&bytes).map_err(|e| format!("Gaussian splats {name}: {e}"))?,
            ))
        }
        Wanted::Flock(name) => match named(name)
            .ok()
            .and_then(|path| flock::Bake::open(&path).ok())
        {
            Some(bake) => Loaded::Flock(Arc::new(bake)),
            None => return Ok(None),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_file_outside_the_folder_is_refused_and_its_fallback_inside_is_read() {
        let root = std::env::temp_dir().join(format!(
            "veylvik-inputs-{}",
            veylvik_document::NodeId::new_v4()
        ));
        let base = root.join("project");
        std::fs::create_dir_all(&base).unwrap();
        let png = |path: &Path| {
            let file = std::fs::File::create(path).unwrap();
            let mut encoder = png::Encoder::new(file, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[10, 20, 30, 255])
                .unwrap();
        };
        png(&root.join("outside.png"));
        png(&base.join("inside.png"));
        let within = Reach::Within(Vec::new());
        let outside = root.join("outside.png").to_string_lossy().into_owned();
        let only = Wanted::Media {
            key: outside.clone(),
            fallback: None,
        };
        let refused = load(&only, &base, &within, 1 << 20)
            .err()
            .unwrap()
            .to_string();
        assert!(
            refused.contains("outside the composition's folder"),
            "{refused}"
        );
        // Trusted, the same file is read.
        assert!(
            load(&only, &base, &Reach::Anywhere, 1 << 20)
                .unwrap()
                .is_some()
        );
        // Refused, but the file beside it is inside: that is read instead.
        let aside = Wanted::Media {
            key: outside,
            fallback: Some("inside.png".into()),
        };
        assert!(matches!(
            load(&aside, &base, &within, 1 << 20).unwrap(),
            Some(Loaded::Media(_))
        ));
        // A host's key nobody here knows, with a missing fallback: the key's own error.
        let lost = Wanted::Media {
            key: "host:clip-7".into(),
            fallback: Some("gone.png".into()),
        };
        assert!(load(&lost, &base, &within, 1 << 20).is_err());
        // A flock bake that cannot be read is simulated, not an error.
        assert!(
            load(
                &Wanted::Flock("../x.veyflock".into()),
                &base,
                &within,
                1 << 20
            )
            .unwrap()
            .is_none()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
