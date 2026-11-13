//! Subject analysis: renders the source node frame by frame, has the worker segment and track it,
//! and writes the matte as a PNG16 sequence with a `.veyseq` manifest. Runs once, like tracking.
use crate::{Point, Result, matte, worker::Worker};
use serde::Serialize;
use std::path::{Path, PathBuf};
use veylvik_document::{Command, Composition, Node, NodeId, Operation, Time};
use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer};

/// Longest analysis, as for tracking.
pub const MAX_FRAMES: i64 = 4096;
pub struct Request {
    pub points: Vec<Point>,
    /// The prompted frame, then the frame tracking runs to (either side, or equal for one frame).
    pub frames: [i64; 2],
    /// Also track backward from the prompted frame to this earlier frame.
    pub back_to: Option<i64>,
}
#[derive(Debug, Serialize)]
pub struct Outcome {
    pub first_frame: i64,
    pub last_frame: i64,
    pub frames: usize,
    /// Frames where the model judged the subject absent (empty matte).
    pub absent: usize,
    pub manifest: PathBuf,
    pub milliseconds_per_frame: f64,
    pub cancelled: bool,
}
/// The frames to analyse, each with whether it carries the prompt, in processing order.
pub fn schedule(request: &Request, frame_count: u32) -> Result<Vec<(i64, bool)>> {
    crate::validate_points(&request.points)?;
    let last = i64::from(frame_count) - 1;
    let [prompt, end] = request.frames;
    if [prompt, end].iter().any(|f| !(0..=last).contains(f)) {
        return Err(format!("frames must be within 0..={last}").into());
    }
    let mut frames = vec![(prompt, true)];
    let step = if end >= prompt { 1 } else { -1 };
    frames.extend((1..=(end - prompt).abs()).map(|i| (prompt + i * step, false)));
    if let Some(back) = request.back_to {
        if back >= prompt || end < prompt || back < 0 {
            return Err("back_to must precede the prompted frame, and the end follow it".into());
        }
        // Backward runs restart from the prompt with an empty memory, as upstream reverses.
        frames.push((prompt, true));
        frames.extend((back..prompt).rev().map(|f| (f, false)));
    }
    if frames.len() as i64 > MAX_FRAMES + 1 {
        return Err(format!("an analysis is limited to {MAX_FRAMES} frames").into());
    }
    Ok(frames)
}
/// File name of composition frame `frame` in a matte directory.
pub fn file_name(frame: i64) -> String {
    format!("matte_{frame:05}.png")
}
/// Analyses `plan` (the source node) and writes `dir/matte_NNNNN.png` plus `dir/subject.veyseq`.
/// `progress(done, total)` returns false to stop; frames finished so far are kept.
#[allow(clippy::too_many_arguments)]
pub fn analyse(
    renderer: &Renderer,
    plan: &Plan,
    inputs: &ImageInputs,
    doc: &Composition,
    request: &Request,
    worker: &mut Worker,
    dir: &Path,
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Outcome> {
    let schedule = schedule(request, doc.frame_count)?;
    std::fs::create_dir_all(dir)?;
    let (width, height) = (doc.width as usize, doc.height as usize);
    let started = std::time::Instant::now();
    let (mut done, mut absent, mut cancelled) = (Vec::new(), 0, false);
    for (index, &(frame, prompted)) in schedule.iter().enumerate() {
        if !progress(index, schedule.len()) {
            cancelled = true;
            break;
        }
        let time = Time::at_frame(frame, doc.frame_rate).map_err(|e| e.to_string())?;
        let rendered = renderer
            .render_frame(plan, RenderRequest::full(time), inputs)
            .map_err(|e| e.to_string())?;
        let input = matte::model_frame(&rendered.pixels, width, height)?;
        let mask = worker.frame(&input, prompted.then_some(request.points.as_slice()))?;
        // The second visit of the prompted frame only restarts memory for the backward run.
        if done.contains(&frame) {
            continue;
        }
        let alpha = if mask.present() {
            matte::alpha(&mask.logits, width, height)?
        } else {
            absent += 1;
            vec![0.0; width * height]
        };
        matte::write_png(&dir.join(file_name(frame)), doc.width, doc.height, &alpha)?;
        done.push(frame);
    }
    if done.is_empty() {
        return Err("analysis was cancelled before the first frame".into());
    }
    done.sort_unstable();
    // Keep the longest contiguous run that contains the prompted frame.
    let prompt = request.frames[0];
    let at = done
        .binary_search(&prompt)
        .map_err(|_| "the prompted frame was not analysed")?;
    let mut first = at;
    while first > 0 && done[first - 1] + 1 == done[first] {
        first -= 1;
    }
    let mut last = at;
    while last + 1 < done.len() && done[last + 1] == done[last] + 1 {
        last += 1;
    }
    let run = &done[first..=last];
    let manifest = dir.join("subject.veyseq");
    let files: Vec<String> = run.iter().map(|f| file_name(*f)).collect();
    let text = serde_json::to_vec_pretty(
        &serde_json::json!({"schema": 1, "frame_rate": doc.frame_rate, "files": files}),
    )?;
    let partial = manifest.with_extension("veyseq.partial");
    std::fs::write(&partial, text)?;
    std::fs::rename(&partial, &manifest)?;
    Ok(Outcome {
        first_frame: run[0],
        last_frame: *run.last().unwrap_or(&run[0]),
        frames: run.len(),
        absent,
        manifest,
        milliseconds_per_frame: started.elapsed().as_secs_f64() * 1000.0 / done.len() as f64,
        cancelled,
    })
}
/// Commands that add the matte as a Footage node starting at `first_frame`; `asset` is the
/// manifest path relative to the project.
pub fn matte_node(
    doc: &Composition,
    asset: String,
    first_frame: i64,
) -> Result<(NodeId, Vec<Command>)> {
    let Operation::Footage(mut footage) =
        serde_json::from_value(veylvik_document::registry::FOOTAGE.default_operation())?
    else {
        return Err("the footage default is not a footage node".into());
    };
    footage.asset = asset;
    footage.timing.start =
        Time::at_frame(first_frame, doc.frame_rate).map_err(|e| e.to_string())?;
    let node = Node::new("Subject matte", Operation::Footage(footage), &[]);
    let id = node.id;
    let commands = vec![Command::Put {
        node: Box::new(node),
    }];
    veylvik_document::Document::new(doc.clone())
        .and_then(|d| d.prepare(0, &commands).map(|_| ()))
        .map_err(|e| e.to_string())?;
    Ok((id, commands))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(frames: [i64; 2], back_to: Option<i64>) -> Request {
        Request {
            points: vec![Point {
                x: 0.5,
                y: 0.5,
                positive: true,
            }],
            frames,
            back_to,
        }
    }
    #[test]
    fn schedule_prompts_first_then_tracks_each_way_from_the_prompt() {
        assert_eq!(
            schedule(&request([2, 4], None), 10).unwrap(),
            [(2, true), (3, false), (4, false)]
        );
        assert_eq!(
            schedule(&request([2, 0], None), 10).unwrap(),
            [(2, true), (1, false), (0, false)]
        );
        assert_eq!(
            schedule(&request([2, 3], Some(0)), 10).unwrap(),
            [(2, true), (3, false), (2, true), (1, false), (0, false)]
        );
        assert_eq!(schedule(&request([5, 5], None), 10).unwrap(), [(5, true)]);
        assert!(schedule(&request([2, 10], None), 10).is_err());
        assert!(schedule(&request([2, 1], Some(0)), 10).is_err());
        let mut none = request([0, 1], None);
        none.points[0].positive = false;
        assert!(schedule(&none, 10).is_err());
    }
}
