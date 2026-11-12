use veylvik_document::{Interpolation, Key, Scalar, Time};
fn curve(mode: Interpolation) -> Scalar {
    let mut value = Scalar::Linear {
        keys: vec![
            Key {
                segment: None,
                time: Time::new(0, 24).unwrap(),
                value: 0.0,
            },
            Key {
                segment: None,
                time: Time::new(48, 24).unwrap(),
                value: 8.0,
            },
            Key {
                segment: None,
                time: Time::new(120, 30).unwrap(),
                value: -8.0,
            },
        ],
    };
    value.set_interpolation(mode);
    value
}
#[test]
fn interpolation_modes_have_exact_key_boundaries_and_hold_endpoints() {
    for mode in [
        Interpolation::Linear,
        Interpolation::Step,
        Interpolation::Smooth,
        Interpolation::Monotone,
    ] {
        let value = curve(mode);
        for (ticks, rate, expected) in [
            (-1, 1, 0.0),
            (0, 1, 0.0),
            (60, 30, 8.0),
            (4, 1, -8.0),
            (9, 1, -8.0),
        ] {
            assert_eq!(value.at(Time::new(ticks, rate).unwrap()).unwrap(), expected);
        }
        let quarter = value.at(Time::new(1, 2).unwrap()).unwrap();
        assert_eq!(
            quarter,
            match mode {
                Interpolation::Linear => 2.0,
                Interpolation::Step => 0.0,
                Interpolation::Smooth => 1.25,
                Interpolation::Monotone => 2.375,
            }
        );
        assert_eq!(
            value.at(Time::new(3, 1).unwrap()).unwrap(),
            if mode == Interpolation::Step {
                8.0
            } else if mode == Interpolation::Monotone {
                2.0
            } else {
                0.0
            }
        );
        let wire = serde_json::to_string(&value).unwrap();
        let decoded: Scalar = serde_json::from_str(&wire).unwrap();
        assert_eq!(decoded.interpolation(), Some(mode));
        assert_eq!(decoded.at(Time::new(1, 2).unwrap()).unwrap(), quarter);
    }
}
#[test]
fn curves_remain_bounded_even_for_large_times_and_extreme_values() {
    for mode in [
        Interpolation::Linear,
        Interpolation::Step,
        Interpolation::Smooth,
        Interpolation::Monotone,
    ] {
        let mut value = Scalar::Linear {
            keys: vec![
                Key {
                    segment: None,
                    time: Time::new(i64::MAX - 4, u32::MAX).unwrap(),
                    value: -f32::MAX,
                },
                Key {
                    segment: None,
                    time: Time::new(i64::MAX, u32::MAX).unwrap(),
                    value: f32::MAX,
                },
            ],
        };
        value.set_interpolation(mode);
        for offset in 0..=4 {
            let result = value
                .at(Time::new(i64::MAX - offset, u32::MAX).unwrap())
                .unwrap();
            assert!(result.is_finite());
        }
        let middle = value
            .at(Time::new(i64::MAX - 2, u32::MAX).unwrap())
            .unwrap();
        assert_eq!(
            middle,
            if mode == Interpolation::Step {
                -f32::MAX
            } else {
                0.0
            }
        );
    }
}
#[test]
fn every_mode_rejects_invalid_keys_and_enforces_resource_limit() {
    for mode in [
        Interpolation::Linear,
        Interpolation::Step,
        Interpolation::Smooth,
        Interpolation::Monotone,
    ] {
        let mut value = curve(mode);
        value.keys_mut().unwrap()[1].time = Time::new(0, 1).unwrap();
        assert!(value.validate().is_err());
        value.keys_mut().unwrap().clear();
        assert!(value.validate().is_err());
        *value.keys_mut().unwrap() = (0..=veylvik_document::animation::MAX_KEYS)
            .map(|i| Key {
                segment: None,
                time: Time::new(i as i64, 1).unwrap(),
                value: 0.0,
            })
            .collect();
        assert!(value.validate().is_err());
        value.keys_mut().unwrap().pop();
        value.validate().unwrap();
    }
    let mut constant = Scalar::from(3.0);
    constant.set_interpolation(Interpolation::Smooth);
    assert_eq!(constant.interpolation(), None);
    assert_eq!(constant.at(Time::new(1, 1).unwrap()).unwrap(), 3.0);
}

