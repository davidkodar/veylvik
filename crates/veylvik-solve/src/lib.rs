//! Camera solving: from 2D feature tracks through a shot, the moving camera (or a tripod pan), its focal length, lens distortion and a point cloud. Deterministic, CPU only.
mod bundle;
mod geometry;
pub mod lens;
pub mod linalg;
mod reconstruct;

pub use geometry::Pose;
pub use lens::Distortion;
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
    /// The lens's radial distortion (see `lens`); NONE when the shot showed none worth
    /// modelling. Poses, points and errors are for the undistorted (ideal) frame.
    pub distortion: Distortion,
    pub keyframes: Vec<usize>,
    /// For a measured lens: the shortest and longest focal lengths (pixels), among
    /// 0.7, 0.85, 1.15 and 1.3 times the answer, that fit the tracks nearly as well.
    /// A wide range means the shot does not pin its lens down.
    pub focal_range: Option<[f64; 2]>,
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
    /// Half the frame diagonal: the distortion model's unit radius.
    pub radius: f64,
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
            radius: lens::radius(settings.width, settings.height),
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
    let search = |shot: &Shot| {
        let mut best: Option<reconstruct::Reconstruction> = None;
        for &focal in &candidates {
            let Some(r) = reconstruct::keyframes(shot, &keyframes, focal) else {
                continue;
            };
            if best.as_ref().is_none_or(|b| better(&r, b)) {
                best = Some(r);
            }
        }
        best
    };
    let mut best = search(&shot);
    // Strong distortion can hide the camera's motion from the straight-lens search:
    // when it found little, look again through a few lenses and keep the best fit.
    let weak = best
        .as_ref()
        .is_none_or(|r| r.nodal || r.solved() * 10 < keyframes.len() * 9 || r.rms > WEAK_RMS);
    if weak {
        for k1 in [-0.08, -0.16, -0.24, 0.08] {
            let lens = Distortion { k1, k2: 0.0 };
            let ideal = undistorted(&shot, lens);
            let Some(mut r) = search(&Shot::new(&ideal, frames, settings)) else {
                continue;
            };
            r.distortion = lens;
            if best.as_ref().is_none_or(|b| better(&r, b)) {
                best = Some(r);
            }
        }
    }
    let mut r = best.ok_or_else(|| Error("No camera motion fits the tracks".into()))?;
    let mut measured = settings.focal.is_none();
    let (range_shot, range_keys) = (&shot, keyframes.clone());
    let mut solved = if measured && r.nodal && r.turn() < 3.0 {
        // A camera that barely turns on a tripod says nothing about its lens.
        measured = false;
        match reconstruct::keyframes(&shot, &keyframes, width) {
            Some(fallback) => fallback.all_frames(&shot, keyframes),
            None => through_lens(r, &shot, settings, keyframes, false),
        }
    } else {
        if measured {
            r.adjust(&shot, &keyframes, true, false, 100);
        }
        through_lens(r, &shot, settings, keyframes, measured)
    };
    solved.focal_measured = measured;
    if measured {
        // How firmly do the tracks hold that lens? Fit the keyframes again through
        // shorter and longer ones: where the error barely rises, they do not.
        let fit = |focal: f64| reconstruct::keyframes(range_shot, &range_keys, focal);
        if let Some(own) = fit(solved.focal) {
            let fits = |factor: f64| {
                fit(solved.focal * factor).is_some_and(|other| {
                    other.nodal == own.nodal
                        && other.solved() * 10 >= own.solved() * 9
                        && other.rms <= own.rms * 1.25 + 0.05
                })
            };
            let reach = |steps: [f64; 2]| {
                steps
                    .into_iter()
                    .take_while(|factor| fits(*factor))
                    .last()
                    .unwrap_or(1.0)
            };
            solved.focal_range = Some([
                solved.focal * reach([0.85, 0.7]),
                solved.focal * reach([1.15, 1.3]),
            ]);
        }
    }
    Ok(solved)
}
/// Keyframe rms error (pixels) above which a straight-lens solve counts as weak.
const WEAK_RMS: f64 = 3.0;
/// Fewest points a reconstruction needs to count at all.
const MIN_POINTS: usize = 20;
/// A real fit beats none, then more keyframes placed, then the lower error. Through
/// different lenses the median error is compared, since a pan keeps the bad tracks a
/// moving camera drops; and parallax is not preferred, since through a wrong lens a
/// pan can look like a moving camera.
fn better(r: &reconstruct::Reconstruction, than: &reconstruct::Reconstruction) -> bool {
    let rank = |r: &reconstruct::Reconstruction| {
        (r.points.iter().flatten().count() >= MIN_POINTS, r.solved())
    };
    let (a, b) = (rank(r), rank(than));
    // A coarse lens guess has to earn its place over the straight lens.
    if r.distortion == than.distortion {
        return a > b || (a == b && r.rms < than.rms);
    }
    let margin = if than.distortion == Distortion::NONE {
        LENS_GUESS_GAIN
    } else {
        0.0
    };
    a > b || (a == b && r.median < than.median * (1.0 - margin))
}
/// How much lower a lens guess's median error must be than the straight lens's: a
/// coarse guess with free camera positions can beat a correct pan by fitting noise
/// (about 1.3 times lower on a clean synthetic pan), while a lens that really bends
/// leaves the straight fit several times worse (7 times on the bent beach test).
const LENS_GUESS_GAIN: f64 = 0.5;
/// The shot's tracks as an ideal lens would have seen them through `lens`.
fn undistorted(shot: &Shot, lens: Distortion) -> Vec<Track> {
    shot.tracks
        .iter()
        .map(|t| Track {
            points: t
                .points
                .iter()
                .map(|&(f, p)| {
                    let u =
                        lens.undistort([p[0] - shot.centre[0], p[1] - shot.centre[1]], shot.radius);
                    (f, [shot.centre[0] + u[0], shot.centre[1] + u[1]])
                })
                .collect(),
        })
        .collect()
}
/// Least improvement in keyframe rms error for distortion to be kept: below it, two
/// more unknowns are only fitting noise.
const DISTORTION_GAIN: f64 = 0.03;
/// Measure the lens's distortion with everything else, from the distortion `r` was
/// found through; when it explains the tracks clearly better than a straight lens,
/// undistort every track and place every frame in the ideal frame.
fn through_lens(
    r: reconstruct::Reconstruction,
    shot: &Shot,
    settings: &Settings,
    keyframes: Vec<usize>,
    solve_focal: bool,
) -> Solved {
    let mut lens = r.clone();
    lens.adjust(shot, &keyframes, solve_focal, true, 100);
    let mut k = lens.distortion;
    if !k.invertible() {
        k = r.distortion;
    }
    let straight = r.distortion == Distortion::NONE;
    if k == Distortion::NONE || (straight && lens.rms >= r.rms * (1.0 - DISTORTION_GAIN)) {
        return r.all_frames(shot, keyframes);
    }
    let ideal = undistorted(shot, k);
    let ideal_shot = Shot::new(&ideal, shot.frames, settings);
    lens.distortion = Distortion::NONE;
    lens.adjust(&ideal_shot, &keyframes, false, false, 30);
    let mut solved = lens.all_frames(&ideal_shot, keyframes);
    solved.distortion = k;
    solved
}
