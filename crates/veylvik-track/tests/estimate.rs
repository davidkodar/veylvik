use veylvik_document::planar::*;
use veylvik_track::estimate::estimate;
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
        let result = estimate(&features, size, 1.5).unwrap();
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
    assert!(estimate(&features, [1920, 1080], 2.).is_err());
    for f in &mut features {
        f.separation = 0.;
    }
    assert!(estimate(&features, [1920, 1080], 2.).is_err());
    assert!(estimate(&features, [0, 1080], 2.).is_err());
    assert!(estimate(&features, [1920, 1080], f64::NAN).is_err());
}
