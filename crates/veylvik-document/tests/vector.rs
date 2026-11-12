use veylvik_document::vector::*;
fn triangle() -> PathMask {
    PathMask {
        keys: vec![],
        fill_rule: FillRule::NonZero,
        commands: vec![
            PathCommand::Move { to: [0.0, 0.0] },
            PathCommand::Line { to: [10.0, 0.0] },
            PathCommand::Line { to: [5.0, 10.0] },
            PathCommand::Close {},
        ],
    }
}
#[test]
fn path_round_trip_preserves_contours_curves_and_fill_rule() {
    let mut path = triangle();
    path.fill_rule = FillRule::EvenOdd;
    path.commands.extend([
        PathCommand::Move { to: [2.0, 2.0] },
        PathCommand::Quad {
            control: [3.0, 8.0],
            to: [4.0, 4.0],
        },
        PathCommand::Cubic {
            control1: [1.0, 2.0],
            control2: [3.0, 4.0],
            to: [2.0, 2.0],
        },
        PathCommand::Close {},
    ]);
    path.validate().unwrap();
    let json = serde_json::to_string(&path).unwrap();
    assert_eq!(serde_json::from_str::<PathMask>(&json).unwrap(), path);
    assert!(serde_json::from_str::<PathMask>(&json.replace("\"even_odd\"", "\"future\"")).is_err());
    assert!(serde_json::from_str::<PathCommand>(r#"{"command":"close","extra":1}"#).is_err());
}
#[test]
fn malformed_and_excessive_geometry_is_rejected_before_rasterization() {
    let mut path = triangle();
    for commands in [
        vec![],
        vec![PathCommand::Close {}],
        vec![PathCommand::Line { to: [0.0; 2] }],
        vec![PathCommand::Move { to: [0.0; 2] }, PathCommand::Close {}],
        vec![PathCommand::Move { to: [0.0; 2] }; 2],
        path.commands[..3].to_vec(),
    ] {
        path.commands = commands;
        assert!(path.validate().is_err());
    }
    for invalid in [f32::NAN, f32::INFINITY, MAX_PATH_COORDINATE + 1.0] {
        path = triangle();
        path.commands[0] = PathCommand::Move { to: [invalid, 0.0] };
        assert!(path.validate().is_err());
    }
    path = triangle();
    path.commands = path.commands.repeat(MAX_PATH_COMMANDS / 4);
    path.validate().unwrap();
    path.commands.push(PathCommand::Close {});
    assert!(path.validate().is_err());
}
