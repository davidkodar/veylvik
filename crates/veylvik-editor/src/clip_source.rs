//! A composition as a source of frames for a host: any frame, at any size, written into a buffer of the host's on the shared device. It needs no editor on screen, and may be asked from any thread while the editor draws; the same composition always gives the same full-size picture.
use crate::project::Project;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use veylvik_document::{Composition, Time};
use veylvik_render::{HostPixels, ImageInputs, Plan, RenderRequest, Renderer};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Made {
    plan: Arc<Plan>,
    inputs: ImageInputs,
    size: [u32; 2],
    frame_rate: Time,
    frames: u32,
}
pub struct ClipSource {
    renderer: Arc<Renderer>,
    assets: PathBuf,
    host: Option<Arc<dyn crate::host::Host>>,
    made: Mutex<Arc<Made>>,
}
fn make(
    composition: Composition,
    assets: &std::path::Path,
    host: &Option<Arc<dyn crate::host::Host>>,
) -> Result<Made> {
    let (size, frame_rate, frames) = (
        [composition.width, composition.height],
        composition.frame_rate,
        composition.frame_count,
    );
    // The project finds every source: the host's own media through the host, the rest on disk.
    let mut project = Project::hosted_by(composition, assets.to_owned(), host.clone())?;
    if let Some((asset, why)) = project.unavailable_assets().into_iter().next() {
        return Err(format!("{asset}: {why}").into());
    }
    Ok(Made {
        plan: project.plan()?,
        inputs: project.inputs.clone(),
        size,
        frame_rate,
        frames,
    })
}
impl ClipSource {
    /// `renderer` is the one the host's device belongs to; an editor's own
    /// ([`crate::Editor::renderer`]) shares its memory budget and caches.
    /// `host` is asked for the media the composition names by the host's keys; it needs
    /// no editor to exist, and each clip source may have its own.
    pub fn new(
        renderer: Arc<Renderer>,
        composition: Composition,
        assets: PathBuf,
        host: Option<Arc<dyn crate::host::Host>>,
    ) -> Result<Self> {
        let made = make(composition, &assets, &host)?;
        needs_hosts(&renderer, &made.plan);
        Ok(Self {
            renderer,
            assets,
            host,
            made: Mutex::new(Arc::new(made)),
        })
    }
    /// The composition changed: frames asked for from now on are of this one. A render
    /// already running finishes with the one it began with.
    pub fn set_composition(&self, composition: Composition) -> Result<()> {
        let made = Arc::new(make(composition, &self.assets, &self.host)?);
        needs_hosts(&self.renderer, &made.plan);
        *self.made.lock().map_err(|_| "clip source unavailable")? = made;
        Ok(())
    }
    /// Compile now every GPU program the composition uses, so its first frame does not
    /// wait for them: a driver's first compile of one can take many seconds. Meant for
    /// a thread of the host's, before frames are asked for; renders may go on meanwhile.
    /// The 3D programs are compiled when the 3D renderer is made, which happens when the
    /// clip source is made or given a composition that films a scene. Returns how many
    /// kernels it compiled (or found compiled).
    pub fn compile(&self) -> Result<usize> {
        let made = self.made()?;
        Ok(self.renderer.compile_for(&made.plan)?)
    }
    fn made(&self) -> Result<Arc<Made>> {
        Ok(self
            .made
            .lock()
            .map_err(|_| "clip source unavailable")?
            .clone())
    }
    /// The composition's own size, its frame rate and its length in frames.
    pub fn format(&self) -> Result<([u32; 2], Time, u32)> {
        let made = self.made()?;
        Ok((made.size, made.frame_rate, made.frames))
    }
    /// The frame at `time` (exact composition time), `size` pixels, written into the
    /// host's storage buffer as the host stores pixels; done when this returns. The
    /// composition's own size gives its render exactly. A smaller size is the same
    /// picture scaled down: it is rendered whole and then reduced, so it is no faster yet.
    pub fn render(
        &self,
        time: Time,
        size: [u32; 2],
        target: &wgpu::Buffer,
        pixels: &HostPixels,
    ) -> Result<()> {
        let made = self.made()?;
        // Through the renderer's kept pictures: a frame asked for again, by this clip, the
        // editor or another clip of the same composition, is not made again.
        let mut frame =
            self.renderer
                .render_gpu_cached(&made.plan, RenderRequest::full(time), &made.inputs)?;
        // Halve while the picture is still at least twice the size wanted, then the rest
        // in one step, so every source pixel counts.
        for divisor in [4, 2] {
            if frame.width() / divisor >= size[0] && frame.height() / divisor >= size[1] {
                frame = self.renderer.reduce_preview(frame, divisor)?;
                break;
            }
        }
        self.renderer
            .frame_to_host_sized(&frame, target, size, pixels)?;
        Ok(())
    }
}

/// A composition that films a 3D scene needs the 3D renderer, and one with OpenFX effects
/// the effect host: a host that has no editor open has neither, and those nodes would
/// fail. Each is made once per renderer.
fn needs_hosts(renderer: &Renderer, plan: &Plan) {
    if plan.runs_effects() {
        renderer.ensure_effect_host(|| Arc::new(veylvik_io::ofx::Runner::new()));
    }
    if plan.films_scenes() {
        renderer.ensure_scene_renderer(|| {
            Arc::new(veylvik_render3d::Engine::with_cache(
                renderer.device(),
                renderer.pipeline_cache().as_ref(),
            ))
        });
    }
}
