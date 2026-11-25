//! OpenFX effects for the renderer: an effect host that runs each plugin in the `veylvik ofx-worker` helper, finding the plugin in its bundle by identifier once and keeping the helper (and its loaded plugins) for later frames.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use veylvik_document::nodes::{OfxEffect, OfxValue};
use veylvik_ofx::{Plugin, Value, Worker};
use veylvik_render::Error;
use veylvik_render::effect_hook::{EffectHost, EffectJob, FramesJob};

pub struct Runner {
    exe: Option<PathBuf>,
    worker: Mutex<Option<Worker>>,
    plugins: Mutex<HashMap<(String, String), Plugin>>,
}
/// The `veylvik` program: this one if it is that, else the one beside it (Studio).
pub fn program() -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    if me.file_name().is_some_and(|n| n == "veylvik") {
        return Some(me);
    }
    let beside = me.with_file_name("veylvik");
    beside.is_file().then_some(beside)
}
fn values_of(values: Vec<(String, OfxValue)>) -> Vec<(String, Value)> {
    values
        .into_iter()
        .map(|(name, value)| {
            let value = match value {
                OfxValue::Numbers(n) => Value::Numbers(n),
                OfxValue::Text(t) => Value::Text(t),
                OfxValue::Curves(c) => Value::Curves(c),
            };
            (name, value)
        })
        .collect()
}
impl Runner {
    pub fn new() -> Self {
        Self::with_program(program())
    }
    /// A runner whose helper is `exe ofx-worker` (tests name the program).
    pub fn with_program(exe: Option<PathBuf>) -> Self {
        Self {
            exe,
            worker: Mutex::new(None),
            plugins: Mutex::new(HashMap::new()),
        }
    }
    /// Runs `f` with the helper and the effect's plugin, found in its bundle once.
    fn with<T>(
        &self,
        effect: &OfxEffect,
        bundle: &std::path::Path,
        f: impl FnOnce(&mut Worker, &Plugin) -> Result<T, String>,
    ) -> Result<T, String> {
        let exe = self
            .exe
            .as_ref()
            .ok_or("OpenFX effects need the veylvik program beside this one")?;
        let mut worker = self
            .worker
            .lock()
            .map_err(|_| "the OpenFX helper lock is poisoned")?;
        let worker = worker.get_or_insert_with(|| Worker::new(exe));
        let key = (bundle.to_string_lossy().into_owned(), effect.plugin.clone());
        let known = self.plugins.lock().ok().and_then(|p| p.get(&key).cloned());
        let plugin = match known {
            Some(plugin) => plugin,
            None => {
                let (plugins, _) = worker.list(bundle)?;
                let plugin = plugins
                    .into_iter()
                    .find(|p| p.identifier == effect.plugin)
                    .ok_or_else(|| {
                        format!("{} has no image effect {}", effect.bundle, effect.plugin)
                    })?;
                if let Ok(mut known) = self.plugins.lock() {
                    known.insert(key, plugin.clone());
                }
                plugin
            }
        };
        f(worker, &plugin)
    }
}
impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}
fn failed(effect: &OfxEffect) -> impl FnOnce(String) -> Error + '_ {
    move |e| Error::Message(format!("OpenFX effect {}: {e}", effect.plugin))
}
impl EffectHost for Runner {
    fn render(
        &self,
        job: EffectJob<'_>,
        deliver: &mut dyn FnMut(&[f32]),
    ) -> veylvik_render::Result<()> {
        let (effect, bundle) = (job.effect, job.bundle);
        let values = values_of(job.values);
        let inputs: Vec<(&str, Option<f64>, &[f32])> = job
            .inputs
            .iter()
            .map(|(clip, at, p)| (*clip, *at, p.as_slice()))
            .collect();
        self.with(effect, bundle, |worker, plugin| {
            worker.render_with(
                plugin,
                effect.context.name(),
                [job.width, job.height],
                job.frame,
                job.duration,
                &values,
                &inputs,
                deliver,
            )?;
            Ok(())
        })
        .map_err(failed(effect))
    }
    fn frames_needed(&self, job: FramesJob<'_>) -> veylvik_render::Result<Vec<(String, Vec<f64>)>> {
        let (effect, bundle) = (job.effect, job.bundle);
        let values = values_of(job.values);
        self.with(effect, bundle, |worker, plugin| {
            worker.frames_needed(
                plugin,
                effect.context.name(),
                [job.width, job.height],
                job.frame,
                job.duration,
                &values,
                &job.connected,
            )
        })
        .map_err(failed(effect))
    }
}
