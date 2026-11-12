use veylvik_document::*;
fn key(ticks: i64, value: f32, segment: Option<Segment>) -> Key {
    Key {
        time: Time::new(ticks, 1).unwrap(),
        value,
        segment,
    }
}
fn bezier() -> Segment {
    Segment::Bezier {
        outgoing: Handle {
            time: 1.0 / 3.0,
            value: 2.0,
        },
        incoming: Handle {
            time: 1.0 / 3.0,
            value: 1.0,
        },
    }
}
#[test]
fn exact_time_bezier_sampling_mixed_modes_and_legacy_roundtrip() {
    let scalar = Scalar::Linear {
        keys: vec![
            key(0, 0.0, Some(bezier())),
            key(2, 1.0, Some(Segment::Hold)),
            key(4, 3.0, None),
        ],
    };
    let compiled = scalar.compile().unwrap();
    assert!((scalar.at(Time::new(1, 1).unwrap()).unwrap() - 1.625).abs() < 1e-6);
    assert_eq!(
        compiled.at(Time::new(1, 1).unwrap()).unwrap(),
        scalar.at(Time::new(1, 1).unwrap()).unwrap()
    );
    assert_eq!(compiled.at(Time::new(3, 1).unwrap()).unwrap(), 1.0);
    assert_eq!(compiled.at(Time::new(4, 1).unwrap()).unwrap(), 3.0);
    let legacy = r#"{"kind":"smooth","keys":[{"time":{"ticks":0,"rate":1},"value":0.0},{"time":{"ticks":2,"rate":1},"value":1.0}]}"#;
    let old: Scalar = serde_json::from_str(legacy).unwrap();
    assert_eq!(
        serde_json::to_value(&old).unwrap(),
        serde_json::from_str::<serde_json::Value>(legacy).unwrap()
    );
    assert_eq!(old.at(Time::new(1, 1).unwrap()).unwrap(), 0.5);
    let mut huge = scalar.clone();
    for k in huge.keys_mut().unwrap() {
        k.time.ticks += i64::MAX - 4;
    }
    assert_eq!(huge.at(Time::new(i64::MAX - 3, 1).unwrap()).unwrap(), 1.625);
    let copied = key_edit::copied(&scalar, &[0, 1].into()).unwrap();
    let pasted = key_edit::pasted(&0.0.into(), &copied, Time::new(0, 1).unwrap()).unwrap();
    assert_eq!(pasted.at(Time::new(1, 1).unwrap()).unwrap(), 1.625);
}
#[test]
fn handles_validate_time_order_bounds_and_atomic_history() {
    let mut doc = Document::new(motion_demo()).unwrap();
    let id = doc.snapshot().output;
    let scalar = Scalar::Linear {
        keys: vec![key(0, 0.0, Some(bezier())), key(2, 1.0, None)],
    };
    let command = |value: &Scalar| Command::SetParameter {
        id,
        parameter: "stops".into(),
        value: serde_json::to_value(value).unwrap(),
    };
    doc.apply(0, &[command(&scalar)]).unwrap();
    let before = serde_json::to_value(doc.snapshot()).unwrap();
    for bad in [
        Segment::Bezier {
            outgoing: Handle {
                time: 0.8,
                value: 0.0,
            },
            incoming: Handle {
                time: 0.8,
                value: 0.0,
            },
        },
        Segment::Bezier {
            outgoing: Handle {
                time: 0.3,
                value: 100.0,
            },
            incoming: Handle {
                time: 0.3,
                value: 0.0,
            },
        },
    ] {
        let mut invalid = scalar.clone();
        invalid.keys_mut().unwrap()[0].segment = Some(bad);
        assert!(doc.apply(1, &[command(&invalid)]).is_err());
        assert_eq!(serde_json::to_value(doc.snapshot()).unwrap(), before);
    }
    doc.undo(1).unwrap();
    doc.redo(2).unwrap();
    assert_eq!(serde_json::to_value(doc.snapshot()).unwrap(), before);
    let mut reset = scalar;
    reset.set_interpolation(Interpolation::Smooth);
    assert!(reset.keys().unwrap().iter().all(|k| k.segment.is_none()));
    let mut invalid = reset;
    invalid.keys_mut().unwrap()[0].segment = Some(Segment::Bezier {
        outgoing: Handle {
            time: f32::NAN,
            value: 0.0,
        },
        incoming: Handle {
            time: 0.3,
            value: 0.0,
        },
    });
    assert!(invalid.validate().is_err());
}

#[test]
fn weighted_time_handles_and_auto_conversion_preserve_the_evaluated_curve() {
    let mut weighted = Scalar::Linear {
        keys: vec![
            key(
                0,
                0.0,
                Some(Segment::Bezier {
                    outgoing: Handle {
                        time: 0.1,
                        value: 2.0,
                    },
                    incoming: Handle {
                        time: 0.2,
                        value: 1.0,
                    },
                }),
            ),
            key(2, 1.0, None),
        ],
    };
    // At Bezier parameter u=1/2, x=.4625 and y=1.625. This verifies
    // inversion of nonuniform horizontal handles rather than treating u as time.
    assert!((weighted.at(Time::new(37, 40).unwrap()).unwrap() - 1.625).abs() < 1e-6);
    let automatic = Scalar::Monotone {
        keys: vec![key(0, 0.0, None), key(1, 2.0, None), key(4, 3.0, None)],
    };
    let mut manual = automatic.clone();
    for index in 0..2 {
        manual.keys_mut().unwrap()[index].segment = Some(automatic.bezier_segment(index).unwrap());
    }
    for tick in 0..=96 {
        let at = Time::new(tick, 24).unwrap();
        assert!((automatic.at(at).unwrap() - manual.at(at).unwrap()).abs() < 1e-6);
    }
    let before = serde_json::to_value(&weighted).unwrap();
    weighted.keys_mut().unwrap()[0].segment = Some(Segment::Bezier {
        outgoing: Handle {
            time: 0.2,
            value: f32::MAX,
        },
        incoming: Handle {
            time: 0.2,
            value: 0.0,
        },
    });
    let valid = serde_json::to_value(&weighted).unwrap();
    assert!(
        weighted
            .set_key_at(Time::new(0, 1).unwrap(), f32::MAX)
            .is_err()
    );
    assert_eq!(serde_json::to_value(&weighted).unwrap(), valid);
    assert_ne!(valid, before);
}
