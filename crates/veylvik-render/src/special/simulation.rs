//! A flock's state at the frame, simulated up to the frame (or read from its bake), copied into the pass's output.
use crate::*;
use veylvik_document::Operation;

pub(super) fn run(cx: &mut super::PassCx<'_, '_>) -> Result<()> {
    let this = cx.renderer;
    let (plan, scheduled, time, inputs, buffers) =
        (cx.plan, cx.pass, cx.time, cx.inputs, cx.buffers);
    let encoder: &mut wgpu::CommandEncoder = &mut *cx.encoder;
    // A flock's state at the frame, simulated up to it.
    let bake = match &plan.nodes[scheduled.node].operation {
        Operation::Particles(p) if !p.bake.is_empty() => inputs.flock(&p.bake).cloned(),
        _ => None,
    };
    // A flock born on its emitter: the emitter as it was at the flock's start, bound as
    // the simulation's first input (see `passes_for` in nodes/particles.rs).
    let emitter = match &scheduled.inputs[0] {
        crate::plan::Input::Asset(key) => cx.sources.get(key.as_str()),
        _ => None,
    };
    let (state, bytes) = this.simulate(plan, scheduled.node, time, bake.as_deref(), emitter)?;
    let output = &buffers[scheduled.output];
    encoder.clear_buffer(output, 0, None);
    encoder.copy_buffer_to_buffer(&state, 0, output, 0, bytes.min(output.size()));
    Ok(())
}

/// What decides this pass's picture besides its node's settings, for the frame cache.
pub(super) fn words(
    plan: &crate::plan::Plan,
    pass: &crate::plan::Pass,
    _request: RenderRequest,
    words: &mut Vec<u32>,
    _value: &mut dyn FnMut(usize, &mut Vec<u32>),
) -> Result<()> {
    // A flock at a frame is made of its whole path there: key all it depends on.
    let key = crate::simulation::key(plan, pass.node);
    words.extend([7, key as u32, (key >> 32) as u32]);
    Ok(())
}