#[test]
fn frame_times_are_exact_reduced_and_preflighted() {
    use veylvik_document::sequence::FrameRange;
    let fps = Time::new(30000, 1001).unwrap();
    let time = Time::at_frame(30, fps).unwrap();
    assert_eq!((time.ticks, time.rate), (1001, 1000));
    assert!(
        Time::at_frame(-30, fps)
            .unwrap()
            .compare(Time::new(-1001, 1000).unwrap())
            .unwrap()
            .is_eq()
    );
    // Reduction makes this representable despite multiplication exceeding i64.
    assert_eq!(
        Time::at_frame(i64::MAX, Time::new(2, 2).unwrap())
            .unwrap()
            .ticks,
        i64::MAX
    );
    assert!(Time::at_frame(i64::MAX, Time::new(1, 2).unwrap()).is_err());
    assert_eq!(FrameRange::new(-1, 1, fps).unwrap().count().unwrap(), 3);
    assert!(FrameRange::new(2, 1, fps).is_err());
    assert!(FrameRange::new(i64::MIN, i64::MAX, fps).is_err());
    assert!(FrameRange::new(0, 10000, fps).is_err());
    assert!(FrameRange::new(0, 0, Time::new(-1, 1).unwrap()).is_err());
    // Both endpoints fit after reduction, but the interior denominator cannot fit.
    let huge = Time::new(i64::from(u32::MAX) + 1, 1).unwrap();
    assert!(FrameRange::new(0, 2, huge).is_err());
}

#[test]
fn playhead_key_edits_preserve_exact_times_modes_and_fail_atomically() {
    for mode in [
        Interpolation::Linear,
        Interpolation::Step,
        Interpolation::Smooth,
        Interpolation::Monotone,
    ] {
        let mut value = curve(mode);
        value.set_key_at(Time::new(60, 30).unwrap(), 3.0).unwrap();
        assert_eq!(value.keys().unwrap().len(), 3);
        let existing = &value.keys().unwrap()[1];
        assert_eq!((existing.time.ticks, existing.time.rate), (48, 24));
        assert_eq!(existing.value, 3.0);
        value.set_key_at(Time::new(1, 3).unwrap(), -0.0).unwrap();
        value.set_key_at(Time::new(-1, 1).unwrap(), 7.0).unwrap();
        value.set_key_at(Time::new(10, 1).unwrap(), -7.0).unwrap();
        assert_eq!(value.interpolation(), Some(mode));
        assert_eq!(value.keys().unwrap().len(), 6);
        assert_eq!(
            value.at(Time::new(2, 6).unwrap()).unwrap().to_bits(),
            (-0.0f32).to_bits()
        );
        value.validate().unwrap();
        let before = serde_json::to_vec(&value).unwrap();
        for (time, sample) in [
            (Time { ticks: 0, rate: 0 }, 1.0),
            (Time::new(2, 1).unwrap(), f32::NAN),
        ] {
            assert!(value.set_key_at(time, sample).is_err());
            assert_eq!(serde_json::to_vec(&value).unwrap(), before);
        }
    }
    let mut constant = Scalar::from(2.0);
    constant
        .set_key_at(Time::new(i64::MAX - 1, u32::MAX).unwrap(), 4.0)
        .unwrap();
    constant
        .set_key_at(Time::new(i64::MAX, u32::MAX).unwrap(), 5.0)
        .unwrap();
    assert_eq!(constant.keys().unwrap().len(), 2);
    assert_eq!(constant.keys().unwrap()[0].value, 4.0);
    let mut full = Scalar::Step {
        keys: (0..veylvik_document::animation::MAX_KEYS)
            .map(|index| Key {
                segment: None,
                time: Time::new(index as i64, 1).unwrap(),
                value: 0.0,
            })
            .collect(),
    };
    full.set_key_at(Time::new(4, 2).unwrap(), 8.0).unwrap();
    assert_eq!(full.keys().unwrap()[2].value, 8.0);
    let before = serde_json::to_vec(&full).unwrap();
    assert!(full.set_key_at(Time::new(1, 2).unwrap(), 1.0).is_err());
    assert_eq!(serde_json::to_vec(&full).unwrap(), before);
}

