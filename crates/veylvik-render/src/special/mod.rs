//! Special passes: the few node kinds the renderer cannot make with one compute dispatch (a 3D scene, an OpenFX effect, a flock simulation, particle shadow maps). Each lives in a module of its own and is reached through `run` and `words` only, so the pass loop and the frame cache stay generic and everything about one such node is in one place.
use crate::plan::{Pass, Plan};
use crate::{ImageInputs, Kernel, RenderRequest, Renderer, Result};
use std::collections::{BTreeMap, BTreeSet};
use veylvik_document::{NodeId, Time};

mod effect;
mod scene;
mod shadows;
mod simulation;

/// What a special pass works with: the frame being made and the loop's shared state.
pub(crate) struct PassCx<'a, 'p> {
    pub renderer: &'a Renderer,
    pub plan: &'a Plan,
    /// The pass's place in the plan.
    pub at: usize,
    pub pass: &'a Pass,
    pub time: Time,
    pub inputs: &'a ImageInputs,
    /// Every slot's buffer; the pass writes `buffers[pass.output]`.
    pub buffers: &'a [wgpu::Buffer],
    /// The plan's input pictures by key.
    pub sources: &'a BTreeMap<&'a str, wgpu::Buffer>,
    /// What each pass's result is made of, when known.
    pub ids: &'a [Option<u64>],
    pub visible: &'a BTreeSet<NodeId>,
    pub byte_size: u64,
    pub scene_view: Option<veylvik_scene::Camera>,
    pub scene_reflections: bool,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub resources: &'a mut crate::pool::FrameBuffers<'p>,
    /// Pictures made early for a later pass (a scene twin), by that pass's place.
    pub made: &'a mut [Option<wgpu::Buffer>],
}
/// Makes a special pass's picture into its output slot.
pub(crate) fn run(cx: &mut PassCx<'_, '_>) -> Result<()> {
    match cx.pass.kernel {
        Kernel::RenderScene => scene::run(cx),
        Kernel::External => effect::run(cx),
        Kernel::ParticleSimulation => simulation::run(cx),
        Kernel::ParticleShadows => shadows::run(cx),
        other => Err(crate::error(format!("{other:?} is not a special pass"))),
    }
}
/// Adds what decides a special pass's picture besides its node's settings to its cache
/// words (nothing for other passes).
pub(crate) fn words(
    plan: &Plan,
    pass: &Pass,
    request: RenderRequest,
    words: &mut Vec<u32>,
    value: &mut dyn FnMut(usize, &mut Vec<u32>),
) -> Result<()> {
    match pass.kernel {
        Kernel::RenderScene => scene::words(plan, pass, request, words, value),
        Kernel::External => effect::words(plan, pass, request, words, value),
        Kernel::ParticleSimulation => simulation::words(plan, pass, request, words, value),
        Kernel::ParticleShadows => shadows::words(plan, pass, request, words, value),
        _ => Ok(()),
    }
}
