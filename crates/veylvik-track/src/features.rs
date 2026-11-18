//! Feature tracks through footage for camera solving: corners over the whole frame, followed frame to frame on the GPU with a forward-backward check, replenished as they leave or fail.
use crate::planar::{PlanarTracker, Settings as Planar};
use crate::{Result, error};
use std::sync::Arc;
use veylvik_document::planar::{Homography, Point};
use veylvik_render::{GpuFrame, Renderer};

#[derive(Debug, Clone, Copy)]
pub struct Settings {
    /// Tracks kept alive at once (at most 256, one GPU batch).
    pub target: usize,
    pub patch_radius: u32,
    pub search_radius: u32,
    /// A match must come back to within this many pixels when tracked backwards.
    pub round_trip: f64,
    /// Lowest accepted patch correlation (-1..1).
    pub min_correlation: f64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            target: 220,
            patch_radius: 7,
            search_radius: 16,
            round_trip: 0.6,
            min_correlation: 0.85,
        }
    }
}
/// Every feature followed through the range, as (frame offset, pixel) pairs;
/// pixels are continuous image coordinates (0..width), frame offsets count from `first`.
#[derive(Debug, Clone, Default)]
pub struct Tracks {
    pub width: u32,
    pub height: u32,
    pub first: i64,
    pub frames: usize,
    pub tracks: Vec<Vec<(usize, [f64; 2])>>,
}
/// Called after each frame with (frames done, total); return false to cancel.
pub type Progress<'a> = dyn FnMut(usize, usize) -> bool + 'a;

