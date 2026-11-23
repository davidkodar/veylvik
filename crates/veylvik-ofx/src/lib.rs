//! OpenFX plugins for Veylvik: found on disk, described and rendered in a helper process (`veylvik ofx-worker`) so that a plugin that crashes or hangs costs a restart of the helper, never the editor. Pixels are premultiplied scene-linear RGBA float, top row first.
mod client;
mod protocol;
mod worker;
pub use client::Worker;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
/// A curve setting's value at a key, as the host evaluates it for plugins.
pub use veylvik_interop::ofx::curve_value;
pub use worker::serve;

pub type Result<T> = std::result::Result<T, String>;
/// Subcommand of the `veylvik` binary that runs [`serve`].
pub const SUBCOMMAND: &str = "ofx-worker";
pub const FILTER: &str = "OfxImageEffectContextFilter";
pub const GENERAL: &str = "OfxImageEffectContextGeneral";
pub const GENERATOR: &str = "OfxImageEffectContextGenerator";

/// A plugin in a bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plugin {
    pub bundle: PathBuf,
    pub index: usize,
    pub identifier: String,
    pub version: [u32; 2],
    pub label: String,
    pub grouping: String,
    pub description: String,
    pub contexts: Vec<String>,
    /// It reads other frames of its inputs.
    #[serde(default)]
    pub temporal: bool,
}
impl Plugin {
    /// The context Veylvik runs it in: a filter if it can be one, else general, else a
    /// generator.
    pub fn context(&self) -> Option<&'static str> {
        [FILTER, GENERAL, GENERATOR]
            .into_iter()
            .find(|c| self.contexts.iter().any(|x| x == c))
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    /// The OpenFX type, e.g. `OfxParamTypeDouble`.
    pub kind: String,
    pub label: String,
    pub hint: String,
    pub parent: String,
    pub default: Vec<f64>,
    pub text: String,
    pub min: Vec<f64>,
    pub max: Vec<f64>,
    pub display_min: Vec<f64>,
    pub display_max: Vec<f64>,
    pub options: Vec<String>,
    pub secret: bool,
    /// A curve setting's default points ([key, value]) per curve, its key range and the
    /// curves' names.
    #[serde(default)]
    pub curves: Vec<Vec<[f64; 2]>>,
    #[serde(default)]
    pub range: [f64; 2],
    #[serde(default)]
    pub curve_labels: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub name: String,
    pub optional: bool,
    pub mask: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Description {
    pub plugin: Plugin,
    pub context: String,
    pub clips: Vec<Clip>,
    pub params: Vec<Param>,
}
impl Description {
    /// The clips a picture goes into (every clip but Output), in the plugin's order.
    pub fn inputs(&self) -> impl Iterator<Item = &Clip> {
        self.clips.iter().filter(|c| c.name != "Output")
    }
}
/// A setting's value for a render.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Numbers(Vec<f64>),
    Text(String),
    /// A curve setting's points per curve.
    Curves(Vec<Vec<[f64; 2]>>),
}
impl From<Value> for veylvik_interop::ofx::Value {
    fn from(v: Value) -> Self {
        match v {
            Value::Numbers(n) => Self::Numbers(n),
            Value::Text(t) => Self::Text(t),
            Value::Curves(c) => Self::Curves(c),
        }
    }
}
/// The folders searched for `.ofx.bundle`s: `OFX_PLUGIN_PATH` first, then the standard
/// Linux places.
pub fn folders() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::env::var_os("OFX_PLUGIN_PATH")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default();
    out.push("/usr/OFX/Plugins".into());
    out.push("/usr/local/OFX/Plugins".into());
    out
}
/// Every `.ofx.bundle` under `folders`, a few levels deep, in name order.
pub fn bundles(folders: &[PathBuf]) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if path.extension().is_some_and(|e| e == "bundle")
                && path.to_string_lossy().ends_with(".ofx.bundle")
            {
                out.push(path);
            } else if depth > 0 {
                walk(&path, depth - 1, out);
            }
        }
    }
    let mut out = Vec::new();
    for folder in folders {
        walk(folder, 3, &mut out);
    }
    out.sort();
    out.dedup();
    out
}
