use veylvik_document::{Operation, Time, nodes::OpticalFlow, registry};
#[test]
fn source_pairs_preserve_fractional_cadence_and_reverse_boundaries() {
    let Operation::OpticalFlow(mut flow): Operation =
        serde_json::from_value(registry::OPTICAL_FLOW.default_operation()).unwrap()
    else {
        panic!()
    };
    for (ticks, rate) in [(24, 1), (24000, 1001), (30000, 1001)] {
        flow.source_rate = Time::new(ticks, rate).unwrap();
        for index in [-100, -1, 0, 1, 100] {
            let (a, b, f) = flow
                .source_pair(Time::at_frame(index * 2, flow.source_rate).unwrap())
                .unwrap();
            assert_eq!(
                a.seconds().unwrap(),
                Time::at_frame(index, flow.source_rate)
                    .unwrap()
                    .seconds()
                    .unwrap()
            );
            assert_eq!(
                b.seconds().unwrap(),
                Time::at_frame(index + 1, flow.source_rate)
                    .unwrap()
                    .seconds()
                    .unwrap()
            );
            assert_eq!(f, 0.);
        }
    }
    flow.source_rate = Time::new(24, 1).unwrap();
    let (a, _, f) = flow.source_pair(Time::new(-1, 24).unwrap()).unwrap();
    assert_eq!(a.seconds().unwrap(), -1. / 24.);
    assert_eq!(f, 0.5);
    let _roundtrip: OpticalFlow =
        serde_json::from_value(serde_json::to_value(flow).unwrap()).unwrap();
}
