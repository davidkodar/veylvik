use veylvik_document::*;
#[test]
fn recipes_expand_as_independent_editable_nodes_in_one_transaction() {
    for recipe in recipes::Recipe::ALL {
        let original = blank(25, 19, Time::new(24, 1).unwrap()).unwrap();
        let input = original.output;
        let effect = recipe.build(&original, input).unwrap();
        assert_eq!(effect.nodes.len(), recipe.node_count());
        assert_eq!(effect.roles["composite"], effect.root);
        let mut seen = std::collections::BTreeSet::from([input]);
        for node in &effect.nodes {
            assert!(!seen.contains(&node.id));
            assert!(node.inputs.values().all(|id| seen.contains(id)));
            seen.insert(node.id);
        }
        let root = effect.root;
        let mut doc = Document::new(original.clone()).unwrap();
        doc.apply(0, &effect.into_commands()).unwrap();
        assert_eq!(doc.snapshot().output, input);
        assert!(doc.snapshot().nodes.contains_key(&root));
        assert_eq!(doc.history().undo_steps, 1);
        doc.undo(1).unwrap();
        assert_eq!(
            serde_json::to_value(doc.snapshot()).unwrap(),
            serde_json::to_value(original).unwrap()
        );
        assert!(recipe.build(doc.snapshot(), NodeId::new_v4()).is_err());
    }
}
#[test]
fn recipes_reject_node_overflow_without_changing_the_source() {
    let mut doc = blank(1, 1, Time::new(24, 1).unwrap()).unwrap();
    for _ in 1..limits::MAX_NODES - 3 {
        let node = Node::new("Unused", Operation::Solid { color: [0.0; 4] }, &[]);
        doc.nodes.insert(node.id, node);
    }
    assert!(recipes::Recipe::Glow.build(&doc, doc.output).is_ok());
    assert!(recipes::Recipe::Outline.build(&doc, doc.output).is_err());
    assert_eq!(doc.nodes.len(), limits::MAX_NODES - 3);
}
