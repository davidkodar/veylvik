use veylvik_document::*;

#[test]
fn copy_remaps_shared_ancestors_once_preserves_curves_assets_and_is_one_transaction() {
    let mut doc = blank(8, 8, Time::new(24, 1).unwrap()).unwrap();
    let source = doc.output;
    doc.nodes.get_mut(&source).unwrap().operation = Operation::Image {
        asset: "relative/source.exr".into(),
    };
    let mut stops = Scalar::from(0.0);
    stops.set_key_at(Time::new(0, 24).unwrap(), 0.0).unwrap();
    stops.set_key_at(Time::new(24, 24).unwrap(), 2.0).unwrap();
    stops.set_interpolation(Interpolation::Monotone);
    let effect = Node::new(
        "Animated",
        Operation::Exposure { stops },
        &[("image", source)],
    );
    let effect_id = effect.id;
    doc.nodes.insert(effect.id, effect);
    let merge = Node::new(
        "Shared",
        Operation::Merge {},
        &[("background", source), ("foreground", effect_id)],
    );
    doc.output = merge.id;
    doc.nodes.insert(merge.id, merge);
    let disconnected = Node::new("Unrelated", Operation::Solid { color: [0.0; 4] }, &[]);
    doc.nodes.insert(disconnected.id, disconnected);
    let before = serde_json::to_value(&doc).unwrap();
    let copy = editing::copy_branch(&doc, doc.output).unwrap();
    assert_eq!(copy.nodes.len(), 3);
    assert_eq!(copy.mapping.len(), 3);
    for (old, new) in &copy.mapping {
        assert!(!doc.nodes.contains_key(new));
        let original = &doc.nodes[old];
        let duplicate = &copy.nodes[new];
        assert_eq!(
            serde_json::to_value(&original.operation).unwrap(),
            serde_json::to_value(&duplicate.operation).unwrap()
        );
        assert_eq!(duplicate.version, original.version);
        assert_eq!(duplicate.label, format!("{} copy", original.label));
        for (port, input) in &original.inputs {
            assert_eq!(duplicate.inputs[port], copy.mapping[input]);
        }
    }
    let root = copy.root;
    let mapped_source = copy.mapping[&source];
    let mut document = Document::new(doc).unwrap();
    document.apply(0, &copy.into_commands()).unwrap();
    assert_eq!(document.history().undo_steps, 1);
    assert_eq!(document.snapshot().nodes.len(), 7);
    assert_ne!(document.snapshot().output, root);
    assert_eq!(
        document.snapshot().nodes[&root].inputs["background"],
        mapped_source
    );
    document.undo(1).unwrap();
    assert_eq!(serde_json::to_value(document.snapshot()).unwrap(), before);
    document.redo(2).unwrap();
    assert!(document.snapshot().nodes.contains_key(&root));
    assert!(editing::copy_branch(document.snapshot(), NodeId::new_v4()).is_err());
}
#[test]
fn deep_branch_copy_is_iterative_and_rejects_overflow_before_publication() {
    let mut doc = blank(1, 1, Time::new(24, 1).unwrap()).unwrap();
    for _ in 1..2048 {
        let node = Node::new(
            "Deep",
            Operation::Exposure { stops: 1.0.into() },
            &[("image", doc.output)],
        );
        doc.output = node.id;
        doc.nodes.insert(node.id, node);
    }
    let copy = editing::copy_branch(&doc, doc.output).unwrap();
    assert_eq!(copy.nodes.len(), 2048);
    let mut document = Document::new(doc).unwrap();
    document.apply(0, &copy.into_commands()).unwrap();
    assert_eq!(document.snapshot().nodes.len(), limits::MAX_NODES);
    assert!(editing::copy_branch(document.snapshot(), document.snapshot().output).is_err());
    assert_eq!(document.revision(), 1);
    assert_eq!(document.history().undo_steps, 1);
}
