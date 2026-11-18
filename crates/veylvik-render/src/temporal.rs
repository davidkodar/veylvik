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
