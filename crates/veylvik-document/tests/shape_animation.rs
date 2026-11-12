use veylvik_document::{Time, vector::*};
fn shifted(mut p: PathMask, x: f32) -> PathMask {
    for c in &mut p.commands {
        match c {
            PathCommand::Move { to } | PathCommand::Line { to } => to[0] += x,
            PathCommand::Quad { control, to } => {
                control[0] += x;
                to[0] += x;
            }
            PathCommand::Cubic {
                control1,
                control2,
                to,
            } => {
                control1[0] += x;
                control2[0] += x;
                to[0] += x;
            }
            _ => {}
        }
    }
    p
}
#[test]
fn interpolates_all_coordinates_holds_ends_and_compares_exact_times() {
    let mut p = PathMask::default();
    p.commands[1] = PathCommand::Cubic {
        control1: [200.0, 50.0],
        control2: [300.0, 50.0],
        to: [400.0, 100.0],
    };
    let original = p.clone();
    let end = shifted(original.clone(), 100.0);
    p.set_key(Time::new(0, 24).unwrap(), &original).unwrap();
    p.set_key(Time::new(24, 24).unwrap(), &end).unwrap();
    assert_eq!(
        p.at(Time::new(1, 2).unwrap()).unwrap(),
        shifted(original.clone(), 50.0)
    );
    assert_eq!(p.at(Time::new(-1, 1).unwrap()).unwrap(), original);
    assert_eq!(p.at(Time::new(2, 1).unwrap()).unwrap(), end);
    p.set_key(Time::new(1, 1).unwrap(), &end).unwrap();
    assert_eq!(p.keys.len(), 2);
    p.remove_key(Time::new(1, 1).unwrap()).unwrap();
    assert_eq!(p.keys.len(), 1);
    p.remove_key(Time::new(0, 1).unwrap()).unwrap();
    assert_eq!(p, original);
}
#[test]
fn incompatible_geometry_and_bad_keys_are_rejected_atomically() {
    let mut p = PathMask::default();
    let original = p.clone();
    p.set_key(Time::new(0, 1).unwrap(), &original).unwrap();
    let before = p.clone();
    let mut bad = original.clone();
    bad.commands.remove(1);
    assert!(p.set_key(Time::new(1, 1).unwrap(), &bad).is_err());
    assert_eq!(p, before);
    assert!(p.set_key(Time::new(-1, 1).unwrap(), &original).is_err());
    assert_eq!(p, before);
    let mut bad = p.clone();
    bad.keys.push(bad.keys[0].clone());
    assert!(bad.validate().is_err());
    let mut bad = p.clone();
    bad.keys[0].commands[0] = PathCommand::Move {
        to: [f32::NAN, 0.0],
    };
    assert!(bad.validate().is_err());
    let mut bad = p.clone();
    bad.keys[0].time.rate = 0;
    assert!(bad.validate().is_err());
    assert!(bad.set_key(Time::new(1, 1).unwrap(), &original).is_err());
}
#[test]
fn static_json_stays_compatible_and_animation_survives_roundtrip() {
    let mut p = PathMask::default();
    let json = serde_json::to_value(&p).unwrap();
    assert!(json.get("keys").is_none());
    assert_eq!(serde_json::from_value::<PathMask>(json).unwrap(), p);
    let shape = p.clone();
    p.set_key(Time::new(0, 1).unwrap(), &shape).unwrap();
    p.set_key(Time::new(1, 1).unwrap(), &shifted(shape, 40.0))
        .unwrap();
    let decoded: PathMask = serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, p);
}
#[test]
fn large_timestamps_interpolate_without_cancellation() {
    let mut p = PathMask::default();
    let a = p.clone();
    let b = shifted(a.clone(), 100.0);
    let base = 9_000_000_000_000_000_i64;
    p.set_key(Time::new(base, 1).unwrap(), &a).unwrap();
    p.set_key(Time::new(base + 2, 1).unwrap(), &b).unwrap();
    assert_eq!(
        p.at(Time::new(base + 1, 1).unwrap()).unwrap(),
        shifted(a, 50.0)
    );
}

#[test]
fn resolution_mapping_scales_base_geometry_and_every_shape_key() {
    use veylvik_document::{
        Node, Operation,
        presets::resolution::{Sizing, map_graph},
    };
    let mut geometry = PathMask::default();
    let first = geometry.clone();
    geometry.set_key(Time::new(0, 1).unwrap(), &first).unwrap();
    geometry
        .set_key(Time::new(1, 1).unwrap(), &shifted(first, 50.0))
        .unwrap();
    let node = Node::new("Mask", Operation::Path { geometry }, &[]);
    let id = node.id;
    let mut nodes = [(id, node)].into();
    map_graph(
        &mut nodes,
        &mut Default::default(),
        [960, 540],
        [3840, 2160],
        Sizing::Fit,
    )
    .unwrap();
    let Operation::Path { geometry } = &nodes[&id].operation else {
        panic!()
    };
    assert_eq!(
        geometry.commands[0],
        PathCommand::Move { to: [400.0, 400.0] }
    );
    assert_eq!(
        geometry.keys[1].commands[0],
        PathCommand::Move { to: [600.0, 400.0] }
    );
    assert!(
        geometry.keys[1]
            .time
            .compare(Time::new(1, 1).unwrap())
            .unwrap()
            .is_eq()
    );
}

#[test]
fn key_limit_failure_preserves_existing_animation() {
    let mut p = PathMask::default();
    let shape = p.clone();
    for i in 0..MAX_SHAPE_KEYS {
        p.set_key(Time::new(i as i64, 1).unwrap(), &shape).unwrap();
    }
    let before = p.clone();
    assert!(
        p.set_key(Time::new(MAX_SHAPE_KEYS as i64, 1).unwrap(), &shape)
            .is_err()
    );
    assert_eq!(p, before);
}
