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
        .find(|n| {
            matches!(
                n.operation,
                Operation::Text(veylvik_document::nodes::Text { .. })
            )
        })
        .unwrap();
    let mut owner = Document::new(scene.clone()).unwrap();
    let mut node = text.clone();
    if let Operation::Text(veylvik_document::nodes::Text { content, .. }) = &mut node.operation {
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
        .find(|n| {
            matches!(
                n.operation,
                Operation::Text(veylvik_document::nodes::Text { .. })
            )
        })
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
    if let Operation::Text(veylvik_document::nodes::Text { content, .. }) = &mut operation {
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
        Operation::Material(veylvik_document::nodes::Material {
            kind: MaterialKind::Silver,
            ..
        })
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
        .find(|n| {
            matches!(
                n.operation,
                Operation::Solid(veylvik_document::nodes::Solid { .. })
            )
        })
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
        Operation::Material(veylvik_document::nodes::Material {
            kind: MaterialKind::Bronze,
            ..
        })
    ));
}

#[test]
fn switching_between_material_and_image_fill_is_atomic_and_reuses_the_fill() {
    use presets::text_style;
    let mut scene = demo();
    let image = scene
        .nodes
        .values()
        .find(|n| {
            matches!(
                n.operation,
                Operation::Solid(veylvik_document::nodes::Solid { .. })
            )
        })
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
        .find(|n| {
            matches!(
                n.operation,
                Operation::Text(veylvik_document::nodes::Text { .. })
            )
        })
        .unwrap()
        .id;
    let style = presets::title_style::TitleStyle::capture(&source, id, "My gold title").unwrap();
    let mut doc = blank(1920, 1080, Time::new(24, 1).unwrap()).unwrap();
    let mut op: Operation = serde_json::from_value(registry::TEXT.default_operation()).unwrap();
    if let Operation::Text(veylvik_document::nodes::Text { content, x, .. }) = &mut op {
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
    if let Operation::Text(veylvik_document::nodes::Text { content, x, .. }) =
        &owner.snapshot().nodes[&id].operation
    {
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
    if let Operation::Text(veylvik_document::nodes::Text {
        content,
        typography,
        ..
    }) = &mut op
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
    if let Operation::Text(veylvik_document::nodes::Text {
        content,
        typography,
        ..
    }) = &owner.snapshot().nodes[&id].operation
    {
        assert_eq!(content, "Long title");
        assert_eq!(typography.spans[0].start, 5);
    }
}

#[test]
fn repeated_gradient_application_reuses_its_nodes_and_preserves_effects() {
    let mut doc = blank(320, 180, Time::new(24, 1).unwrap()).unwrap();
    let node = Node::new(
        "Title",
        serde_json::from_value(registry::TEXT.default_operation()).unwrap(),
        &[],
    );
    let id = node.id;
    doc.nodes.insert(id, node);
    doc.output = id;
    let mut owner = Document::new(doc).unwrap();
    owner
        .apply(
            0,
            &presets::text_style::gradient(owner.snapshot(), id).unwrap(),
        )
        .unwrap();
    let source = presets::text_style::fill_source(owner.snapshot(), id).unwrap();
    assert!(
        presets::text_style::gradient(owner.snapshot(), id)
            .unwrap()
            .is_empty()
    );
    owner
        .apply(
            1,
            &presets::text_style::apply(owner.snapshot(), id, Some("gold_text"), None).unwrap(),
        )
        .unwrap();
    owner
        .apply(
            2,
            &presets::text_style::shadow(
                owner.snapshot(),
                presets::text_style::output(owner.snapshot(), id).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    let count = owner.snapshot().nodes.len();
    let output = owner.snapshot().output;
    owner
        .apply(
            3,
            &presets::text_style::gradient(owner.snapshot(), id).unwrap(),
        )
        .unwrap();
    assert_eq!(count, owner.snapshot().nodes.len());
    assert_eq!(output, owner.snapshot().output);
    assert_eq!(
        source,
        presets::text_style::fill_source(owner.snapshot(), id).unwrap()
    );
}

#[test]
fn legacy_shadow_upgrade_preserves_wiring_and_is_one_undo_step() {
    use presets::text_style;
    let mut doc = blank(64, 64, Time::new(24, 1).unwrap()).unwrap();
    let source = doc.output;
    let alpha = Node::new(
        "alpha",
        Operation::ChannelMask(nodes::ChannelMask {
            channel: MaskChannel::Alpha,
        }),
        &[("image", source)],
    );
    let tint = Node::new(
        "tint",
        Operation::Colorize(nodes::Colorize {
            low: [0.; 4],
            high: [0., 0., 0., 0.7],
        }),
        &[("image", alpha.id)],
    );
    let offset = Node::new(
        "offset",
        Operation::Transform(nodes::Transform {
            x: 8.0.into(),
            y: 12.0.into(),
        }),
        &[("image", tint.id)],
    );
    let blur = Node::new(
        "blur",
        Operation::Blur(nodes::Blur {
            radius: 5.0.into(),
            filter: Default::default(),
        }),
        &[("image", offset.id)],
    );
    let merge = Node::new(
        "merge",
        Operation::Merge(nodes::Merge {}),
        &[("background", blur.id), ("foreground", source)],
    );
    let output = merge.id;
    let shadow_id = blur.id;
    let members = [alpha, tint, offset, blur, merge];
    let ids = members.iter().map(|n| n.id).collect();
    for n in members {
        doc.nodes.insert(n.id, n);
    }
    doc.output = output;
    let group = groups::Group::capture(&doc, ids, output, "My shadow").unwrap();
    let gid = group.id;
    doc.groups.insert(gid, group);
    let before = serde_json::to_value(&doc).unwrap();
    let mut owner = Document::new(doc).unwrap();
    assert!(text_style::legacy_shadow_nodes(owner.snapshot(), output).is_some());
    let commands = text_style::upgrade_shadow(owner.snapshot(), output).unwrap();
    owner.apply(0, &commands).unwrap();
    let after = owner.snapshot();
    assert_eq!(after.output, output);
    assert_eq!(after.groups[&gid].members.len(), 2);
    assert_eq!(after.nodes.len(), 3);
    assert_eq!(after.nodes[&shadow_id].inputs["lighting"], source);
    let Operation::Shadow(s) = &after.nodes[&shadow_id].operation else {
        panic!()
    };
    assert!(s.follow_lighting);
    assert_eq!(s.strength.at(Time::new(0, 1).unwrap()).unwrap(), 0.7);
    assert_eq!(s.softness.at(Time::new(0, 1).unwrap()).unwrap(), 5.);
    assert!(
        after.groups[&gid]
            .controls
            .values()
            .any(|c| c.parameter == "follow_lighting")
    );
    assert_eq!(owner.history().undo_steps, 1);
    owner.undo(owner.revision()).unwrap();
    assert_eq!(serde_json::to_value(owner.snapshot()).unwrap(), before);
    // Never remove a legacy internal node used by another branch.
    let mut shared = owner.snapshot().clone();
    let ids = text_style::legacy_shadow_nodes(&shared, output).unwrap();
    let consumer = Node::new(
        "shared",
        Operation::Exposure(nodes::Exposure { stops: 0.0.into() }),
        &[("image", ids[0])],
    );
    shared.nodes.insert(consumer.id, consumer);
    assert!(text_style::upgrade_shadow(&shared, output).is_err());
}

#[test]
fn drip_templates_are_neutral_and_surface_selection_styles_the_complete_silhouette() {
    for id in ["fireflies", "liquid_shimmer", "wet_drips"] {
        let doc = showcase::build(id).unwrap().composition;
        assert!(!doc.nodes.values().any(|n| matches!(
            n.operation,
            Operation::Ellipse(_) | Operation::Atmosphere(_)
        )));
        let text = doc
            .nodes
            .values()
            .find(|n| matches!(n.operation, Operation::Text(_)))
            .unwrap()
            .id;
        let material = presets::text_style::current(&doc, text).unwrap().unwrap();
        let commands = presets::text_style::apply(&doc, text, Some("gold_text"), None).unwrap();
        let mut owner = Document::new(doc.clone()).unwrap();
        owner.apply(0, &commands).unwrap();
        let styled = owner.snapshot();
        assert_eq!(styled.nodes.len(), doc.nodes.len());
        assert!(
            matches!(&styled.nodes[&material].operation,Operation::Material(m) if m.kind==MaterialKind::Gold && m.optics.is_none())
        );
        let mut input = styled.nodes[&material].inputs["image"];
        if matches!(styled.nodes[&input].operation, Operation::Colorize(_)) {
            input = styled.nodes[&input].inputs["image"];
        }
        assert!(matches!(styled.nodes[&input].operation, Operation::Drip(_)));
        assert!(
            styled
                .nodes
                .values()
                .any(|n| n.inputs.get("image") == Some(&material)
                    && matches!(n.operation, Operation::Transform(_)))
        );
        assert_eq!(
            presets::text_style::current(styled, text).unwrap(),
            Some(material)
        );
        let again = presets::text_style::apply(styled, text, Some("bronze_text"), None).unwrap();
        assert!(
            Document::new(styled.clone())
                .unwrap()
                .prepare(0, &again)
                .is_ok()
        );
        owner.undo(1).unwrap();
        assert_eq!(
            serde_json::to_value(owner.snapshot()).unwrap(),
            serde_json::to_value(doc).unwrap()
        );
    }
}
