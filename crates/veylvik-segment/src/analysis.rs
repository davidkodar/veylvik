//! Subject analysis: renders the source node frame by frame, has the worker segment and track it,
//! and writes the matte as a PNG16 sequence with a `.veyseq` manifest. Runs once, like tracking.
use crate::{Point, Result, edges, matte, worker::Worker, zoom};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
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
    /// Run a second, zoomed pass around the subject for small parts (feet, hands, hair).
    pub detail: bool,
    /// Matting model that refines the edge band (hair, soft edges), if any.
    pub matting: Option<&'static str>,
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
    /// Linear detail gain of the zoomed pass, or `None` when it was not used.
    pub zoom: Option<f64>,
    /// Matting model used on the edge band, and its time per frame.
    pub matting: Option<&'static str>,
    pub matting_milliseconds: f64,
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
/// A new folder for one analysis: `<parent>/Subject mattes/<source id prefix>[-n]`.
pub fn unique_dir(parent: &Path, source: NodeId) -> PathBuf {
    let folder = format!("Subject mattes/{}", &source.to_string()[..8]);
    let mut dir = parent.join(&folder);
    let mut n = 1;
    while dir.exists() {
        n += 1;
        dir = parent.join(format!("{folder}-{n}"));
    }
    dir
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
    let (fw, fh) = (width as f64, height as f64);
    let full = [0.0, 0.0, fw, fh];
    let started = std::time::Instant::now();
    let passes = if request.detail { 2 } else { 1 };
    let total = schedule.len() * passes;
    let render = |frame: i64| -> Result<Vec<[f32; 4]>> {
        let time = Time::at_frame(frame, doc.frame_rate).map_err(|e| e.to_string())?;
        Ok(renderer
            .render_frame(plan, RenderRequest::full(time), inputs)
            .map_err(|e| e.to_string())?
            .pixels)
    };
    // First pass, for the detail pass: where the subject is on every frame.
    let mut windows = None;
    if request.detail {
        let mut boxes = BTreeMap::new();
        for (index, &(frame, prompted)) in schedule.iter().enumerate() {
            if !progress(index, total) {
                return Err("Stopped while locating the subject; nothing was saved".into());
            }
            let input = matte::model_frame(&render(frame)?, width, height)?;
            let mask = worker.frame(&input, prompted.then_some(request.points.as_slice()))?;
            boxes.entry(frame).or_insert_with(|| {
                mask.present()
                    .then(|| zoom::bounds(&mask.logits, width, height))
                    .flatten()
            });
        }
        // The zoomed pass needs a marking point inside the prompted frame's window.
        windows = zoom::windows(&boxes, fw, fh).filter(|w| {
            zoom::map_points(&request.points, w[&request.frames[0]], fw, fh)
                .iter()
                .any(|p| p.positive)
        });
    }
    let gain = windows
        .as_ref()
        .and_then(|w| w.values().next())
        .map(|r| ((fw / (r[2] - r[0])) * (fh / (r[3] - r[1]))).sqrt());
    let offset = schedule.len() * (passes - 1);
    let (mut done, mut absent, mut cancelled) = (Vec::new(), 0, false);
    let mut matting_time = 0.0;
    for (index, &(frame, prompted)) in schedule.iter().enumerate() {
        if !progress(offset + index, total) {
            cancelled = true;
            break;
        }
        let rect = windows.as_ref().map_or(full, |w| w[&frame]);
        let pixels = render(frame)?;
        let input = matte::model_crop(&pixels, width, height, rect)?;
        let points = match &windows {
            Some(_) => zoom::map_points(&request.points, rect, fw, fh),
            None => request.points.clone(),
        };
        let mask = worker.frame(&input, prompted.then_some(points.as_slice()))?;
        // The second visit of the prompted frame only restarts memory for the backward run.
        if done.contains(&frame) {
            continue;
        }
        let mut alpha = if mask.present() {
            matte::alpha_crop(&mask.logits, width, height, rect)?
        } else {
            absent += 1;
            vec![0.0; width * height]
        };
        if let Some(model) = request.matting
            && let Some(band) = edges::band(&alpha, width)
            && let Some(crop) = edges::crop(&alpha, width, height, band * 2 + 16)
        {
            let started = std::time::Instant::now();
            let trimap = edges::trimap(&alpha, width, crop, band);
            let srgb = matte::srgb_pixels(&pixels);
            let (bytes, w, h) = edges::input(&srgb, width, crop, &trimap);
            let matted = worker.matte(model, &bytes, w, h)?;
            edges::merge(&mut alpha, width, crop, &trimap, &matted, w, h)?;
            matting_time += started.elapsed().as_secs_f64();
        }
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
        zoom: gain,
        matting: request.matting,
        matting_milliseconds: matting_time * 1000.0 / done.len() as f64,
    })
}
/// Nodes an analysis adds: the matte sequence, and the refined cutout of the source.
pub struct Added {
    pub matte: NodeId,
    pub cutout: NodeId,
    pub commands: Vec<Command>,
}
/// Commands that add the matte as a Footage node starting at `first_frame` (`asset` is the
/// manifest path relative to the project) and a Refine matte node that cuts the subject out of
/// `source` with it.
pub fn matte_nodes(
    doc: &Composition,
    source: NodeId,
    asset: String,
    first_frame: i64,
) -> Result<Added> {
    let Operation::Footage(mut footage) =
        serde_json::from_value(veylvik_document::registry::FOOTAGE.default_operation())?
    else {
        return Err("the footage default is not a footage node".into());
    };
    footage.asset = asset;
    footage.timing.start =
        Time::at_frame(first_frame, doc.frame_rate).map_err(|e| e.to_string())?;
    let matte = Node::new("Subject matte", Operation::Footage(footage), &[]);
    let refine: Operation =
        serde_json::from_value(veylvik_document::registry::REFINE_MATTE.default_operation())?;
    let cutout = Node::new(
        "Subject cutout",
        refine,
        &[("image", source), ("matte", matte.id)],
    );
    let added = Added {
        matte: matte.id,
        cutout: cutout.id,
        commands: vec![
            Command::Put {
                node: Box::new(matte),
            },
            Command::Put {
                node: Box::new(cutout),
            },
        ],
    };
    veylvik_document::Document::new(doc.clone())
        .and_then(|d| d.prepare(0, &added.commands).map(|_| ()))
        .map_err(|e| e.to_string())?;
    Ok(added)
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
            detail: true,
            matting: None,
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
    #[test]
    fn analysis_adds_the_matte_and_a_cutout_wired_to_the_source() {
        let rate = Time { ticks: 30, rate: 1 };
        let doc = veylvik_document::blank(64, 64, rate).unwrap();
        let added = matte_nodes(&doc, doc.output, "mattes/subject.veyseq".into(), 12).unwrap();
        let mut document = veylvik_document::Document::new(doc.clone()).unwrap();
        document
            .apply(document.revision(), &added.commands)
            .unwrap();
        let after = document.snapshot();
        let cutout = &after.nodes[&added.cutout];
        assert_eq!(cutout.inputs["image"], doc.output);
        assert_eq!(cutout.inputs["matte"], added.matte);
        let Operation::Footage(f) = &after.nodes[&added.matte].operation else {
            panic!("matte is footage")
        };
        assert_eq!(f.asset, "mattes/subject.veyseq");
        assert_eq!(f.timing.start.seconds().unwrap(), 0.4);
    }
}
