//! Bounded triangle meshes: closed vector outlines extruded into watertight solids (caps and walls share one flattening), and primitive solids.
//!
//! Frame: x right and y down in composition pixels, z towards the viewer. The front
//! cap sits at z = +depth/2 facing +z, the back cap at -depth/2 facing -z.
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillRule as LyonRule, FillTessellator, FillVertex, VertexBuffers,
    math::point, path::Path,
};
use veylvik_document::{
    Error,
    vector::{FillRule, PathCommand},
};

pub mod animation;
mod model;
mod primitives;
pub use model::{MAX_PARTS, MAX_TEXTURE, MAX_VERTICES, Model, Part, PointCache, Skin, Texture};
pub use primitives::{Primitive, SEGMENTS, bendable_plane, primitive};

pub type Result<T> = std::result::Result<T, Error>;
/// Bound on output size so a pathological outline cannot exhaust memory.
pub const MAX_TRIANGLES: usize = 2_000_000;
/// Side-wall corners sharper than this keep a hard edge; gentler ones are smoothed.
pub const SMOOTH_ANGLE_DEGREES: f32 = 35.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Caps: composition pixels. Sides: distance along the outline and depth, in pixels.
    pub uv: [f32; 2],
}
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    /// Triangles as vertex index triples; caps first, then sides.
    pub indices: Vec<u32>,
    /// Index count of the two caps; the rest are side walls.
    pub cap_indices: usize,
}
fn fail<T>(message: &str) -> Result<T> {
    Err(Error(message.into()))
}

