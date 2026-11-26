//! Closed primitive solids centred on the origin, sized in pixels, with outward normals and wrap-around uv.
use crate::{Mesh, Result, Vertex, fail};
use std::f32::consts::{PI, TAU};

/// Primitive kinds; `Plane` is a single two-sided-free quad facing +z.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Primitive {
    Cube,
    Sphere,
    Plane,
    Cylinder,
    Cone,
    Torus,
    /// A flat ring facing +z; z is its width from the rim inwards.
    Ring,
}
/// Segments around curved primitives are clamped to this range.
pub const SEGMENTS: std::ops::RangeInclusive<u32> = 3..=256;

/// `size` is the bounding box (x, y, z) in pixels; a torus uses z as its tube
/// thickness. `segments` sets the smoothness of curved primitives.
pub fn primitive(kind: Primitive, size: [f32; 3], segments: u32) -> Result<Mesh> {
    if size.iter().any(|s| !(s.is_finite() && *s >= 0.0)) {
        return fail("primitive size must be finite and non-negative");
    }
    let n = segments.clamp(*SEGMENTS.start(), *SEGMENTS.end()) as usize;
    let [hx, hy, hz] = size.map(|s| s * 0.5);
    let mut mesh = Mesh::default();
    match kind {
        Primitive::Plane => quad(
            &mut mesh,
            [
                [-hx, -hy, 0.0],
                [hx, -hy, 0.0],
                [hx, hy, 0.0],
                [-hx, hy, 0.0],
            ],
            [0.0, 0.0, 1.0],
        ),
        Primitive::Cube => {
            for (axis, sign) in [
                (0, 1.0),
                (0, -1.0),
                (1, 1.0),
                (1, -1.0),
                (2, 1.0),
                (2, -1.0),
            ] {
                let mut normal = [0.0; 3];
                normal[axis] = sign;
                let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                let h = [hx, hy, hz];
                let corner = |a: f32, b: f32| {
                    let mut p = [0.0; 3];
                    p[axis] = sign * h[axis];
                    p[u] = a * h[u];
                    p[v] = b * h[v];
                    p
                };
                quad(
                    &mut mesh,
                    [
                        corner(-1., -1.),
                        corner(1., -1.),
                        corner(1., 1.),
                        corner(-1., 1.),
                    ],
                    normal,
                );
            }
        }
        Primitive::Sphere => {
            let rings = n.div_ceil(2).max(2);
            grid(&mut mesh, n, rings, |i, j| {
                // A picture wrapped round reads the right way from outside, its middle
                // facing the viewer and its seam at the back.
                let (theta, phi) = (around(i, n), j as f32 / rings as f32 * PI);
                let d = [phi.sin() * theta.cos(), -phi.cos(), phi.sin() * theta.sin()];
                (
                    [d[0] * hx, d[1] * hy, d[2] * hz],
                    normalize([
                        d[0] / hx.max(1e-6),
                        d[1] / hy.max(1e-6),
                        d[2] / hz.max(1e-6),
                    ]),
                )
            });
        }
        Primitive::Cylinder | Primitive::Cone => {
            let top = if kind == Primitive::Cone { 0.0 } else { 1.0 };
            // Side: radius shrinks from the base (y = +hy) to the top (y = -hy).
            let slope = (1.0 - top) * hx.max(hz) / (2.0 * hy).max(1e-6);
            grid(&mut mesh, n, 1, |i, j| {
                let theta = around(i, n);
                let r = if j == 0 { top } else { 1.0 };
                let (c, s) = (theta.cos(), theta.sin());
                (
                    [c * hx * r, if j == 0 { -hy } else { hy }, s * hz * r],
                    // Narrowing towards -y, the wall leans its normal that way.
                    normalize([c, -slope, s]),
                )
            });
            disc(&mut mesh, n, [hx, hz], hy, [0.0, 1.0, 0.0]);
            if top > 0.0 {
                disc(&mut mesh, n, [hx, hz], -hy, [0.0, -1.0, 0.0]);
            }
        }
        Primitive::Torus => {
            let tube = hz.min(hx.min(hy)).max(0.0);
            let (rx, ry) = ((hx - tube).max(0.0), (hy - tube).max(0.0));
            grid(&mut mesh, n, n, |i, j| {
                let (u, v) = (i as f32 / n as f32 * TAU, j as f32 / n as f32 * TAU);
                let ring = [u.cos(), u.sin()];
                let normal = [ring[0] * v.cos(), ring[1] * v.cos(), v.sin()];
                (
                    [
                        ring[0] * rx + normal[0] * tube,
                        ring[1] * ry + normal[1] * tube,
                        normal[2] * tube,
                    ],
                    normal,
                )
            });
        }
        Primitive::Ring => {
            // From the inner edge (row 0) to the rim (row 1); a width of the whole
            // radius closes it into a disc.
            let width = (size[2] / hx.min(hy).max(1e-6)).clamp(0.0, 1.0);
            grid(&mut mesh, n, 1, |i, j| {
                let u = i as f32 / n as f32 * TAU;
                let reach = 1.0 - width * (1 - j) as f32;
                (
                    [u.cos() * hx * reach, u.sin() * hy * reach, 0.0],
                    [0.0, 0.0, 1.0],
                )
            });
        }
    }
    mesh.cap_indices = mesh.indices.len();
    Ok(mesh)
}
/// The angle of column `i` of `n` round the vertical axis, from +x towards +z: uv
/// runs left to right as seen from the viewer (+z), 0.5 at the front and the seam
/// (0 and 1) at the back.
fn around(i: usize, n: usize) -> f32 {
    (0.75 - i as f32 / n as f32) * TAU
}
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
    v.map(|x| x / l)
}
/// A quad (corners in order around `normal`), wound to face it.
/// A flat plane of `size` (x by y) facing +z, cut into `segments` by `segments` squares,
/// so it can be bent (the Plane primitive is one quad, which cannot).
pub fn bendable_plane(size: [f32; 2], segments: u32) -> Mesh {
    let n = segments.clamp(1, 512);
    let mut mesh = Mesh::default();
    for j in 0..=n {
        for i in 0..=n {
            let (u, v) = (i as f32 / n as f32, j as f32 / n as f32);
            mesh.vertices.push(Vertex {
                position: [(u - 0.5) * size[0], (v - 0.5) * size[1], 0.0],
                normal: [0.0, 0.0, 1.0],
                uv: [u, v],
            });
        }
    }
    let row = n + 1;
    for j in 0..n {
        for i in 0..n {
            let a = j * row + i;
            // As the Plane primitive winds its quad, facing +z.
            mesh.indices
                .extend([a, a + 1, a + row + 1, a, a + row + 1, a + row]);
        }
    }
    mesh
}
fn quad(mesh: &mut Mesh, corners: [[f32; 3]; 4], normal: [f32; 3]) {
    let base = mesh.vertices.len() as u32;
    for (corner, uv) in corners.iter().zip([[0., 0.], [1., 0.], [1., 1.], [0., 1.]]) {
        mesh.vertices.push(Vertex {
            position: *corner,
            normal,
            uv,
        });
    }
    let [a, b, c] = [corners[0], corners[1], corners[2]];
    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    let along = n[0] * normal[0] + n[1] * normal[1] + n[2] * normal[2] >= 0.0;
    if along {
        mesh.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    } else {
        mesh.indices
            .extend([base, base + 2, base + 1, base, base + 3, base + 2]);
    }
}
/// A (columns + 1) x (rows + 1) vertex grid wrapped around in columns, wound so each
/// triangle faces its vertices' normals.
fn grid(
    mesh: &mut Mesh,
    columns: usize,
    rows: usize,
    at: impl Fn(usize, usize) -> ([f32; 3], [f32; 3]),
) {
    let base = mesh.vertices.len() as u32;
    for j in 0..=rows {
        for i in 0..=columns {
            let (position, normal) = at(i % columns, j);
            mesh.vertices.push(Vertex {
                position,
                normal,
                uv: [i as f32 / columns as f32, j as f32 / rows as f32],
            });
        }
    }
    let index = |i: usize, j: usize| base + (j * (columns + 1) + i) as u32;
    for j in 0..rows {
        for i in 0..columns {
            for tri in [
                [index(i, j), index(i + 1, j), index(i + 1, j + 1)],
                [index(i, j), index(i + 1, j + 1), index(i, j + 1)],
            ] {
                let p = tri.map(|k| mesh.vertices[k as usize].position);
                let e1 = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
                let e2 = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
                let n = [
                    e1[1] * e2[2] - e1[2] * e2[1],
                    e1[2] * e2[0] - e1[0] * e2[2],
                    e1[0] * e2[1] - e1[1] * e2[0],
                ];
                if n.iter().all(|v| v.abs() < 1e-9) {
                    continue; // collapsed at a pole or apex
                }
                let normal: [f32; 3] = (0..3)
                    .map(|k| {
                        tri.iter()
                            .map(|&v| mesh.vertices[v as usize].normal[k])
                            .sum::<f32>()
                    })
                    .collect::<Vec<_>>()
                    .try_into()
                    .unwrap();
                if n[0] * normal[0] + n[1] * normal[1] + n[2] * normal[2] >= 0.0 {
                    mesh.indices.extend(tri);
                } else {
                    mesh.indices.extend([tri[0], tri[2], tri[1]]);
                }
            }
        }
    }
}
/// A flat disc of radii `r` at height `y`, facing `normal`.
fn disc(mesh: &mut Mesh, n: usize, r: [f32; 2], y: f32, normal: [f32; 3]) {
    let centre = mesh.vertices.len() as u32;
    mesh.vertices.push(Vertex {
        position: [0.0, y, 0.0],
        normal,
        uv: [0.5, 0.5],
    });
    for i in 0..n {
        let theta = i as f32 / n as f32 * TAU;
        mesh.vertices.push(Vertex {
            position: [theta.cos() * r[0], y, theta.sin() * r[1]],
            normal,
            uv: [0.5 + theta.cos() * 0.5, 0.5 + theta.sin() * 0.5],
        });
    }
    for i in 0..n as u32 {
        let (a, b) = (centre + 1 + i, centre + 1 + (i + 1) % n as u32);
        let pa = mesh.vertices[a as usize].position;
        let pb = mesh.vertices[b as usize].position;
        // (pa - c) x (pb - c) has y = pa.z*pb.x - pa.x*pb.z.
        let up = pa[2] * pb[0] - pa[0] * pb[2];
        if (up >= 0.0) == (normal[1] >= 0.0) {
            mesh.indices.extend([centre, a, b]);
        } else {
            mesh.indices.extend([centre, b, a]);
        }
    }
}
