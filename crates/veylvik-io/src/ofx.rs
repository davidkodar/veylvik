//! OpenFX effects for the renderer: an effect host that runs each plugin in the `veylvik ofx-worker` helper, finding the plugin in its bundle by identifier once and keeping the helper (and its loaded plugins) for later frames.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use veylvik_document::nodes::OfxValue;
use veylvik_ofx::{Plugin, Value, Worker};
use veylvik_render::Error;
use veylvik_render::effect_hook::{EffectHost, EffectJob};

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
    fn run(&self, job: EffectJob<'_>, deliver: &mut dyn FnMut(&[f32])) -> Result<(), String> {
        let effect = job.effect;
        let exe = self
            .exe
            .as_ref()
            .ok_or("OpenFX effects need the veylvik program beside this one")?;
        let mut worker = self
            .worker
            .lock()
            .map_err(|_| "the OpenFX helper lock is poisoned")?;
        let worker = worker.get_or_insert_with(|| Worker::new(exe));
        let key = (effect.bundle.clone(), effect.plugin.clone());
        let known = self.plugins.lock().ok().and_then(|p| p.get(&key).cloned());
        let plugin = match known {
            Some(plugin) => plugin,
            None => {
                let (plugins, _) = worker.list(std::path::Path::new(&effect.bundle))?;
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
        let values: Vec<(String, Value)> = job
            .values
            .into_iter()
            .map(|(name, value)| {
                let value = match value {
                    OfxValue::Numbers(n) => Value::Numbers(n),
                    OfxValue::Text(t) => Value::Text(t),
                    OfxValue::Curves(c) => Value::Curves(c),
                };
                (name, value)
            })
            .collect();
        let inputs: Vec<(&str, &[f32])> = job
            .inputs
            .iter()
            .map(|(clip, p)| (*clip, p.as_slice()))
            .collect();
        worker.render_with(
            &plugin,
            effect.context.name(),
            [job.width, job.height],
            job.frame,
            &values,
            &inputs,
            deliver,
        )?;
        Ok(())
    }
}
impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}
impl EffectHost for Runner {
    fn render(
        &self,
        job: EffectJob<'_>,
        deliver: &mut dyn FnMut(&[f32]),
    ) -> veylvik_render::Result<()> {
        let plugin = job.effect.plugin.clone();
        self.run(job, deliver)
            .map_err(|e| Error::Message(format!("OpenFX effect {plugin}: {e}")))
    }
}
