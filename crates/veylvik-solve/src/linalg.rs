//! Small dense linear algebra for camera solving: 3-vectors, rotations, symmetric eigen decomposition, 3x3 SVD and Cholesky.
pub type V2 = [f64; 2];
pub type V3 = [f64; 3];
pub type M3 = [[f64; 3]; 3];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
pub fn normalize(a: V3) -> V3 {
    scale(a, 1.0 / norm(a).max(1e-300))
}
pub const IDENTITY: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
pub fn mat_vec(m: &M3, v: V3) -> V3 {
    [dot(m[0], v), dot(m[1], v), dot(m[2], v)]
}
pub fn transpose(m: &M3) -> M3 {
    [
        [m[0][0], m[1][0], m[2][0]],
        [m[0][1], m[1][1], m[2][1]],
        [m[0][2], m[1][2], m[2][2]],
    ]
}
pub fn mat_mul(a: &M3, b: &M3) -> M3 {
    let mut out = [[0.0; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, value) in row.iter_mut().enumerate() {
            *value = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}
pub fn det(m: &M3) -> f64 {
    dot(m[0], cross(m[1], m[2]))
}
pub fn skew(v: V3) -> M3 {
    [[0.0, -v[2], v[1]], [v[2], 0.0, -v[0]], [-v[1], v[0], 0.0]]
}
/// Rotation by axis-angle `w` (Rodrigues).
pub fn exp_so3(w: V3) -> M3 {
    let theta = norm(w);
    let k = skew(w);
    let k2 = mat_mul(&k, &k);
    let (a, b) = if theta < 1e-8 {
        (1.0, 0.5)
    } else {
        (theta.sin() / theta, (1.0 - theta.cos()) / (theta * theta))
    };
    let mut r = IDENTITY;
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] += a * k[i][j] + b * k2[i][j];
        }
    }
    r
}
/// The axis-angle of rotation `r`.
pub fn log_so3(r: &M3) -> V3 {
    let cos = ((r[0][0] + r[1][1] + r[2][2] - 1.0) * 0.5).clamp(-1.0, 1.0);
    let theta = cos.acos();
    let vee = [r[2][1] - r[1][2], r[0][2] - r[2][0], r[1][0] - r[0][1]];
    if theta < 1e-6 {
        return scale(vee, 0.5);
    }
    if theta > std::f64::consts::PI - 1e-4 {
        // Near a half turn: the axis is the column of r + I with the largest norm.
        let column = |j: usize| {
            [
                r[0][j] + IDENTITY[0][j],
                r[1][j] + IDENTITY[1][j],
                r[2][j] + IDENTITY[2][j],
            ]
        };
        let best = (0..3)
            .max_by(|&a, &b| norm(column(a)).total_cmp(&norm(column(b))))
            .unwrap_or(0);
        return scale(normalize(column(best)), theta);
    }
    scale(vee, theta / (2.0 * theta.sin()))
}
/// Eigen decomposition of the symmetric `n`x`n` row-major matrix `a`: eigenvalues
/// ascending, and eigenvectors as the columns of the row-major result.
pub fn symmetric_eigen(mut a: Vec<f64>, n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut v = vec![0.0; n * n];
    for i in 0..n {
        v[i * n + i] = 1.0;
    }
    for _ in 0..64 {
        let off: f64 = (0..n)
            .flat_map(|p| (p + 1..n).map(move |q| (p, q)))
            .map(|(p, q)| a[p * n + q] * a[p * n + q])
            .sum();
        let diagonal: f64 = (0..n).map(|i| a[i * n + i] * a[i * n + i]).sum();
        if off <= 1e-30 * diagonal.max(1e-300) {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                let apq = a[p * n + q];
                if apq.abs() < 1e-300 {
                    continue;
                }
                let theta = (a[q * n + q] - a[p * n + p]) / (2.0 * apq);
                let t = if theta >= 0.0 { 1.0 } else { -1.0 }
                    / (theta.abs() + (theta * theta + 1.0).sqrt());
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n {
                    let (akp, akq) = (a[k * n + p], a[k * n + q]);
                    a[k * n + p] = c * akp - s * akq;
                    a[k * n + q] = s * akp + c * akq;
                }
                for k in 0..n {
                    let (apk, aqk) = (a[p * n + k], a[q * n + k]);
                    a[p * n + k] = c * apk - s * aqk;
                    a[q * n + k] = s * apk + c * aqk;
                }
                for k in 0..n {
                    let (vkp, vkq) = (v[k * n + p], v[k * n + q]);
                    v[k * n + p] = c * vkp - s * vkq;
                    v[k * n + q] = s * vkp + c * vkq;
                }
            }
        }
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| a[i * n + i].total_cmp(&a[j * n + j]));
    let values = order.iter().map(|&i| a[i * n + i]).collect();
    let mut vectors = vec![0.0; n * n];
    for (column, &i) in order.iter().enumerate() {
        for k in 0..n {
            vectors[k * n + column] = v[k * n + i];
        }
    }
    (values, vectors)
}
/// The unit vector x minimising |A x| for the rows of `a` (each `n` wide).
pub fn null_vector(rows: &[Vec<f64>], n: usize) -> Vec<f64> {
    let mut ata = vec![0.0; n * n];
    for row in rows {
        for i in 0..n {
            for j in 0..n {
                ata[i * n + j] += row[i] * row[j];
            }
        }
    }
    let (_, vectors) = symmetric_eigen(ata, n);
    (0..n).map(|k| vectors[k * n]).collect()
}
/// Singular value decomposition m = U diag(s) Vᵀ, singular values descending,
/// U and V proper or improper rotations as the maths requires.
pub fn svd3(m: &M3) -> (M3, V3, M3) {
    let mtm = mat_mul(&transpose(m), m);
    let (values, vectors) = symmetric_eigen(mtm.iter().flatten().copied().collect(), 3);
    let column = |j: usize| [vectors[j], vectors[3 + j], vectors[6 + j]];
    let v_cols = [column(2), column(1), column(0)];
    let s = [
        values[2].max(0.0).sqrt(),
        values[1].max(0.0).sqrt(),
        values[0].max(0.0).sqrt(),
    ];
    // Singular values this far below the largest are rounding noise: their
    // vectors come from orthogonality, not from dividing by them.
    let eps = 1e-6 * s[0].max(1e-300);
    let mut u_cols = [[0.0; 3]; 3];
    u_cols[0] = if s[0] > 1e-300 {
        normalize(mat_vec(m, v_cols[0]))
    } else {
        [1.0, 0.0, 0.0]
    };
    u_cols[1] = if s[1] > eps {
        let u = mat_vec(m, v_cols[1]);
        normalize(sub(u, scale(u_cols[0], dot(u, u_cols[0]))))
    } else {
        let any = if u_cols[0][0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        normalize(cross(u_cols[0], any))
    };
    u_cols[2] = cross(u_cols[0], u_cols[1]);
    if s[2] > eps && dot(u_cols[2], mat_vec(m, v_cols[2])) < 0.0 {
        u_cols[2] = scale(u_cols[2], -1.0);
    }
    let from_columns = |c: [V3; 3]| transpose(&c);
    (from_columns(u_cols), s, from_columns(v_cols))
}
/// The rotation R minimising Σ w |b - R a|² over pairs (a, b) (Kabsch).
pub fn best_rotation(pairs: impl Iterator<Item = (V3, V3, f64)>) -> M3 {
    let mut h = [[0.0; 3]; 3];
    for (a, b, w) in pairs {
        for i in 0..3 {
            for j in 0..3 {
                h[i][j] += w * b[i] * a[j];
            }
        }
    }
    let (u, _, v) = svd3(&h);
    let d = det(&mat_mul(&u, &transpose(&v))).signum();
    let mut ud = u;
    for row in &mut ud {
        row[2] *= d;
    }
    mat_mul(&ud, &transpose(&v))
}
/// Solve the symmetric positive definite system a x = b in place (b becomes x);
/// false when `a` is not positive definite.
pub fn cholesky_solve(a: &mut [f64], n: usize, b: &mut [f64]) -> bool {
    for j in 0..n {
        let mut d = a[j * n + j];
        for k in 0..j {
            d -= a[j * n + k] * a[j * n + k];
        }
        if d <= 0.0 || !d.is_finite() {
            return false;
        }
        let d = d.sqrt();
        a[j * n + j] = d;
        for i in j + 1..n {
            let mut s = a[i * n + j];
            for k in 0..j {
                s -= a[i * n + k] * a[j * n + k];
            }
            a[i * n + j] = s / d;
        }
    }
    for i in 0..n {
        let mut s = b[i];
        for k in 0..i {
            s -= a[i * n + k] * b[k];
        }
        b[i] = s / a[i * n + i];
    }
    for i in (0..n).rev() {
        let mut s = b[i];
        for k in i + 1..n {
            s -= a[k * n + i] * b[k];
        }
        b[i] = s / a[i * n + i];
    }
    true
}
/// Inverse of a symmetric positive definite 3x3 matrix.
pub fn inverse3(m: &M3) -> Option<M3> {
    let d = det(m);
    if d.abs() < 1e-300 || !d.is_finite() {
        return None;
    }
    let c =
        |r0: usize, r1: usize, c0: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
    Some([
        [c(1, 2, 1, 2) / d, -c(0, 2, 1, 2) / d, c(0, 1, 1, 2) / d],
        [-c(1, 2, 0, 2) / d, c(0, 2, 0, 2) / d, -c(0, 1, 0, 2) / d],
        [c(1, 2, 0, 1) / d, -c(0, 2, 0, 1) / d, c(0, 1, 0, 1) / d],
    ])
}
/// A small deterministic generator, so solves repeat exactly.
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    pub fn draw(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.draw() % n.max(1) as u64) as usize
    }
    pub fn unit(&mut self) -> f64 {
        (self.draw() >> 11) as f64 / (1u64 << 53) as f64
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotations_round_trip_and_svd_rebuilds() {
        for w in [[0.1, -0.2, 0.3], [0.0, 0.0, 1e-9], [3.0, 0.5, -0.2]] {
            let back = log_so3(&exp_so3(w));
            let r = exp_so3(back);
            let r0 = exp_so3(w);
            for i in 0..3 {
                for j in 0..3 {
                    assert!((r[i][j] - r0[i][j]).abs() < 1e-9);
                }
            }
        }
        let m = [[2.0, -1.0, 0.5], [0.3, 1.5, -2.0], [1.0, 0.2, 0.7]];
        let (u, s, v) = svd3(&m);
        let mut us = u;
        for row in &mut us {
            for j in 0..3 {
                row[j] *= s[j];
            }
        }
        let back = mat_mul(&us, &transpose(&v));
        for i in 0..3 {
            for j in 0..3 {
                assert!((back[i][j] - m[i][j]).abs() < 1e-9, "{back:?}");
            }
        }
        let mut a = vec![4.0, 1.0, 0.5, 1.0, 3.0, 0.2, 0.5, 0.2, 2.0];
        let mut b = vec![1.0, 2.0, 3.0];
        assert!(cholesky_solve(&mut a, 3, &mut b));
        let a0 = [[4.0, 1.0, 0.5], [1.0, 3.0, 0.2], [0.5, 0.2, 2.0]];
        let check = mat_vec(&a0, [b[0], b[1], b[2]]);
        assert!((check[0] - 1.0).abs() < 1e-9 && (check[2] - 3.0).abs() < 1e-9);
    }
}
