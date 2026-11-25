//! The body round a pose: smoothly blended rounded shapes (pelvis and buttocks, waist, ribcage, shoulders, neck and head with jaw and nose, tapered limbs with calf and thigh muscle, feet with heel, instep and toes) as a distance field, turned into a smooth mesh by surface nets with normals from the field. Lengths are shares of the height; the mesh is scaled to pixels.
use crate::Pose;
use veylvik_mesh::{Mesh, Vertex};

type V = [f32; 3];
fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn addv(a: V, b: V, s: f32) -> V {
    [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s]
}
fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn len(a: V) -> f32 {
    dot(a, a).sqrt()
}
fn norm(a: V) -> V {
    let l = len(a).max(1e-9);
    [a[0] / l, a[1] / l, a[2] / l]
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    b + (a - b) * h - k * h * (1.0 - h)
}
/// Distance to a cone with round ends: radius ra at a, rb at b.
fn round_cone(p: V, a: V, b: V, ra: f32, rb: f32) -> f32 {
    let ba = sub(b, a);
    let l2 = dot(ba, ba).max(1e-12);
    let rr = ra - rb;
    let a2 = l2 - rr * rr;
    let il2 = 1.0 / l2;
    let pa = sub(p, a);
    let y = dot(pa, ba);
    let z = y - l2;
    let xv = addv(pa, ba, -y * il2);
    let x2 = dot(xv, xv) * l2 * l2;
    let y2 = y * y * l2;
    let z2 = z * z * l2;
    let k = rr.signum() * rr * rr * x2;
    if z.signum() * a2 * z2 > k {
        return (x2 + z2).sqrt() * il2 - rb;
    }
    if y.signum() * a2 * y2 < k {
        return (x2 + y2).sqrt() * il2 - ra;
    }
    ((x2 * a2 * il2).sqrt() + y * rr) * il2 - ra
}
/// Distance (approximate) to an ellipsoid with these radii along the axes u, v and w.
fn ellipsoid(p: V, c: V, axes: [V; 3], r: V) -> f32 {
    let d = sub(p, c);
    let q = [
        dot(d, axes[0]) / r[0],
        dot(d, axes[1]) / r[1],
        dot(d, axes[2]) / r[2],
    ];
    let k0 = len(q);
    let q2 = [q[0] / r[0], q[1] / r[1], q[2] / r[2]];
    let k1 = len(q2).max(1e-9);
    k0 * (k0 - 1.0) / k1
}
fn sphere(p: V, c: V, r: f32) -> f32 {
    len(sub(p, c)) - r
}
/// A point turned by `deg` degrees in the side plane (x, y) about `base`.
fn nod(base: V, offset: [f32; 2], deg: f32) -> V {
    let r = deg.to_radians();
    [
        base[0] + offset[0] * r.cos() - offset[1] * r.sin(),
        base[1] + offset[0] * r.sin() + offset[1] * r.cos(),
        base[2],
    ]
}

