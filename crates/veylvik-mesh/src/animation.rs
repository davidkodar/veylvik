//! A model's own animation as plain data: its node tree at rest and the clips that key the nodes' moves, turns and sizes (glTF, rigid: no skins or morph targets), and each part's motion at a moment of a clip, in the model's normalised frame.
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
}
impl Clip {
    /// Its length: the last key of any channel.
    pub fn seconds(&self) -> f32 {
        self.channels
            .iter()
            .filter_map(|c| c.times.last().copied())
            .fold(0.0, f32::max)
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
    /// How each part has moved at `seconds` into the clip numbered `clip` (from 0) from
    /// where it rests, in the model's normalised frame (x right, y down, z towards the
    /// viewer, largest side 1): wrapped round the clip's length when `looping`, else held
    /// at its ends. None when there is no such clip.
    pub fn poses(&self, clip: usize, seconds: f32, looping: bool) -> Option<Vec<Mat>> {
        let clip = self.clips.get(clip)?;
        let length = clip.seconds();
        let t = if !seconds.is_finite() {
            0.0
        } else if looping && length > 0.0 {
            seconds.rem_euclid(length)
        } else {
            seconds.clamp(0.0, length)
        };
        // Each node's place now: its rest, with what the clip keys put in its stead.
        let mut local: Vec<([f32; 3], [f32; 4], [f32; 3])> = self
            .joints
            .iter()
            .map(|j| (j.translation, j.rotation, j.scale))
            .collect();
        for channel in &clip.channels {
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
        // From glTF's frame (y up) to the normalised one: flip y, centre, shrink.
        let (c, l) = (self.centre, self.largest.max(1e-12));
        let mut into = IDENTITY;
        into[0][0] = 1.0 / l;
        into[1][1] = -1.0 / l;
        into[2][2] = 1.0 / l;
        into[0][3] = -c[0] / l;
        into[1][3] = -c[1] / l;
        into[2][3] = -c[2] / l;
        let back = affine_inverse(&into)?;
        Some(
            self.parts
                .iter()
                .map(|part| match part.joint {
                    Some(k) if k < self.joints.len() => {
                        let now = place(k, &self.joints, &local, &mut world, 0);
                        mul(&into, &mul(&mul(&now, &part.rest_inverse), &back))
                    }
                    _ => IDENTITY,
                })
                .collect(),
        )
    }
}
