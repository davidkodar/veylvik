//! Camera solving: from 2D feature tracks through a shot, the moving camera (or a tripod pan), its focal length and a point cloud. Deterministic, CPU only.
mod bundle;
mod geometry;
pub mod linalg;
mod reconstruct;

pub use geometry::Pose;
use linalg::*;

/// One feature followed through the shot: (frame, pixel) in increasing frames.
#[derive(Debug, Clone, Default)]
pub struct Track {
    pub points: Vec<(usize, V2)>,
}
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub width: u32,
    pub height: u32,
    /// The focal length in pixels when known; None searches for it.
    pub focal: Option<f64>,
}
/// A solved shot. Cameras use x right, y down and look along +z; x_cam = R x + t.
#[derive(Debug, Clone)]
pub struct Solved {
    /// Per frame; None where too few tracks were left to place the camera.
    pub poses: Vec<Option<Pose>>,
    /// Per track: its 3D position, None for tracks rejected as unreliable.
    pub points: Vec<Option<V3>>,
    pub focal: f64,
    /// Root mean square reprojection error over every kept sighting (pixels).
    pub rms: f64,
    /// Per frame reprojection error (pixels).
    pub frame_error: Vec<Option<f64>>,
    /// The camera only turned (no parallax): positions are fixed, points are at unit distance.
    pub nodal: bool,
    /// The lens was measured from the shot; false when it was given, or when the
    /// camera barely turned on a tripod and a 36 mm-equivalent lens was assumed.
    pub focal_measured: bool,
    pub keyframes: Vec<usize>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Error(pub String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

/// The tracks seen in every frame, for quick lookups.
pub(crate) struct Shot<'a> {
    pub tracks: &'a [Track],
    pub frames: usize,
    pub by_frame: Vec<Vec<(usize, V2)>>,
    pub centre: V2,
}
impl<'a> Shot<'a> {
    fn new(tracks: &'a [Track], frames: usize, settings: &Settings) -> Self {
        let mut by_frame = vec![Vec::new(); frames];
        for (id, track) in tracks.iter().enumerate() {
            for &(frame, pixel) in &track.points {
                if frame < frames {
                    by_frame[frame].push((id, pixel));
                }
            }
        }
        Self {
            tracks,
            frames,
            by_frame,
            centre: [settings.width as f64 * 0.5, settings.height as f64 * 0.5],
        }
    }
    /// Tracks seen in both frames, with their pixels there.
    pub fn shared(&self, a: usize, b: usize) -> Vec<(usize, V2, V2)> {
        let lookup: std::collections::HashMap<usize, V2> =
            self.by_frame[b].iter().copied().collect();
        self.by_frame[a]
            .iter()
            .filter_map(|(id, pa)| lookup.get(id).map(|pb| (*id, *pa, *pb)))
            .collect()
    }
    pub fn ray(&self, pixel: V2, focal: f64) -> V2 {
        [
            (pixel[0] - self.centre[0]) / focal,
            (pixel[1] - self.centre[1]) / focal,
        ]
    }
    /// Frames spread through the shot that still share enough tracks with the previous one.
    fn keyframes(&self) -> Vec<usize> {
        let Some(first) = (0..self.frames).find(|&f| self.by_frame[f].len() >= 12) else {
            return Vec::new();
        };
        let step = self.frames.div_ceil(50).max(1);
        let mut keys = vec![first];
        let mut k = first;
        while k + 1 < self.frames {
            let need = 12.max(self.by_frame[k].len() * 2 / 5);
            let mut next = None;
            for j in k + 1..=(k + step).min(self.frames - 1) {
                if self.shared(k, j).len() >= need {
                    next = Some(j);
                } else {
                    break;
                }
            }
            let next = match next {
                Some(j) => j,
                None if self.shared(k, k + 1).len() >= 8 => k + 1,
                None => break,
            };
            keys.push(next);
            k = next;
        }
        keys
    }
}
/// Solve the camera of a shot of `frames` frames from its feature tracks.
pub fn solve(tracks: &[Track], frames: usize, settings: &Settings) -> Result<Solved> {
    if frames < 2 {
        return Err(Error("A camera solve needs at least two frames".into()));
    }
    if settings.width == 0 || settings.height == 0 {
        return Err(Error("The frame has no size".into()));
    }
    let shot = Shot::new(tracks, frames, settings);
    let keyframes = shot.keyframes();
    if keyframes.len() < 2 {
        return Err(Error(
            "Too few features stay visible from frame to frame; the shot needs more texture or less motion blur".into(),
        ));
    }
    let width = settings.width as f64;
    let candidates: Vec<f64> = match settings.focal {
        Some(f) => vec![f],
        None => [0.5, 0.7, 0.9, 1.2, 1.6, 2.1, 2.8, 3.6]
            .iter()
            .map(|m| m * width)
            .collect(),
    };
    let mut best: Option<reconstruct::Reconstruction> = None;
    for focal in candidates {
        let Some(r) = reconstruct::keyframes(&shot, &keyframes, focal) else {
            continue;
        };
        let better = best.as_ref().is_none_or(|b| {
            let (solved, best_solved) = (r.solved(), b.solved());
            solved > best_solved || (solved == best_solved && r.rms < b.rms)
        });
        if better {
            best = Some(r);
        }
    }
    let mut r = best.ok_or_else(|| Error("No camera motion fits the tracks".into()))?;
    let mut measured = settings.focal.is_none();
    if measured && r.nodal && r.turn() < 3.0 {
        // A camera that barely turns on a tripod says nothing about its lens.
        measured = false;
        if let Some(fallback) = reconstruct::keyframes(&shot, &keyframes, width) {
            r = fallback;
        }
    } else if measured {
        r.adjust(&shot, &keyframes, true, 100);
    }
    let mut solved = r.all_frames(&shot, keyframes);
    solved.focal_measured = measured;
    Ok(solved)
}
