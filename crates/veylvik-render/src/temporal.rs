//! Temporal boundaries resolve upstream branches independently, entirely on the GPU.
use crate::{GpuFrame, ImageInputs, Plan, RenderRequest, Renderer, Result, Shutter, error};
use std::sync::Arc;
use veylvik_document::Operation;
struct Work<'a> {
    remaining: u32,
    preview: bool,
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
        self.temporal_frame(
            plan,
            request,
            inputs,
            &mut Work {
                remaining: 256,
                preview,
                cancelled: &mut cancelled,
            },
        )
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
