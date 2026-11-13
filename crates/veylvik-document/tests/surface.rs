use veylvik_document::{surface::Look, *};
#[test]
fn tracked_surface_upgrade_preserves_graph_and_supports_generic_occlusion() {
    let mut doc = blank(320, 180, Time::new(24, 1).unwrap()).unwrap();
    let source = doc.output;
    let track = Node::new(
        "Track",
        Operation::PlanarTrack(veylvik_document::nodes::PlanarTrack {
            data: Box::default(),
        }),
        &[("image", source)],
    );
    let fg = Node::new(
        "Layer",
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [1., 0.5, 0.1, 1.],
        }),
        &[],
    );
    let warp = Node::new(
        "Warp",
        Operation::PlanarWarp(veylvik_document::nodes::PlanarWarp {
            mode: planar::WarpMode::Fit,
        }),
        &[("image", fg.id), ("track", track.id)],
    );
    let warp_id = warp.id;
    let merge = Node::new(
        "Composite",
        Operation::Merge(veylvik_document::nodes::Merge {}),
        &[("background", source), ("foreground", warp_id)],
    );
    doc.output = merge.id;
    for node in [track, fg, warp, merge] {
        doc.nodes.insert(node.id, node);
    }
    let original = serde_json::to_value(&doc).unwrap();
    let original_inputs = doc.nodes[&doc.output].inputs.clone();
    let (id, commands) = surface::integrate(&doc, warp_id, Look::Weathered).unwrap();
    assert_eq!(id, doc.output);
    let mut owner = Document::new(doc).unwrap();
    owner.apply(0, &commands).unwrap();
    assert_eq!(owner.snapshot().nodes[&id].inputs, original_inputs);
    assert!(matches!(
        owner.snapshot().nodes[&id].operation,
        Operation::SurfaceBlend(veylvik_document::nodes::SurfaceBlend { .. })
    ));
    let (occlusion, commands) =
        occlusion::create(owner.snapshot(), id, source, vector::PathMask::default()).unwrap();
    owner.apply(owner.revision(), &commands).unwrap();
    assert_eq!(
        owner.snapshot().nodes[&occlusion.output].inputs["background"],
        id
    );
    let decoded: Composition =
        serde_json::from_value(serde_json::to_value(owner.snapshot()).unwrap()).unwrap();
    decoded.validate().unwrap();
    owner.undo(owner.revision()).unwrap();
    owner.undo(owner.revision()).unwrap();
    assert_eq!(serde_json::to_value(owner.snapshot()).unwrap(), original);
}
#[test]
fn surface_parameters_support_animation_and_reject_invalid_ranges() {
    for look in Look::ALL {
        let operation = look.operation();
        operation.validate().unwrap();
    }
    let mut operation = Look::Painted.operation();
    *operation.scalar_mut("wear").unwrap() = Scalar::Linear {
        keys: vec![
            Key {
                time: Time::new(0, 1).unwrap(),
                value: 0.,
                segment: None,
            },
            Key {
                time: Time::new(1, 1).unwrap(),
                value: 1.,
                segment: None,
            },
        ],
    };
    operation.validate().unwrap();
    assert_eq!(
        operation
            .scalar("wear")
            .unwrap()
            .at(Time::new(1, 2).unwrap())
            .unwrap(),
        0.5
    );
    *operation.scalar_mut("distortion").unwrap() = 17.0.into();
    assert!(operation.validate().is_err());
}
