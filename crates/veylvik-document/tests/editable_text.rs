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

#[test]
fn text_style_replaces_material_without_stacking_and_keeps_connections_and_history() {
    use presets::text_style;
    let mut scene = demo();
    let text = Node::new(
        "Title",
        serde_json::from_value(registry::TEXT.default_operation()).unwrap(),
        &[],
    );
    let id = text.id;
    scene.nodes.insert(id, text);
    scene.output = id;
    let mut owner = Document::new(scene).unwrap();
    let commands = text_style::apply(owner.snapshot(), id, Some("gold_text"), None).unwrap();
    owner.apply(0, &commands).unwrap();
    let material = text_style::current(owner.snapshot(), id).unwrap().unwrap();
    let count = owner.snapshot().nodes.len();
    let before = owner.snapshot().nodes[&material]
        .operation
        .scalar("lighting")
        .unwrap()
        .clone();
    let commands = text_style::apply(owner.snapshot(), id, Some("silver_text"), None).unwrap();
    owner.apply(1, &commands).unwrap();
    assert_eq!(owner.snapshot().nodes.len(), count);
    assert_eq!(
        text_style::current(owner.snapshot(), id).unwrap(),
        Some(material)
    );
    assert!(matches!(
        owner.snapshot().nodes[&material].operation,
        Operation::Material {
            kind: MaterialKind::Silver,
            ..
        }
    ));
    assert_eq!(
        serde_json::to_value(
            owner.snapshot().nodes[&material]
                .operation
                .scalar("lighting")
        )
        .unwrap(),
        serde_json::to_value(Some(before)).unwrap()
    );
    let commands = text_style::apply(owner.snapshot(), id, None, None).unwrap();
    owner.apply(2, &commands).unwrap();
    assert_eq!(owner.snapshot().output, id);
    assert_eq!(text_style::current(owner.snapshot(), id).unwrap(), None);
    owner.undo(3).unwrap();
    assert_eq!(owner.snapshot().output, material);
    let image = owner
        .snapshot()
        .nodes
        .values()
        .find(|n| matches!(n.operation, Operation::Solid { .. }))
        .unwrap()
        .id;
    let (_, commands) = presets::fill_layer(owner.snapshot(), id, image).unwrap();
    owner.apply(4, &commands).unwrap();
    assert_eq!(
        text_style::current(owner.snapshot(), id).unwrap(),
        Some(material)
    );
    let commands = text_style::apply(owner.snapshot(), id, Some("bronze_text"), None).unwrap();
    owner.apply(5, &commands).unwrap();
    assert_eq!(owner.snapshot().nodes.len(), count + 1);
    assert!(matches!(
        owner.snapshot().nodes[&material].operation,
        Operation::Material {
            kind: MaterialKind::Bronze,
            ..
        }
    ));
}

#[test]
fn switching_between_material_and_image_fill_is_atomic_and_reuses_the_fill() {
    use presets::text_style;
    let mut scene = demo();
    let image = scene
        .nodes
        .values()
        .find(|n| matches!(n.operation, Operation::Solid { .. }))
        .unwrap()
        .id;
    let title = Node::new(
        "Title",
        serde_json::from_value(registry::TEXT.default_operation()).unwrap(),
        &[],
    );
    let text = title.id;
    scene.nodes.insert(text, title);
    scene.output = text;
    let mut owner = Document::new(scene).unwrap();
    owner
        .apply(
            0,
            &text_style::apply(owner.snapshot(), text, Some("gold_text"), None).unwrap(),
        )
        .unwrap();
    let material = text_style::output(owner.snapshot(), text).unwrap();
    let (fill, commands) = text_style::fill(owner.snapshot(), text, image).unwrap();
    owner.apply(1, &commands).unwrap();
    assert_eq!(owner.snapshot().output, fill);
    assert_eq!(text_style::current(owner.snapshot(), text).unwrap(), None);
    let count = owner.snapshot().nodes.len();
    let (again, commands) = text_style::fill(owner.snapshot(), text, image).unwrap();
    owner.apply(2, &commands).unwrap();
    assert_eq!(fill, again);
    assert_eq!(owner.snapshot().nodes.len(), count);
    owner.undo(3).unwrap();
    owner.undo(4).unwrap();
    assert_eq!(owner.snapshot().output, material);
    assert!(text_style::fill(owner.snapshot(), text, material).is_err());
}
