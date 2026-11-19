//! Steadies mattes over time: each frame's matte against its neighbours, moved to line up with it along the footage's own motion, so an edge solved frame by frame does not flicker and a patch that appears for one frame goes.
use crate::{Result, analysis::file_name, matte};
use std::path::Path;
use veylvik_document::{Composition, Time};
use veylvik_render::{GpuFrame, ImageInputs, Plan, RenderRequest, Renderer};

/// Long edge of the motion analysis: enough to follow a head or a hand, cheap at 4K.
const MOTION_EDGE: u32 = 768;
/// How far a neighbour's motion must be trusted before its value counts.
const TRUST: f32 = 0.5;

/// A matte as written by [`matte::write_png`].
fn read(path: &Path, pixels: usize) -> Result<Vec<f32>> {
    let image = image::open(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?
        .into_luma_alpha16();
    if image.width() as usize * image.height() as usize != pixels {
        return Err(format!("{} is not the composition's size", path.display()).into());
    }
    Ok(image
        .pixels()
        .map(|p| f32::from(p.0[1]) / 65535.0)
        .collect())
}
/// The middle of three values.
fn median(a: f32, b: f32, c: f32) -> f32 {
    a.max(b).min(a.min(b).max(c))
}
/// Rewrites the mattes of `frames` (consecutive composition frames, in order) in `dir`.
/// Every pixel becomes the middle value of the frame before, itself and the frame
/// after, the neighbours fetched along the motion of `plan`'s picture; where a
/// neighbour's motion cannot be trusted it can only fill (its value at the same place,
/// never below the frame's own), and at the ends of the run the frame is left as it is. `progress(done)` returns false to stop: frames already
/// rewritten stay steadied, the rest stay as they were. Returns the seconds it took.
pub fn steady(
    renderer: &Renderer,
    plan: &Plan,
    inputs: &ImageInputs,
    doc: &Composition,
    dir: &Path,
    frames: &[i64],
    progress: &mut dyn FnMut(usize) -> bool,
) -> Result<f64> {
    let started = std::time::Instant::now();
    if frames.len() < 3 {
        return Ok(0.0);
    }
    let pixels = doc.width as usize * doc.height as usize;
    let load = |frame: i64| -> Result<(std::sync::Arc<GpuFrame>, Vec<f32>)> {
        let time = Time::at_frame(frame, doc.frame_rate).map_err(|e| e.to_string())?;
        let picture = renderer
            .render_gpu_cached(plan, RenderRequest::full(time), inputs)
            .map_err(|e| e.to_string())?;
        Ok((picture, read(&dir.join(file_name(frame)), pixels)?))
    };
    // The untouched mattes of the frame before, this one and the one after.
    let mut before = None;
    let mut here = load(frames[0])?;
    let mut after = Some(load(frames[1])?);
    for (index, &frame) in frames.iter().enumerate() {
        if !progress(index) {
            break;
        }
        if let (Some(previous), Some(next)) = (&before, &after) {
            let carry = |other: &(std::sync::Arc<GpuFrame>, Vec<f32>)| {
                renderer
                    .carry_along_motion(&here.0, &other.0, &other.1, MOTION_EDGE)
                    .map_err(|e| e.to_string())
            };
            let (back, forth) = (carry(previous)?, carry(next)?);
            let steadied: Vec<f32> = here
                .1
                .iter()
                .enumerate()
                .map(|(i, &own)| {
                    // A neighbour whose motion is trusted gives its value from where
                    // this pixel was or will be. One whose motion is not (a fast, blurred
                    // hand) gives its value at this same place, and only to fill: a hole
                    // that opens for one frame in something opaque before and after
                    // closes, while a fast edge crossing the background is left alone.
                    let from = |carried: &[f32; 2], still: f32| {
                        if carried[1] >= TRUST {
                            carried[0]
                        } else {
                            own.max(still)
                        }
                    };
                    median(
                        from(&back[i], previous.1[i]),
                        own,
                        from(&forth[i], next.1[i]),
                    )
                })
                .collect();
            matte::write_png(
                &dir.join(file_name(frame)),
                doc.width,
                doc.height,
                &steadied,
            )?;
        }
        before = Some(here);
        let Some(next) = after.take() else { break };
        here = next;
        after = match frames.get(index + 2) {
            Some(&following) => Some(load(following)?),
            None => None,
        };
    }
    Ok(started.elapsed().as_secs_f64())
}
#[cfg(test)]
mod tests {
    #[test]
    fn the_middle_value_drops_a_one_frame_jump_and_keeps_a_steady_change() {
        assert_eq!(super::median(0.0, 1.0, 0.0), 0.0);
        assert_eq!(super::median(1.0, 0.0, 1.0), 1.0);
        assert_eq!(super::median(0.2, 0.5, 0.8), 0.5);
        assert_eq!(super::median(0.8, 0.5, 0.2), 0.5);
    }
}
