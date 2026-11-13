use veylvik_document::{Operation, Time, nodes::FadeTiming, registry};

fn t(frames: i64) -> Time {
    Time::at_frame(frames, Time { ticks: 24, rate: 1 }).unwrap()
}
fn timing(start: i64, end: Option<i64>, fade_in: i64, fade_out: i64) -> FadeTiming {
    FadeTiming {
        start: t(start),
        end: end.map(t),
        fade_in: t(fade_in),
        fade_out: t(fade_out),
    }
}

#[test]
fn envelope_ramps_in_holds_ramps_out_and_hides_outside() {
    let fade = timing(24, Some(96), 12, 24);
    fade.validate().unwrap();
    let at = |frame| fade.envelope(t(frame)).unwrap();
    assert_eq!(at(0), 0.);
    assert_eq!(at(23), 0.);
    assert_eq!(at(24), 0.);
    assert!((at(30) - 0.5).abs() < 1e-12);
    assert_eq!(at(36), 1.);
    assert_eq!(at(72), 1.);
    assert!((at(84) - 0.5).abs() < 1e-12);
    assert_eq!(at(96), 0.);
    assert_eq!(at(500), 0.);
    let forever = timing(0, None, 0, 0);
    assert!(forever.always_visible());
    assert_eq!(forever.envelope(t(10_000)).unwrap(), 1.);
    assert!(!timing(0, None, 12, 0).always_visible());
}

#[test]
fn invalid_timings_are_refused_and_defaults_round_trip() {
    assert!(timing(48, Some(24), 0, 0).validate().is_err());
    assert!(timing(0, Some(24), 12, 13).validate().is_err());
    timing(0, Some(24), 12, 12).validate().unwrap();
    let operation: Operation = serde_json::from_value(registry::FADE.default_operation()).unwrap();
    operation.validate().unwrap();
    let saved = serde_json::to_value(&operation).unwrap();
    assert_eq!(saved["type"], "fade");
    assert!(saved["timing"]["end"].is_null());
    let back: Operation = serde_json::from_value(saved).unwrap();
    back.validate().unwrap();
}
