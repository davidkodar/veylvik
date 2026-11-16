use veylvik_solve::linalg::*;
use veylvik_solve::{Pose, Settings, Track, solve};

const W: u32 = 1920;
const H: u32 = 1080;
/// Gaussian noise (Box-Muller).
fn gauss(rng: &mut Rng) -> f64 {
    let (a, b) = (rng.unit().max(1e-12), rng.unit());
    (-2.0 * a.ln()).sqrt() * (std::f64::consts::TAU * b).cos()
}
/// A synthetic shot: the true camera per frame and the tracks it would produce,
/// with pixel noise and some tracks drifting off as bad matches.
fn shot(
    frames: usize,
    focal: f64,
    camera: impl Fn(f64) -> Pose,
    seed: u64,
) -> (Vec<Pose>, Vec<Track>) {
    let mut rng = Rng::new(seed);
    let truth: Vec<Pose> = (0..frames)
        .map(|f| camera(f as f64 / (frames - 1) as f64))
        .collect();
    let mut tracks = Vec::new();
    for _ in 0..600 {
        let p = [
            rng.unit() * 16.0 - 8.0,
            rng.unit() * 8.0 - 4.0,
            6.0 + rng.unit() * 16.0,
        ];
        // As in footage, many features are there from the first frame.
        let start = if rng.unit() < 0.3 {
            0
        } else {
            rng.below(frames * 4 / 5)
        };
        let life = 12 + rng.below(frames);
        let bad = rng.unit() < 0.08;
        let drift_at = start + 5 + rng.below(10);
        let mut track = Track::default();
        for (f, pose) in truth.iter().enumerate().skip(start).take(life) {
            let c = pose.apply(p);
            if c[2] <= 0.1 {
                break;
            }
            let mut u = [
                W as f64 * 0.5 + focal * c[0] / c[2],
                H as f64 * 0.5 + focal * c[1] / c[2],
            ];
            if !(8.0..W as f64 - 8.0).contains(&u[0]) || !(8.0..H as f64 - 8.0).contains(&u[1]) {
                break;
            }
            u[0] += 0.3 * gauss(&mut rng);
            u[1] += 0.3 * gauss(&mut rng);
            if bad && f >= drift_at {
                u[0] += (f - drift_at) as f64 * 1.5;
            }
            track.points.push((f, u));
        }
        if track.points.len() >= 2 {
            tracks.push(track);
        }
    }
    (truth, tracks)
}
fn pose(yaw: f64, pitch: f64, centre: V3) -> Pose {
    let r = mat_mul(
        &exp_so3([pitch.to_radians(), 0.0, 0.0]),
        &exp_so3([0.0, yaw.to_radians(), 0.0]),
    );
    Pose {
        r,
        t: scale(mat_vec(&r, centre), -1.0),
    }
}
/// Rotation angle (degrees) between the solved and true camera orientations, both
/// taken relative to their first frame, so the gauge does not matter.
fn rotation_error(solved: &[Option<Pose>], truth: &[Pose]) -> f64 {
    let (s0, t0) = (solved[0].unwrap().r, truth[0].r);
    solved
        .iter()
        .zip(truth)
        .filter_map(|(s, t)| {
            let rs = mat_mul(&s.as_ref()?.r, &transpose(&s0));
            let rt = mat_mul(&t.r, &transpose(&t0));
            Some(norm(log_so3(&mat_mul(&rs, &transpose(&rt)))).to_degrees())
        })
        .fold(0.0, f64::max)
}
/// Worst camera position error after the best similarity alignment, as a share
/// of the true path's length.
fn path_error(solved: &[Option<Pose>], truth: &[Pose]) -> f64 {
    let pairs: Vec<(V3, V3)> = solved
        .iter()
        .zip(truth)
        .filter_map(|(s, t)| Some((s.as_ref()?.centre(), t.centre())))
        .collect();
    let mean = |pick: fn(&(V3, V3)) -> V3| {
        scale(
            pairs.iter().map(pick).fold([0.0; 3], add),
            1.0 / pairs.len() as f64,
        )
    };
    let (ms, mt) = (mean(|p| p.0), mean(|p| p.1));
    let centred: Vec<(V3, V3)> = pairs
        .iter()
        .map(|(s, t)| (sub(*s, ms), sub(*t, mt)))
        .collect();
    let r = best_rotation(centred.iter().map(|(s, t)| (*s, *t, 1.0)));
    let k = centred
        .iter()
        .map(|(s, t)| dot(mat_vec(&r, *s), *t))
        .sum::<f64>()
        / centred.iter().map(|(s, _)| dot(*s, *s)).sum::<f64>();
    let length = norm(sub(truth[truth.len() - 1].centre(), truth[0].centre())).max(1e-9);
    centred
        .iter()
        .map(|(s, t)| norm(sub(scale(mat_vec(&r, *s), k), *t)))
        .fold(0.0, f64::max)
        / length
}
fn moving(t: f64) -> Pose {
    pose(
        -6.0 + 12.0 * t,
        2.0 - 3.0 * t,
        [-1.5 + 3.0 * t, 0.3 * t, 2.5 * t],
    )
}
#[test]
fn solves_a_moving_camera_with_a_known_lens() {
    let (truth, tracks) = shot(60, 1400.0, moving, 3);
    let settings = Settings {
        width: W,
        height: H,
        focal: Some(1400.0),
    };
    let solved = solve(&tracks, 60, &settings).unwrap();
    assert!(!solved.nodal);
    assert!(
        solved.poses.iter().all(Option::is_some),
        "every frame placed"
    );
    assert!(solved.rms < 0.6, "rms {}", solved.rms);
    assert!(
        rotation_error(&solved.poses, &truth) < 0.2,
        "{}",
        rotation_error(&solved.poses, &truth)
    );
    assert!(
        path_error(&solved.poses, &truth) < 0.02,
        "{}",
        path_error(&solved.poses, &truth)
    );
}
#[test]
fn finds_the_focal_length_when_it_is_unknown() {
    let (truth, tracks) = shot(60, 1400.0, moving, 5);
    let settings = Settings {
        width: W,
        height: H,
        focal: None,
    };
    let solved = solve(&tracks, 60, &settings).unwrap();
    assert!(
        (solved.focal - 1400.0).abs() < 1400.0 * 0.03,
        "focal {}",
        solved.focal
    );
    assert!(solved.rms < 0.6, "rms {}", solved.rms);
    assert!(
        path_error(&solved.poses, &truth) < 0.03,
        "{}",
        path_error(&solved.poses, &truth)
    );
}
#[test]
fn solves_a_tripod_pan_as_rotation_only() {
    let (truth, tracks) = shot(
        60,
        1400.0,
        |t| pose(-15.0 + 30.0 * t, 3.0 - 6.0 * t, [0.0; 3]),
        9,
    );
    let settings = Settings {
        width: W,
        height: H,
        focal: None,
    };
    let solved = solve(&tracks, 60, &settings).unwrap();
    assert!(solved.nodal, "recognised as a pan");
    assert!(solved.poses.iter().all(Option::is_some));
    assert!(
        (solved.focal - 1400.0).abs() < 1400.0 * 0.05,
        "focal {}",
        solved.focal
    );
    assert!(
        rotation_error(&solved.poses, &truth) < 0.3,
        "{}",
        rotation_error(&solved.poses, &truth)
    );
}
