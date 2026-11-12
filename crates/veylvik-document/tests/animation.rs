use veylvik_document::{Interpolation, Key, Scalar, Time};
fn curve(mode: Interpolation) -> Scalar {
    let mut value = Scalar::Linear {
        keys: vec![
            Key {
                time: Time::new(0, 24).unwrap(),
                value: 0.0,
            },
            Key {
                time: Time::new(48, 24).unwrap(),
                value: 8.0,
            },
            Key {
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
            }
        );
        assert_eq!(
            value.at(Time::new(3, 1).unwrap()).unwrap(),
            if mode == Interpolation::Step {
                8.0
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
    ] {
        let mut value = Scalar::Linear {
            keys: vec![
                Key {
                    time: Time::new(i64::MAX - 4, u32::MAX).unwrap(),
                    value: -f32::MAX,
                },
                Key {
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
    ] {
        let mut value = curve(mode);
        value.keys_mut().unwrap()[1].time = Time::new(0, 1).unwrap();
        assert!(value.validate().is_err());
        value.keys_mut().unwrap().clear();
        assert!(value.validate().is_err());
        *value.keys_mut().unwrap() = (0..=veylvik_document::animation::MAX_KEYS)
            .map(|i| Key {
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
