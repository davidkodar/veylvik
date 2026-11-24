//! Depth maps of the particles' scene as its lights see it, drawn by the 3D renderer; without one loaded, a cleared buffer says there are none.
use crate::*;
use veylvik_document::Operation;

pub(super) fn run(cx: &mut super::PassCx<'_, '_>) -> Result<()> {
    let this = cx.renderer;
    let (plan, scheduled, time, inputs, buffers) =
        (cx.plan, cx.pass, cx.time, cx.inputs, cx.buffers);
    let encoder: &mut wgpu::CommandEncoder = &mut *cx.encoder;
    // Depth maps of the particles' scene as its lights see it, by the 3D renderer.
    // Without one loaded there are no maps (a cleared buffer says so): the
    // particles are lit, unshadowed.
    let Some(renderer) = this
        .scene_renderer
        .read()
        .map_err(|_| error("scene renderer lock poisoned"))?
        .clone()
    else {
        encoder.clear_buffer(&buffers[scheduled.output], 0, None);
        return Ok(());
    };
    let scene = scene_hook::scene_of(plan, scheduled.node, time)?;
    let lights = nodes::particles::shining(&scene.lights);
    let sheet = match &plan.nodes[scheduled.node].operation {
        Operation::Particles(p) => nodes::particles::sheet(p),
        _ => 64 * 64,
    };
    let side = nodes::particles::shadow_side(plan.width, plan.height, sheet, lights.len());
    renderer.shadows(scene_hook::ShadowJob {
        device: &this.device,
        queue: &this.queue,
        encoder: &mut *encoder,
        scene: &scene,
        lights: &lights,
        side,
        target: &buffers[scheduled.output],
        models: inputs.models(),
    })?;
    Ok(())
}

/// What decides this pass's picture besides its node's settings, for the frame cache.
pub(super) fn words(
    plan: &crate::plan::Plan,
    pass: &crate::plan::Pass,
    request: RenderRequest,
    words: &mut Vec<u32>,
    _value: &mut dyn FnMut(usize, &mut Vec<u32>),
) -> Result<()> {
    // Depth maps of the scene the particles are lit in: key what it holds.
    words.push(6);
    words.extend(crate::scene_hook::scene_fingerprint(
        plan,
        pass.node,
        request.time,
    )?);
    Ok(())
}
