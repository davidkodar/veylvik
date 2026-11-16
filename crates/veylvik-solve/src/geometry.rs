//! Two-view geometry on calibrated rays: the essential matrix by RANSAC, the camera motion it implies, and triangulation.
use crate::linalg::*;

/// A camera as world-to-camera rotation and translation: x_cam = R x + t.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub r: M3,
    pub t: V3,
}
impl Pose {
    pub const ORIGIN: Pose = Pose {
        r: IDENTITY,
        t: [0.0; 3],
    };
    pub fn apply(&self, x: V3) -> V3 {
        add(mat_vec(&self.r, x), self.t)
    }
    /// The camera centre in the world.
    pub fn centre(&self) -> V3 {
        scale(mat_vec(&transpose(&self.r), self.t), -1.0)
    }
}
fn eight_point(pairs: &[(V2, V2)]) -> Option<M3> {
    let rows: Vec<Vec<f64>> = pairs
        .iter()
        .map(|(a, b)| {
            vec![
                b[0] * a[0],
                b[0] * a[1],
                b[0],
                b[1] * a[0],
                b[1] * a[1],
                b[1],
                a[0],
                a[1],
                1.0,
            ]
        })
        .collect();
    let e = null_vector(&rows, 9);
    let m = [[e[0], e[1], e[2]], [e[3], e[4], e[5]], [e[6], e[7], e[8]]];
    // Project onto essential matrices: two equal singular values, one zero.
    let (u, _, v) = svd3(&m);
    let mut ud = u;
    for row in &mut ud {
        row[2] = 0.0;
    }
    let out = mat_mul(&ud, &transpose(&v));
    out.iter().flatten().all(|x| x.is_finite()).then_some(out)
}
/// Squared Sampson distance of a correspondence (calibrated coordinates).
pub fn sampson(e: &M3, a: V2, b: V2) -> f64 {
    let ea = mat_vec(e, [a[0], a[1], 1.0]);
    let etb = mat_vec(&transpose(e), [b[0], b[1], 1.0]);
    let r = dot([b[0], b[1], 1.0], ea);
    r * r / (ea[0] * ea[0] + ea[1] * ea[1] + etb[0] * etb[0] + etb[1] * etb[1]).max(1e-300)
}
/// The essential matrix relating rays `a` (first view) to `b` (second), with its
/// inlier flags, by RANSAC over 8-point samples then a refit on all inliers.
pub fn essential(pairs: &[(V2, V2)], threshold: f64, rng: &mut Rng) -> Option<(M3, Vec<bool>)> {
    if pairs.len() < 12 {
        return None;
    }
    let limit = threshold * threshold;
    let mut best: Option<(usize, M3)> = None;
    for _ in 0..200 {
        let mut sample = Vec::with_capacity(8);
        while sample.len() < 8 {
            let i = rng.below(pairs.len());
            if !sample.contains(&i) {
                sample.push(i);
            }
        }
        let Some(e) = eight_point(&sample.iter().map(|&i| pairs[i]).collect::<Vec<_>>()) else {
            continue;
        };
        let count = pairs
            .iter()
            .filter(|(a, b)| sampson(&e, *a, *b) < limit)
            .count();
        if best.as_ref().is_none_or(|(n, _)| count > *n) {
            best = Some((count, e));
        }
    }
    let (_, e) = best?;
    let inliers: Vec<bool> = pairs
        .iter()
        .map(|(a, b)| sampson(&e, *a, *b) < limit)
        .collect();
    let chosen: Vec<(V2, V2)> = pairs
        .iter()
        .zip(&inliers)
        .filter(|(_, ok)| **ok)
        .map(|(p, _)| *p)
        .collect();
    let e = eight_point(&chosen).unwrap_or(e);
    let inliers = pairs
        .iter()
        .map(|(a, b)| sampson(&e, *a, *b) < limit)
        .collect();
    Some((e, inliers))
}
/// The four camera motions an essential matrix allows.
pub fn motions(e: &M3) -> [Pose; 4] {
    let (mut u, _, mut v) = svd3(e);
    if det(&u) < 0.0 {
        u = mat_mul(&u, &[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]]);
    }
    if det(&v) < 0.0 {
        v = mat_mul(&v, &[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]]);
    }
    let w = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    let r1 = mat_mul(&mat_mul(&u, &w), &transpose(&v));
    let r2 = mat_mul(&mat_mul(&u, &transpose(&w)), &transpose(&v));
    let t = [u[0][2], u[1][2], u[2][2]];
    [
        Pose { r: r1, t },
        Pose {
            r: r1,
            t: scale(t, -1.0),
        },
        Pose { r: r2, t },
        Pose {
            r: r2,
            t: scale(t, -1.0),
        },
    ]
}
/// The point seen along calibrated rays `x` from each pose (linear least squares).
pub fn triangulate(views: &[(Pose, V2)]) -> Option<V3> {
    let mut rows = Vec::with_capacity(views.len() * 2);
    for (pose, x) in views {
        let row = |k: usize| [pose.r[k][0], pose.r[k][1], pose.r[k][2], pose.t[k]];
        let (p1, p2, p3) = (row(0), row(1), row(2));
        rows.push((0..4).map(|i| x[0] * p3[i] - p1[i]).collect::<Vec<_>>());
        rows.push((0..4).map(|i| x[1] * p3[i] - p2[i]).collect::<Vec<_>>());
    }
    let h = null_vector(&rows, 4);
    if h[3].abs() < 1e-12 {
        return None;
    }
    let p = [h[0] / h[3], h[1] / h[3], h[2] / h[3]];
    p.iter().all(|v| v.is_finite()).then_some(p)
}
/// The largest angle (degrees) between the rays from the poses' centres to `p`.
pub fn ray_angle(poses: &[Pose], p: V3) -> f64 {
    let rays: Vec<V3> = poses
        .iter()
        .map(|c| normalize(sub(p, c.centre())))
        .collect();
    let mut best = 0.0_f64;
    for i in 0..rays.len() {
        for j in i + 1..rays.len() {
            best = best.max(dot(rays[i], rays[j]).clamp(-1.0, 1.0).acos().to_degrees());
        }
    }
    best
}
/// Of the four motions, the one that puts the most triangulated inliers in front of
/// both cameras, with how many.
pub fn choose_motion(e: &M3, pairs: &[(V2, V2)], inliers: &[bool]) -> (Pose, usize) {
    motions(e)
        .into_iter()
        .map(|pose| {
            let front = pairs
                .iter()
                .zip(inliers)
                .filter(|(_, ok)| **ok)
                .filter(|((a, b), _)| {
                    triangulate(&[(Pose::ORIGIN, *a), (pose, *b)])
                        .is_some_and(|p| p[2] > 0.0 && pose.apply(p)[2] > 0.0)
                })
                .count();
            (pose, front)
        })
        .max_by_key(|(_, n)| *n)
        .unwrap_or((Pose::ORIGIN, 0))
}
