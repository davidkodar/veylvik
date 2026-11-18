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
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn unit(v: [f32; 3]) -> Option<[f32; 3]> {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (length > 1e-6).then(|| v.map(|c| c / length))
}
/// The rotation whose -z points from `from` to `to` with y kept as far down as it
/// can be (no roll); None when the two points coincide.
pub fn look_at(from: [f32; 3], to: [f32; 3]) -> Option<Mat4> {
    let forward = unit([to[0] - from[0], to[1] - from[1], to[2] - from[2]])?;
    let z = forward.map(|c| -c);
    // Straight up or down has no sideways direction of its own: keep x to the right.
    let x = unit(cross([0.0, 1.0, 0.0], z)).unwrap_or([1.0, 0.0, 0.0]);
    let y = cross(z, x);
    let mut m = IDENTITY;
    for row in 0..3 {
        m[row][0] = x[row];
        m[row][1] = y[row];
        m[row][2] = z[row];
    }
    Some(m)
}
/// `world` turned about `pivot`'s position by the rotation that makes `pivot`
/// look at `target` along its -z. Scale and anything offset from the pivot (the
/// strips of a row of lights) turn with it. Unchanged when the pivot sits on the
/// target or has a zero scale.
pub fn aimed(world: &Mat4, pivot: &Mat4, target: [f32; 3]) -> Mat4 {
    let at = point(pivot, [0.0; 3]);
    let axis = |k: usize| unit([pivot[0][k], pivot[1][k], pivot[2][k]]);
    let (Some(look), Some(x), Some(y), Some(z)) = (look_at(at, target), axis(0), axis(1), axis(2))
    else {
        return *world;
    };
    // The pivot's own rotation, undone by its transpose.
    let mut undo = IDENTITY;
    for (k, column) in [x, y, z].into_iter().enumerate() {
        undo[k][..3].copy_from_slice(&column);
    }
    let turn = mul(&look, &undo);
    mul(
        &mul(&translate(at), &turn),
        &mul(&translate(at.map(|c| -c)), world),
    )
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
