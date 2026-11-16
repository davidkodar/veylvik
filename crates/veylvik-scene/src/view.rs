//! The editor's navigable 3D view: orbit, pan, dolly and first-person walk, convertible to and from cameras.
//!
//! The view looks at `target` from `distance`, turned by `yaw` (about Y) and `pitch`
//! (about X; positive looks down, as y points down). Its camera matrix is
//! `rotate([pitch, yaw, 0])`, which is also how a Camera node's placement turns it.
use crate::{Camera, home_distance, math};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FreeView {
    pub target: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub focal_length: f32,
    pub sensor_width: f32,
}
const PITCH_LIMIT: f32 = 89.0;
const MIN_DISTANCE: f32 = 1.0;
fn add(a: [f32; 3], b: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s]
}
impl FreeView {
    /// The home view: exactly the 2D frame at depth 0.
    pub fn home(width: u32, height: u32) -> Self {
        Self {
            target: [width as f32 * 0.5, height as f32 * 0.5, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            distance: home_distance(width, 50.0, 36.0),
            focal_length: 50.0,
            sensor_width: 36.0,
        }
    }
    /// Continue from a camera, looking `distance` ahead of it.
    pub fn from_camera(camera: &Camera, distance: f32) -> Self {
        let forward = math::direction(&camera.world, [0.0, 0.0, -1.0]);
        let l = (forward.iter().map(|v| v * v).sum::<f32>())
            .sqrt()
            .max(1e-9);
        let f = forward.map(|v| v / l);
        let eye = math::point(&camera.world, [0.0; 3]);
        let distance = distance.max(MIN_DISTANCE);
        Self {
            target: add(eye, f, distance),
            yaw: (-f[0]).atan2(-f[2]).to_degrees(),
            pitch: f[1].clamp(-1.0, 1.0).asin().to_degrees(),
            distance,
            focal_length: camera.focal_length,
            sensor_width: camera.sensor_width,
        }
    }
    fn rotation(&self) -> math::Mat4 {
        math::rotate([self.pitch, self.yaw, 0.0])
    }
    pub fn eye(&self) -> [f32; 3] {
        let back = math::direction(&self.rotation(), [0.0, 0.0, 1.0]);
        add(self.target, back, self.distance)
    }
    pub fn camera(&self) -> Camera {
        Camera {
            node: None,
            world: math::mul(&math::translate(self.eye()), &self.rotation()),
            focal_length: self.focal_length,
            sensor_width: self.sensor_width,
            f_stop: 0.0,
            focus_distance: 0.0,
            focus_on: None,
        }
    }
    /// Turn around the target (degrees).
    pub fn orbit(&mut self, yaw: f32, pitch: f32) {
        self.yaw = (self.yaw + yaw).rem_euclid(360.0);
        self.pitch = (self.pitch + pitch).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }
    /// Slide the view sideways and up by fractions of what is visible at the target.
    pub fn pan(&mut self, right: f32, down: f32) {
        let width = self.distance * self.sensor_width / self.focal_length;
        let r = self.rotation();
        let x = math::direction(&r, [1.0, 0.0, 0.0]);
        let y = math::direction(&r, [0.0, 1.0, 0.0]);
        self.target = add(add(self.target, x, -right * width), y, -down * width);
    }
    /// Move towards (factor < 1) or away from (> 1) the target.
    pub fn dolly(&mut self, factor: f32) {
        self.distance = (self.distance * factor.clamp(0.01, 100.0)).clamp(MIN_DISTANCE, 1.0e7);
    }
    /// First person: look around from where the eye is (degrees), then move it
    /// along the view (forward, right, down) in pixels.
    pub fn walk(&mut self, look_yaw: f32, look_pitch: f32, forward: f32, right: f32, down: f32) {
        let eye = self.eye();
        self.yaw = (self.yaw + look_yaw).rem_euclid(360.0);
        self.pitch = (self.pitch + look_pitch).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        let r = self.rotation();
        let ahead = math::direction(&r, [0.0, 0.0, -1.0]);
        let side = math::direction(&r, [1.0, 0.0, 0.0]);
        let below = math::direction(&r, [0.0, 1.0, 0.0]);
        let eye = add(add(add(eye, ahead, forward), side, right), below, down);
        self.target = add(eye, ahead, self.distance);
    }
    /// Stand at `eye` and look at `target`.
    pub fn look_at(&mut self, eye: [f32; 3], target: [f32; 3]) {
        let d = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
        let distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if distance < MIN_DISTANCE {
            return;
        }
        let f = d.map(|v| v / distance);
        self.yaw = (-f[0]).atan2(-f[2]).to_degrees().rem_euclid(360.0);
        self.pitch = f[1]
            .clamp(-1.0, 1.0)
            .asin()
            .to_degrees()
            .clamp(-PITCH_LIMIT, PITCH_LIMIT);
        self.distance = distance;
        self.target = target;
    }
    /// Keep the direction, and stand back far enough to see a sphere of `radius`
    /// around `centre` with a little room.
    pub fn frame(&mut self, centre: [f32; 3], radius: f32) {
        let half = (self.sensor_width * 0.5 / self.focal_length).atan();
        self.target = centre;
        self.distance = (radius.max(1.0) * 1.25 / half.sin()).clamp(MIN_DISTANCE, 1.0e7);
    }
    /// A Camera node's placement (offset from its home view, and rotation) that
    /// films exactly this view on a `width`-wide composition.
    pub fn placement(&self, width: u32, height: u32) -> ([f32; 3], [f32; 3]) {
        let home = [
            width as f32 * 0.5,
            height as f32 * 0.5,
            home_distance(width, self.focal_length, self.sensor_width),
        ];
        let eye = self.eye();
        (
            [eye[0] - home[0], eye[1] - home[1], eye[2] - home[2]],
            [self.pitch, self.yaw, 0.0],
        )
    }
}

/// One camera sample: time, Camera placement position and rotation.
pub type Pose = (crate::Time, [f32; 3], [f32; 3]);
/// Turn a hand-walked path into a clean camera move: unwrap rotations so turning
/// past 360 degrees never jumps, average out shake over `window` samples (the ends
/// stay where they were), then keep every `every`-th sample and the last.
pub fn smooth_walk(samples: &[Pose], window: usize, every: usize) -> Vec<Pose> {
    if samples.len() < 3 {
        return samples.to_vec();
    }
    let mut poses = samples.to_vec();
    for i in 1..poses.len() {
        for axis in 0..3 {
            let before = poses[i - 1].2[axis];
            let mut v = poses[i].2[axis];
            while v - before > 180.0 {
                v -= 360.0;
            }
            while v - before < -180.0 {
                v += 360.0;
            }
            poses[i].2[axis] = v;
        }
    }
    let half = window / 2;
    let n = poses.len();
    let smoothed: Vec<Pose> = (0..n)
        .map(|i| {
            // Shrink the window towards the ends, so first and last stay put.
            let reach = half.min(i).min(n - 1 - i);
            let span = &poses[i - reach..=i + reach];
            let mean = |pick: &dyn Fn(&Pose) -> [f32; 3]| {
                let mut m = [0.0; 3];
                for p in span {
                    let v = pick(p);
                    for k in 0..3 {
                        m[k] += v[k] / span.len() as f32;
                    }
                }
                m
            };
            (poses[i].0, mean(&|p| p.1), mean(&|p| p.2))
        })
        .collect();
    let every = every.max(1);
    let mut kept: Vec<Pose> = smoothed.iter().step_by(every).copied().collect();
    if !(n - 1).is_multiple_of(every) {
        kept.push(smoothed[n - 1]);
    }
    kept
}
