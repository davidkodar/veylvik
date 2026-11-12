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

#[test]
fn text_stack_reorders_removes_compound_effects_and_preserves_wording() {
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
    let commands = text_style::shadow(owner.snapshot(), id).unwrap();
    owner.apply(0, &commands).unwrap();
    let shadow = owner.snapshot().output;
    let effect = recipes::Recipe::Outline
        .build(owner.snapshot(), shadow)
        .unwrap();
    let output = effect.root;
    let commands = presets::connect_after(
        owner.snapshot(),
        shadow,
        output,
        effect.into_grouped_commands(owner.snapshot()).unwrap(),
    )
    .unwrap();
    owner.apply(1, &commands).unwrap();
    assert_eq!(
        text_style::chain(owner.snapshot(), id).unwrap(),
        vec![shadow, output]
    );
    let commands = text_style::swap(owner.snapshot(), id, 0).unwrap();
    owner.apply(2, &commands).unwrap();
    assert_eq!(
        text_style::chain(owner.snapshot(), id).unwrap(),
        vec![output, shadow]
    );
    let commands = text_style::remove_effect(owner.snapshot(), id, output).unwrap();
    owner.apply(3, &commands).unwrap();
    assert_eq!(
        text_style::chain(owner.snapshot(), id).unwrap(),
        vec![shadow]
    );
    let commands = text_style::gradient(owner.snapshot(), id).unwrap();
    owner.apply(4, &commands).unwrap();
    assert!(
        text_style::fill_source(owner.snapshot(), id)
            .unwrap()
            .is_some()
    );
    let commands = text_style::plain(owner.snapshot(), id).unwrap();
    owner.apply(5, &commands).unwrap();
    assert!(
        text_style::fill_source(owner.snapshot(), id)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        text_style::chain(owner.snapshot(), id).unwrap(),
        vec![shadow]
    );
}

#[test]
fn saved_title_styles_preserve_words_position_and_are_undoable() {
    let source = presets::showcase::build("gold_text").unwrap().composition;
    let id = source
        .nodes
        .values()
        .find(|n| matches!(n.operation, Operation::Text { .. }))
        .unwrap()
        .id;
    let style = presets::title_style::TitleStyle::capture(&source, id, "My gold title").unwrap();
    let mut doc = blank(1920, 1080, Time::new(24, 1).unwrap()).unwrap();
    let mut op: Operation = serde_json::from_value(registry::TEXT.default_operation()).unwrap();
    if let Operation::Text { content, x, .. } = &mut op {
        *content = "Keep my wording".into();
        *x = 320.0.into();
    }
    let node = Node::new("My title", op, &[]);
    let id = node.id;
    doc.nodes.insert(id, node);
    doc.output = id;
    let commands = style.apply(&doc, id).unwrap();
    let mut owner = Document::new(doc.clone()).unwrap();
    owner.apply(0, &commands).unwrap();
    if let Operation::Text { content, x, .. } = &owner.snapshot().nodes[&id].operation {
        assert_eq!(content, "Keep my wording");
        assert_eq!(x.at(Time::new(0, 1).unwrap()).unwrap(), 320.0);
    } else {
        panic!()
    }
    owner.undo(1).unwrap();
    assert_eq!(
        serde_json::to_value(owner.snapshot()).unwrap(),
        serde_json::to_value(doc).unwrap()
    );
}

#[test]
fn character_styles_track_unicode_edits_and_reject_overlapping_ranges() {
    let mut t = text::Typography::default();
    t.set_span(text::TextSpan {
        start: 2,
        end: 5,
        font: None,
        scale: 1.5,
    });
    t.remap("Åabc", "ÅaXYZc");
    t.validate().unwrap();
    assert_eq!((t.spans[0].start, t.spans[0].end), (2, 7));
    t.set_span(text::TextSpan {
        start: 3,
        end: 6,
        font: None,
        scale: 0.8,
    });
    t.validate().unwrap();
    assert_eq!(t.spans.len(), 3);
    t.remap("ÅaXYZc", "Z");
    t.validate().unwrap();
    t.spans.push(text::TextSpan {
        start: 0,
        end: 1,
        font: None,
        scale: f32::NAN,
    });
    assert!(t.validate().is_err());
}

#[test]
fn parameter_commands_preserve_character_ranges_when_replacing_words() {
    let mut doc = blank(320, 180, Time::new(24, 1).unwrap()).unwrap();
    let mut op: Operation = serde_json::from_value(registry::TEXT.default_operation()).unwrap();
    if let Operation::Text {
        content,
        typography,
        ..
    } = &mut op
    {
        *content = "Long title".into();
        typography.spans.push(text::TextSpan {
            start: 5,
            end: 10,
            font: None,
            scale: 1.2,
        });
    }
    let node = Node::new("Text", op, &[]);
    let id = node.id;
    doc.nodes.insert(id, node);
    doc.output = id;
    let mut owner = Document::new(doc).unwrap();
    owner
        .apply(
            0,
            &[Command::SetParameter {
                id,
                parameter: "content".into(),
                value: serde_json::json!("Hi"),
            }],
        )
        .unwrap();
    owner.snapshot().validate().unwrap();
    owner.undo(1).unwrap();
    if let Operation::Text {
        content,
        typography,
        ..
    } = &owner.snapshot().nodes[&id].operation
    {
        assert_eq!(content, "Long title");
        assert_eq!(typography.spans[0].start, 5);
    }
}
