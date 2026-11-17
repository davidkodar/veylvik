use veylvik_document::*;

fn blur(label: &str, radius: Scalar, image: NodeId) -> Node {
    Node::new(
        label,
        Operation::Blur(nodes::Blur {
            radius,
            filter: Default::default(),
        }),
        &[("image", image)],
    )
}
fn radius(doc: &Document, id: NodeId) -> &Scalar {
    doc.snapshot().nodes[&id]
        .operation
        .scalar("radius")
        .unwrap()
}
fn set(doc: &mut Document, id: NodeId, expression: &str) -> Result<u64> {
    doc.apply(
        doc.revision(),
        &[Command::SetParameter {
            id,
            parameter: "radius".into(),
            value: serde_json::json!({"kind": "expression", "expression": expression}),
        }],
    )
}
fn at(seconds: i64) -> Time {
    Time::new(seconds, 1).unwrap()
}
/// A document with a keyed Source blur and a Follower blur.
fn setup() -> (Document, NodeId, NodeId) {
    let mut keyed = Scalar::from(0.0);
    keyed.set_key_at(at(0), 10.0).unwrap();
    keyed.set_key_at(at(2), 30.0).unwrap();
    let mut doc = Document::new(demo()).unwrap();
    let image = doc.snapshot().output;
    let source = blur("Source", keyed, image);
    let follower = blur("Follower", 1.0.into(), image);
    let (s, f) = (source.id, follower.id);
    doc.apply(
        0,
        &[
            Command::Put {
                node: Box::new(source),
            },
            Command::Put {
                node: Box::new(follower),
            },
        ],
    )
    .unwrap();
    (doc, s, f)
}

#[test]
fn time_expressions_sample_and_stay_in_range() {
    let (mut doc, _, f) = setup();
    set(&mut doc, f, "time * 100").unwrap();
    assert_eq!(radius(&doc, f).at(at(1)).unwrap(), 100.0);
    // Blur radius is 0..=256.
    assert_eq!(radius(&doc, f).at(at(9)).unwrap(), 256.0);
    assert!(set(&mut doc, f, "sin(").is_err());
}
#[test]
fn links_by_name_are_stored_by_id_and_follow_the_keys() {
    let (mut doc, s, f) = setup();
    set(&mut doc, f, "[Source].radius * 0.5 + 1").unwrap();
    let Scalar::Expression { expression, .. } = radius(&doc, f) else {
        panic!("an expression");
    };
    assert_eq!(expression, &format!("[{s}].radius * 0.5 + 1"));
    assert_eq!(radius(&doc, f).at(at(1)).unwrap(), 11.0);
    // Renaming the source keeps the link; changing it changes the follower.
    let mut renamed = doc.snapshot().nodes[&s].clone();
    renamed.label = "Renamed".into();
    renamed.operation = Operation::Blur(nodes::Blur {
        radius: 40.0.into(),
        filter: Default::default(),
    });
    doc.apply(
        doc.revision(),
        &[Command::Put {
            node: Box::new(renamed),
        }],
    )
    .unwrap();
    assert_eq!(radius(&doc, f).at(at(1)).unwrap(), 21.0);
    assert_eq!(
        doc.snapshot().readable(&format!("[{s}].radius")),
        "[Renamed].radius"
    );
    // Survives saving and loading.
    let json = serde_json::to_string(doc.snapshot()).unwrap();
    let loaded = Document::new(serde_json::from_str(&json).unwrap()).unwrap();
    assert_eq!(radius(&loaded, f).at(at(1)).unwrap(), 21.0);
}
#[test]
fn bad_links_and_loops_are_refused_with_reasons() {
    let (mut doc, s, f) = setup();
    let error = set(&mut doc, f, "[Nobody].radius").unwrap_err().0;
    assert!(error.contains("no node is called Nobody"), "{error}");
    let error = set(&mut doc, f, "[Source].size").unwrap_err().0;
    assert!(error.contains("no animatable parameter 'size'"), "{error}");
    set(&mut doc, f, "[Source].radius").unwrap();
    let error = set(&mut doc, s, "[Follower].radius + 1").unwrap_err().0;
    assert!(error.contains("loop"), "{error}");
    let error = set(&mut doc, f, "[Follower].radius").unwrap_err().0;
    assert!(error.contains("loop"), "{error}");
    // Removing a linked node is refused while something reads it.
    assert!(
        doc.apply(doc.revision(), &[Command::Remove { id: s }])
            .is_err()
    );
}
#[test]
fn chains_resolve_and_keys_are_not_set_on_expressions() {
    let (mut doc, s, f) = setup();
    let third = blur("Third", 0.0.into(), doc.snapshot().output);
    let t = third.id;
    doc.apply(
        doc.revision(),
        &[Command::Put {
            node: Box::new(third),
        }],
    )
    .unwrap();
    set(&mut doc, f, "[Source].radius + 1").unwrap();
    set(&mut doc, t, "[Follower].radius * 2").unwrap();
    assert_eq!(radius(&doc, t).at(at(2)).unwrap(), 62.0);
    let mut scalar = radius(&doc, t).clone();
    assert!(scalar.set_key_at(at(0), 3.0).is_err());
    let _ = s;
}
