use veylvik_document::planar::*;
use veylvik_track::estimate::{Motion, estimate};
const QUAD: Quad = [[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]];

fn close(a: Point, b: Point) {
    assert!(
        (a[0] - b[0]).abs() < 1e-7 && (a[1] - b[1]).abs() < 1e-7,
        "{a:?} != {b:?}"
    );
}

#[test]
fn consensus_rejects_moving_occluders_and_handles_4k_coordinates() {
    let h =
        Homography::between(QUAD, [[0.15, 0.2], [0.8, 0.05], [0.95, 0.85], [0.25, 0.95]]).unwrap();
    let features: Vec<_> = (0..64)
        .map(|i| {
            let reference = [0.15 + (i % 8) as f64 * 0.1, 0.15 + (i / 8) as f64 * 0.1];
            let mut position = h.project(reference).unwrap();
            if i % 3 == 0 {
                position = [0.92 - reference[1] * 0.6, reference[0] * 0.7];
            }
            Feature {
                reference,
                position,
                correlation: 0.95,
                separation: 0.1,
                accepted: false,
            }
        })
        .collect();
    for size in [[1920, 1080], [3840, 2160], [1080, 1920]] {
        let result = estimate(&features, size, 1.5, Motion::Perspective).unwrap();
        assert!(result.rms_pixels < 1e-6);
        for (i, &accepted) in result.accepted.iter().enumerate() {
            assert_eq!(accepted, i % 3 != 0);
        }
        close(
            result.motion.project([0.5, 0.5]).unwrap(),
            h.project([0.5, 0.5]).unwrap(),
        );
    }
}
#[test]
fn rejects_flat_ambiguous_and_collinear_evidence() {
    let mut features: Vec<_> = (0..16)
        .map(|i| Feature {
            reference: [i as f64 / 20., 0.5],
            position: [i as f64 / 20. + 0.01, 0.5],
            correlation: 0.9,
            separation: 0.1,
            accepted: false,
        })
        .collect();
    assert!(estimate(&features, [1920, 1080], 2., Motion::Perspective).is_err());
    for f in &mut features {
        f.separation = 0.;
    }
    assert!(estimate(&features, [1920, 1080], 2., Motion::Perspective).is_err());
    assert!(estimate(&features, [0, 1080], 2., Motion::Perspective).is_err());
    assert!(estimate(&features, [1920, 1080], f64::NAN, Motion::Perspective).is_err());
}

/// Portrait frame, so rotation is only correct if fitted in pixels.
const PORTRAIT: [u32; 2] = [1080, 1920];
fn pixel_motion(m: [f64; 6]) -> Homography {
    let (w, h) = (f64::from(PORTRAIT[0]), f64::from(PORTRAIT[1]));
    Homography {
        matrix: [
            m[0],
            m[1] * h / w,
            m[2] / w,
            m[3] * w / h,
            m[4],
            m[5] / h,
            0.,
            0.,
            1.,
        ],
    }
}
fn matches(h: Homography, points: &[Point], noise: f64) -> Vec<Feature> {
    let mut seed = 7_u64;
    let mut jitter = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 33) as f64 / (1u64 << 31) as f64 - 0.5) * 2. * noise
    };
    points
        .iter()
        .map(|&p| {
            let q = h.project(p).unwrap();
            let position = [
                q[0] + jitter() / f64::from(PORTRAIT[0]),
                q[1] + jitter() / f64::from(PORTRAIT[1]),
            ];
            Feature {
                reference: p,
                position,
                correlation: 0.95,
                separation: 0.5,
                accepted: false,
            }
        })
        .collect()
}
fn grid(n: usize, lo: f64, hi: f64) -> Vec<Point> {
    let step = (hi - lo) / (n - 1) as f64;
    (0..n * n)
        .map(|i| {
            [
                lo + step * (i % n) as f64 + 0.003 * (i % 3) as f64,
                lo + step * (i / n) as f64,
            ]
        })
        .collect()
}
fn corner_error(found: Homography, truth: Homography, quad: Quad) -> f64 {
    quad.iter()
        .map(|&p| {
            let (a, b) = (found.project(p).unwrap(), truth.project(p).unwrap());
            (((a[0] - b[0]) * f64::from(PORTRAIT[0])).powi(2)
                + ((a[1] - b[1]) * f64::from(PORTRAIT[1])).powi(2))
            .sqrt()
        })
        .fold(0., f64::max)
}

#[test]
fn each_motion_model_recovers_its_own_motion_in_portrait_pixels() {
    let (angle, scale) = (10_f64.to_radians(), 1.1);
    let (c, s) = (scale * angle.cos(), scale * angle.sin());
    let cases = [
        (
            Motion::Translation,
            pixel_motion([1., 0., 24., 0., 1., -31.]),
        ),
        (Motion::Similarity, pixel_motion([c, -s, 40., s, c, -12.])),
        (
            Motion::Affine,
            pixel_motion([1.05, 0.2, 10., -0.05, 0.92, 30.]),
        ),
        (
            Motion::Perspective,
            Homography::between(QUAD, [[0.12, 0.1], [0.92, 0.16], [0.86, 0.88], [0.08, 0.9]])
                .unwrap(),
        ),
    ];
    let points = grid(6, 0.15, 0.85);
    for (model, truth) in cases {
        let found = estimate(&matches(truth, &points, 0.), PORTRAIT, 1.5, model).unwrap();
        assert_eq!(found.model, model);
        assert!(corner_error(found.motion, truth, QUAD) < 0.01, "{model:?}");
    }
}

#[test]
fn constrained_models_track_with_fewer_matches_than_perspective() {
    let truth = pixel_motion([1., 0., 12., 0., 1., 8.]);
    let few = matches(
        truth,
        &[[0.3, 0.3], [0.7, 0.32], [0.5, 0.7], [0.32, 0.66]],
        0.,
    );
    assert!(estimate(&few, PORTRAIT, 1.5, Motion::Perspective).is_err());
    let found = estimate(&few, PORTRAIT, 1.5, Motion::Similarity).unwrap();
    assert!(corner_error(found.motion, truth, QUAD) < 0.01);
    assert_eq!(
        estimate(&few, PORTRAIT, 1.5, Motion::Auto).unwrap().model,
        Motion::Similarity
    );
}

#[test]
fn automatic_model_avoids_invented_perspective_but_keeps_real_perspective() {
    // Noisy matches clustered in a small patch: perspective overfits and its extrapolated
    // corners swing far, while the affine truth is recovered by the automatic choice.
    let truth = pixel_motion([1.02, 0.03, 15., -0.02, 0.99, -9.]);
    let patch = grid(5, 0.42, 0.58);
    let noisy = matches(truth, &patch, 0.8);
    let auto = estimate(&noisy, PORTRAIT, 2., Motion::Auto).unwrap();
    let perspective = estimate(&noisy, PORTRAIT, 2., Motion::Perspective).unwrap();
    assert_eq!(auto.model, Motion::Affine);
    let far = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    assert!(
        corner_error(auto.motion, truth, far) * 2.
            < corner_error(perspective.motion, truth, far).max(1.),
        "auto {} perspective {}",
        corner_error(auto.motion, truth, far),
        corner_error(perspective.motion, truth, far)
    );
    let strong =
        Homography::between(QUAD, [[0.2, 0.15], [0.8, 0.1], [0.98, 0.9], [0.02, 0.95]]).unwrap();
    let chosen = estimate(
        &matches(strong, &grid(6, 0.15, 0.85), 0.2),
        PORTRAIT,
        1.5,
        Motion::Auto,
    )
    .unwrap();
    assert_eq!(chosen.model, Motion::Perspective);
}
