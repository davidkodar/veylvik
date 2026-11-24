//! A Render scene pass: the scene drawn by the registered 3D renderer, its motion-blur moments averaged in place, and a twin depth (or picture) pass made in the same render when it can be.
use crate::*;

pub(super) fn run(cx: &mut super::PassCx<'_, '_>) -> Result<()> {
    let this = cx.renderer;
    let (plan, at_pass, scheduled, time, inputs) = (cx.plan, cx.at, cx.pass, cx.time, cx.inputs);
    let (buffers, sources, ids, visible, byte_size) =
        (cx.buffers, cx.sources, cx.ids, cx.visible, cx.byte_size);
    let (scene_view, scene_reflections) = (cx.scene_view, cx.scene_reflections);
    let encoder: &mut wgpu::CommandEncoder = &mut *cx.encoder;
    let resources = &mut *cx.resources;
    let made = &mut *cx.made;
    let renderer = this
        .scene_renderer
        .read()
        .map_err(|_| error("scene renderer lock poisoned"))?
        .clone()
        .ok_or_else(|| {
            error("Render scene needs the 3D renderer, which this host has not loaded")
        })?;
    let images: Vec<scene_hook::Picture<'_>> = scheduled
        .cards
        .iter()
        .filter_map(|(source, input)| match input {
            plan::Input::Value(slot) => {
                // Named by the pass that made it this frame.
                let made = (0..at_pass)
                    .rev()
                    .find(|&q| plan.passes[q].output == *slot)
                    .and_then(|q| ids[q]);
                Some((*source, &buffers[*slot], [plan.width, plan.height], made))
            }
            // A picture of its own size (an Own size node).
            plan::Input::Asset(key) => {
                let picture = inputs.source(key)?;
                Some((
                    *source,
                    &sources[key.as_str()],
                    [picture.region().width, picture.region().height],
                    Some(picture.identity()),
                ))
            }
            _ => None,
        })
        .collect();
    // Motion blur renders several moments and averages them in place.
    let moments = scene_hook::sample_times(plan, scheduled.node, time)?;
    // A picture and a depth picture of the same scene filmed the same way are
    // one render with two outputs: find the other, if writing it now is safe.
    // The other's picture goes into a frame of its own, if there is room for one.
    let twin = if this.share_scene_renders {
        scene_hook::twin(plan, at_pass, time, visible)?
    } else {
        None
    };
    let spare = match twin {
        Some(other) => resources
            .take(
                &this.device,
                byte_size,
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                "scene twin",
            )
            .ok()
            .map(|buffer| (other, buffer)),
        None => None,
    };
    for (k, at) in moments.into_iter().enumerate() {
        let (scene, camera, occlusion) = scene_hook::evaluate(plan, scheduled.node, at)?;
        let camera = scene_view.unwrap_or(camera);
        let copies = this.instances_for(plan, &scene, at, Some(inputs))?;
        renderer.render(SceneJob {
            device: &this.device,
            queue: &this.queue,
            encoder: &mut *encoder,
            scene: &scene,
            camera,
            occlusion,
            blend: 1.0 / (k + 1) as f32,
            time: at,
            width: plan.width,
            height: plan.height,
            depth: scene_hook::shows_depth(plan, scheduled.node),
            reflections: scene_reflections,
            images: &images,
            models: inputs.models(),
            splats: inputs.all_splats(),
            environment: &this.studio_environment,
            ltc: &this.ltc,
            instances: &copies,
            target: &buffers[scheduled.output],
            also: spare.as_ref().map(|(other, buffer)| {
                (
                    buffer,
                    scene_hook::shows_depth(plan, plan.passes[*other].node),
                )
            }),
        })?;
    }
    if let Some((other, buffer)) = spare {
        made[other] = Some(buffer);
    }
    Ok(())
}

/// What decides this pass's picture besides its node's settings, for the frame cache.
pub(super) fn words(
    plan: &crate::plan::Plan,
    pass: &crate::plan::Pass,
    request: RenderRequest,
    words: &mut Vec<u32>,
    value: &mut dyn FnMut(usize, &mut Vec<u32>),
) -> Result<()> {
    // Scene nodes have no passes of their own: key what the scene shows.
    words.extend(crate::scene_hook::fingerprint(
        plan,
        pass.node,
        request.time,
    )?);
    if request.scene_reflections {
        words.push(4);
    }
    if let Some(view) = request.scene_view {
        words.push(3);
        words.extend(view.world.iter().flatten().map(|v| v.to_bits()));
        words.extend([view.focal_length.to_bits(), view.sensor_width.to_bits()]);
    }
    for (_, input) in &pass.cards {
        if let crate::plan::Input::Value(slot) = input {
            words.push(2);
            value(*slot, words);
        }
    }
    Ok(())
}
