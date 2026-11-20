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
