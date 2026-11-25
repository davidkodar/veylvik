//! A human walk as joint positions over time: legs on the measured joint angles of normal walking with the standing foot locked to the ground, a body that glides on its feet, a double-pendulum arm swing and the weight passing from side to side. Lengths are shares of the figure's height; x is forward, y down (the ground at 0), z towards the near side.
use crate::Pose;

/// One stride (two steps) is 0.79 of the height.
pub const STRIDE: f32 = 0.79;
/// The pelvis's forward tilt (degrees), taken off the measured hip angle for the thigh.
const TILT: f32 = 7.0;
/// Thigh and shank lengths.
const THIGH: f32 = 0.245;
const SHANK: f32 = 0.246;
/// Half the distance between the hip joints, and between the feet.
const HIP_HALF: f32 = 0.052;
const FOOT_HALF: f32 = 0.045;
/// Half the distance between the shoulder joints.
const SHOULDER_HALF: f32 = 0.118;

/// Periodic Catmull-Rom through twenty samples, x in strides (any value).
fn at20(v: &[f32; 20], x: f32) -> f32 {
    let u = x.rem_euclid(1.0) * 20.0;
    let i = u.floor() as usize;
    let f = u - i as f32;
    let p0 = v[(i + 19) % 20];
    let p1 = v[i % 20];
    let p2 = v[(i + 1) % 20];
    let p3 = v[(i + 2) % 20];
    0.5 * (2.0 * p1
        + (p2 - p0) * f
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * f * f
        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * f * f * f)
}
// Degrees every 5% of the stride from this leg's heel strike (normal adult walking).
const HIP_FLEX: [f32; 20] = [
    30.0, 30.0, 28.0, 25.0, 20.0, 15.0, 10.0, 5.0, 0.0, -5.0, -9.0, -11.0, -7.0, 2.0, 12.0, 21.0,
    28.0, 32.0, 33.0, 31.0,
];
const KNEE_FLEX: [f32; 20] = [
    5.0, 12.0, 17.0, 19.0, 17.0, 12.0, 8.0, 5.0, 4.0, 5.0, 9.0, 20.0, 36.0, 52.0, 61.0, 60.0, 51.0,
    36.0, 19.0, 8.0,
];
const ANKLE_DORSI: [f32; 20] = [
    0.0, -4.0, -5.0, -2.0, 2.0, 4.0, 6.0, 8.0, 9.0, 10.0, 7.0, 0.0, -12.0, -17.0, -12.0, -6.0,
    -2.0, 0.0, 1.0, 1.0,
];
/// This side's arm, forward positive: furthest back at its own stride position 0.
const ARM_SWING: [f32; 20] = [
    -12.0, -12.0, -10.0, -7.0, -3.0, 1.0, 5.0, 9.0, 12.0, 14.0, 14.0, 13.0, 10.0, 7.0, 3.0, -1.0,
    -5.0, -9.0, -11.0, -12.0,
];
/// A direction `deg` degrees from straight down, positive forward.
fn dir(deg: f32) -> [f32; 2] {
    let r = deg.to_radians();
    [r.sin(), r.cos()]
}
fn add(a: [f32; 2], b: [f32; 2], s: f32) -> [f32; 2] {
    [a[0] + b[0] * s, a[1] + b[1] * s]
}
fn ease(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * t * (t * (6.0 * t - 15.0) + 10.0)
}
/// A leg in its own plane, from its hip joint at the origin.
#[derive(Clone, Copy)]
struct Leg {
    knee: [f32; 2],
    ankle: [f32; 2],
    heel: [f32; 2],
    ball: [f32; 2],
    toe: [f32; 2],
}
impl Leg {
    fn lowest(&self) -> f32 {
        self.heel[1].max(self.ball[1]).max(self.toe[1])
    }
    fn shifted(mut self, by: [f32; 2]) -> Self {
        for p in [
            &mut self.knee,
            &mut self.ankle,
            &mut self.heel,
            &mut self.ball,
            &mut self.toe,
        ] {
            *p = add(*p, by, 1.0);
        }
        self
    }
}
fn fk(p: f32, pace: f32) -> Leg {
    let thigh = (at20(&HIP_FLEX, p) - TILT) * pace;
    let shank = thigh - at20(&KNEE_FLEX, p) * pace - 3.0 * (1.0 - pace);
    let knee = add([0.0, 0.0], dir(thigh), THIGH);
    let ankle = add(knee, dir(shank), SHANK);
    let foot = shank + 90.0 + at20(&ANKLE_DORSI, p) * pace;
    let fwd = dir(foot);
    let down = dir(foot - 90.0);
    let heel = add(add(ankle, fwd, -0.05), down, 0.04);
    let ball = add(add(ankle, fwd, 0.075), down, 0.036);
    // The toes stay flat while the heel rises (they bend at the ball).
    let toes = if foot < 90.0 { 90.0 } else { foot };
    let toe = add(add(ball, dir(toes), 0.035), dir(toes - 90.0), 0.004);
    Leg {
        knee,
        ankle,
        heel,
        ball,
        toe,
    }
}
/// How far below the hips the lowest foot point is (smoothly where one takes over).
fn support(x: f32, pace: f32) -> f32 {
    let a = fk(x.rem_euclid(1.0), pace).lowest();
    let b = fk((x + 0.5).rem_euclid(1.0), pace).lowest();
    let k = 0.006;
    let t = (0.5 + 0.5 * (a - b) / k).clamp(0.0, 1.0);
    b + (a - b) * t + k * t * (1.0 - t)
}
/// The body's height above its feet, averaged over a little of the stride so it glides
/// through the moment the weight passes from foot to foot.
fn body_low(x: f32, pace: f32) -> f32 {
    let mut low = 0.0;
    for i in -3i32..=3 {
        low += (4 - i.abs()) as f32 * support(x + i as f32 * 0.025, pace);
    }
    low / 16.0
}
/// Where the body is along the walk at stride position x (with a small surge).
fn body_x(x: f32, pace: f32) -> f32 {
    x * STRIDE
        + 0.1 * STRIDE / (4.0 * std::f32::consts::PI)
            * (4.0 * std::f32::consts::PI * (x - 0.05)).sin()
            * pace
}
/// How far a leg's foot moves along the ground so the point carrying weight (the heel,
/// then the ball) stays where it landed; it fades away in swing.
fn pin(x: f32, ahead: f32, pace: f32) -> f32 {
    let y = x + ahead;
    let p = y.rem_euclid(1.0);
    let strike = y.floor() - ahead;
    let heel_pin = body_x(strike, pace) + fk(0.0, pace).heel[0];
    let heel_now = body_x(x, pace) + fk(p, pace).heel[0];
    // When the ball takes over (comes as low as the heel), found by halving.
    let (mut a, mut b) = (0.0f32, 0.62f32);
    for _ in 0..14 {
        let m = 0.5 * (a + b);
        let l = fk(m, pace);
        if l.ball[1] >= l.heel[1] - 0.0005 {
            b = m;
        } else {
            a = m;
        }
    }
    let roll = b;
    let held = heel_pin - (body_x(strike + roll, pace) + fk(roll, pace).heel[0]);
    let ball_pin = body_x(strike + roll, pace) + fk(roll, pace).ball[0] + held;
    let off = 0.62;
    let ball_off = ball_pin - (body_x(strike + off, pace) + fk(off, pace).ball[0]);
    if p < roll {
        heel_pin - heel_now
    } else if p < off {
        ball_pin - (body_x(x, pace) + fk(p, pace).ball[0])
    } else {
        ball_off * (1.0 - ease(off, 0.92, p))
    }
}
/// How much a leg at stride position p carries the weight (0 in the air, 1 standing).
fn bearing(p: f32) -> f32 {
    ease(0.0, 0.1, p) * (1.0 - ease(0.5, 0.62, p))
}
/// The give that runs up the body from a heel strike `t` strides ago.
fn jolt(t: f32) -> f32 {
    (-t / 0.09).exp() * (std::f32::consts::TAU * t / 0.2).sin()
}
/// The knee for a hip and an ankle (in the leg's plane), bending forward.
fn knee_of(hip: [f32; 2], ankle: [f32; 2]) -> [f32; 2] {
    let d = [ankle[0] - hip[0], ankle[1] - hip[1]];
    let full = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-6);
    let len = full.min((THIGH + SHANK) * 0.9999);
    let along = [d[0] / full, d[1] / full];
    let x = (THIGH * THIGH - SHANK * SHANK + len * len) / (2.0 * len);
    let y = (THIGH * THIGH - x * x).max(0.0).sqrt();
    let mut fwd = [along[1], -along[0]];
    if fwd[0] < 0.0 {
        fwd = [-fwd[0], -fwd[1]];
    }
    [
        hip[0] + along[0] * x + fwd[0] * y,
        hip[1] + along[1] * x + fwd[1] * y,
    ]
}
/// The arm (upper and forearm angles from straight down, degrees) for this arm's stride
/// position: a double pendulum, the forearm a beat behind and further.
fn arm(x: f32, pace: f32) -> (f32, f32) {
    let s_upper = at20(&ARM_SWING, x - 0.04) / 13.0;
    let s_fore = at20(&ARM_SWING, x - 0.1) / 13.0;
    let upper = 2.0 + 18.0 * s_upper * pace;
    let fore =
        upper + 14.0 + (24.0 * s_fore - 18.0 * s_upper) * pace + 8.0 * s_fore.max(0.0) * pace;
    (upper, fore)
}