#[test]
fn monotone_curves_share_slopes_without_overshoot_on_uneven_intervals() {
    let scalar = Scalar::Monotone {
        keys: vec![
            Key {
                segment: None,
                time: Time::new(0, 1).unwrap(),
                value: 0.0,
            },
            Key {
                segment: None,
                time: Time::new(1, 1).unwrap(),
                value: 1.0,
            },
            Key {
                segment: None,
                time: Time::new(3, 1).unwrap(),
                value: 2.0,
            },
        ],
    };
    // Interior slope is the weighted harmonic mean: 9 / (5/1 + 4/0.5).
    assert!((scalar.at(Time::new(1, 2).unwrap()).unwrap() - 7.0 / 13.0).abs() < 1e-6);
    assert!((scalar.at(Time::new(2, 1).unwrap()).unwrap() - (1.5 + 5.0 / 104.0)).abs() < 1e-6);
    let left = (1.0 - scalar.at(Time::new(9999, 10000).unwrap()).unwrap()) * 10000.0;
    let right = (scalar.at(Time::new(10001, 10000).unwrap()).unwrap() - 1.0) * 10000.0;
    assert!((left - 9.0 / 13.0).abs() < 0.002);
    assert!((right - left).abs() < 0.002);
    let segments = scalar.monotone_segments().unwrap().unwrap();
    assert!(segments[0].sample(f64::NAN).is_err());
    assert_eq!(segments[0].sample(-1.0).unwrap(), 0.0);
    assert_eq!(segments[0].sample(2.0).unwrap(), 1.0);
    let mut last = 0.0;
    for tick in 0..=300 {
        let value = scalar.at(Time::new(tick, 100).unwrap()).unwrap();
        assert!(value >= last && value <= 2.0);
        last = value;
    }
    for values in [
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 1.0],
        [-f32::MAX, f32::MAX, -f32::MAX],
        [1e-30, 1e30, 1.0],
    ] {
        let mut curve = scalar.clone();
        for (key, value) in curve.keys_mut().unwrap().iter_mut().zip(values) {
            key.value = value;
        }
        for (index, segment) in curve
            .monotone_segments()
            .unwrap()
            .unwrap()
            .iter()
            .enumerate()
        {
            for step in 0..=100 {
                let value = segment.sample(step as f64 / 100.0).unwrap();
                assert!(value.is_finite());
                assert!(
                    (values[index].min(values[index + 1])..=values[index].max(values[index + 1]))
                        .contains(&value)
                );
            }
        }
    }
}

#[test]
fn monotone_segments_handle_extreme_uneven_rational_spacing() {
    let scalar = Scalar::Monotone {
        keys: vec![
            Key {
                segment: None,
                time: Time::new(i64::MIN, 1).unwrap(),
                value: -f32::MAX,
            },
            Key {
                segment: None,
                time: Time::new(2147483646, 2147483647).unwrap(),
                value: 0.0,
            },
            Key {
                segment: None,
                time: Time::new(2147483647, 2147483648).unwrap(),
                value: 1.0,
            },
            Key {
                segment: None,
                time: Time::new(i64::MAX, 1).unwrap(),
                value: f32::MAX,
            },
        ],
    };
    for segment in scalar.monotone_segments().unwrap().unwrap() {
        let mut previous = segment.sample(0.0).unwrap();
        for i in 1..=1000 {
            let value = segment.sample(i as f64 / 1000.0).unwrap();
            assert!(value.is_finite() && value >= previous);
            previous = value;
        }
    }
}

#[test]
fn compiled_curves_are_validated_immutable_snapshots_with_exact_samples() {
    for mode in [
        Interpolation::Linear,
        Interpolation::Step,
        Interpolation::Smooth,
        Interpolation::Monotone,
    ] {
        let mut scalar = curve(mode);
        let compiled = scalar.compile().unwrap();
        for tick in -20..=200 {
            let time = Time::new(tick, 24).unwrap();
            assert_eq!(
                compiled.at(time).unwrap().to_bits(),
                scalar.at(time).unwrap().to_bits()
            );
        }
        let exact = Time::new(2, 1).unwrap();
        scalar.set_key_at(exact, -3.0).unwrap();
        assert_eq!(compiled.at(exact).unwrap(), 8.0);
        assert_eq!(scalar.compile().unwrap().at(exact).unwrap(), -3.0);
        assert!(compiled.at(Time { ticks: 0, rate: 0 }).is_err());
        scalar.keys_mut().unwrap()[1].time.rate = 0;
        assert!(scalar.compile().is_err());
    }
    let constant = Scalar::from(-0.0).compile().unwrap();
    assert_eq!(
        constant.at(Time::new(0, 1).unwrap()).unwrap().to_bits(),
        (-0.0_f32).to_bits()
    );
    assert!(Scalar::from(f32::NAN).compile().is_err());
    let extreme = Scalar::Monotone {
        keys: vec![
            Key {
                segment: None,
                time: Time::new(i64::MAX - 2, u32::MAX).unwrap(),
                value: -1.0,
            },
            Key {
                segment: None,
                time: Time::new(i64::MAX, u32::MAX).unwrap(),
                value: 1.0,
            },
        ],
    }
    .compile()
    .unwrap();
    assert_eq!(
        extreme
            .at(Time::new(i64::MAX - 1, u32::MAX).unwrap())
            .unwrap(),
        0.0
    );
}
