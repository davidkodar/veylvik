use veylvik_document::{Time, planar::*};
const QUAD: Quad = [[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]];
fn close(a: Point, b: Point) {
    assert!(
        (a[0] - b[0]).abs() < 1e-7 && (a[1] - b[1]).abs() < 1e-7,
        "{a:?} != {b:?}"
    );
}
#[test]
fn perspective_inverse_and_composition_preserve_points() {
    let target = [[0.15, 0.2], [0.8, 0.05], [0.95, 0.85], [0.25, 0.95]];
    let h = Homography::between(QUAD, target).unwrap();
    for (a, b) in QUAD.into_iter().zip(target) {
        close(h.project(a).unwrap(), b);
        close(h.inverse().unwrap().project(b).unwrap(), a);
    }
    close(
        h.inverse()
            .unwrap()
            .compose(h)
            .project([0.45, 0.63])
            .unwrap(),
        [0.45, 0.63],
    );
    assert!(Homography::between([[0., 0.]; 4], target).is_err());
    assert!(validate_quad([[0., 0.], [1., 1.], [1., 0.], [0., 1.]]).is_err());
    assert!(
        Homography {
            matrix: [1., 0., 0., 0., 1., 0., 2., 0., -1.]
        }
        .transform_quad(QUAD)
        .is_err()
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
#[test]
fn track_roundtrip_interpolation_and_invalid_times() {
    let shifted = QUAD.map(|[x, y]| [x + 0.2, y - 0.1]);
    let mut track = Track {
        reference_time: Time::new(0, 1).unwrap(),
        reference: QUAD,
        region: None,
        samples: vec![
            Sample {
                time: Time::new(0, 1).unwrap(),
                corners: QUAD,
                features: vec![],
                rms_pixels: 0.,
            },
            Sample {
                time: Time::new(2, 1).unwrap(),
                corners: shifted,
                features: vec![],
                rms_pixels: 0.,
            },
        ],
    };
    track.validate().unwrap();
    let track2: Track = serde_json::from_slice(&serde_json::to_vec(&track).unwrap()).unwrap();
    track2.validate().unwrap();
    close(
        track2
            .motion_at(Time::new(1, 1).unwrap())
            .unwrap()
            .project([0.5, 0.5])
            .unwrap(),
        [0.6, 0.45],
    );
    close(
        track2
            .motion_at(Time::new(-1, 1).unwrap())
            .unwrap()
            .project([0.5, 0.5])
            .unwrap(),
        [0.5, 0.5],
    );
    close(
        track2
            .motion_at(Time::new(3, 1).unwrap())
            .unwrap()
            .project([0.5, 0.5])
            .unwrap(),
        [0.7, 0.4],
    );
    track.samples[1].time = Time::new(0, 2).unwrap();
    assert!(track.validate().is_err());
}

#[test]
fn saved_track_is_independent_reusable_and_undoable() {
    use veylvik_document::{Command, Document, Node, Operation};
    let doc = veylvik_document::blank(320, 180, Time::new(24, 1).unwrap()).unwrap();
    let original = doc.output;
    let (id, commands) = save(&doc, original, Track::default()).unwrap();
    let mut document = Document::new(doc).unwrap();
    document.apply(0, &commands).unwrap();
    assert_eq!(document.snapshot().output, original);
    let warp = Node::new(
        "Tracked layer",
        Operation::PlanarWarp {
            mode: WarpMode::Follow,
        },
        &[("image", original), ("track", id)],
    );
    let warp_id = warp.id;
    document
        .apply(
            document.revision(),
            &[
                Command::Put {
                    node: Box::new(warp),
                },
                Command::Output { id: warp_id },
            ],
        )
        .unwrap();
    let roundtrip: veylvik_document::Composition =
        serde_json::from_slice(&serde_json::to_vec(document.snapshot()).unwrap()).unwrap();
    roundtrip.validate().unwrap();
    let mut invalid = roundtrip.clone();
    invalid
        .nodes
        .get_mut(&warp_id)
        .unwrap()
        .inputs
        .insert("track".into(), original);
    assert!(invalid.validate().is_err());
    let mut invalid = roundtrip;
    invalid.nodes.get_mut(&id).unwrap().bypass = true;
    assert!(invalid.validate().is_err());
    document.undo(document.revision()).unwrap();
    assert_eq!(document.snapshot().output, original);
    document.undo(document.revision()).unwrap();
    assert!(!document.snapshot().nodes.contains_key(&id));
}

#[test]
fn corrected_section_preserves_reference_other_frames_and_feature_basis() {
    let mut original = Track::default();
    original.samples = (0..10)
        .map(|i| Sample {
            time: Time::new(i, 24).unwrap(),
            corners: original.reference.map(|p| [p[0] + i as f64 * 0.01, p[1]]),
            features: vec![],
            rms_pixels: 0.,
        })
        .collect();
    let corrected = original.reference.map(|p| [p[0] + 0.05, p[1] + 0.02]);
    let mut section = Track {
        reference_time: Time::new(4, 24).unwrap(),
        reference: corrected,
        region: None,
        samples: vec![],
    };
    // A backward analysis returns sorted successful frames, stopping at frame 2.
    for i in 2..=4 {
        section.samples.push(Sample {
            time: Time::new(i, 24).unwrap(),
            corners: corrected.map(|p| [p[0] + (i - 4) as f64 * 0.01, p[1]]),
            features: vec![Feature {
                reference: [0.55, 0.52],
                position: [0.55 + (i - 4) as f64 * 0.01, 0.52],
                correlation: 1.,
                separation: 0.5,
                accepted: true,
            }],
            rms_pixels: 0.2,
        });
    }
    let merged = original.splice(&section).unwrap();
    assert_eq!(merged.reference, original.reference);
    assert!(
        merged
            .reference_time
            .compare(original.reference_time)
            .unwrap()
            .is_eq()
    );
    assert_eq!(merged.samples.len(), 10);
    for i in [0, 1, 5, 6, 7, 8, 9] {
        assert_eq!(
            serde_json::to_value(&merged.samples[i]).unwrap(),
            serde_json::to_value(&original.samples[i]).unwrap()
        );
    }
    for i in 2..=4 {
        close(merged.samples[i].features[0].reference, [0.5, 0.5]);
        close(
            merged
                .motion_at(merged.samples[i].time)
                .unwrap()
                .project([0.5, 0.5])
                .unwrap(),
            merged.samples[i].features[0].position,
        );
    }
    let mut invalid = section.clone();
    invalid.samples[0].corners[0] = invalid.samples[0].corners[1];
    assert!(original.splice(&invalid).is_err());
    original.validate().unwrap();
}