/// One rounded shape of the body, ready to measure.
#[derive(Clone, Copy)]
enum Shape {
    Cone(V, V, f32, f32),
    Ellipsoid(V, [V; 3], V),
    Sphere(V, f32),
}
impl Shape {
    fn distance(&self, p: V) -> f32 {
        match *self {
            Shape::Cone(a, b, ra, rb) => round_cone(p, a, b, ra, rb),
            Shape::Ellipsoid(c, axes, r) => ellipsoid(p, c, axes, r),
            Shape::Sphere(c, r) => sphere(p, c, r),
        }
    }
    /// A sphere holding the shape: its centre and radius.
    fn bounds(&self) -> (V, f32) {
        match *self {
            Shape::Cone(a, b, ra, rb) => {
                let c = addv(a, sub(b, a), 0.5);
                (c, len(sub(b, a)) * 0.5 + ra.max(rb))
            }
            Shape::Ellipsoid(c, _, r) => (c, r[0].max(r[1]).max(r[2])),
            Shape::Sphere(c, r) => (c, r),
        }
    }
}
/// A part of the body: shapes blended in order (each with its own blend), then blended
/// into the body with `k`.
struct Part {
    shapes: Vec<(Shape, f32, V, f32)>,
    k: f32,
}
/// The body round one pose, its shapes worked out once.
pub struct Body {
    parts: Vec<Part>,
}
impl Body {
    pub fn new(pose: &Pose) -> Self {
        let up = pose.up;
        let fwd = [-up[1], up[0], 0.0];
        let side = [0.0, 0.0, 1.0];
        let frame = [up, fwd, side];
        let hip = pose.pelvis;
        let top = pose.chest;
        let mut parts = Vec::new();
        let mut part = |k: f32, shapes: Vec<(Shape, f32)>| {
            parts.push(Part {
                shapes: shapes
                    .into_iter()
                    .map(|(s, blend)| {
                        let (c, r) = s.bounds();
                        (s, blend, c, r)
                    })
                    .collect(),
                k,
            })
        };
        // Pelvis, buttocks behind the hip joints, waist, ribcage and shoulders (widths as
        // shares of the height: hips about 0.2, chest about 0.19 across).
        let mut trunk = vec![
            (
                Shape::Ellipsoid(
                    addv(addv(hip, up, 0.035), fwd, -0.008),
                    frame,
                    [0.075, 0.062, 0.1],
                ),
                0.0,
            ),
            (
                Shape::Ellipsoid(
                    addv(addv(hip, fwd, -0.04), up, 0.005),
                    frame,
                    [0.06, 0.05, 0.09],
                ),
                0.02,
            ),
            (
                Shape::Ellipsoid(
                    addv(addv(hip, up, 0.13), fwd, 0.004),
                    frame,
                    [0.085, 0.06, 0.085],
                ),
                0.06,
            ),
            (
                Shape::Ellipsoid(
                    addv(addv(top, up, 0.215), fwd, 0.01),
                    frame,
                    [0.085, 0.072, 0.095],
                ),
                0.07,
            ),
        ];
        for s in &pose.shoulder {
            trunk.push((
                Shape::Ellipsoid(
                    addv(addv(*s, up, -0.004), side, -0.008 * s[2].signum()),
                    [fwd, up, side],
                    [0.046, 0.05, 0.036],
                ),
                0.03,
            ));
        }
        trunk.push((Shape::Cone(pose.neck, pose.head, 0.034, 0.029), 0.016));
        part(0.0, trunk);
        // The head (skull, jaw, nose), turned by its small nod.
        let pitch = pose.head_pitch;
        let along = nod([0.0; 3], [0.97, -0.24], pitch);
        let jaw_dir = nod([0.0; 3], [0.8, 0.6], pitch);
        part(
            0.012,
            vec![
                (
                    Shape::Ellipsoid(
                        nod(pose.head, [-0.006, -0.06], pitch),
                        [norm(along), norm(cross(side, along)), side],
                        [0.066, 0.06, 0.052],
                    ),
                    0.0,
                ),
                (
                    Shape::Ellipsoid(
                        nod(pose.head, [0.028, -0.03], pitch),
                        [norm(jaw_dir), norm(cross(side, jaw_dir)), side],
                        [0.04, 0.03, 0.04],
                    ),
                    0.016,
                ),
                (
                    Shape::Sphere(nod(pose.head, [0.058, -0.06], pitch), 0.011),
                    0.012,
                ),
            ],
        );
        // Legs and feet.
        for i in 0..2 {
            let (h, k, a) = (pose.hip[i], pose.knee[i], pose.ankle[i]);
            let thigh = norm(sub(k, h));
            let front = norm(cross(side, thigh));
            let front = if front[0] < 0.0 {
                [-front[0], -front[1], 0.0]
            } else {
                front
            };
            let shin = norm(sub(a, k));
            let back = norm(cross(shin, side));
            let back = if back[0] > 0.0 {
                [-back[0], -back[1], 0.0]
            } else {
                back
            };
            let (heel, ball, toe) = (pose.heel[i], pose.ball[i], pose.toe[i]);
            let sole = norm(sub(ball, heel));
            let topv = norm(cross(sole, side));
            let topv = if topv[1] > 0.0 {
                [-topv[0], -topv[1], 0.0]
            } else {
                topv
            };
            let toe_dir = norm(sub(toe, ball));
            let toe_top = norm(cross(toe_dir, side));
            let toe_top = if toe_top[1] > 0.0 {
                [-toe_top[0], -toe_top[1], 0.0]
            } else {
                toe_top
            };
            part(
                0.012,
                vec![
                    (Shape::Cone(h, k, 0.066, 0.04), 0.0),
                    (
                        Shape::Ellipsoid(
                            addv(addv(h, sub(k, h), 0.42), front, 0.006),
                            [thigh, front, side],
                            [0.11, 0.05, 0.055],
                        ),
                        0.02,
                    ),
                    (Shape::Cone(k, a, 0.038, 0.022), 0.015),
                    (
                        Shape::Ellipsoid(
                            addv(addv(k, sub(a, k), 0.3), back, 0.016),
                            [shin, back, side],
                            [0.085, 0.034, 0.04],
                        ),
                        0.02,
                    ),
                    (
                        Shape::Sphere(addv(addv(heel, topv, 0.024), sole, 0.012), 0.025),
                        0.012,
                    ),
                    (
                        Shape::Cone(
                            addv(addv(heel, topv, 0.014), sole, 0.02),
                            addv(ball, topv, 0.011),
                            0.016,
                            0.013,
                        ),
                        0.01,
                    ),
                    (
                        Shape::Cone(a, addv(addv(ball, topv, 0.016), sole, -0.012), 0.023, 0.014),
                        0.014,
                    ),
                    (
                        Shape::Cone(
                            addv(ball, topv, 0.01),
                            addv(toe, toe_top, 0.007),
                            0.013,
                            0.009,
                        ),
                        0.006,
                    ),
                ],
            );
        }
        // Arms and hands (the shoulders sit just outside the ribcage, so an arm lies against
        // the body rather than through it).
        for i in 0..2 {
            let (s, e, w, h) = (pose.shoulder[i], pose.elbow[i], pose.wrist[i], pose.hand[i]);
            let along = norm(sub(h, w));
            let flat = norm(cross(along, side));
            part(
                0.01,
                vec![
                    (Shape::Cone(s, e, 0.034, 0.025), 0.0),
                    (Shape::Cone(e, w, 0.024, 0.016), 0.01),
                    (
                        Shape::Ellipsoid(
                            addv(w, sub(h, w), 0.5),
                            [along, flat, side],
                            [0.05, 0.018, 0.035],
                        ),
                        0.008,
                    ),
                ],
            );
        }
        Body { parts }
    }
    /// The distance field at p (inside negative), in heights. Shapes too far away to
    /// change the blend are skipped (a blend never reaches beyond its own width).
    pub fn distance(&self, p: V) -> f32 {
        let mut d = f32::MAX;
        for part in &self.parts {
            let mut own = f32::MAX;
            for (shape, blend, c, r) in &part.shapes {
                if own != f32::MAX && len(sub(p, *c)) - r >= own + blend {
                    continue;
                }
                let v = shape.distance(p);
                own = if own == f32::MAX {
                    v
                } else {
                    smin(own, v, *blend)
                };
            }
            d = if d == f32::MAX || part.k == 0.0 {
                d.min(own)
            } else if own >= d + part.k {
                d
            } else {
                smin(d, own, part.k)
            };
        }
        d
    }
}
/// The body's distance field at p (inside negative), in heights.
pub fn distance(pose: &Pose, p: V) -> f32 {
    Body::new(pose).distance(p)
}

