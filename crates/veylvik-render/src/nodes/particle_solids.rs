//! Solids particles bounce off: the 3D scene's shapes, texts, cards and models as distances to their surfaces, each in its own space with its place over the last life, so a particle's whole path is the same whichever frame computes it.
use super::{Cx, particles::ROWS};
use crate::{Result, error};
use veylvik_document::{NodeId, Time};
use veylvik_scene::{Geometry, Object, Scene, math};

/// Most solids particles bounce off (the first in the scene).
pub(crate) const MOST: usize = 16;
/// Values before each solid's places: kind, three sizes, scale, then its distance map's
/// start, width, height, corner and cell size.
const HEAD: usize = 12;
/// A text's distance map, in cells along its longer side: the finest that fits.
const GRIDS: [usize; 4] = [128, 64, 32, 16];

/// How each kind of solid is measured (as `particle_solids.wgsl` reads them).
#[derive(Clone, Copy)]
enum Kind {
    Box = 0,
    Ellipsoid = 1,
    Cylinder = 2,
    Cone = 3,
    Torus = 4,
    Text = 5,
    Ring = 6,
}
/// A text's footprint as signed distances (negative inside) on a grid in its own space.
struct Map {
    width: usize,
    height: usize,
    corner: [f32; 2],
    cell: f32,
    values: Vec<f32>,
}