/// The pose after walking `distance` (in heights) at `pace` (1 walking, 0 standing).
pub fn walk(distance: f32, pace: f32) -> Pose {
    let pace = pace.clamp(0.0, 1.0);
    let x = distance / STRIDE;
    let near_p = x.rem_euclid(1.0);
    let far_p = (x + 0.5).rem_euclid(1.0);
    let near0 = fk(near_p, pace);
    let far0 = fk(far_p, pace);
    let low = body_low(x, pace);
    let hip = [body_x(x, pace), -low];
    // Each heel strike sends a small give up the body; the spine takes up a little of
    // each rise and fall, so the chest follows the hips a moment later and less.
    let give = (jolt(near_p) + jolt(far_p)) * pace;
    let give_late = (jolt((x - 0.03).rem_euclid(1.0)) + jolt((x + 0.47).rem_euclid(1.0))) * pace;
    let chest_y = -(low + (body_low(x - 0.04, pace) - low) * 0.85 * pace) + 0.005 * give;
    // The pelvis turns (each hip joint forward with its leg), sags on the side whose leg
    // is in the air, and sways a little over the standing foot.
    let turn = 0.004 * pace;
    let sway = 0.012 * (bearing(near_p) - bearing(far_p)) * pace;
    let mut pose = Pose::default();
    for (side, (leg, ahead)) in [(near0, 0.0f32), (far0, 0.5f32)].into_iter().enumerate() {
        let p = (x + ahead).rem_euclid(1.0);
        let joint = [
            hip[0] + turn * (std::f32::consts::TAU * (x + ahead)).cos(),
            hip[1] + 0.009 * (1.0 - bearing(p)) * pace,
        ];
        let gap = -(hip[1] + leg.lowest());
        let on = ease(0.0, 0.03, p) * (1.0 - ease(0.58, 0.66, p));
        let settle = gap.min(0.0) + gap.max(0.0) * on;
        let mut placed = leg.shifted([hip[0] + pin(x, ahead, pace) * pace, hip[1] + settle]);
        placed.knee = knee_of(joint, placed.ankle);
        let z_hip = if side == 0 { HIP_HALF } else { -HIP_HALF } + sway;
        let z_foot = if side == 0 { FOOT_HALF } else { -FOOT_HALF };
        let lift = |a: [f32; 2], z: f32| [a[0], a[1], z];
        pose.hip[side] = lift(joint, z_hip);
        pose.knee[side] = lift(placed.knee, 0.5 * (z_hip + z_foot));
        pose.ankle[side] = lift(placed.ankle, z_foot);
        pose.heel[side] = lift(placed.heel, z_foot);
        pose.ball[side] = lift(placed.ball, z_foot);
        pose.toe[side] = lift(placed.toe, z_foot);
    }
    // The trunk leans forward a little and pitches twice a stride.
    let lean = (3.0 + 0.5 * (x * 4.0 * std::f32::consts::PI).cos()) * pace + 1.0;
    let up = {
        let d = dir(lean);
        [-d[0], -d[1]]
    };
    let fwd = [-up[1], up[0]];
    pose.pelvis = [hip[0], hip[1], sway];
    pose.up = [up[0], up[1], 0.0];
    let top = [hip[0], chest_y];
    pose.chest = [top[0], top[1], sway * 0.6];
    let shoulder = add(add(top, up, 0.288), fwd, -0.006);
    for side in 0..2 {
        let own = if side == 0 { x + 0.5 } else { x };
        let swing = at20(&ARM_SWING, own - 0.04) / 13.0 * pace;
        // The shoulders turn against the pelvis and drop over the standing leg.
        let carrying = bearing(if side == 0 { near_p } else { far_p });
        let s = add(
            add(shoulder, fwd, 0.012 * swing),
            [0.0, 1.0],
            0.006 * carrying * pace,
        );
        let z = if side == 0 {
            SHOULDER_HALF
        } else {
            -SHOULDER_HALF
        } + sway * 0.5;
        // Left and right are not mirror images: one arm swings a little more.
        let amount = pace * if side == 0 { 1.08 } else { 0.92 };
        let (upper, fore) = arm(own, amount);
        let elbow = add(s, dir(upper), 0.186);
        let wrist = add(elbow, dir(fore), 0.146);
        let hand = add(wrist, dir(fore + 10.0), 0.09);
        // The arm hangs a little out from the body (the elbow and hand clear the hips).
        let out = if side == 0 { 1.0 } else { -1.0 };
        pose.shoulder[side] = [s[0], s[1], z];
        pose.elbow[side] = [elbow[0], elbow[1], z + out * 0.03];
        pose.wrist[side] = [wrist[0], wrist[1], z + out * 0.05];
        pose.hand[side] = [hand[0], hand[1], z + out * 0.056];
    }
    // The neck and head: nearly level, nodding a touch against the bob.
    let neck_base = add(add(top, up, 0.305), fwd, 0.006);
    let neck_top = [
        neck_base[0] + 0.014,
        neck_base[1] - 0.048 + 0.003 * give_late,
    ];
    pose.neck = [neck_base[0], neck_base[1], sway * 0.4];
    pose.head = [neck_top[0], neck_top[1], sway * 0.3];
    pose.head_pitch = 1.5 * (4.0 * std::f32::consts::PI * (x - 0.1)).sin() * pace;
    pose
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_walking_foot_stays_put_while_it_carries_weight_and_the_body_glides() {
        // Over a stride, the ball of a foot pushing off does not move along the ground,
        // the lowest foot point is always on the ground, and the hips rise and fall a few
        // centimetres (shares of a 1.75 m height) without a jolt between frames.
        let frames = 120;
        let mut hips = Vec::new();
        let mut ball_x = Vec::new();
        for k in 0..frames {
            let d = k as f32 / frames as f32 * STRIDE * 1.5;
            let pose = walk(d, 1.0);
            let lowest = pose
                .heel
                .iter()
                .chain(&pose.ball)
                .chain(&pose.toe)
                .map(|p| p[1])
                .fold(f32::MIN, f32::max);
            assert!(
                lowest.abs() < 0.004,
                "a foot touches the ground at {d}: {lowest}"
            );
            hips.push(pose.pelvis[1]);
            let p = (d / STRIDE).rem_euclid(1.0);
            if d < STRIDE && (0.35..0.6).contains(&p) {
                ball_x.push(pose.ball[0][0]);
            }
        }
        let range = hips.iter().cloned().fold(f32::MIN, f32::max)
            - hips.iter().cloned().fold(f32::MAX, f32::min);
        assert!((0.008..0.03).contains(&range), "hips rise and fall {range}");
        let step = hips
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(step < 0.002, "no jolt between frames: {step}");
        let slide = ball_x.iter().cloned().fold(f32::MIN, f32::max)
            - ball_x.iter().cloned().fold(f32::MAX, f32::min);
        assert!(slide < 0.004, "the pushing-off ball stays put: {slide}");
    }
}
