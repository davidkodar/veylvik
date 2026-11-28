//! A scene read from a file and sampled over time (USD, FBX), made into a model as an imported glTF is: each part's mesh placed where it rests, turned y down and normalised (centred, largest side 1); a part keyed at several places becomes a joint of the model's clip, a part with points keyed over time a point cache, and cameras keys in the same normalised frame.
use crate::animation::{self as anim, Between, Channel, Clip, Joint, Keyed, Mat};
use crate::{MAX_PARTS, MAX_VERTICES, Mesh, Model, Part, PointCache};
use std::sync::Arc;
use veylvik_document::nodes::{CameraKey, MAX_MODEL_CAMERAS, ModelCamera};

/// The most values (three a point) all of a model's point caches may hold together.
pub const MAX_CACHE_VALUES: usize = 64 * 1024 * 1024;

/// A part as the file has it, in the file's right-handed y-up frame (any unit).
pub struct BakedPart {
    /// Its geometry in its own space, triangles counter-clockwise seen from outside.
    pub mesh: Mesh,
    pub colour: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    /// Where it is (geometry to world) at moments of the clip, in seconds; the first is
    /// where it rests, and more than one key it as moving.
    pub places: Vec<(f32, Mat)>,
    /// Its points keyed over time, already in the world, when it bends.
    pub cache: Option<BakedCache>,
}
pub struct BakedCache {
    pub seconds: Vec<f32>,
    pub frames: Vec<Vec<[f32; 3]>>,
    /// The point each vertex of `mesh` stands on.
    pub point_of: Vec<u32>,
}
/// A camera's keys: seconds, camera to world (looking along -z, y up), focal length and
/// sensor width in the same units.
pub struct BakedCamera {
    pub name: String,
    pub keys: Vec<(f32, Mat, f32, f32)>,
}
fn point(m: &Mat, p: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
}
fn unit(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 0.0 {
        v.map(|x| x / l)
    } else {
        [0.0, 1.0, 0.0]
    }
}
/// A matrix as a move, a turn (unit quaternion x y z w) and sizes; a mirror goes into
/// the x size.
pub fn decompose(m: &Mat) -> ([f32; 3], [f32; 4], [f32; 3]) {
    let t = [m[0][3], m[1][3], m[2][3]];
    let length = |c: usize| (m[0][c] * m[0][c] + m[1][c] * m[1][c] + m[2][c] * m[2][c]).sqrt();
    let mut s = [length(0), length(1), length(2)];
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det < 0.0 {
        s[0] = -s[0];
    }
    let r = |i: usize, j: usize| {
        if s[j].abs() > 1e-12 {
            m[i][j] / s[j]
        } else {
            0.0
        }
    };
    // A rotation matrix to a quaternion (Shepperd's method).
    let trace = r(0, 0) + r(1, 1) + r(2, 2);
    let q = if trace > 0.0 {
        let w = (1.0 + trace).sqrt() * 2.0;
        [
            (r(2, 1) - r(1, 2)) / w,
            (r(0, 2) - r(2, 0)) / w,
            (r(1, 0) - r(0, 1)) / w,
            0.25 * w,
        ]
    } else if r(0, 0) > r(1, 1) && r(0, 0) > r(2, 2) {
        let w = (1.0 + r(0, 0) - r(1, 1) - r(2, 2)).sqrt() * 2.0;
        [
            0.25 * w,
            (r(0, 1) + r(1, 0)) / w,
            (r(0, 2) + r(2, 0)) / w,
            (r(2, 1) - r(1, 2)) / w,
        ]
    } else if r(1, 1) > r(2, 2) {
        let w = (1.0 + r(1, 1) - r(0, 0) - r(2, 2)).sqrt() * 2.0;
        [
            (r(0, 1) + r(1, 0)) / w,
            0.25 * w,
            (r(1, 2) + r(2, 1)) / w,
            (r(0, 2) - r(2, 0)) / w,
        ]
    } else {
        let w = (1.0 + r(2, 2) - r(0, 0) - r(1, 1)).sqrt() * 2.0;
        [
            (r(0, 2) + r(2, 0)) / w,
            (r(1, 2) + r(2, 1)) / w,
            0.25 * w,
            (r(1, 0) - r(0, 1)) / w,
        ]
    };
    let l = q.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
    (t, q.map(|v| v / l), s)
}
/// The parts and cameras as a model; `length` is the clip's length in seconds.
pub fn bake(
    parts: Vec<BakedPart>,
    cameras: Vec<BakedCamera>,
    length: f32,
) -> Result<Model, String> {
    if parts.is_empty() {
        return Err("the file has no meshes".into());
    }
    if parts.len() > MAX_PARTS
        || parts.iter().map(|p| p.mesh.vertices.len()).sum::<usize>() > MAX_VERTICES
    {
        return Err(format!(
            "the scene is too large (at most {MAX_VERTICES} vertices in {MAX_PARTS} parts)"
        ));
    }
    let values: usize = parts
        .iter()
        .filter_map(|p| p.cache.as_ref())
        .map(|c| c.frames.iter().map(|f| f.len() * 3).sum::<usize>())
        .sum();
    if values > MAX_CACHE_VALUES {
        return Err("the scene's point caches are too large".into());
    }
    let mut out = Vec::with_capacity(parts.len());
    let mut joints = Vec::new();
    let mut channels = Vec::new();
    for baked in parts {
        let rest = baked.places.first().map_or(anim::IDENTITY, |p| p.1);
        let normals = anim::affine_inverse(&rest).unwrap_or(anim::IDENTITY);
        let mut mesh = baked.mesh;
        for v in &mut mesh.vertices {
            v.position = point(&rest, v.position);
            let n = v.normal;
            v.normal = unit(
                [0, 1, 2]
                    .map(|c| normals[0][c] * n[0] + normals[1][c] * n[1] + normals[2][c] * n[2]),
            );
        }
        let mut part = Part {
            mesh: Mesh::default(),
            colour: baked.colour,
            metallic: baked.metallic,
            roughness: baked.roughness,
            texture: None,
            joint: None,
            rest_inverse: anim::IDENTITY,
            skin: None,
            cache: None,
        };
        if let Some(cache) = baked.cache.filter(|c| c.frames.len() > 1) {
            part.cache = Some(Arc::new(PointCache {
                seconds: cache.seconds,
                frames: cache.frames,
                point_of: cache.point_of,
            }));
        } else if baked.places.len() > 1 {
            // Moved as one piece: a joint of its own keyed at every moment.
            let joint = joints.len();
            let (t, q, s) = decompose(&rest);
            joints.push(Joint {
                parent: None,
                translation: t,
                rotation: q,
                scale: s,
            });
            let mut keys: [Vec<[f32; 4]>; 3] = Default::default();
            for (_, place) in &baked.places {
                let (t, q, s) = decompose(place);
                keys[0].push([t[0], t[1], t[2], 0.0]);
                keys[1].push(q);
                keys[2].push([s[0], s[1], s[2], 0.0]);
            }
            let times: Vec<f32> = baked.places.iter().map(|p| p.0).collect();
            for (keyed, values) in [Keyed::Translation, Keyed::Rotation, Keyed::Scale]
                .into_iter()
                .zip(keys)
            {
                channels.push(Channel {
                    node: joint,
                    keyed,
                    between: Between::Linear,
                    times: times.clone(),
                    values,
                });
            }
            part.joint = Some(joint);
            part.rest_inverse = anim::affine_inverse(&rest).unwrap_or(anim::IDENTITY);
        }
        // Veylvik's frame: y down (which mirrors, so triangles turn round).
        for v in &mut mesh.vertices {
            v.position[1] = -v.position[1];
            v.normal[1] = -v.normal[1];
        }
        for t in mesh.indices.as_chunks_mut::<3>().0 {
            t.swap(1, 2);
        }
        part.mesh = mesh;
        out.push(part);
    }
    // Normalise: centre the box on the origin and make its largest side 1.
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for v in out.iter().flat_map(|p| &p.mesh.vertices) {
        for k in 0..3 {
            lo[k] = lo[k].min(v.position[k]);
            hi[k] = hi[k].max(v.position[k]);
        }
    }
    let size = [0, 1, 2].map(|k| hi[k] - lo[k]);
    let largest = size.iter().copied().fold(0.0f32, f32::max);
    if !largest.is_finite() || largest <= 0.0 {
        return Err("the scene has no size".into());
    }
    let centre = [0, 1, 2].map(|k| (lo[k] + hi[k]) * 0.5);
    for v in out.iter_mut().flat_map(|p| &mut p.mesh.vertices) {
        v.position = [0, 1, 2].map(|k| (v.position[k] - centre[k]) / largest);
    }
    let cameras: Vec<ModelCamera> = cameras
        .into_iter()
        .filter(|c| !c.keys.is_empty())
        .take(MAX_MODEL_CAMERAS)
        .map(|c| ModelCamera {
            name: c.name.chars().take(256).collect(),
            keys: c
                .keys
                .into_iter()
                .map(|(seconds, world, focal_length, sensor_width)| {
                    camera_key(seconds, &world, centre, largest, focal_length, sensor_width)
                })
                .collect(),
        })
        .collect();
    let animated = !channels.is_empty()
        || out.iter().any(|p| p.cache.is_some())
        || cameras.iter().any(|c| c.keys.len() > 1);
    Ok(Model {
        parts: out,
        extent: size.map(|s| s / largest),
        joints,
        clips: if animated {
            vec![Clip {
                name: "Animation".into(),
                channels,
                length: length.max(0.0),
            }]
        } else {
            Vec::new()
        },
        // As glTF's: the box centre in Veylvik's frame (y down), which the normalised
        // frame is measured from.
        centre,
        largest,
        cameras,
    })
}
/// A camera's key in the model's normalised frame: its place y down, less the centre,
/// over the largest side; its turn (looking along -z with y up in the file) mirrored the
/// same way, so it looks along -z with y down as Veylvik's cameras do.
fn camera_key(
    seconds: f32,
    world: &Mat,
    centre: [f32; 3],
    largest: f32,
    focal_length: f32,
    sensor_width: f32,
) -> CameraKey {
    let at = [world[0][3], -world[1][3], world[2][3]];
    // F R F with F = diag(1, -1, 1): the turn seen in the mirrored frame.
    let mut turn = anim::IDENTITY;
    for (r, row) in turn.iter_mut().take(3).enumerate() {
        for (c, v) in row.iter_mut().take(3).enumerate() {
            let sign = if (r == 1) != (c == 1) { -1.0 } else { 1.0 };
            *v = world[r][c] * sign;
        }
    }
    let (_, rotation, _) = decompose(&turn);
    CameraKey {
        seconds,
        position: [0, 1, 2].map(|k| (at[k] - centre[k]) / largest),
        rotation,
        focal_length,
        sensor_width,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_turned_moved_sized_matrix_comes_apart_and_back_together() {
        let (s, c) = (0.3f32.sin(), 0.3f32.cos());
        let m = anim::compose([1.0, -2.0, 3.0], [0.0, s, 0.0, c], [2.0, 0.5, 1.5]);
        let (t, q, sz) = decompose(&m);
        let back = anim::compose(t, q, sz);
        for (a, b) in m.iter().flatten().zip(back.iter().flatten()) {
            assert!((a - b).abs() < 1e-5, "{m:?} {back:?}");
        }
    }
}
