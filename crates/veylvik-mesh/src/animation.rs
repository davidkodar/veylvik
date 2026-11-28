//! A model's own animation as plain data: its node tree at rest and the clips that key the nodes' moves, turns and sizes (glTF; no morph targets), each rigid part's motion at a moment of a clip, and each skinned part's mesh bent by its joints then, in the model's normalised frame.
use crate::Model;

/// Row-major 4x4 matrices acting on column vectors, as the scene's.
pub type Mat = [[f32; 4]; 4];
pub const IDENTITY: Mat = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];
/// The most keys a model's clips may hold, all together.
pub const MAX_KEYS: usize = 2_000_000;

/// A node of the file's tree as it rests (in glTF's own frame, y up).
#[derive(Debug, Clone)]
pub struct Joint {
    pub parent: Option<usize>,
    pub translation: [f32; 3],
    /// A unit quaternion, x y z w.
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Keyed {
    Translation,
    Rotation,
    Scale,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Between {
    Step,
    Linear,
    /// Values come three to a key: in-tangent, value, out-tangent.
    CubicSpline,
}
/// The keys of one property of one node.
#[derive(Debug, Clone)]
pub struct Channel {
    pub node: usize,
    pub keyed: Keyed,
    pub between: Between,
    pub times: Vec<f32>,
    /// Three values (or four for a rotation) a key; three keys' worth a key for a cubic spline.
    pub values: Vec<[f32; 4]>,
}
#[derive(Debug, Clone)]
pub struct Clip {
    pub name: String,
    pub channels: Vec<Channel>,
    /// How long it lasts when its keys alone do not say (a point cache's last frame).
    pub length: f32,
}
impl Clip {
    /// Its length: the last key of any channel, or `length` if longer.
    pub fn seconds(&self) -> f32 {
        self.channels
            .iter()
            .filter_map(|c| c.times.last().copied())
            .fold(self.length.max(0.0), f32::max)
    }
}
fn mul(a: &Mat, b: &Mat) -> Mat {
    let mut out = [[0.0; 4]; 4];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}
/// The matrix of a move, a turn and a size, applied size first.
pub fn compose(t: [f32; 3], q: [f32; 4], s: [f32; 3]) -> Mat {
    let [x, y, z, w] = q;
    let r = [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ];
    let mut m = IDENTITY;
    for ((row, turn), along) in m.iter_mut().zip(r).zip(t) {
        for ((v, rv), sv) in row.iter_mut().zip(turn).zip(s) {
            *v = rv * sv;
        }
        row[3] = along;
    }
    m
}
/// The inverse of an affine matrix (a turn, sizes and a move), or None when it is flat.
pub fn affine_inverse(m: &Mat) -> Option<Mat> {
    let a = |i: usize, j: usize| m[i][j];
    let det = a(0, 0) * (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1))
        - a(0, 1) * (a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0))
        + a(0, 2) * (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0));
    if det.abs() < 1e-12 || !det.is_finite() {
        return None;
    }
    let mut inv = IDENTITY;
    for (i, row) in inv.iter_mut().take(3).enumerate() {
        for (j, v) in row.iter_mut().take(3).enumerate() {
            // The cofactor of (j, i) over the determinant.
            let (r0, r1) = ((j + 1) % 3, (j + 2) % 3);
            let (c0, c1) = ((i + 1) % 3, (i + 2) % 3);
            *v = (a(r0, c0) * a(r1, c1) - a(r0, c1) * a(r1, c0)) / det;
        }
        row[3] = -(0..3).map(|k| row[k] * m[k][3]).sum::<f32>();
    }
    Some(inv)
}
fn normalise(q: [f32; 4]) -> [f32; 4] {
    let l = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    if l > 0.0 {
        q.map(|v| v / l)
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}
fn slerp(a: [f32; 4], b: [f32; 4], s: f32) -> [f32; 4] {
    let mut d: f32 = (0..4).map(|k| a[k] * b[k]).sum();
    let b = if d < 0.0 {
        d = -d;
        b.map(|v| -v)
    } else {
        b
    };
    if d > 0.9995 {
        return normalise([0, 1, 2, 3].map(|k| a[k] + (b[k] - a[k]) * s));
    }
    let angle = d.clamp(-1.0, 1.0).acos();
    let (wa, wb) = (
        ((1.0 - s) * angle).sin() / angle.sin(),
        (s * angle).sin() / angle.sin(),
    );
    [0, 1, 2, 3].map(|k| a[k] * wa + b[k] * wb)
}
impl Channel {
    /// Its value at `t` seconds (held before the first key and after the last).
    fn at(&self, t: f32) -> Option<[f32; 4]> {
        let n = self.times.len();
        let value = |k: usize| match self.between {
            Between::CubicSpline => self.values.get(3 * k + 1).copied(),
            _ => self.values.get(k).copied(),
        };
        if n == 0 {
            return None;
        }
        if t <= self.times[0] || n == 1 {
            return value(0);
        }
        if t >= self.times[n - 1] {
            return value(n - 1);
        }
        let k = self
            .times
            .partition_point(|&x| x <= t)
            .saturating_sub(1)
            .min(n - 2);
        let (t0, t1) = (self.times[k], self.times[k + 1]);
        let dt = (t1 - t0).max(1e-9);
        let s = ((t - t0) / dt).clamp(0.0, 1.0);
        let (v0, v1) = (value(k)?, value(k + 1)?);
        let out = match self.between {
            Between::Step => v0,
            Between::Linear if self.keyed == Keyed::Rotation => slerp(v0, v1, s),
            Between::Linear => [0, 1, 2, 3].map(|i| v0[i] + (v1[i] - v0[i]) * s),
            Between::CubicSpline => {
                let (b0, a1) = (self.values.get(3 * k + 2)?, self.values.get(3 * (k + 1))?);
                let (s2, s3) = (s * s, s * s * s);
                let v = [0, 1, 2, 3].map(|i| {
                    (2.0 * s3 - 3.0 * s2 + 1.0) * v0[i]
                        + (s3 - 2.0 * s2 + s) * dt * b0[i]
                        + (-2.0 * s3 + 3.0 * s2) * v1[i]
                        + (s3 - s2) * dt * a1[i]
                });
                if self.keyed == Keyed::Rotation {
                    normalise(v)
                } else {
                    v
                }
            }
        };
        Some(out)
    }
}
impl Model {
    /// The time `seconds` into `clip` as it is played: wrapped round its length when
    /// `looping`, else held at its ends.
    fn clip_time(clip: &Clip, seconds: f32, looping: bool) -> f32 {
        let length = clip.seconds();
        if !seconds.is_finite() {
            0.0
        } else if looping && length > 0.0 {
            seconds.rem_euclid(length)
        } else {
            seconds.clamp(0.0, length)
        }
    }
    /// Every node's place in the file's frame (glTF, y up) at `t` seconds into `clip`, or
    /// at rest with no clip.
    fn node_worlds(&self, clip: Option<&Clip>, t: f32) -> Vec<Mat> {
        // Each node's place now: its rest, with what the clip keys put in its stead.
        let mut local: Vec<([f32; 3], [f32; 4], [f32; 3])> = self
            .joints
            .iter()
            .map(|j| (j.translation, j.rotation, j.scale))
            .collect();
        for channel in clip.map_or(&[][..], |c| &c.channels[..]) {
            let (Some(slot), Some(v)) = (local.get_mut(channel.node), channel.at(t)) else {
                continue;
            };
            match channel.keyed {
                Keyed::Translation => slot.0 = [v[0], v[1], v[2]],
                Keyed::Rotation => slot.1 = normalise(v),
                Keyed::Scale => slot.2 = [v[0], v[1], v[2]],
            }
        }
        let mut world: Vec<Option<Mat>> = vec![None; self.joints.len()];
        fn place(
            k: usize,
            joints: &[Joint],
            local: &[([f32; 3], [f32; 4], [f32; 3])],
            world: &mut [Option<Mat>],
            depth: usize,
        ) -> Mat {
            if let Some(m) = world[k] {
                return m;
            }
            let own = compose(local[k].0, local[k].1, local[k].2);
            let m = match joints[k].parent {
                Some(p) if p < joints.len() && depth < 256 => {
                    mul(&place(p, joints, local, world, depth + 1), &own)
                }
                _ => own,
            };
            world[k] = Some(m);
            m
        }
        (0..self.joints.len())
            .map(|k| place(k, &self.joints, &local, &mut world, 0))
            .collect()
    }
    /// Every node's place in the file's frame at rest.
    pub fn rest_worlds(&self) -> Vec<Mat> {
        self.node_worlds(None, 0.0)
    }
    /// From glTF's frame (y up) to the normalised one: flip y, centre, shrink.
    fn to_normalised(&self) -> Mat {
        let (c, l) = (self.centre, self.largest.max(1e-12));
        let mut into = IDENTITY;
        into[0][0] = 1.0 / l;
        into[1][1] = -1.0 / l;
        into[2][2] = 1.0 / l;
        into[0][3] = -c[0] / l;
        into[1][3] = -c[1] / l;
        into[2][3] = -c[2] / l;
        into
    }
    /// How each part has moved at `seconds` into the clip numbered `clip` (from 0) from
    /// where it rests, in the model's normalised frame (x right, y down, z towards the
    /// viewer, largest side 1): wrapped round the clip's length when `looping`, else held
    /// at its ends. None when there is no such clip. A skinned part does not move as one
    /// piece (its mesh bends instead: `deformed`).
    pub fn poses(&self, clip: usize, seconds: f32, looping: bool) -> Option<Vec<Mat>> {
        let clip = self.clips.get(clip)?;
        let worlds = self.node_worlds(Some(clip), Self::clip_time(clip, seconds, looping));
        let into = self.to_normalised();
        let back = affine_inverse(&into)?;
        Some(
            self.parts
                .iter()
                .map(|part| match part.joint {
                    Some(k) if k < self.joints.len() && part.skin.is_none() => {
                        mul(&into, &mul(&mul(&worlds[k], &part.rest_inverse), &back))
                    }
                    _ => IDENTITY,
                })
                .collect(),
        )
    }
    /// A deforming part's mesh in the normalised frame where `play` (clip, seconds,
    /// looping) has it, or at rest: a skinned part with each vertex moved by its joints'
    /// motions blended by its weights, a cached part with its points from the frames
    /// either side (straight between them). None for a part that does not deform.
    pub fn deformed(&self, part: usize, play: Option<(usize, f32, bool)>) -> Option<crate::Mesh> {
        let p = self.parts.get(part)?;
        let clip = play.and_then(|(c, ..)| self.clips.get(c));
        let t = match (clip, play) {
            (Some(clip), Some((_, seconds, looping))) => Self::clip_time(clip, seconds, looping),
            _ => 0.0,
        };
        let into = self.to_normalised();
        if let Some(skin) = &p.skin {
            let worlds = self.node_worlds(clip, t);
            return Some(skin_mesh(&p.mesh, skin, &worlds, &into));
        }
        let cache = p.cache.as_ref()?;
        // At rest (no clip playing) a cache shows its first frame.
        let t = if clip.is_some() {
            t
        } else {
            cache.seconds.first().copied().unwrap_or(0.0)
        };
        Some(cached_mesh(&p.mesh, cache, t, &into))
    }
}
/// `mesh` (for its triangles and uvs) with its points taken from `cache` at `t` seconds
/// (straight between the frames either side, held past the ends), into the normalised
/// frame by `into`, with smooth normals made again from the moved faces.
pub fn cached_mesh(
    mesh: &crate::Mesh,
    cache: &crate::PointCache,
    t: f32,
    into: &Mat,
) -> crate::Mesh {
    let n = cache.frames.len().min(cache.seconds.len());
    let mut out = mesh.clone();
    if n == 0 {
        return out;
    }
    let k = cache.seconds[..n]
        .partition_point(|&s| s <= t)
        .saturating_sub(1)
        .min(n - 1);
    let (a, b, s) = if k + 1 < n && t > cache.seconds[k] {
        let span = (cache.seconds[k + 1] - cache.seconds[k]).max(1e-9);
        (k, k + 1, ((t - cache.seconds[k]) / span).clamp(0.0, 1.0))
    } else {
        (k, k, 0.0)
    };
    let (fa, fb) = (&cache.frames[a], &cache.frames[b]);
    let points: Vec<[f32; 3]> = (0..fa.len().min(fb.len()))
        .map(|i| {
            let q = [0, 1, 2].map(|c| fa[i][c] + (fb[i][c] - fa[i][c]) * s);
            [0, 1, 2]
                .map(|r| into[r][0] * q[0] + into[r][1] * q[1] + into[r][2] * q[2] + into[r][3])
        })
        .collect();
    // Smooth normals: each point's faces summed (by area), shared by its vertices.
    let mut sums = vec![[0.0f32; 3]; points.len()];
    let point_at = |v: u32| cache.point_of.get(v as usize).map(|p| *p as usize);
    for t in out.indices.as_chunks::<3>().0 {
        let ps: Option<Vec<usize>> = t.iter().map(|&v| point_at(v)).collect();
        let Some(ps) = ps.filter(|ps| ps.iter().all(|&p| p < points.len())) else {
            continue;
        };
        let [a, b, c] = [points[ps[0]], points[ps[1]], points[ps[2]]];
        let (u, v) = (
            [0, 1, 2].map(|k| b[k] - a[k]),
            [0, 1, 2].map(|k| c[k] - a[k]),
        );
        let face = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        for p in ps {
            for k in 0..3 {
                sums[p][k] += face[k];
            }
        }
    }
    for (i, v) in out.vertices.iter_mut().enumerate() {
        let Some(p) = point_at(i as u32).filter(|&p| p < points.len()) else {
            continue;
        };
        v.position = points[p];
        let l =
            (sums[p][0] * sums[p][0] + sums[p][1] * sums[p][1] + sums[p][2] * sums[p][2]).sqrt();
        if l > 0.0 {
            v.normal = sums[p].map(|x| x / l);
        }
    }
    out
}
/// `mesh` (for its triangles and uvs) with every vertex bent by `skin`'s joints placed at
/// `worlds`, then taken into the normalised frame by `into`.
pub fn skin_mesh(
    mesh: &crate::Mesh,
    skin: &crate::Skin,
    worlds: &[Mat],
    into: &Mat,
) -> crate::Mesh {
    let joints: Vec<Mat> = skin
        .joints
        .iter()
        .zip(&skin.inverse_bind)
        .map(|(&node, bind)| worlds.get(node).map_or(IDENTITY, |w| mul(w, bind)))
        .collect();
    let mut out = mesh.clone();
    for (i, v) in out.vertices.iter_mut().enumerate() {
        let (Some(at), Some(normal), Some(which), Some(weights)) = (
            skin.positions.get(i),
            skin.normals.get(i),
            skin.influences.get(i),
            skin.weights.get(i),
        ) else {
            continue;
        };
        let mut m = [[0.0f32; 4]; 4];
        for (j, w) in which.iter().zip(weights) {
            if *w == 0.0 {
                continue;
            }
            let joint = joints.get(*j as usize).unwrap_or(&IDENTITY);
            for (row, from) in m.iter_mut().zip(joint) {
                for (a, b) in row.iter_mut().zip(from) {
                    *a += b * w;
                }
            }
        }
        let m = mul(into, &m);
        v.position =
            [0, 1, 2].map(|r| m[r][0] * at[0] + m[r][1] * at[1] + m[r][2] * at[2] + m[r][3]);
        // Normals by the inverse transpose of the blended joint's turn and sizes.
        let n = match affine_inverse(&m) {
            Some(inv) => [0, 1, 2]
                .map(|c| inv[0][c] * normal[0] + inv[1][c] * normal[1] + inv[2][c] * normal[2]),
            None => *normal,
        };
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        v.normal = if l > 0.0 { n.map(|x| x / l) } else { v.normal };
    }
    out
}
