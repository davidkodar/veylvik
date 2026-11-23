//! Temporal boundaries resolve upstream branches independently, entirely on the GPU.
use crate::{GpuFrame, ImageInputs, Plan, RenderRequest, Renderer, Result, Shutter, error};
use std::sync::Arc;
use veylvik_document::Operation;
struct Work<'a> {
    remaining: u32,
    preview: bool,
    cache_sources: bool,
    report: Option<crate::flow::FlowReport>,
    cancelled: &'a mut dyn FnMut() -> bool,
}
impl Renderer {
    /// What the picture `plan` gives for `request` is made of, its branches of other
    /// moments included: each branch stands in by what it is made of in turn, at the
    /// moments it reads. `left` bounds the evaluations, as rendering does.
    pub(crate) fn key(
        &self,
        plan: &Plan,
        request: RenderRequest,
        inputs: &ImageInputs,
        preview: bool,
        left: &mut u32,
    ) -> Result<crate::cache::Key> {
        if *left == 0 {
            return Err(error("too many moments to name this picture by"));
        }
        *left -= 1;
        let mut names = std::collections::BTreeMap::new();
        for branch in &plan.temporal {
            // What decides the branch besides its own pictures, then those pictures.
            let mut words: Vec<u32> = Vec::new();
            let mut lasting = true;
            let mut transformed = false;
            let mut of = |time: veylvik_document::Time, words: &mut Vec<u32>, left: &mut u32| {
                let key = self.key(
                    &branch.plan,
                    RenderRequest::full(time),
                    inputs,
                    preview,
                    left,
                )?;
                let name = crate::fnv(bytemuck::cast_slice(key.words()), 0);
                words.extend([name as u32, (name >> 32) as u32]);
                lasting &= key.portable();
                Ok::<_, crate::Error>(())
            };
            match &branch.operation {
                Operation::MotionBlur(blur) => {
                    let shutter = Shutter {
                        angle: blur.angle.at(request.time)?,
                        phase: blur.phase.at(request.time)?,
                        samples: if preview {
                            &blur.preview_samples
                        } else {
                            &blur.final_samples
                        }
                        .at(request.time)?
                        .round() as u32,
                        frame_rate: plan.frame_rate,
                    };
                    words.push(1);
                    for time in shutter.sample_times(request.time)? {
                        of(time, &mut words, left)?;
                    }
                }
                Operation::TimeRemap(remap) => {
                    words.push(2);
                    of(remap.source_at(request.time)?, &mut words, left)?;
                }
                Operation::OpticalFlow(flow) => {
                    let (first, second, fraction) = flow.source_pair(request.time)?;
                    words.extend([
                        3,
                        fraction.to_bits(),
                        flow.confidence.at(request.time)?.to_bits(),
                        u32::from(flow.diagnostic),
                        flow.quality as u32,
                        u32::from(preview),
                    ]);
                    of(first, &mut words, left)?;
                    if fraction != 0. || flow.diagnostic {
                        of(second, &mut words, left)?;
                    }
                }
                Operation::Ocio(node) => {
                    words.push(4);
                    let text = format!("{:?}", node.transform);
                    let name = crate::fnv(text.as_bytes(), 0);
                    words.extend([name as u32, (name >> 32) as u32]);
                    of(request.time, &mut words, left)?;
                    // The transform's own files are not part of this name.
                    transformed = true;
                }
                Operation::Particles(particles) => {
                    let grid = crate::history::grid(particles.history);
                    words.extend([5, grid]);
                    for time in crate::history::moments(
                        request.time,
                        particles.life.at(request.time)?,
                        grid,
                        plan.frame_rate,
                    )? {
                        of(time, &mut words, left)?;
                    }
                }
                Operation::OfxEffect(effect) => {
                    // Its own frame, then each other frame the plugin names.
                    words.push(7);
                    let others = self.ofx_other_frames(plan, &branch.key, effect, request.time)?;
                    of(request.time, &mut words, left)?;
                    for other in others {
                        let bits = other.to_bits();
                        words.extend([bits as u32, (bits >> 32) as u32]);
                        of(time_of(other, plan.frame_rate)?, &mut words, left)?;
                    }
                }
                Operation::OwnSize(_) => {
                    words.push(6);
                    of(request.time, &mut words, left)?;
                    // A picture of its own size is no extra moment in time.
                    *left = left.saturating_add(1);
                }
                _ => unreachable!("compiled temporal boundary"),
            }
            names.insert(
                branch.key.clone(),
                (
                    crate::fnv(bytemuck::cast_slice(&words), 0),
                    lasting && !transformed,
                ),
            );
        }
        let resolved = inputs.resolve_named(plan, request.time, Some(self), Some(&mut names))?;
        crate::cache::Key::with(plan, request, &resolved, &names)
    }
    /// Cancellable evaluation with preview shutter quality. Export APIs use final quality.
    /// At most 256 branch evaluations per request; nesting is limited when compiling.
    pub fn render_gpu_temporal(
        &self,
        plan: &Plan,
        request: RenderRequest,
        inputs: &ImageInputs,
        preview: bool,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Option<GpuFrame>> {
        let mut work = Work {
            remaining: 256,
            preview,
            cache_sources: false,
            report: None,
            cancelled: &mut cancelled,
        };
        let mut frame = self.temporal_frame(plan, request, inputs, &mut work)?;
        if let Some(frame) = &mut frame {
            frame.flow_report = work.report;
        }
        Ok(frame)
    }
    fn temporal_frame(
        &self,
        plan: &Plan,
        request: RenderRequest,
        inputs: &ImageInputs,
        work: &mut Work<'_>,
    ) -> Result<Option<GpuFrame>> {
        if (work.cancelled)() {
            return Ok(None);
        }
        if work.remaining == 0 {
            return Err(error(
                "Temporal work exceeds 256 evaluations; reduce samples or nested motion blur",
            ));
        }
        work.remaining -= 1;
        if work.cache_sources && plan.temporal.is_empty() {
            let frame = self.render_gpu_cached(plan, request, inputs)?;
            return Ok((!(work.cancelled)()).then(|| frame.as_ref().clone()));
        }
        let mut resolved = inputs.clone();
        for branch in &plan.temporal {
            let frame = match &branch.operation {
                Operation::MotionBlur(blur) => {
                    let shutter = Shutter {
                        angle: blur.angle.at(request.time)?,
                        phase: blur.phase.at(request.time)?,
                        samples: if work.preview {
                            &blur.preview_samples
                        } else {
                            &blur.final_samples
                        }
                        .at(request.time)?
                        .round() as u32,
                        frame_rate: plan.frame_rate,
                    };
                    self.sample_shutter(
                        &branch.plan,
                        RenderRequest::full(request.time),
                        shutter,
                        |time| {
                            self.temporal_frame(
                                &branch.plan,
                                RenderRequest::full(time),
                                inputs,
                                work,
                            )
                        },
                    )?
                }
                Operation::TimeRemap(remap) => {
                    let time = remap.source_at(request.time)?;
                    self.temporal_frame(&branch.plan, RenderRequest::full(time), inputs, work)?
                }
                Operation::OpticalFlow(flow) => {
                    let (first, second, fraction) = flow.source_pair(request.time)?;
                    let previous = work.cache_sources;
                    work.cache_sources = true;
                    let a = self.temporal_frame(
                        &branch.plan,
                        RenderRequest::full(first),
                        inputs,
                        work,
                    )?;
                    let b = if fraction == 0. && !flow.diagnostic {
                        None
                    } else {
                        self.temporal_frame(
                            &branch.plan,
                            RenderRequest::full(second),
                            inputs,
                            work,
                        )?
                    };
                    work.cache_sources = previous;
                    let Some(a) = a else {
                        return Ok(None);
                    };
                    if fraction == 0. && !flow.diagnostic {
                        Some(a)
                    } else {
                        let Some(b) = b else {
                            return Ok(None);
                        };
                        use veylvik_document::nodes::FlowQuality;
                        let edge = match (flow.quality, work.preview) {
                            (FlowQuality::Fast, _) => 512,
                            (FlowQuality::Balanced, true) => 768,
                            (FlowQuality::Fine, true) => 1024,
                            (FlowQuality::Balanced, false) => 1024,
                            (FlowQuality::Fine, false) => 1536,
                        };
                        let frame = self.interpolate_flow(
                            &a,
                            &b,
                            crate::flow::FlowSettings {
                                fraction,
                                analysis_edge: edge,
                                confidence: flow.confidence.at(request.time)?,
                                diagnostic: flow.diagnostic,
                            },
                            &mut *work.cancelled,
                        )?;
                        if let Some(report) = frame.as_ref().and_then(|frame| frame.flow_report) {
                            work.report = Some(report);
                        }
                        frame
                    }
                }
                Operation::Ocio(node) => {
                    let Some(frame) = self.temporal_frame(
                        &branch.plan,
                        RenderRequest::full(request.time),
                        inputs,
                        work,
                    )?
                    else {
                        return Ok(None);
                    };
                    Some(self.ocio_transform(&node.transform, inputs, &frame)?)
                }
                Operation::Particles(particles) => {
                    // The emitter at earlier moments, from the cache where it can be.
                    let grid = crate::history::grid(particles.history);
                    let times = crate::history::moments(
                        request.time,
                        particles.life.at(request.time)?,
                        grid,
                        plan.frame_rate,
                    )?;
                    let previous = work.cache_sources;
                    work.cache_sources = true;
                    let atlas = self.pack_history(&branch.plan, grid, times, |time| {
                        self.temporal_frame(&branch.plan, RenderRequest::full(time), inputs, work)
                    });
                    work.cache_sources = previous;
                    atlas?
                }
                Operation::OwnSize(_) => {
                    // The branch at its own size, from the cache when nothing in it moved.
                    let previous = work.cache_sources;
                    work.cache_sources = true;
                    let frame = self.temporal_frame(
                        &branch.plan,
                        RenderRequest::full(request.time),
                        inputs,
                        work,
                    );
                    work.cache_sources = previous;
                    // A picture of its own size is no extra moment in time: it does not
                    // use up the request's evaluations.
                    work.remaining = work.remaining.saturating_add(1);
                    frame?
                }
                Operation::OfxEffect(effect) => {
                    // The input at the frames the plugin names, then at its own.
                    let previous = work.cache_sources;
                    work.cache_sources = true;
                    for other in self.ofx_other_frames(plan, &branch.key, effect, request.time)? {
                        let at = time_of(other, plan.frame_rate)?;
                        let Some(made) = self.temporal_frame(
                            &branch.plan,
                            RenderRequest::full(at),
                            inputs,
                            work,
                        )?
                        else {
                            work.cache_sources = previous;
                            return Ok(None);
                        };
                        let port = branch.key.rsplit(':').next().unwrap_or_default();
                        let id = plan
                            .nodes
                            .iter()
                            .find(|n| {
                                n.operation.descriptor().type_id == "ofx_effect"
                                    && crate::plan::ofx_key(n.id, port) == branch.key
                            })
                            .map(|n| n.id);
                        if let Some(id) = id {
                            resolved.insert_gpu(
                                crate::plan::ofx_frame_key(id, port, other),
                                Arc::new(made),
                            );
                        }
                    }
                    let frame = self.temporal_frame(
                        &branch.plan,
                        RenderRequest::full(request.time),
                        inputs,
                        work,
                    );
                    work.cache_sources = previous;
                    frame?
                }
                _ => unreachable!("compiled temporal boundary"),
            };
            let Some(frame) = frame else {
                return Ok(None);
            };
            resolved.insert_gpu(branch.key.clone(), Arc::new(frame));
        }
        if (work.cancelled)() {
            return Ok(None);
        }
        let frame = self.render_gpu_resolved(plan, request, &resolved)?;
        Ok((!(work.cancelled)()).then_some(frame))
    }
}

impl Renderer {
    /// The frames other than its own that an OpenFX effect needs of the input `branch`
    /// feeds, asked of the effect host.
    pub(crate) fn ofx_other_frames(
        &self,
        plan: &Plan,
        branch_key: &str,
        effect: &veylvik_document::nodes::OfxEffect,
        time: veylvik_document::Time,
    ) -> Result<Vec<f64>> {
        let host = self.effect_host().ok_or_else(|| {
            error("OpenFX effects need the effect host, which this host has not loaded")
        })?;
        let port = branch_key.rsplit(':').next().unwrap_or_default();
        let at = veylvik_document::nodes::OFX_PORTS
            .iter()
            .position(|p| *p == port);
        let clip = at
            .and_then(|k| effect.clips.get(k))
            .cloned()
            .unwrap_or_default();
        let now = frame_of(time, plan.frame_rate)?;
        // The clips the effect's wired ports feed.
        let node = plan.nodes.iter().find(|n| {
            veylvik_document::nodes::OFX_PORTS
                .iter()
                .any(|p| crate::plan::ofx_key(n.id, p) == branch_key)
        });
        let connected: Vec<&str> = match node {
            Some(node) => veylvik_document::nodes::OFX_PORTS
                .iter()
                .zip(&effect.clips)
                .filter(|(p, c)| !c.is_empty() && node.inputs.contains_key(**p))
                .map(|(_, c)| c.as_str())
                .collect(),
            None => vec![clip.as_str()],
        };
        let needed = host.frames_needed(crate::effect_hook::FramesJob {
            effect,
            connected,
            values: effect.values_at(time)?,
            frame: now,
            width: plan.width,
            height: plan.height,
            duration: plan.frame_count,
        })?;
        Ok(needed
            .into_iter()
            .filter(|(c, _)| *c == clip)
            .flat_map(|(_, frames)| frames)
            .filter(|f| (f - now).abs() > 1e-6)
            .collect())
    }
}
/// The frame number of `time`.
pub(crate) fn frame_of(time: veylvik_document::Time, rate: veylvik_document::Time) -> Result<f64> {
    Ok(time.seconds()? * rate.seconds()?)
}
/// The time of frame number `frame`: exact for whole frames, rational to a thousandth
/// of a frame otherwise.
pub(crate) fn time_of(frame: f64, rate: veylvik_document::Time) -> Result<veylvik_document::Time> {
    if !frame.is_finite() {
        return Err(error(
            "an OpenFX effect asked for a frame that is not a number",
        ));
    }
    if frame.fract() == 0.0 {
        return Ok(veylvik_document::Time::at_frame(frame as i64, rate)?);
    }
    // seconds = frame / fps = frame * rate.rate / rate.ticks
    let ticks = (frame * 1000.0).round() as i64 * i64::from(rate.rate);
    let per = u32::try_from(
        rate.ticks
            .checked_mul(1000)
            .ok_or_else(|| error("frame rate too fine"))?,
    )
    .map_err(|_| error("frame rate too fine"))?;
    Ok(veylvik_document::Time::new(ticks, per)?)
}
