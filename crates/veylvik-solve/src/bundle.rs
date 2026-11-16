//! Bundle adjustment: robust Levenberg-Marquardt over camera poses, points and the focal length, points eliminated by the Schur complement.
use crate::geometry::Pose;
use crate::linalg::*;

/// One sighting of `point` by `camera`, in pixels.
#[derive(Debug, Clone, Copy)]
pub struct Observation {
    pub camera: usize,
    pub point: usize,
    pub pixel: V2,
}
/// A point's normal-equation blocks: V, its gradient, and its W column rows by parameter.
type PointBlock = (M3, V3, Vec<(usize, Vec<V3>)>);
pub struct Problem<'a> {
    pub poses: &'a mut [Pose],
    /// Poses held still (the gauge, or frames already settled).
    pub fixed: &'a [bool],
    pub points: &'a mut [V3],
    pub observations: &'a [Observation],
    pub focal: &'a mut f64,
    pub centre: V2,
    pub solve_focal: bool,
    /// Rotation only: every camera stays where it is (a tripod pan).
    pub nodal: bool,
    /// Points stay where they are: only cameras move (resection).
    pub points_fixed: bool,
    /// Residuals beyond this many pixels count linearly (Huber).
    pub huber: f64,
}
/// The projection of camera-space `p`, or None behind the camera.
pub fn project(p: V3, focal: f64, centre: V2) -> Option<V2> {
    (p[2] > 1e-9).then(|| {
        [
            centre[0] + focal * p[0] / p[2],
            centre[1] + focal * p[1] / p[2],
        ]
    })
}
fn huber_weight(r: f64, delta: f64) -> f64 {
    if r <= delta { 1.0 } else { delta / r }
}
fn huber_cost(r: f64, delta: f64) -> f64 {
    if r <= delta {
        r * r
    } else {
        2.0 * delta * r - delta * delta
    }
}
impl Problem<'_> {
    /// Robust cost of the current state.
    fn cost(&self) -> f64 {
        self.observations
            .iter()
            .map(|o| {
                match project(
                    self.poses[o.camera].apply(self.points[o.point]),
                    *self.focal,
                    self.centre,
                ) {
                    Some(p) => huber_cost(
                        ((p[0] - o.pixel[0]).powi(2) + (p[1] - o.pixel[1]).powi(2)).sqrt(),
                        self.huber,
                    ),
                    None => huber_cost(1e4, self.huber),
                }
            })
            .sum()
    }
    /// Root mean square reprojection error (pixels) of every observation.
    pub fn rms(&self) -> f64 {
        let (sum, n) = self.observations.iter().fold((0.0, 0usize), |(s, n), o| {
            match project(
                self.poses[o.camera].apply(self.points[o.point]),
                *self.focal,
                self.centre,
            ) {
                Some(p) => (
                    s + (p[0] - o.pixel[0]).powi(2) + (p[1] - o.pixel[1]).powi(2),
                    n + 1,
                ),
                None => (s + 1e8, n + 1),
            }
        });
        (sum / n.max(1) as f64).sqrt()
    }
    /// Reprojection error (pixels) of one observation.
    pub fn error(&self, o: &Observation) -> f64 {
        project(
            self.poses[o.camera].apply(self.points[o.point]),
            *self.focal,
            self.centre,
        )
        .map_or(f64::INFINITY, |p| {
            ((p[0] - o.pixel[0]).powi(2) + (p[1] - o.pixel[1]).powi(2)).sqrt()
        })
    }
    /// Run up to `iterations` LM steps; returns the final rms error.
    pub fn optimise(&mut self, iterations: usize) -> f64 {
        let d = if self.nodal { 3 } else { 6 };
        let mut index = vec![usize::MAX; self.poses.len()];
        let mut m = 0;
        for (c, slot) in index.iter_mut().enumerate() {
            if !self.fixed[c] {
                *slot = m;
                m += d;
            }
        }
        let focal_index = m;
        if self.solve_focal {
            m += 1;
        }
        let mut by_point: Vec<Vec<usize>> = vec![Vec::new(); self.points.len()];
        for (k, o) in self.observations.iter().enumerate() {
            by_point[o.point].push(k);
        }
        let mut lambda = 1e-3;
        let mut cost = self.cost();
        for _ in 0..iterations {
            let mut u = vec![0.0; m * m];
            let mut gy = vec![0.0; m];
            // Per point: V (3x3), gx, and W blocks (index, rows of 3).
            let mut point_blocks: Vec<PointBlock> = Vec::with_capacity(self.points.len());
            for (p, ks) in by_point.iter().enumerate() {
                let mut v = [[0.0; 3]; 3];
                let mut gx = [0.0; 3];
                let mut w_blocks: Vec<(usize, Vec<V3>)> = Vec::new();
                for &k in ks {
                    let o = self.observations[k];
                    let pose = self.poses[o.camera];
                    let rx = mat_vec(&pose.r, self.points[p]);
                    let pc = add(rx, pose.t);
                    let Some(pix) = project(pc, *self.focal, self.centre) else {
                        continue;
                    };
                    let r = [pix[0] - o.pixel[0], pix[1] - o.pixel[1]];
                    let w = huber_weight((r[0] * r[0] + r[1] * r[1]).sqrt(), self.huber);
                    let (x, y, z) = (pc[0], pc[1], pc[2]);
                    let f = *self.focal;
                    let jp = [
                        [f / z, 0.0, -f * x / (z * z)],
                        [0.0, f / z, -f * y / (z * z)],
                    ];
                    let mul = |a: &[[f64; 3]; 2], b: &M3| -> [[f64; 3]; 2] {
                        let mut out = [[0.0; 3]; 2];
                        for i in 0..2 {
                            for j in 0..3 {
                                out[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
                            }
                        }
                        out
                    };
                    let j_rot = mul(&jp, &scale_m(&skew(rx), -1.0));
                    let j_x = mul(&jp, &pose.r);
                    let j_f = [x / z, y / z];
                    // The camera's columns: rotation, then translation (pixel rows 0, 1).
                    let mut cam_cols: Vec<[f64; 2]> =
                        (0..3).map(|j| [j_rot[0][j], j_rot[1][j]]).collect();
                    if !self.nodal {
                        cam_cols.extend((0..3).map(|j| [jp[0][j], jp[1][j]]));
                    }
                    let mut cols: Vec<(usize, [f64; 2])> = Vec::new();
                    if !self.fixed[o.camera] {
                        let base = index[o.camera];
                        cols.extend(cam_cols.iter().enumerate().map(|(i, c)| (base + i, *c)));
                    }
                    if self.solve_focal {
                        cols.push((focal_index, j_f));
                    }
                    for (a, ca) in &cols {
                        gy[*a] += w * (ca[0] * r[0] + ca[1] * r[1]);
                        for (b, cb) in &cols {
                            u[a * m + b] += w * (ca[0] * cb[0] + ca[1] * cb[1]);
                        }
                    }
                    if self.points_fixed {
                        continue;
                    }
                    for i in 0..3 {
                        gx[i] += w * (j_x[0][i] * r[0] + j_x[1][i] * r[1]);
                        for j in 0..3 {
                            v[i][j] += w * (j_x[0][i] * j_x[0][j] + j_x[1][i] * j_x[1][j]);
                        }
                    }
                    for (a, ca) in &cols {
                        let row = [0, 1, 2].map(|j| w * (ca[0] * j_x[0][j] + ca[1] * j_x[1][j]));
                        match w_blocks.iter_mut().find(|(start, _)| *start == *a) {
                            Some((_, rows)) => {
                                for j in 0..3 {
                                    rows[0][j] += row[j];
                                }
                            }
                            None => w_blocks.push((*a, vec![row])),
                        }
                    }
                }
                point_blocks.push((v, gx, w_blocks));
            }
            loop {
                // Damped normal equations, points eliminated.
                let mut s = u.clone();
                for i in 0..m {
                    s[i * m + i] += lambda * u[i * m + i].max(1e-6);
                }
                let mut rhs: Vec<f64> = gy.iter().map(|g| -g).collect();
                let mut inverses: Vec<Option<M3>> = Vec::with_capacity(point_blocks.len());
                for (v, gx, w_blocks) in &point_blocks {
                    if self.points_fixed {
                        inverses.push(None);
                        continue;
                    }
                    let mut vd = *v;
                    for i in 0..3 {
                        vd[i][i] += lambda * v[i][i].max(1e-6);
                    }
                    let Some(vi) = inverse3(&vd) else {
                        inverses.push(None);
                        continue;
                    };
                    for (a, ra) in w_blocks {
                        let wv = mat_vec(&transpose(&vi), ra[0]);
                        rhs[*a] += dot(wv, *gx);
                        for (b, rb) in w_blocks {
                            s[a * m + b] -= dot(wv, rb[0]);
                        }
                    }
                    inverses.push(Some(vi));
                }
                let mut dy = rhs;
                if m > 0 && !cholesky_solve(&mut s, m, &mut dy) {
                    lambda *= 10.0;
                    if lambda > 1e12 {
                        return self.rms();
                    }
                    continue;
                }
                // Tentative update.
                let saved_poses = self.poses.to_vec();
                let saved_points = self.points.to_vec();
                let saved_focal = *self.focal;
                for (c, pose) in self.poses.iter_mut().enumerate() {
                    if self.fixed[c] {
                        continue;
                    }
                    let base = index[c];
                    pose.r = mat_mul(&exp_so3([dy[base], dy[base + 1], dy[base + 2]]), &pose.r);
                    if !self.nodal {
                        pose.t = add(pose.t, [dy[base + 3], dy[base + 4], dy[base + 5]]);
                    }
                }
                if self.solve_focal {
                    *self.focal = (*self.focal + dy[focal_index]).max(1.0);
                }
                for (p, (_, gx, w_blocks)) in point_blocks.iter().enumerate() {
                    let Some(vi) = inverses[p] else {
                        continue;
                    };
                    let mut b = scale(*gx, -1.0);
                    for (a, ra) in w_blocks {
                        b = sub(b, scale(ra[0], dy[*a]));
                    }
                    self.points[p] = add(self.points[p], mat_vec(&vi, b));
                }
                let next = self.cost();
                if next < cost {
                    let gain = (cost - next) / cost.max(1e-300);
                    cost = next;
                    lambda = (lambda / 3.0).max(1e-9);
                    if gain < 1e-7 {
                        return self.rms();
                    }
                    break;
                }
                self.poses.copy_from_slice(&saved_poses);
                self.points.copy_from_slice(&saved_points);
                *self.focal = saved_focal;
                lambda *= 4.0;
                if lambda > 1e12 {
                    return self.rms();
                }
            }
        }
        self.rms()
    }
}
fn scale_m(m: &M3, s: f64) -> M3 {
    m.map(|row| row.map(|v| v * s))
}
