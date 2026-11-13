use veylvik_document::*;
use veylvik_render::Plan;
#[test]
fn plan_reuse_tracks_reachable_definitions_and_validates_disconnected_edits() {
    let image = Node::new(
        "image",
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [0.2, 0.4, 0.6, 1.0],
        }),
        &[],
    );
    let output = Node::new(
        "exposure",
        Operation::Exposure(veylvik_document::nodes::Exposure {
            stops: Scalar::Linear {
                keys: vec![
                    Key {
                        segment: None,
                        time: Time::new(0, 1).unwrap(),
                        value: 0.0,
                    },
                    Key {
                        segment: None,
                        time: Time::new(1, 1).unwrap(),
                        value: 2.0,
                    },
                ],
            },
        }),
        &[("image", image.id)],
    );
    let mut doc = Composition {
        display: Default::default(),
        groups: Default::default(),
        schema: 1,
        frame_count: 240,
        width: 13,
        height: 9,
        frame_rate: Time::new(24, 1).unwrap(),
        output: output.id,
        nodes: [image.clone(), output.clone()]
            .into_iter()
            .map(|n| (n.id, n))
            .collect(),
    };
    let plan = Plan::compile(&doc).unwrap();
    doc.nodes.get_mut(&image.id).unwrap().label = "renamed".into();
    doc.nodes.get_mut(&output.id).unwrap().position = Some(NodePosition { x: 200.0, y: -40.0 });
    doc.frame_rate = Time::new(30000, 1001).unwrap();
    doc.frame_count = 800;
    let other = Node::new(
        "disconnected",
        Operation::Noise(veylvik_document::nodes::Noise {
            seed: 17,
            scale: 8.0.into(),
        }),
        &[],
    );
    doc.nodes.insert(other.id, other.clone());
    assert!(plan.matches_composition(&doc, output.id).unwrap());
    // Timestamp representation changes recompile to preserve exact evaluation.
    let Operation::Exposure(veylvik_document::nodes::Exposure { stops }) =
        &mut doc.nodes.get_mut(&output.id).unwrap().operation
    else {
        unreachable!()
    };
    stops.keys_mut().unwrap()[1].time = Time::new(24, 24).unwrap();
    assert!(!plan.matches_composition(&doc, output.id).unwrap());
    doc.nodes.get_mut(&output.id).unwrap().operation = output.operation.clone();
    assert!(!plan.matches_composition(&doc, image.id).unwrap());
    doc.width += 1;
    assert!(!plan.matches_composition(&doc, output.id).unwrap());
    doc.width -= 1;
    doc.nodes
        .get_mut(&output.id)
        .unwrap()
        .inputs
        .insert("image".into(), other.id);
    assert!(!plan.matches_composition(&doc, output.id).unwrap());
    doc.nodes
        .get_mut(&output.id)
        .unwrap()
        .inputs
        .insert("image".into(), image.id);
    doc.nodes.get_mut(&image.id).unwrap().operation =
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [0.3, 0.4, 0.6, 1.0],
        });
    assert!(!plan.matches_composition(&doc, output.id).unwrap());
    doc.nodes.insert(image.id, image);
    assert!(plan.matches_composition(&doc, output.id).unwrap());
    doc.nodes.get_mut(&other.id).unwrap().operation =
        Operation::Noise(veylvik_document::nodes::Noise {
            seed: 17,
            scale: 0.0.into(),
        });
    assert!(plan.matches_composition(&doc, output.id).is_err());
}

#[test]
fn plan_reuse_preserves_signed_zero_in_authored_parameters() {
    let node = Node::new(
        "solid",
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [0.0, 0.0, 0.0, 1.0],
        }),
        &[],
    );
    let mut doc = Composition {
        display: Default::default(),
        groups: Default::default(),
        schema: 1,
        frame_count: 240,
        width: 1,
        height: 1,
        frame_rate: Time::new(24, 1).unwrap(),
        output: node.id,
        nodes: [(node.id, node)].into(),
    };
    let plan = Plan::compile(&doc).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation =
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [-0.0, 0.0, 0.0, 1.0],
        });
    assert!(!plan.matches_composition(&doc, doc.output).unwrap());
}
