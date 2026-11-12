use veylvik_document::*;
#[test]
fn foreground_setup_preserves_text_effects_and_is_one_undoable_transaction() {
    let doc = presets::showcase::build("gold_text").unwrap().composition;
    let text = doc
        .nodes
        .values()
        .find(|n| matches!(n.operation, Operation::Text { .. }))
        .unwrap()
        .id;
    let source = doc
        .nodes
        .values()
        .find(|n| matches!(n.operation, Operation::Solid { .. }))
        .unwrap()
        .id;
    let sources = occlusion::independent_sources(&doc, text);
    assert!(sources.contains(&source));
    assert!(!sources.contains(&text));
    assert!(!sources.contains(&doc.output));
    let (setup, commands) =
        occlusion::create(&doc, text, source, vector::PathMask::default()).unwrap();
    let mut owner = Document::new(doc.clone()).unwrap();
    owner.apply(0, &commands).unwrap();
    assert_eq!(owner.history().undo_steps, 1);
    assert_eq!(owner.snapshot().nodes.len(), doc.nodes.len() + 5);
    for (id, node) in &doc.nodes {
        assert_eq!(
            serde_json::to_value(&owner.snapshot().nodes[id]).unwrap(),
            serde_json::to_value(node).unwrap()
        );
    }
    assert_eq!(
        owner.snapshot().nodes[&setup.output].inputs["background"],
        doc.output
    );
    assert_eq!(owner.snapshot().output, setup.output);
    let loaded: Composition =
        serde_json::from_slice(&serde_json::to_vec(owner.snapshot()).unwrap()).unwrap();
    let found = occlusion::find(&loaded, setup.outline).unwrap();
    assert_eq!(found.source, source);
    assert_eq!(found.softness, setup.softness);
    owner.undo(1).unwrap();
    assert_eq!(
        serde_json::to_value(owner.snapshot()).unwrap(),
        serde_json::to_value(&doc).unwrap()
    );
}
#[test]
fn rejects_sources_containing_text_and_disconnected_text_without_mutation() {
    let mut doc = presets::showcase::build("gold_text").unwrap().composition;
    let text = doc
        .nodes
        .values()
        .find(|n| matches!(n.operation, Operation::Text { .. }))
        .unwrap()
        .id;
    assert!(occlusion::create(&doc, text, doc.output, vector::PathMask::default()).is_err());
    assert!(occlusion::create(&doc, text, text, vector::PathMask::default()).is_err());
    let source = doc
        .nodes
        .values()
        .find(|n| matches!(n.operation, Operation::Solid { .. }))
        .unwrap()
        .id;
    doc.output = source;
    assert!(occlusion::create(&doc, text, source, vector::PathMask::default()).is_err());
}
