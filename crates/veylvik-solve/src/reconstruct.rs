//! Incremental reconstruction over keyframes: the best-parallax starting pair, cameras added outward by resection, new points triangulated, robust bundle adjustment; a rotation-only path for tripod pans.
use crate::bundle::{Observation, Problem};
use crate::geometry::{self, Pose};
use crate::linalg::*;
use crate::{Shot, Solved};

/// Rays closer than this to parallel give no depth (degrees).
const MIN_ANGLE: f64 = 1.5;
/// Reprojection error (pixels) beyond which a sighting is an outlier.
const OUTLIER: f64 = 3.0;
const HUBER: f64 = 2.0;

pub(crate) struct Reconstruction {
    /// Per keyframe.
    pub poses: Vec<Option<Pose>>,
    /// Per track.
    pub points: Vec<Option<V3>>,
    pub focal: f64,
    pub nodal: bool,
    pub rms: f64,
}
impl Reconstruction {
    pub fn solved(&self) -> usize {
        self.poses.iter().flatten().count()
    }
    /// The largest turn (degrees) of any solved keyframe from the first.
    pub fn turn(&self) -> f64 {
        let mut poses = self.poses.iter().flatten();
        let Some(first) = poses.next() else {
            return 0.0;
        };
        poses
            .map(|p| norm(log_so3(&mat_mul(&p.r, &transpose(&first.r)))).to_degrees())
            .fold(0.0, f64::max)
    }
    /// Bundle adjust every solved keyframe and point; drop outlier sightings' points
    /// when they have too few good sightings left, then adjust again.
    pub fn adjust(&mut self, shot: &Shot, keys: &[usize], solve_focal: bool, iterations: usize) {
        for round in 0..2 {
            let cams: Vec<usize> = (0..keys.len())
                .filter(|&k| self.poses[k].is_some())
                .collect();
            let mut poses: Vec<Pose> = cams
                .iter()
                .map(|&k| self.poses[k].unwrap_or(Pose::ORIGIN))
                .collect();
            let fixed: Vec<bool> = (0..cams.len()).map(|i| i == 0).collect();
            let ids: Vec<usize> = (0..self.points.len())
                .filter(|&t| self.points[t].is_some())
                .collect();
            let local: std::collections::HashMap<usize, usize> =
                ids.iter().enumerate().map(|(i, &t)| (t, i)).collect();
            let mut points: Vec<V3> = ids
                .iter()
                .map(|&t| self.points[t].unwrap_or([0.0; 3]))
                .collect();
            let mut observations = Vec::new();
            for (c, &k) in cams.iter().enumerate() {
                for &(track, pixel) in &shot.by_frame[keys[k]] {
                    if let Some(&point) = local.get(&track) {
                        observations.push(Observation {
                            camera: c,
                            point,
                            pixel,
                        });
                    }
                }
            }
            let mut focal = self.focal;
            let mut problem = Problem {
                poses: &mut poses,
                fixed: &fixed,
                points: &mut points,
                observations: &observations,
                focal: &mut focal,
                centre: shot.centre,
                solve_focal: solve_focal && round == 1,
                nodal: self.nodal,
                points_fixed: false,
                huber: HUBER,
            };
            self.rms = problem.optimise(iterations);
            // Points with fewer than two good sightings are not trusted.
            let mut good = vec![0usize; ids.len()];
            for o in &observations {
                if problem.error(o) < OUTLIER.max(3.0 * self.rms) {
                    good[o.point] += 1;
                }
            }
            for (c, &k) in cams.iter().enumerate() {
                self.poses[k] = Some(poses[c]);
            }
            for (i, &t) in ids.iter().enumerate() {
                self.points[t] = (good[i] >= 2).then_some(points[i]);
            }
            self.focal = focal;
        }
    }
    /// Place every frame against the settled points: keyframes keep their poses,
    /// the frames between are resected from their neighbour's pose.
    pub fn all_frames(&self, shot: &Shot, keys: Vec<usize>) -> Solved {
        let mut poses: Vec<Option<Pose>> = vec![None; shot.frames];
        for (k, &frame) in keys.iter().enumerate() {
            poses[frame] = self.poses[k];
        }
        let mut errors: Vec<Option<f64>> = vec![None; shot.frames];
        let first = keys
            .iter()
            .zip(&self.poses)
            .find(|(_, p)| p.is_some())
            .map(|(f, _)| *f)
            .unwrap_or(0);
        // Forward from the first solved keyframe, then backward before it.
        let order: Vec<(usize, usize)> = (first + 1..shot.frames)
            .map(|f| (f, f - 1))
            .chain((0..first).rev().map(|f| (f, f + 1)))
            .collect();
        for (frame, from) in order {
            let key = keys.binary_search(&frame).ok().and_then(|k| self.poses[k]);
            let start = key.or(poses[from]);
            let Some(start) = start else {
                continue;
            };
            if let Some((pose, rms)) = resect(
                shot,
                frame,
                start,
                &self.points,
                self.focal,
                self.nodal,
                key.is_some(),
            ) {
                poses[frame] = Some(pose);
                errors[frame] = Some(rms);
            } else if key.is_some() {
                poses[frame] = key;
            }
        }
        if let Some(pose) = poses[first]
            && let Some((_, rms)) = resect(
                shot,
                first,
                pose,
                &self.points,
                self.focal,
                self.nodal,
                true,
            )
        {
            errors[first] = Some(rms);
        }
        let (sum, n) = errors
            .iter()
            .enumerate()
            .filter_map(|(f, e)| e.map(|e| (e, shot.by_frame[f].len())))
            .fold((0.0, 0usize), |(s, n), (e, k)| {
                (s + e * e * k as f64, n + k)
            });
        Solved {
            poses,
            points: self.points.clone(),
            focal: self.focal,
            rms: (sum / n.max(1) as f64).sqrt(),
            frame_error: errors,
            nodal: self.nodal,
            focal_measured: false,
            keyframes: keys,
        }
    }
}
/// The pose of `frame` against fixed points, refined from `start`; with its rms
/// over the inlier sightings. `hold` keeps the pose and only measures it.
fn resect(
    shot: &Shot,
    frame: usize,
    start: Pose,
    points: &[Option<V3>],
    focal: f64,
    nodal: bool,
    hold: bool,
) -> Option<(Pose, f64)> {
    let mut seen: Vec<V3> = Vec::new();
    let mut observations = Vec::new();
    for &(track, pixel) in &shot.by_frame[frame] {
        if let Some(p) = points[track] {
            observations.push(Observation {
                camera: 0,
                point: seen.len(),
                pixel,
            });
            seen.push(p);
        }
    }
    if observations.len() < 8 {
        return None;
    }
    let mut poses = [start];
    let mut focal = focal;
    let fixed = [hold];
    let mut problem = Problem {
        poses: &mut poses,
        fixed: &fixed,
        points: &mut seen,
        observations: &observations,
        focal: &mut focal,
        centre: shot.centre,
        solve_focal: false,
        nodal,
        points_fixed: true,
        huber: HUBER,
    };
    problem.optimise(if hold { 0 } else { 30 });
    let inliers: Vec<Observation> = observations
        .iter()
        .copied()
        .filter(|o| problem.error(o) < OUTLIER * 2.0)
        .collect();
    if inliers.len() < 8 {
        return None;
    }
    let mut problem = Problem {
        observations: &inliers,
        ..problem
    };
    problem.optimise(if hold { 0 } else { 20 });
    // The frame's error over the sightings that agree with it.
    let errors: Vec<f64> = inliers
        .iter()
        .map(|o| problem.error(o))
        .filter(|e| *e < OUTLIER)
        .collect();
    if errors.len() < 8 {
        return None;
    }
    let rms = (errors.iter().map(|e| e * e).sum::<f64>() / errors.len() as f64).sqrt();
    Some((poses[0], rms))
}
/// The point of `track` from every solved keyframe that sees it, when it is in
/// front of all of them, reprojects well and has enough parallax.
fn triangulate_track(
    shot: &Shot,
    keys: &[usize],
    poses: &[Option<Pose>],
    track: usize,
    focal: f64,
) -> Option<V3> {
    let mut views = Vec::new();
    for (k, &frame) in keys.iter().enumerate() {
        let Some(pose) = poses[k] else { continue };
        if let Some((_, pixel)) = shot.by_frame[frame].iter().find(|(t, _)| *t == track) {
            views.push((pose, shot.ray(*pixel, focal), *pixel));
        }
    }
    if views.len() < 2 {
        return None;
    }
    let rays: Vec<(Pose, V2)> = views.iter().map(|(p, r, _)| (*p, *r)).collect();
    let p = geometry::triangulate(&rays)?;
    let ok = views.iter().all(|(pose, _, pixel)| {
        crate::bundle::project(pose.apply(p), focal, shot.centre).is_some_and(|q| {
            ((q[0] - pixel[0]).powi(2) + (q[1] - pixel[1]).powi(2)).sqrt() < OUTLIER
        })
    });
    let poses: Vec<Pose> = views.iter().map(|(p, ..)| *p).collect();
    (ok && geometry::ray_angle(&poses, p) >= MIN_ANGLE).then_some(p)
}
/// The keyframe pair to start from: most parallax and support.
fn starting_pair(shot: &Shot, keys: &[usize], focal: f64) -> Option<(usize, usize, Pose)> {
    let mut rng = Rng::new(0x5eed);
    let mut best: Option<(f64, usize, usize, Pose)> = None;
    let stride = (keys.len() / 8).max(1);
    for i in (0..keys.len()).step_by(stride) {
        for j in i + 1..(i + 12).min(keys.len()) {
            let shared = shot.shared(keys[i], keys[j]);
            if shared.len() < 30 {
                continue;
            }
            let pairs: Vec<(V2, V2)> = shared
                .iter()
                .map(|(_, a, b)| (shot.ray(*a, focal), shot.ray(*b, focal)))
                .collect();
            let Some((e, inliers)) = geometry::essential(&pairs, 1.5 / focal, &mut rng) else {
                continue;
            };
            let count = inliers.iter().filter(|ok| **ok).count();
            if count < 30 || count * 2 < shared.len() {
                continue;
            }
            let (pose, front) = geometry::choose_motion(&e, &pairs, &inliers);
            if front * 10 < count * 8 {
                continue;
            }
            let mut angles: Vec<f64> = pairs
                .iter()
                .zip(&inliers)
                .filter(|(_, ok)| **ok)
                .filter_map(|((a, b), _)| geometry::triangulate(&[(Pose::ORIGIN, *a), (pose, *b)]))
                .map(|p| geometry::ray_angle(&[Pose::ORIGIN, pose], p))
                .collect();
            if angles.len() < 30 {
                continue;
            }
            angles.sort_by(f64::total_cmp);
            let median = angles[angles.len() / 2];
            if median < 1.0 {
                continue;
            }
            let score = median.min(8.0) * (angles.len() as f64).sqrt();
            if best.as_ref().is_none_or(|b| score > b.0) {
                best = Some((score, i, j, pose));
            }
        }
    }
    best.map(|(_, i, j, pose)| (i, j, pose))
}
/// Reconstruct the keyframes at a fixed focal length (pixels).
pub(crate) fn keyframes(shot: &Shot, keys: &[usize], focal: f64) -> Option<Reconstruction> {
    match starting_pair(shot, keys, focal) {
        Some(start) => general(shot, keys, focal, start),
        None => nodal(shot, keys, focal),
    }
}
fn general(
    shot: &Shot,
    keys: &[usize],
    focal: f64,
    (a, b, pose): (usize, usize, Pose),
) -> Option<Reconstruction> {
    let mut r = Reconstruction {
        poses: vec![None; keys.len()],
        points: vec![None; shot.tracks.len()],
        focal,
        nodal: false,
        rms: 0.0,
    };
    r.poses[a] = Some(Pose::ORIGIN);
    r.poses[b] = Some(pose);
    let triangulate_new = |r: &mut Reconstruction, frame: usize| {
        for &(track, _) in &shot.by_frame[frame] {
            if r.points[track].is_none() {
                r.points[track] = triangulate_track(shot, keys, &r.poses, track, focal);
            }
        }
    };
    triangulate_new(&mut r, keys[a]);
    r.adjust(shot, keys, false, 30);
    // Add keyframes outward from the pair, the best-supported first. One that
    // cannot be placed is skipped for good, so those beyond it (which need a
    // placed neighbour) stay unsolved too.
    let mut failed = vec![false; keys.len()];
    let mut added = 0;
    loop {
        let candidate = (0..keys.len())
            .filter(|&k| r.poses[k].is_none() && !failed[k])
            .filter_map(|k| {
                let near = [k.checked_sub(1), Some(k + 1)]
                    .into_iter()
                    .flatten()
                    .find(|&n| n < keys.len() && r.poses[n].is_some())?;
                Some((k, near))
            })
            .max_by_key(|(k, _)| {
                shot.by_frame[keys[*k]]
                    .iter()
                    .filter(|(t, _)| r.points[*t].is_some())
                    .count()
            });
        let Some((k, near)) = candidate else {
            break;
        };
        let start = r.poses[near]?;
        match resect(shot, keys[k], start, &r.points, focal, false, false) {
            Some((pose, _)) => r.poses[k] = Some(pose),
            None => {
                failed[k] = true;
                continue;
            }
        }
        triangulate_new(&mut r, keys[k]);
        added += 1;
        if added % 4 == 0 {
            r.adjust(shot, keys, false, 20);
        }
    }
    r.adjust(shot, keys, false, 60);
    (r.solved() >= 2).then_some(r)
}
/// A tripod pan: the camera only turns. Rotations chain from frame to frame by
/// aligning shared rays; points sit at unit distance along their first ray.
fn nodal(shot: &Shot, keys: &[usize], focal: f64) -> Option<Reconstruction> {
    let bearing = |pixel: V2| {
        let x = shot.ray(pixel, focal);
        normalize([x[0], x[1], 1.0])
    };
    let mut poses: Vec<Option<Pose>> = vec![None; keys.len()];
    poses[0] = Some(Pose::ORIGIN);
    for k in 1..keys.len() {
        let shared = shot.shared(keys[k - 1], keys[k]);
        if shared.len() < 8 {
            break;
        }
        let pairs: Vec<(V3, V3)> = shared
            .iter()
            .map(|(_, a, b)| (bearing(*a), bearing(*b)))
            .collect();
        let mut weights = vec![1.0; pairs.len()];
        let mut relative = IDENTITY;
        for _ in 0..4 {
            relative = best_rotation(pairs.iter().zip(&weights).map(|((a, b), w)| (*a, *b, *w)));
            for ((a, b), w) in pairs.iter().zip(weights.iter_mut()) {
                let miss = norm(sub(mat_vec(&relative, *a), *b)) * focal;
                *w = if miss < OUTLIER { 1.0 } else { 0.0 };
            }
        }
        let previous = poses[k - 1]?;
        poses[k] = Some(Pose {
            r: mat_mul(&relative, &previous.r),
            t: [0.0; 3],
        });
    }
    let mut points = vec![None; shot.tracks.len()];
    for (k, &frame) in keys.iter().enumerate() {
        let Some(pose) = poses[k] else { continue };
        for &(track, pixel) in &shot.by_frame[frame] {
            if points[track].is_none() {
                points[track] = Some(mat_vec(&transpose(&pose.r), bearing(pixel)));
            }
        }
    }
    let mut r = Reconstruction {
        poses,
        points,
        focal,
        nodal: true,
        rms: 0.0,
    };
    r.adjust(shot, keys, false, 40);
    (r.solved() >= 2).then_some(r)
}
