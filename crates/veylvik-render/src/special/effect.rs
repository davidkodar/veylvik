//! An OpenFX effect: what came before is finished on the card, the inputs (and other frames it reads) are read back, the effect host makes the picture and it is uploaded in the pass's place.
use crate::*;
use veylvik_document::Operation;

pub(super) fn run(cx: &mut super::PassCx<'_, '_>) -> Result<()> {
    let this = cx.renderer;
    let (plan, scheduled, time, inputs) = (cx.plan, cx.pass, cx.time, cx.inputs);
    let (buffers, sources, byte_size) = (cx.buffers, cx.sources, cx.byte_size);
    let encoder: &mut wgpu::CommandEncoder = &mut *cx.encoder;
    // No plugin chosen and nothing to pass on (else the plan passed it on).
    if matches!(&plan.nodes[scheduled.node].operation, Operation::OfxEffect(e) if e.plugin.is_empty())
    {
        encoder.clear_buffer(&buffers[scheduled.output], 0, None);
        return Ok(());
    }
    // Computed by the host: what came before is finished on the card, the
    // inputs are read back, and the host's picture is uploaded in its place.
    let host = this
        .effect_host
        .read()
        .map_err(|_| error("effect host lock poisoned"))?
        .clone()
        .ok_or_else(|| {
            error("OpenFX effects need the effect host, which this host has not loaded")
        })?;
    let Operation::OfxEffect(effect) = &plan.nodes[scheduled.node].operation else {
        return Err(error("an external pass belongs to an OpenFX effect"));
    };
    let mut reads = Vec::new();
    let read_back = |encoder: &mut wgpu::CommandEncoder, from: &wgpu::Buffer| {
        let read = this.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effect input"),
            size: byte_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(from, 0, &read, 0, byte_size);
        read
    };
    for (port, input) in scheduled.inputs.iter().enumerate() {
        let clip = effect.clips.get(port).map(String::as_str).unwrap_or("");
        match input {
            plan::Input::Value(slot) if !clip.is_empty() => {
                reads.push((clip, None, read_back(encoder, &buffers[*slot])));
            }
            // An input read as a branch (the plugin reads other frames): its
            // own frame, then each other frame it named.
            plan::Input::Asset(key) if !clip.is_empty() && sources.contains_key(key.as_str()) => {
                reads.push((clip, None, read_back(encoder, &sources[key.as_str()])));
                let name = veylvik_document::nodes::OFX_PORTS[port];
                let id = plan.nodes[scheduled.node].id;
                for other in this.ofx_other_frames(plan, id, name, effect, time)? {
                    let extra = plan::ofx_frame_key(id, name, other);
                    let Some(frame::Source::Gpu(made)) = inputs.source(&extra) else {
                        return Err(error(format!(
                            "frame {other} of {clip} was not made for the OpenFX effect"
                        )));
                    };
                    reads.push((clip, Some(other), read_back(encoder, made.buffer())));
                }
            }
            plan::Input::Empty | plan::Input::Value(_) | plan::Input::Asset(_) => {}
            _ => {
                return Err(error(
                    "an OpenFX effect reads only pictures of the frame's size",
                ));
            }
        }
    }
    let done = std::mem::replace(
        encoder,
        this.device.create_command_encoder(&Default::default()),
    );
    let submission = this.queue.submit([done.finish()]);
    let mut pictures = Vec::with_capacity(reads.len());
    for (clip, at, read) in &reads {
        pictures.push((
            *clip,
            *at,
            this.map_values::<f32>(read, submission.clone())?,
        ));
    }
    let mut delivered = None;
    host.render(
        effect_hook::EffectJob {
            effect,
            values: effect.values_at(time)?,
            frame: time.seconds()? * plan.frame_rate.seconds()?,
            width: plan.width,
            height: plan.height,
            duration: plan.frame_count,
            inputs: pictures,
        },
        &mut |picture| {
            delivered = Some(picture.len() as u64 * 4 == byte_size);
            if delivered == Some(true) {
                this.queue.write_buffer(
                    &buffers[scheduled.output],
                    0,
                    bytemuck::cast_slice(picture),
                );
            }
        },
    )?;
    match delivered {
        Some(true) => {}
        Some(false) => {
            return Err(error("the OpenFX effect gave a picture of the wrong size"));
        }
        None => return Err(error("the OpenFX effect gave no picture")),
    }
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
    let Operation::OfxEffect(effect) = &plan.nodes[pass.node].operation else {
        return Ok(());
    };
    // Which plugin, how it is wired and its settings at this moment make its picture.
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (
        &effect.bundle,
        &effect.plugin,
        effect.context.name(),
        &effect.clips,
    )
        .hash(&mut hasher);
    for (name, value) in effect.values_at(request.time)? {
        name.hash(&mut hasher);
        match value {
            veylvik_document::nodes::OfxValue::Numbers(n) => {
                n.iter().for_each(|v| v.to_bits().hash(&mut hasher))
            }
            veylvik_document::nodes::OfxValue::Text(t) => t.hash(&mut hasher),
            veylvik_document::nodes::OfxValue::Curves(c) => c.iter().for_each(|curve| {
                curve
                    .iter()
                    .flatten()
                    .for_each(|v| v.to_bits().hash(&mut hasher))
            }),
        }
    }
    let made = hasher.finish();
    words.extend([8, made as u32, (made >> 32) as u32]);
    Ok(())
}
