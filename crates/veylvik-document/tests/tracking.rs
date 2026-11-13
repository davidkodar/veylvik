use veylvik_document::{
    Document, Operation, Time,
    tracking::{Sample, attach},
};
#[test]
fn tracking_wraps_atomically_and_survives_undo_and_serialization() {
    let doc = veylvik_document::blank(96, 64, Time::new(24, 1).unwrap()).unwrap();
    let samples = vec![
        Sample {
            time: Time::new(1, 1).unwrap(),
            position: [30.0, 20.0],
        },
        Sample {
            time: Time::new(0, 1).unwrap(),
            position: [10.0, 15.0],
        },
    ];
    let (id, commands) =
        attach(&doc, doc.output, [30.0, 20.0], &samples, [2.0, 3.0], false).unwrap();
    let mut document = Document::new(doc.clone()).unwrap();
    document.apply(0, &commands).unwrap();
    let op = &document.snapshot().nodes[&id].operation;
    assert_eq!(
        op.scalar("x")
            .unwrap()
            .at(Time::new(0, 1).unwrap())
            .unwrap(),
        -18.0
    );
    assert_eq!(
        op.scalar("y")
            .unwrap()
            .at(Time::new(1, 1).unwrap())
            .unwrap(),
        3.0
    );
    assert!(matches!(
        op,
        Operation::Transform(veylvik_document::nodes::Transform { .. })
    ));
    let encoded = serde_json::to_vec(document.snapshot()).unwrap();
    let decoded: veylvik_document::Composition = serde_json::from_slice(&encoded).unwrap();
    decoded.validate().unwrap();
    document.undo(document.revision()).unwrap();
    assert_eq!(
        serde_json::to_value(document.snapshot()).unwrap(),
        serde_json::to_value(&doc).unwrap()
    );
    assert!(
        attach(
            &doc,
            doc.output,
            [0.0; 2],
            &[samples[0].clone(), samples[0].clone()],
            [0.0; 2],
            false
        )
        .is_err()
    );
}

#[test]
fn retracking_preserves_outer_keys_and_curve_interpolation() {
    use veylvik_document::{Key, Node, Scalar};
    let mut doc = veylvik_document::blank(96, 64, Time::new(24, 1).unwrap()).unwrap();
    let scalar = Scalar::Step {
        keys: (0..=3)
            .map(|n| Key {
                time: Time::new(n, 1).unwrap(),
                value: n as f32 * 10.0,
                segment: None,
            })
            .collect(),
    };
    let node = Node::new(
        "Existing motion",
        Operation::Transform(veylvik_document::nodes::Transform {
            x: scalar.clone(),
            y: scalar,
        }),
        &[("image", doc.output)],
    );
    let id = node.id;
    doc.nodes.insert(id, node);
    doc.output = id;
    let samples = [
        Sample {
            time: Time::new(1, 1).unwrap(),
            position: [20.0, 20.0],
        },
        Sample {
            time: Time::new(2, 1).unwrap(),
            position: [25.0, 20.0],
        },
    ];
    let (_, commands) = attach(&doc, id, [20.0, 20.0], &samples, [0.0; 2], true).unwrap();
    let next = Document::new(doc).unwrap().prepare(0, &commands).unwrap();
    let curve = next.nodes[&id].operation.scalar("x").unwrap();
    assert!(matches!(curve, Scalar::Step { .. }));
    assert_eq!(curve.keys().unwrap()[0].value, 0.0);
    assert_eq!(curve.keys().unwrap()[3].value, 30.0);
    assert_eq!(curve.at(Time::new(3, 2).unwrap()).unwrap(), 12.5);
    assert_eq!(curve.at(Time::new(1, 2).unwrap()).unwrap(), 0.0);
}