/// Closed polylines of an outline, flattened to within `tolerance` pixels.
pub fn contours(commands: &[PathCommand], tolerance: f32) -> Result<Vec<Vec<[f32; 2]>>> {
    if !(tolerance.is_finite() && tolerance > 0.0) {
        return fail("flattening tolerance must be positive");
    }
    let mut builder = Path::builder();
    let mut open = false;
    for command in commands {
        match *command {
            PathCommand::Move { to } => {
                if open {
                    builder.end(true);
                }
                builder.begin(point(to[0], to[1]));
                open = true;
            }
            PathCommand::Line { to } if open => {
                builder.line_to(point(to[0], to[1]));
            }
            PathCommand::Quad { control, to } if open => {
                builder.quadratic_bezier_to(point(control[0], control[1]), point(to[0], to[1]));
            }
            PathCommand::Cubic {
                control1,
                control2,
                to,
            } if open => {
                builder.cubic_bezier_to(
                    point(control1[0], control1[1]),
                    point(control2[0], control2[1]),
                    point(to[0], to[1]),
                );
            }
            PathCommand::Close {} if open => {
                builder.end(true);
                open = false;
            }
            _ => return fail("path segment without a starting point"),
        }
    }
    if open {
        builder.end(true);
    }
    let path = builder.build();
    let mut out = Vec::new();
    let mut current: Vec<[f32; 2]> = Vec::new();
    use lyon_tessellation::path::{Event, iterator::PathIterator};
    for event in path.iter().flattened(tolerance) {
        match event {
            Event::Begin { at } => current = vec![[at.x, at.y]],
            Event::Line { to, .. } => {
                if current.last() != Some(&[to.x, to.y]) {
                    current.push([to.x, to.y]);
                }
            }
            Event::End { .. } => {
                while current.len() > 1 && current.first() == current.last() {
                    current.pop();
                }
                if current.len() >= 3 {
                    out.push(std::mem::take(&mut current));
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Winding number of `p` against all contours.
fn winding(contours: &[Vec<[f32; 2]>], p: [f32; 2]) -> i32 {
    let mut winding = 0;
    for contour in contours {
        for (i, a) in contour.iter().enumerate() {
            let b = contour[(i + 1) % contour.len()];
            let cross = (b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1]);
            if a[1] <= p[1] {
                if b[1] > p[1] && cross > 0.0 {
                    winding += 1;
                }
            } else if b[1] <= p[1] && cross < 0.0 {
                winding -= 1;
            }
        }
    }
    winding
}
fn filled(rule: FillRule, winding: i32) -> bool {
    match rule {
        FillRule::NonZero => winding != 0,
        FillRule::EvenOdd => winding % 2 != 0,
    }
}

/// The edge between a cap and the side walls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bevel {
    /// How far the bevel reaches in and down, in pixels; at most half the depth.
    pub width: f32,
    /// Rounded (a quarter circle in four steps) or a flat 45-degree chamfer.
    pub round: bool,
}
impl Bevel {
    pub const NONE: Self = Self {
        width: 0.0,
        round: false,
    };
}
/// Extrude an outline `depth` pixels deep with square edges.
pub fn extrude(
    commands: &[PathCommand],
    rule: FillRule,
    depth: f32,
    tolerance: f32,
) -> Result<Mesh> {
    extrude_bevelled(commands, rule, depth, tolerance, Bevel::NONE)
}
fn cross_z(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let (u, v) = (
        [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
        [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
    );
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}
/// Add a quad, wound so it faces along its vertices' normals.
fn quad(mesh: &mut Mesh, corners: [Vertex; 4]) {
    let base = mesh.vertices.len() as u32;
    let n = cross_z(
        corners[0].position,
        corners[1].position,
        corners[2].position,
    );
    let want: f32 = corners
        .iter()
        .map(|v| v.normal[0] * n[0] + v.normal[1] * n[1] + v.normal[2] * n[2])
        .sum();
    mesh.vertices.extend(corners);
    if want >= 0.0 {
        mesh.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    } else {
        mesh.indices
            .extend([base, base + 2, base + 1, base, base + 3, base + 2]);
    }
}
/// Extrude an outline `depth` pixels deep, with `bevel` on its front and back
/// edges. Empty outlines give an empty mesh.
pub fn extrude_bevelled(
    commands: &[PathCommand],
    rule: FillRule,
    depth: f32,
    tolerance: f32,
    bevel: Bevel,
) -> Result<Mesh> {
    if !(depth.is_finite() && depth >= 0.0) {
        return fail("extrusion depth must be finite and non-negative");
    }
    if !bevel.width.is_finite() {
        return fail("bevel width must be finite");
    }
    let contours = contours(commands, tolerance)?;
    let mut mesh = Mesh::default();
    if contours.is_empty() {
        return Ok(mesh);
    }
    let width = bevel.width.clamp(0.0, depth * 0.5);
    let (front, back) = (depth * 0.5, -depth * 0.5);
    let smooth = SMOOTH_ANGLE_DEGREES.to_radians().cos();
    // Which way each contour's walls face (+1 / -1), or None for a contour inside
    // the solid whose walls would be hidden.
    let facing: Vec<Option<f32>> = contours
        .iter()
        .map(|contour| {
            let (a, b) = (contour[0], contour[1]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let length = (dx * dx + dy * dy).sqrt().max(1e-6);
            let left = [-dy / length, dx / length];
            let probe = 0.01_f32.max(length * 1e-3);
            let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
            let at = |s: f32| {
                winding(
                    &contours,
                    [mid[0] + left[0] * probe * s, mid[1] + left[1] * probe * s],
                )
            };
            let (solid_left, solid_right) = (filled(rule, at(1.0)), filled(rule, at(-1.0)));
            (solid_left != solid_right).then_some(if solid_left { -1.0 } else { 1.0 })
        })
        .collect();
    // Outward edge normals, and for each vertex its miter (with length) and the
    // smoothed or split normals of the two walls meeting there.
    struct Ring {
        normals: Vec<[f32; 2]>,
        miter: Vec<[f32; 2]>,
    }
    let rings: Vec<Option<Ring>> = contours
        .iter()
        .zip(&facing)
        .map(|(contour, sign)| {
            let sign = (*sign)?;
            let n = contour.len();
            let normals: Vec<[f32; 2]> = (0..n)
                .map(|i| {
                    let (a, b) = (contour[i], contour[(i + 1) % n]);
                    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                    let l = (dx * dx + dy * dy).sqrt().max(1e-12);
                    [-dy / l * sign, dx / l * sign]
                })
                .collect();
            let miter = (0..n)
                .map(|i| {
                    let (a, b) = (normals[(i + n - 1) % n], normals[i]);
                    let s = [a[0] + b[0], a[1] + b[1]];
                    let l = (s[0] * s[0] + s[1] * s[1]).sqrt();
                    if l < 1e-6 {
                        return b;
                    }
                    let m = [s[0] / l, s[1] / l];
                    // Longer at corners so both edges move in evenly; capped at 4x.
                    let stretch = 1.0 / (m[0] * b[0] + m[1] * b[1]).max(0.25);
                    [m[0] * stretch, m[1] * stretch]
                })
                .collect();
            Some(Ring { normals, miter })
        })
        .collect();
    let inset = |contour: usize, i: usize, by: f32| -> [f32; 2] {
        let p = contours[contour][i];
        match &rings[contour] {
            Some(r) => [p[0] - r.miter[i][0] * by, p[1] - r.miter[i][1] * by],
            None => p,
        }
    };
    // Caps from the innermost ring of the same polylines, so edges meet exactly.
    let mut builder = Path::builder();
    for (c, contour) in contours.iter().enumerate() {
        let first = inset(c, 0, width);
        builder.begin(point(first[0], first[1]));
        for i in 1..contour.len() {
            let p = inset(c, i, width);
            builder.line_to(point(p[0], p[1]));
        }
        builder.end(true);
    }
    let path = builder.build();
    let mut cap: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    let options = FillOptions::tolerance(tolerance).with_fill_rule(match rule {
        FillRule::NonZero => LyonRule::NonZero,
        FillRule::EvenOdd => LyonRule::EvenOdd,
    });
    FillTessellator::new()
        .tessellate_path(
            &path,
            &options,
            &mut BuffersBuilder::new(&mut cap, |v: FillVertex| v.position().to_array()),
        )
        .map_err(|e| Error(format!("cannot triangulate the outline: {e:?}")))?;
    if cap.indices.len() / 3 * 2 > MAX_TRIANGLES {
        return fail("outline is too detailed to extrude");
    }
    for (z, normal) in [(front, 1.0f32), (back, -1.0)] {
        let base = mesh.vertices.len() as u32;
        mesh.vertices.extend(cap.vertices.iter().map(|p| Vertex {
            position: [p[0], p[1], z],
            normal: [0.0, 0.0, normal],
            uv: *p,
        }));
        for t in cap.indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| cap.vertices[i as usize]);
            // Orient every triangle to face along its cap normal (x right, y down, z out).
            let facing = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            let flip = (facing < 0.0) == (normal > 0.0);
            let (i1, i2) = if flip { (t[2], t[1]) } else { (t[1], t[2]) };
            mesh.indices.extend([base + t[0], base + i1, base + i2]);
        }
    }
    mesh.cap_indices = mesh.indices.len();
    if depth == 0.0 {
        return Ok(mesh);
    }
    // The edge profile from the wall (step 0) to the cap (last step): how far in,
    // how far down from the cap, and the angle of its normal from the wall.
    let steps: Vec<(f32, f32, f32)> = if width <= 0.0 {
        Vec::new()
    } else if bevel.round {
        (0..=4)
            .map(|k| {
                let phi = k as f32 / 4.0 * std::f32::consts::FRAC_PI_2;
                (width * (1.0 - phi.cos()), width * (1.0 - phi.sin()), phi)
            })
            .collect()
    } else {
        let flat = std::f32::consts::FRAC_PI_4;
        vec![(0.0, width, flat), (width, 0.0, flat)]
    };
    for (c, contour) in contours.iter().enumerate() {
        let Some(ring) = &rings[c] else {
            continue;
        };
        let n = contour.len();
        if (mesh.indices.len() / 3) + n * 2 * (1 + steps.len() * 2) > MAX_TRIANGLES {
            return fail("outline is too detailed to extrude");
        }
        // A vertex shared by two walls is smoothed only when they meet gently.
        let vertex_normal = |edge: usize, at_start: bool| {
            let other = if at_start {
                (edge + n - 1) % n
            } else {
                (edge + 1) % n
            };
            let (m, o) = (ring.normals[edge], ring.normals[other]);
            if m[0] * o[0] + m[1] * o[1] >= smooth {
                let s = [m[0] + o[0], m[1] + o[1]];
                let l = (s[0] * s[0] + s[1] * s[1]).sqrt().max(1e-12);
                [s[0] / l, s[1] / l]
            } else {
                m
            }
        };
        let mut along = 0.0f32;
        for i in 0..n {
            let j = (i + 1) % n;
            let (a, b) = (contour[i], contour[j]);
            let length = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            let (na, nb) = (vertex_normal(i, true), vertex_normal(i, false));
            let vertex = |p: [f32; 2], z: f32, nxy: [f32; 2], phi: f32, side: f32, u: f32| Vertex {
                position: [p[0], p[1], z],
                normal: [nxy[0] * phi.cos(), nxy[1] * phi.cos(), side * phi.sin()],
                uv: [u, front - z],
            };
            // The straight wall between the two bevels.
            let (top, bottom) = (front - width, back + width);
            if top > bottom + 1e-6 || width <= 0.0 {
                quad(
                    &mut mesh,
                    [
                        vertex(a, top, na, 0.0, 1.0, along),
                        vertex(b, top, nb, 0.0, 1.0, along + length),
                        vertex(b, bottom, nb, 0.0, 1.0, along + length),
                        vertex(a, bottom, na, 0.0, 1.0, along),
                    ],
                );
            }
            // Front and back bevels, step by step from the wall to the cap.
            for (side, cap_z) in [(1.0f32, front), (-1.0f32, back)] {
                for k in 0..steps.len().saturating_sub(1) {
                    let (d0, h0, p0) = steps[k];
                    let (d1, h1, p1) = steps[k + 1];
                    let (z0, z1) = (cap_z - side * h0, cap_z - side * h1);
                    quad(
                        &mut mesh,
                        [
                            vertex(inset(c, i, d0), z0, na, p0, side, along),
                            vertex(inset(c, j, d0), z0, nb, p0, side, along + length),
                            vertex(inset(c, j, d1), z1, nb, p1, side, along + length),
                            vertex(inset(c, i, d1), z1, na, p1, side, along),
                        ],
                    );
                }
            }
            along += length;
        }
    }
    Ok(mesh)
}
