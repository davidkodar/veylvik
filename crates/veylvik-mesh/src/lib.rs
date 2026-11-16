//! Extrudes closed vector outlines into watertight, bounded triangle meshes: two caps and side walls sharing one flattening.
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

/// Extrude an outline `depth` pixels deep. Empty outlines give an empty mesh.
pub fn extrude(
    commands: &[PathCommand],
    rule: FillRule,
    depth: f32,
    tolerance: f32,
) -> Result<Mesh> {
    if !(depth.is_finite() && depth >= 0.0) {
        return fail("extrusion depth must be finite and non-negative");
    }
    let contours = contours(commands, tolerance)?;
    let mut mesh = Mesh::default();
    if contours.is_empty() {
        return Ok(mesh);
    }
    let (front, back) = (depth * 0.5, -depth * 0.5);
    // Caps from the same polylines the walls use, so edges meet exactly.
    let mut builder = Path::builder();
    for contour in &contours {
        builder.begin(point(contour[0][0], contour[0][1]));
        for p in &contour[1..] {
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
    let cap_triangles = cap.indices.len() / 3;
    if cap_triangles * 2 > MAX_TRIANGLES {
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
    let smooth = SMOOTH_ANGLE_DEGREES.to_radians().cos();
    for contour in &contours {
        let n = contour.len();
        if (mesh.indices.len() / 3) + n * 2 > MAX_TRIANGLES {
            return fail("outline is too detailed to extrude");
        }
        // Which side of this contour is solid decides where its walls face.
        let (a, b) = (contour[0], contour[1]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length = (dx * dx + dy * dy).sqrt().max(1e-6);
        let left = [-dy / length, dx / length];
        let probe = 0.01_f32.max(length * 1e-3);
        let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        let solid_left = filled(
            rule,
            winding(
                &contours,
                [mid[0] + left[0] * probe, mid[1] + left[1] * probe],
            ),
        );
        let solid_right = filled(
            rule,
            winding(
                &contours,
                [mid[0] - left[0] * probe, mid[1] - left[1] * probe],
            ),
        );
        if solid_left == solid_right {
            // Overlapping or internal contour: its walls would be hidden inside.
            continue;
        }
        let sign = if solid_left { -1.0 } else { 1.0 };
        let edge_normal = |i: usize| {
            let (a, b) = (contour[i], contour[(i + 1) % n]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l = (dx * dx + dy * dy).sqrt().max(1e-12);
            [-dy / l * sign, dx / l * sign]
        };
        let normals: Vec<[f32; 2]> = (0..n).map(edge_normal).collect();
        // A vertex shared by two walls is smoothed only when they meet gently.
        let vertex_normal = |edge: usize, at_start: bool| {
            let other = if at_start {
                (edge + n - 1) % n
            } else {
                (edge + 1) % n
            };
            let (m, o) = (normals[edge], normals[other]);
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
            let (a, b) = (contour[i], contour[(i + 1) % n]);
            let length = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            let (na, nb) = (vertex_normal(i, true), vertex_normal(i, false));
            let base = mesh.vertices.len() as u32;
            for (p, normal, u) in [(a, na, along), (b, nb, along + length)] {
                for z in [front, back] {
                    mesh.vertices.push(Vertex {
                        position: [p[0], p[1], z],
                        normal: [normal[0], normal[1], 0.0],
                        uv: [u, front - z],
                    });
                }
            }
            along += length;
            // Quad a-front, a-back, b-front, b-back, wound to face its normal.
            let [af, ab, bf, bb] = [base, base + 1, base + 2, base + 3];
            if sign > 0.0 {
                mesh.indices.extend([af, bf, ab, bf, bb, ab]);
            } else {
                mesh.indices.extend([af, ab, bf, bf, ab, bb]);
            }
        }
    }
    Ok(mesh)
}