/// The body round `pose` as a smooth mesh in pixels for a figure `height` pixels tall,
/// sampled `detail` cells to the height (finer is smoother and slower).
pub fn mesh(pose: &Pose, height: f32, detail: u32) -> Mesh {
    let body = Body::new(pose);
    let detail = detail.clamp(24, 400) as f32;
    let cell = 1.0 / detail;
    // The box round every joint, with room for the flesh.
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for p in pose.points() {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    for a in 0..3 {
        lo[a] -= 0.16;
        hi[a] += 0.16;
    }
    let n = [0, 1, 2].map(|a| ((hi[a] - lo[a]) / cell).ceil() as usize + 1);
    let at = |i: usize, j: usize, k: usize| {
        [
            lo[0] + i as f32 * cell,
            lo[1] + j as f32 * cell,
            lo[2] + k as f32 * cell,
        ]
    };
    let index = |i: usize, j: usize, k: usize| (k * n[1] + j) * n[0] + i;
    // Coarse first: only near the surface is the field sampled finely.
    let step = 4;
    let mut field = vec![f32::NAN; n[0] * n[1] * n[2]];
    let coarse = |c: usize, len: usize| (c * step).min(len - 1);
    let (cn0, cn1, cn2) = (
        n[0].div_ceil(step) + 1,
        n[1].div_ceil(step) + 1,
        n[2].div_ceil(step) + 1,
    );
    let mut rough = vec![0.0f32; cn0 * cn1 * cn2];
    for ck in 0..cn2 {
        for cj in 0..cn1 {
            for ci in 0..cn0 {
                rough[(ck * cn1 + cj) * cn0 + ci] =
                    body.distance(at(coarse(ci, n[0]), coarse(cj, n[1]), coarse(ck, n[2])));
            }
        }
    }
    let reach = cell * step as f32 * 1.8;
    // Slices of the grid share out across the processor's cores.
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(1, 32);
    let slice = n[0] * n[1];
    std::thread::scope(|scope| {
        for (t, chunk) in field.chunks_mut(slice * n[2].div_ceil(threads)).enumerate() {
            let (body, rough, at) = (&body, &rough, &at);
            scope.spawn(move || {
                let first = t * n[2].div_ceil(threads);
                for (offset, value) in chunk.iter_mut().enumerate() {
                    let flat = offset;
                    let (i, j, k) = (flat % n[0], (flat / n[0]) % n[1], first + flat / slice);
                    let (ci, cj, ck) = (i / step, j / step, k / step);
                    let mut near = false;
                    let mut sign = 0.0;
                    for dk in 0..2 {
                        for dj in 0..2 {
                            for di in 0..2 {
                                let v = rough[((ck + dk).min(cn2 - 1) * cn1
                                    + (cj + dj).min(cn1 - 1))
                                    * cn0
                                    + (ci + di).min(cn0 - 1)];
                                near |= v.abs() < reach;
                                sign = v;
                            }
                        }
                    }
                    *value = if near {
                        body.distance(at(i, j, k))
                    } else {
                        sign
                    };
                }
            });
        }
    });
    // Surface nets: a vertex in every cell the surface crosses, at the mean of where it
    // crosses the cell's edges; a quad across every grid edge it crosses.
    let mut vertex_of = vec![u32::MAX; n[0] * n[1] * n[2]];
    let mut mesh = Mesh::default();
    const CORNERS: [[usize; 3]; 8] = [
        [0, 0, 0],
        [1, 0, 0],
        [0, 1, 0],
        [1, 1, 0],
        [0, 0, 1],
        [1, 0, 1],
        [0, 1, 1],
        [1, 1, 1],
    ];
    const EDGES: [[usize; 2]; 12] = [
        [0, 1],
        [2, 3],
        [4, 5],
        [6, 7],
        [0, 2],
        [1, 3],
        [4, 6],
        [5, 7],
        [0, 4],
        [1, 5],
        [2, 6],
        [3, 7],
    ];
    let gradient = |p: V| {
        let e = cell * 0.5;
        norm([
            body.distance([p[0] + e, p[1], p[2]]) - body.distance([p[0] - e, p[1], p[2]]),
            body.distance([p[0], p[1] + e, p[2]]) - body.distance([p[0], p[1] - e, p[2]]),
            body.distance([p[0], p[1], p[2] + e]) - body.distance([p[0], p[1], p[2] - e]),
        ])
    };
    for k in 0..n[2] - 1 {
        for j in 0..n[1] - 1 {
            for i in 0..n[0] - 1 {
                let v: [f32; 8] = CORNERS.map(|c| field[index(i + c[0], j + c[1], k + c[2])]);
                let inside = v.iter().filter(|x| **x < 0.0).count();
                if inside == 0 || inside == 8 {
                    continue;
                }
                let mut sum = [0.0f32; 3];
                let mut count = 0.0;
                for e in EDGES {
                    let (a, b) = (v[e[0]], v[e[1]]);
                    if (a < 0.0) != (b < 0.0) {
                        let t = a / (a - b);
                        let pa = at(
                            i + CORNERS[e[0]][0],
                            j + CORNERS[e[0]][1],
                            k + CORNERS[e[0]][2],
                        );
                        let pb = at(
                            i + CORNERS[e[1]][0],
                            j + CORNERS[e[1]][1],
                            k + CORNERS[e[1]][2],
                        );
                        for a in 0..3 {
                            sum[a] += pa[a] + (pb[a] - pa[a]) * t;
                        }
                        count += 1.0;
                    }
                }
                let p = [sum[0] / count, sum[1] / count, sum[2] / count];
                let normal = gradient(p);
                vertex_of[index(i, j, k)] = mesh.vertices.len() as u32;
                mesh.vertices.push(Vertex {
                    position: [p[0] * height, p[1] * height, p[2] * height],
                    normal,
                    uv: [p[0] * height, p[1] * height],
                });
            }
        }
    }
    let mut quad = |a: usize, b: usize, c: usize, d: usize| {
        let q = [vertex_of[a], vertex_of[b], vertex_of[c], vertex_of[d]];
        if q.contains(&u32::MAX) {
            return;
        }
        for tri in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
            let [p0, p1, p2] = tri.map(|t| mesh.vertices[t as usize].position);
            let face = cross(sub(p1, p0), sub(p2, p0));
            let normal = mesh.vertices[tri[0] as usize].normal;
            // Faces turn outwards, the way the field rises.
            if dot(face, normal) >= 0.0 {
                mesh.indices.extend(tri);
            } else {
                mesh.indices.extend([tri[0], tri[2], tri[1]]);
            }
        }
    };
    for k in 1..n[2] - 1 {
        for j in 1..n[1] - 1 {
            for i in 1..n[0] - 1 {
                let here = field[index(i, j, k)] < 0.0;
                if here != (field[index(i + 1, j, k)] < 0.0) {
                    quad(
                        index(i, j - 1, k - 1),
                        index(i, j, k - 1),
                        index(i, j, k),
                        index(i, j - 1, k),
                    );
                }
                if here != (field[index(i, j + 1, k)] < 0.0) {
                    quad(
                        index(i - 1, j, k - 1),
                        index(i, j, k - 1),
                        index(i, j, k),
                        index(i - 1, j, k),
                    );
                }
                if here != (field[index(i, j, k + 1)] < 0.0) {
                    quad(
                        index(i - 1, j - 1, k),
                        index(i, j - 1, k),
                        index(i, j, k),
                        index(i - 1, j, k),
                    );
                }
            }
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_round_cone_measures_its_own_surface() {
        // Points on the side and the ends of a cone from radius 0.1 to 0.05 are at
        // distance 0; the axis inside is at minus the radius there.
        let (a, b) = ([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let d = |p: V| round_cone(p, a, b, 0.1, 0.05);
        assert!(d([0.0, 0.0, 0.0]) < -0.099 && d([0.0, 0.0, 0.0]) > -0.101);
        assert!(d([0.0, 1.0, 0.0]).abs() - 0.05 < 0.001);
        assert!(d([0.0, -0.1, 0.0]).abs() < 0.001, "{}", d([0.0, -0.1, 0.0]));
        assert!(d([0.0, 1.05, 0.0]).abs() < 0.001, "{}", d([0.0, 1.05, 0.0]));
        assert!(
            d([0.075, 0.5, 0.0]).abs() < 0.003,
            "{}",
            d([0.075, 0.5, 0.0])
        );
        assert!(d([0.5, 0.5, 0.0]) > 0.4);
    }
    #[test]
    fn the_body_is_one_closed_surface_round_the_pose() {
        // Every joint is inside the body (the heel, ball and toe are where the sole meets
        // the ground, on its skin), and the mesh's every edge is shared by two faces.
        let pose = crate::walk(0.3, 1.0);
        let mut joints = vec![pose.pelvis, pose.neck, pose.head];
        for s in 0..2 {
            joints.extend([
                pose.hip[s],
                pose.knee[s],
                pose.ankle[s],
                pose.shoulder[s],
                pose.elbow[s],
                pose.wrist[s],
            ]);
        }
        for p in joints {
            assert!(
                distance(&pose, p) < 0.0,
                "a joint is outside the body: {p:?}"
            );
        }
        let mesh = mesh(&pose, 1.0, 80);
        let mut edges = std::collections::HashMap::new();
        for t in mesh.indices.chunks(3) {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        let open = edges.values().filter(|n| **n != 2).count();
        assert!(
            open * 100 < edges.len(),
            "{open} of {} edges are not shared by two faces",
            edges.len()
        );
    }
}
