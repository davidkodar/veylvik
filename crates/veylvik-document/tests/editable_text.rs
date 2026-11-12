use veylvik_document::{
    presets::{self, builtin, showcase},
    *,
};
#[test]
fn editable_titles_keep_effects_and_background_when_words_change() {
    let scene = showcase::build("gold_text").unwrap().composition;
    let text = scene
        .nodes
        .values()
        .find(|n| matches!(n.operation, Operation::Text { .. }))
        .unwrap();
    let mut owner = Document::new(scene.clone()).unwrap();
    let mut node = text.clone();
    if let Operation::Text { content, .. } = &mut node.operation {
        *content = "Veylvik Å".into();
    }
    owner
        .apply(
            0,
            &[Command::Put {
                node: Box::new(node),
            }],
        )
        .unwrap();
    assert_eq!(owner.snapshot().output, scene.output);
    assert_eq!(owner.snapshot().nodes.len(), scene.nodes.len());
    let decoded: Composition =
        serde_json::from_slice(&serde_json::to_vec(owner.snapshot()).unwrap()).unwrap();
    decoded.validate().unwrap();
    owner.undo(1).unwrap();
    assert_eq!(
        serde_json::to_value(owner.snapshot()).unwrap(),
        serde_json::to_value(scene).unwrap()
    );
}
#[test]
fn library_splice_and_image_fill_preserve_consumers_and_reject_cycles_atomically() {
    let scene = showcase::build("gold_text").unwrap().composition;
    let text = scene
        .nodes
        .values()
        .find(|n| matches!(n.operation, Operation::Text { .. }))
        .unwrap()
        .id;
    let effect = builtin::build("neon_outline").unwrap();
    let inputs = effect
        .group
        .inputs
        .keys()
        .map(|k| (k.clone(), text))
        .collect();
    let instance = effect.instantiate(&scene, &inputs, None).unwrap();
    let commands =
        presets::connect_after(&scene, text, instance.output, instance.commands).unwrap();
    let mut owner = Document::new(scene.clone()).unwrap();
    owner.apply(0, &commands).unwrap();
    for (id, node) in &scene.nodes {
        for (port, input) in &node.inputs {
            if *input == text {
                assert_eq!(owner.snapshot().nodes[id].inputs[port], instance.output);
            }
        }
    }
    assert_eq!(owner.snapshot().output, scene.output);
    owner.undo(1).unwrap();
    assert!(presets::fill_layer(owner.snapshot(), text, scene.output).is_err());
    assert_eq!(owner.revision(), 2);
    let bg = presets::background::automatic(&scene).unwrap();
    let (output, commands) = presets::fill_layer(&scene, text, bg).unwrap();
    owner.apply(2, &commands).unwrap();
    assert_eq!(owner.snapshot().nodes[&output].inputs["mask"], text);
    assert_eq!(owner.snapshot().nodes[&output].inputs["image"], bg);
    owner.undo(3).unwrap();
    assert_eq!(
        serde_json::to_value(owner.snapshot()).unwrap(),
        serde_json::to_value(scene).unwrap()
    );
}
#[test]
fn text_font_and_content_validation_is_bounded() {
    assert!(TextFont::embedded("invalid".into(), 0, b"not a font").is_err());
    let mut operation: Operation =
        serde_json::from_value(registry::TEXT.default_operation()).unwrap();
    operation.validate().unwrap();
    if let Operation::Text { content, .. } = &mut operation {
        *content = "x".repeat(4097);
    }
    assert!(operation.validate().is_err());
    for source in ["sans", "display", "mono", "italic", "bold_italic"] {
        let font: TextFont = serde_json::from_value(serde_json::json!({"source":source})).unwrap();
        font.validate().unwrap();
    }
}