struct Frame {
    image: Arc<GpuFrame>,
    levels: Vec<GpuFrame>,
}
/// A live track: where it is now, and the frame and point it is matched against.
#[derive(Clone, Copy)]
struct Live {
    id: usize,
    at: Point,
    anchor: usize,
    origin: Point,
}
/// Follow features through frames `start..=end` of `plan`'s output. Each frame is
/// matched from the one before (the estimate), then refined against the frame where
/// the track was anchored, so small matching errors do not add up into drift; a
/// track re-anchors when its look has changed too much to match its anchor.
#[allow(clippy::too_many_arguments)]
pub fn track(
    tracker: &PlanarTracker,
    renderer: &Renderer,
    plan: &veylvik_render::Plan,
    inputs: &veylvik_render::ImageInputs,
    [start, end]: [i64; 2],
    rate: veylvik_document::Time,
    settings: Settings,
    progress: &mut Progress<'_>,
) -> Result<Tracks> {
    if end <= start {
        return Err(error("A camera solve needs at least two frames"));
    }
    let total = (end - start + 1) as usize;
    let planar = Planar {
        patch_radius: settings.patch_radius,
        search_radius: settings.search_radius,
        tolerance_pixels: 3.0,
        reanchor_below: 0,
        screen_area: false,
        motion: crate::estimate::Motion::Perspective,
        refine: false,
        grid: 16,
    };
    let refine = Planar {
        search_radius: 3,
        ..planar
    };
    let load = |frame: i64| -> Result<Frame> {
        let time =
            veylvik_document::Time::at_frame(frame, rate).map_err(|e| error(e.to_string()))?;
        let image =
            renderer.render_gpu_cached(plan, veylvik_render::RenderRequest::full(time), inputs)?;
        let levels = tracker.pyramid(renderer, &image)?;
        Ok(Frame { image, levels })
    };
    let mut previous = load(start)?;
    let (w, h) = (previous.image.width(), previous.image.height());
    let size = [f64::from(w), f64::from(h)];
    let margin = f64::from(settings.patch_radius + settings.search_radius / 2 + 2);
    let inside =
        |p: Point| (0..2).all(|a| p[a] * size[a] > margin && p[a] * size[a] < size[a] - margin);
    let pixels = |p: Point| [p[0] * size[0], p[1] * size[1]];
    let distance = |a: Point, b: Point| ((a[0] - b[0]) * size[0]).hypot((a[1] - b[1]) * size[1]);
    let mut out = Tracks {
        width: w,
        height: h,
        first: start,
        frames: total,
        tracks: Vec::new(),
    };
    let mut anchors: std::collections::HashMap<usize, Arc<GpuFrame>> = Default::default();
    // A third of the GPU budget for anchor frames (16 bytes a pixel), at least four.
    let most_anchors =
        ((renderer.budget_bytes / 3) / (u64::from(w) * u64::from(h) * 16).max(1)).max(4) as usize;
    let mut live: Vec<Live> = Vec::new();
    let replenish = |live: &mut Vec<Live>,
                     out: &mut Tracks,
                     anchors: &mut std::collections::HashMap<usize, Arc<GpuFrame>>,
                     frame: &Frame,
                     offset: usize| {
        if live.len() * 10 >= settings.target * 7 {
            return;
        }
        let quad = [[0.02, 0.02], [0.98, 0.02], [0.98, 0.98], [0.02, 0.98]];
        let Ok(found) = tracker.detect(renderer, &frame.image, quad, planar) else {
            return;
        };
        let spacing = f64::from(settings.patch_radius * 3);
        let before = live.len();
        for p in found {
            if live.len() >= settings.target.min(256) {
                break;
            }
            if inside(p) && live.iter().all(|l| distance(p, l.at) >= spacing) {
                out.tracks.push(vec![(offset, pixels(p))]);
                live.push(Live {
                    id: out.tracks.len() - 1,
                    at: p,
                    anchor: offset,
                    origin: p,
                });
            }
        }
        if live.len() > before {
            anchors.insert(offset, frame.image.clone());
        }
    };
    replenish(&mut live, &mut out, &mut anchors, &previous, 0);
    if !progress(1, total) {
        return Err(error("Cancelled"));
    }
    for offset in 1..total {
        let current = load(start + offset as i64)?;
        live.retain(|l| inside(l.at));
        if live.len() >= 6 {
            let points: Vec<Point> = live.iter().map(|l| l.at).collect();
            // Coarse-to-fine finds the frame's overall motion; each point's own match
            // then allows parallax within the search window.
            let (forward, motion) = match tracker.track_multiscale(
                renderer,
                &previous.image,
                &current.image,
                &previous.levels,
                &current.levels,
                &points,
                Homography::IDENTITY,
                planar,
            ) {
                Ok(t) => (t.features, t.consensus.motion),
                Err(_) => (
                    tracker.match_points(
                        renderer,
                        &previous.image,
                        &current.image,
                        &points,
                        None,
                        Homography::IDENTITY,
                        planar,
                        None,
                    )?,
                    Homography::IDENTITY,
                ),
            };
            // A match reports where its whole-pixel patch moved; add back the part of a
            // pixel the point sat off it, so tracks keep sub-pixel positions.
            let moved = |f: &veylvik_document::planar::Feature, from: Point| -> Point {
                [
                    f.position[0] + from[0] - f.reference[0],
                    f.position[1] + from[1] - f.reference[1],
                ]
            };
            let ahead: Vec<Point> = forward
                .iter()
                .zip(&live)
                .map(|(f, l)| moved(f, l.at))
                .collect();
            // Track the matches back; keep those that return home.
            let landed: Vec<usize> = (0..forward.len())
                .filter(|&i| inside(ahead[i]) && forward[i].correlation >= settings.min_correlation)
                .collect();
            let back = if landed.is_empty() {
                Vec::new()
            } else {
                tracker.match_points(
                    renderer,
                    &current.image,
                    &previous.image,
                    &landed.iter().map(|&i| ahead[i]).collect::<Vec<_>>(),
                    None,
                    motion.inverse().unwrap_or(Homography::IDENTITY),
                    planar,
                    None,
                )?
            };
            let mut kept: Vec<(Live, Point)> = Vec::new();
            for (k, &i) in landed.iter().enumerate() {
                let home = moved(&back[k], ahead[i]);
                if back[k].correlation >= settings.min_correlation
                    && distance(home, live[i].at) <= settings.round_trip
                {
                    kept.push((live[i], ahead[i]));
                }
            }
            // Refine against each anchor: one GPU batch per anchor frame.
            let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
            for (k, (l, _)) in kept.iter().enumerate() {
                groups.entry(l.anchor).or_default().push(k);
            }
            let mut next = Vec::with_capacity(kept.len());
            for (anchor, members) in groups {
                let Some(reference) = anchors.get(&anchor).cloned() else {
                    continue;
                };
                let origins: Vec<Point> = members.iter().map(|&k| kept[k].0.origin).collect();
                let offsets: Vec<[f64; 2]> = members
                    .iter()
                    .map(|&k| {
                        let (l, estimate) = (kept[k].0, kept[k].1);
                        // From the anchor's whole pixel to the estimate, in pixels.
                        let whole = |v: f64, s: f64| ((v * s - 0.5).round() + 0.5) / s;
                        [
                            (estimate[0] - whole(l.origin[0], size[0])) * size[0],
                            (estimate[1] - whole(l.origin[1], size[1])) * size[1],
                        ]
                    })
                    .collect();
                let refined = tracker.match_points(
                    renderer,
                    &reference,
                    &current.image,
                    &origins,
                    Some(&offsets),
                    Homography::IDENTITY,
                    refine,
                    None,
                )?;
                for (j, &k) in members.iter().enumerate() {
                    let (mut l, estimate) = kept[k];
                    let f = &refined[j];
                    let anchored = moved(f, l.origin);
                    if f.correlation >= settings.min_correlation
                        && distance(anchored, estimate) < 1.5
                    {
                        l.at = anchored;
                    } else {
                        // Its look has drifted from the anchor: keep the frame-to-frame
                        // match and anchor afresh on the previous frame.
                        l.at = estimate;
                        l.anchor = offset - 1;
                        l.origin = live
                            .iter()
                            .find(|m| m.id == l.id)
                            .map_or(estimate, |m| m.at);
                        anchors
                            .entry(offset - 1)
                            .or_insert_with(|| previous.image.clone());
                    }
                    out.tracks[l.id].push((offset, pixels(l.at)));
                    next.push(l);
                }
            }
            live = next;
        } else {
            live.clear();
        }
        replenish(&mut live, &mut out, &mut anchors, &current, offset);
        anchors.retain(|frame, _| live.iter().any(|l| l.anchor == *frame));
        // Every anchor is a whole frame held on the GPU, and long-lived points keep
        // old ones alive: past a share of the budget, the oldest anchors are let go
        // and their points anchor afresh on this frame.
        while anchors.len() > most_anchors {
            let Some(&oldest) = anchors.keys().min() else {
                break;
            };
            anchors.remove(&oldest);
            for l in live.iter_mut().filter(|l| l.anchor == oldest) {
                l.anchor = offset;
                l.origin = l.at;
            }
            anchors
                .entry(offset)
                .or_insert_with(|| current.image.clone());
        }
        previous = current;
        if !progress(offset + 1, total) {
            return Err(error("Cancelled"));
        }
    }
    out.tracks.retain(|t| t.len() >= 3);
    Ok(out)
}