/// The scene connected at `root` at the ROWS moments of the last `life` seconds, as the
/// settings are kept, the last at the frame.
pub(crate) fn scenes(cx: &Cx<'_>, root: NodeId, life: f32) -> Result<Vec<Scene>> {
    let by_id: std::collections::BTreeMap<_, _> = cx.nodes.iter().map(|n| (n.id, n)).collect();
    let lookup = |id| by_id.get(&id).copied();
    let graph = veylvik_scene::Graph {
        width: cx.width,
        height: cx.height,
        node: &lookup,
    };
    let seconds = cx.time.seconds().map_err(error)?;
    (0..ROWS)
        .map(|r| {
            let time = if r == ROWS - 1 {
                cx.time
            } else {
                let at = seconds - f64::from(life) + f64::from(life) * r as f64 / (ROWS - 1) as f64;
                Time::new((at * 24000.0).round() as i64, 24000).map_err(error)?
            };
            veylvik_scene::evaluate_in(&graph, root, time).map_err(error)
        })
        .collect()
}
/// The solids of `scenes` within `room` values: all of them with the finest text maps
/// that fit, else fewer.
pub(crate) fn fitting(scenes: &[Scene], room: usize) -> Vec<f32> {
    (1..=MOST)
        .rev()
        .flat_map(|most| GRIDS.map(|grid| (most, grid)))
        .map(|(most, grid)| solids(scenes, most, grid))
        .find(|values| values.len() <= room)
        .unwrap_or_default()
}
/// The first `most` solids of `scenes` (the scene at the ROWS moments of the last life,
/// the last the frame's) as data: how many; for each, its kind and sizes, its scale, its
/// distance map (texts) and its world-to-own rows at each moment; then the maps.
fn solids(scenes: &[Scene], most: usize, grid: usize) -> Vec<f32> {
    let Some(now) = scenes.last() else {
        return Vec::new();
    };
    let chosen: Vec<(usize, &Object)> = now
        .objects
        .iter()
        .filter(|o| !o.surface.catcher && o.instanced.is_none())
        .enumerate()
        .take(most)
        .collect();
    if chosen.is_empty() {
        return Vec::new();
    }
    let rows = scenes.len();
    let mut values = vec![chosen.len() as f32];
    let mut maps: Vec<f32> = Vec::new();
    let maps_at = 1 + chosen.len() * (HEAD + rows * 12);
    for (index, object) in &chosen {
        let (kind, sizes, map) = measure(object, grid);
        let world = object.world;
        // Lengths in its own space times this are lengths in the scene (the smallest
        // stretch, so a distance is never overstated).
        let scale = (0..3)
            .map(|c| {
                (0..3)
                    .map(|r| world[r][c] * world[r][c])
                    .sum::<f32>()
                    .sqrt()
            })
            .fold(f32::MAX, f32::min);
        let mut head = [0.0f32; HEAD];
        head[0] = kind as u8 as f32;
        head[1..4].copy_from_slice(&sizes);
        head[4] = scale;
        if let Some(map) = map {
            head[5] = (maps_at + maps.len()) as f32;
            head[6] = map.width as f32;
            head[7] = map.height as f32;
            head[8] = map.corner[0];
            head[9] = map.corner[1];
            head[10] = map.cell;
            maps.extend_from_slice(&map.values);
        }
        values.extend_from_slice(&head);
        // Its place at each moment: the same solid among the scene's solids then.
        for scene in scenes {
            let then = scene
                .objects
                .iter()
                .filter(|o| !o.surface.catcher && o.instanced.is_none())
                .nth(*index)
                .filter(|o| o.node == object.node)
                .map_or(world, |o| o.world);
            values.extend(inverse(&then).iter().take(3).flatten());
        }
    }
    values.extend(maps);
    values
}
/// World to own space for an object's placement.
fn inverse(world: &math::Mat4) -> math::Mat4 {
    let Some(mut inverse) = math::linear_inverse(world) else {
        return math::IDENTITY;
    };
    let t = [world[0][3], world[1][3], world[2][3]];
    for row in inverse.iter_mut().take(3) {
        row[3] = -(0..3).map(|c| row[c] * t[c]).sum::<f32>();
    }
    inverse
}
/// How an object is measured, its half sizes (or its own numbers) and, for a text, its
/// distance map.
fn measure(object: &Object, grid: usize) -> (Kind, [f32; 3], Option<std::sync::Arc<Map>>) {
    use veylvik_document::nodes::ShapeKind;
    match &object.geometry {
        Geometry::Shape { kind, size, .. } => {
            let h = size.map(|s| s * 0.5);
            match kind {
                ShapeKind::Cube => (Kind::Box, h, None),
                ShapeKind::Sphere => (Kind::Ellipsoid, h, None),
                ShapeKind::Plane => (Kind::Box, [h[0], h[1], 1.0], None),
                ShapeKind::Cylinder => (Kind::Cylinder, h, None),
                ShapeKind::Cone => (Kind::Cone, h, None),
                // A torus about z: ring radius, tube radius; a ring is a thin one.
                ShapeKind::Torus => {
                    let tube = h[2].min(h[0].min(h[1]));
                    (Kind::Torus, [(h[0] + h[1]) * 0.5 - tube, tube, 0.0], None)
                }
                // A flat ring: outer and inner radius.
                ShapeKind::Ring => {
                    let outer = (h[0] + h[1]) * 0.5;
                    (Kind::Ring, [outer, (outer - size[2]).max(0.0), 0.0], None)
                }
            }
        }
        Geometry::Card { size, .. } => (Kind::Box, [size[0] * 0.5, size[1] * 0.5, 1.0], None),
        Geometry::Model { size, extent, .. } => (Kind::Box, extent.map(|e| e * size * 0.5), None),
        Geometry::Text { depth, .. } => match text_map(object, grid) {
            Some(map) => (Kind::Text, [depth * 0.5, 0.0, 0.0], Some(map)),
            None => (Kind::Box, [0.0; 3], None),
        },
    }
}
/// A text's footprint as signed distances, from its solid's front face; kept for the
/// same outline.
fn text_map(object: &Object, grid: usize) -> Option<std::sync::Arc<Map>> {
    type Kept = Vec<(u64, std::sync::Arc<Map>)>;
    static KEPT: std::sync::Mutex<Kept> = std::sync::Mutex::new(Vec::new());
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!("{:?}", object.geometry).hash(&mut hasher);
    grid.hash(&mut hasher);
    let key = hasher.finish();
    if let Some((_, map)) = KEPT.lock().ok()?.iter().find(|(k, _)| *k == key) {
        return Some(map.clone());
    }
    let mesh = veylvik_scene::mesh(object).ok()?;
    let caps = &mesh.indices[..mesh.cap_indices.min(mesh.indices.len())];
    if caps.is_empty() {
        return None;
    }
    let xy = |i: u32| {
        let p = mesh.vertices[i as usize].position;
        [p[0], p[1]]
    };
    let (mut low, mut high) = ([f32::MAX; 2], [f32::MIN; 2]);
    for &i in caps {
        let p = xy(i);
        for k in 0..2 {
            low[k] = low[k].min(p[k]);
            high[k] = high[k].max(p[k]);
        }
    }
    let cell = (high[0] - low[0]).max(high[1] - low[1]).max(1.0) / grid as f32;
    // Two cells round it, so the map reaches past the outline.
    let corner = [low[0] - 2.0 * cell, low[1] - 2.0 * cell];
    let width = ((high[0] - low[0]) / cell).ceil() as usize + 5;
    let height = ((high[1] - low[1]) / cell).ceil() as usize + 5;
    // Inside: covered by a cap triangle at the cell's middle.
    let mut inside = vec![false; width * height];
    for tri in caps.as_chunks::<3>().0 {
        let [a, b, c] = [xy(tri[0]), xy(tri[1]), xy(tri[2])];
        let cell_of = |v: f32, k: usize| ((v - corner[k]) / cell).floor() as isize;
        let x0 = cell_of(a[0].min(b[0]).min(c[0]), 0).max(0) as usize;
        let x1 = (cell_of(a[0].max(b[0]).max(c[0]), 0) as usize).min(width - 1);
        let y0 = cell_of(a[1].min(b[1]).min(c[1]), 1).max(0) as usize;
        let y1 = (cell_of(a[1].max(b[1]).max(c[1]), 1) as usize).min(height - 1);
        let edge = |p: [f32; 2], q: [f32; 2], r: [f32; 2]| {
            (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
        };
        let area = edge(a, b, c);
        if area.abs() < 1e-9 {
            continue;
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                let p = [
                    corner[0] + (x as f32 + 0.5) * cell,
                    corner[1] + (y as f32 + 0.5) * cell,
                ];
                let (u, v, w) = (
                    edge(b, c, p) / area,
                    edge(c, a, p) / area,
                    edge(a, b, p) / area,
                );
                if u >= 0.0 && v >= 0.0 && w >= 0.0 {
                    inside[y * width + x] = true;
                }
            }
        }
    }
    // The cells on the outline: inside with a neighbour outside.
    let at = |x: usize, y: usize| inside[y * width + x];
    let mut edge_cells = Vec::new();
    for y in 0..height {
        for x in 0..width {
            if !at(x, y) {
                continue;
            }
            let outside_next = [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)]
                .iter()
                .any(|(dx, dy)| {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    nx < 0
                        || ny < 0
                        || nx >= width as i32
                        || ny >= height as i32
                        || !at(nx as usize, ny as usize)
                });
            if outside_next {
                edge_cells.push((x as f32, y as f32));
            }
        }
    }
    if edge_cells.is_empty() {
        return None;
    }
    // Each cell's distance to the outline (half a cell inside the outline cells),
    // negative inside.
    let mut values = vec![0.0f32; width * height];
    for y in 0..height {
        for x in 0..width {
            let nearest = edge_cells
                .iter()
                .map(|(ex, ey)| (ex - x as f32).powi(2) + (ey - y as f32).powi(2))
                .fold(f32::MAX, f32::min)
                .sqrt();
            let d = (nearest + 0.5) * cell;
            values[y * width + x] = if at(x, y) {
                -(nearest - 0.5).max(0.0) * cell
            } else {
                d
            };
        }
    }
    let map = std::sync::Arc::new(Map {
        width,
        height,
        corner,
        cell,
        values,
    });
    if let Ok(mut kept) = KEPT.lock() {
        if kept.len() >= 32 {
            kept.remove(0);
        }
        kept.push((key, map.clone()));
    }
    Some(map)
}
