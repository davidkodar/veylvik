//! Temporal boundaries resolve upstream branches independently, entirely on the GPU.
use crate::{GpuFrame, ImageInputs, Plan, RenderRequest, Renderer, Result, error};
use std::sync::Arc;
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
            let need = self.need(plan, branch, request.time, preview, inputs)?;
            for part in need.name() {
                match part {
                    crate::moments::Part::Word(word) => words.push(word),
                    crate::moments::Part::Moment(time) => of(time, &mut words, left)?,
                }
            }
            // A picture of its own size is no extra moment in time.
            if !need.counts() {
                *left = left.saturating_add(1);
            }
            // The transform's own files are not part of an OCIO branch's name.
            let transformed = !need.lasting();
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
            use crate::moments::Need;
            let frame = match self.need(plan, branch, request.time, work.preview, inputs)? {
                Need::Shutter { shutter, .. } => self.sample_shutter(
                    &branch.plan,
                    RenderRequest::full(request.time),
                    shutter,
                    |time| {
                        self.temporal_frame(&branch.plan, RenderRequest::full(time), inputs, work)
                    },
                )?,
                Need::At(time) => {
                    self.temporal_frame(&branch.plan, RenderRequest::full(time), inputs, work)?
                }
                Need::Flow {
                    first,
                    second,
                    settings,
                    ..
                } => {
                    let previous = work.cache_sources;
                    work.cache_sources = true;
                    let a = self.temporal_frame(
                        &branch.plan,
                        RenderRequest::full(first),
                        inputs,
                        work,
                    )?;
                    let b = match second {
                        Some(second) => self.temporal_frame(
                            &branch.plan,
                            RenderRequest::full(second),
                            inputs,
                            work,
                        )?,
                        None => None,
                    };
                    work.cache_sources = previous;
                    let Some(a) = a else {
                        return Ok(None);
                    };
                    if second.is_none() {
                        Some(a)
                    } else {
                        let Some(b) = b else {
                            return Ok(None);
                        };
                        let frame =
                            self.interpolate_flow(&a, &b, settings, &mut *work.cancelled)?;
                        if let Some(report) = frame.as_ref().and_then(|frame| frame.flow_report) {
                            work.report = Some(report);
                        }
                        frame
                    }
                }
                Need::Transformed { transform, time } => {
                    let Some(frame) =
                        self.temporal_frame(&branch.plan, RenderRequest::full(time), inputs, work)?
                    else {
                        return Ok(None);
                    };
                    Some(self.ocio_transform(&transform, inputs, &frame)?)
                }
                Need::History { grid, times } => {
                    // The emitter at earlier moments, from the cache where it can be.
                    let previous = work.cache_sources;
                    work.cache_sources = true;
                    let atlas = self.pack_history(&branch.plan, grid, times, |time| {
                        self.temporal_frame(&branch.plan, RenderRequest::full(time), inputs, work)
                    });
                    work.cache_sources = previous;
                    atlas?
                }
                Need::Resized(time) => {
                    // The branch at its own size, from the cache when nothing in it moved.
                    let previous = work.cache_sources;
                    work.cache_sources = true;
                    let frame =
                        self.temporal_frame(&branch.plan, RenderRequest::full(time), inputs, work);
                    work.cache_sources = previous;
                    // A picture of its own size is no extra moment in time: it does not
                    // use up the request's evaluations.
                    work.remaining = work.remaining.saturating_add(1);
                    frame?
                }
                Need::Frames {
                    node,
                    port,
                    now,
                    others,
                } => {
                    // The input at the frames the plugin names, then at its own.
                    let previous = work.cache_sources;
                    work.cache_sources = true;
                    for (other, at) in others {
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
                        resolved.insert_gpu(
                            crate::plan::ofx_frame_key(node, port, other),
                            Arc::new(made),
                        );
                    }
                    let frame =
                        self.temporal_frame(&branch.plan, RenderRequest::full(now), inputs, work);
                    work.cache_sources = previous;
                    frame?
                }
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
    /// The frames other than its own that OpenFX effect `node` needs of the input at
    /// `port`, asked of the effect host.
    pub(crate) fn ofx_other_frames(
        &self,
        plan: &Plan,
        node: veylvik_document::NodeId,
        port: &str,
        effect: &veylvik_document::nodes::OfxEffect,
        time: veylvik_document::Time,
        inputs: &ImageInputs,
    ) -> Result<Vec<f64>> {
        let (host, bundle) = self.effect_host_for(effect, inputs)?;
        let at = veylvik_document::nodes::OFX_PORTS
            .iter()
            .position(|p| *p == port);
        let clip = at
            .and_then(|k| effect.clips.get(k))
            .cloned()
            .unwrap_or_default();
        // The clips the effect's wired ports feed.
        let wired = plan.nodes.iter().find(|n| n.id == node);
        let connected: Vec<&str> = match wired {
            Some(wired) => veylvik_document::nodes::OFX_PORTS
                .iter()
                .zip(&effect.clips)
                .filter(|(p, c)| !c.is_empty() && wired.inputs.contains_key(**p))
                .map(|(_, c)| c.as_str())
                .collect(),
            None => vec![clip.as_str()],
        };
        let now = frame_of(time, plan.frame_rate)?;
        let needed = host.frames_needed(crate::effect_hook::FramesJob {
            effect,
            bundle: &bundle,
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
