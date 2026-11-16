//! Row-major 4x4 matrices acting on column vectors, in the scene frame (x right, y down, z towards the viewer).
pub type Mat4 = [[f32; 4]; 4];

pub const IDENTITY: Mat4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0; 4]; 4];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}
pub fn translate(t: [f32; 3]) -> Mat4 {
    let mut m = IDENTITY;
    m[0][3] = t[0];
    m[1][3] = t[1];
    m[2][3] = t[2];
    m
}
pub fn scale(s: [f32; 3]) -> Mat4 {
    let mut m = IDENTITY;
    m[0][0] = s[0];
    m[1][1] = s[1];
    m[2][2] = s[2];
    m
}
/// Rotation by degrees about X, then Y, then Z (so R = Rz * Ry * Rx).
pub fn rotate(degrees: [f32; 3]) -> Mat4 {
    let [x, y, z] = degrees.map(f32::to_radians);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    let rx = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, cx, -sx, 0.0],
        [0.0, sx, cx, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let ry = [
        [cy, 0.0, sy, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [-sy, 0.0, cy, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let rz = [
        [cz, -sz, 0.0, 0.0],
        [sz, cz, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    mul(&rz, &mul(&ry, &rx))
}
pub fn point(m: &Mat4, p: [f32; 3]) -> [f32; 3] {
    let v = [p[0], p[1], p[2], 1.0];
    let r = |i: usize| (0..4).map(|k| m[i][k] * v[k]).sum::<f32>();
    let w = r(3);
    [r(0) / w, r(1) / w, r(2) / w]
}
pub fn direction(m: &Mat4, d: [f32; 3]) -> [f32; 3] {
    let r = |i: usize| (0..3).map(|k| m[i][k] * d[k]).sum::<f32>();
    [r(0), r(1), r(2)]
}
/// Inverse of a rigid transform (rotation and translation, no scale), such as a camera.
pub fn rigid_inverse(m: &Mat4) -> Mat4 {
    let mut out = IDENTITY;
    for (i, row) in out.iter_mut().take(3).enumerate() {
        for (j, v) in row.iter_mut().take(3).enumerate() {
            *v = m[j][i];
        }
        row[3] = -(0..3).map(|k| m[k][i] * m[k][3]).sum::<f32>();
    }
    out
}
/// Inverse of the upper 3x3 (rotation and scale) as a matrix without translation;
/// None when it is singular (a zero scale).
pub fn linear_inverse(m: &Mat4) -> Option<Mat4> {
    let a = |r: usize, c: usize| m[r][c];
    let cof = |r0, r1, c0, c1| a(r0, c0) * a(r1, c1) - a(r0, c1) * a(r1, c0);
    let c = [
        [cof(1, 2, 1, 2), -cof(1, 2, 0, 2), cof(1, 2, 0, 1)],
        [-cof(0, 2, 1, 2), cof(0, 2, 0, 2), -cof(0, 2, 0, 1)],
        [cof(0, 1, 1, 2), -cof(0, 1, 0, 2), cof(0, 1, 0, 1)],
    ];
    let det = a(0, 0) * c[0][0] + a(0, 1) * c[0][1] + a(0, 2) * c[0][2];
    if det.abs() < 1e-12 {
        return None;
    }
    let mut out = IDENTITY;
    for (r, row) in out.iter_mut().take(3).enumerate() {
        for (k, v) in row.iter_mut().take(3).enumerate() {
            // The inverse is the transposed cofactor matrix over the determinant.
            *v = c[k][r] / det;
        }
    }
    Some(out)
}
