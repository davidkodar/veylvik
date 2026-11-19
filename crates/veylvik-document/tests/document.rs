use veylvik_document::*;
#[test]
fn time_compares_rates_exactly_and_keys_interpolate() {
    assert!(
        Time::new(1, 2)
            .unwrap()
            .compare(Time::new(12, 24).unwrap())
            .unwrap()
            .is_eq()
    );
    assert!(Time::new(0, 0).is_err());
    let value = Scalar::Linear {
        keys: vec![
            Key {
                segment: None,
                time: Time::new(0, 24).unwrap(),
                value: 0.0,
            },
            Key {
                segment: None,
                time: Time::new(48, 24).unwrap(),
                value: 4.0,
            },
        ],
    };
    assert_eq!(value.at(Time::new(1, 1).unwrap()).unwrap(), 2.0);
    assert_eq!(value.at(Time::new(-1, 1).unwrap()).unwrap(), 0.0);
    assert_eq!(value.at(Time::new(3, 1).unwrap()).unwrap(), 4.0);
}
#[test]
fn transaction_is_atomic_and_history_revisions_never_go_backwards() {
    let mut doc = Document::new(demo()).unwrap();
    let original = serde_json::to_value(doc.snapshot()).unwrap();
    let commands = [
        Command::Resize {
            width: 100,
            height: 50,
        },
        Command::Remove {
            id: doc.snapshot().output,
        },
    ];
    assert!(doc.apply(0, &commands).is_err());
    assert_eq!(doc.revision(), 0);
    assert_eq!(serde_json::to_value(doc.snapshot()).unwrap(), original);
    doc.apply(
        0,
        &[Command::Resize {
            width: 100,
            height: 50,
        }],
    )
    .unwrap();
    assert!(
        doc.apply(
            0,
            &[Command::Resize {
                width: 30,
                height: 30
            }]
        )
        .is_err()
    );
    doc.undo(1).unwrap();
    assert_eq!(serde_json::to_value(doc.snapshot()).unwrap(), original);
    doc.redo(2).unwrap();
    assert_eq!(doc.snapshot().width, 100);
    assert_eq!(doc.revision(), 3);
}
#[test]
fn disconnected_cycles_and_wrong_ports_are_rejected() {
    let mut composition = demo();
    let mut a = Node::new(
        "a",
        Operation::Exposure(veylvik_document::nodes::Exposure { stops: 0.0.into() }),
        &[],
    );
    a.inputs.insert("image".into(), a.id);
    composition.nodes.insert(a.id, a);
    assert!(
        composition
            .validate()
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );
    let mut composition = demo();
    composition
        .nodes
        .get_mut(&composition.output)
        .unwrap()
        .inputs
        .insert("typo".into(), composition.output);
    assert!(composition.validate().is_err());
}
#[test]
fn future_fields_and_versions_are_rejected_without_data_loss() {
    let mut value = serde_json::to_value(demo()).unwrap();
    value["future"] = true.into();
    assert!(serde_json::from_value::<Composition>(value).is_err());
    let mut doc = demo();
    doc.schema = 100;
    assert!(doc.validate().is_err());
}
#[test]
fn save_round_trip_and_replacement() {
    let path = std::env::temp_dir().join(format!("veylvik-test-{}.json", uuid::Uuid::new_v4()));
    let mut doc = demo();
    doc.save(&path).unwrap();
    doc.width = 12;
    doc.save(&path).unwrap();
    assert_eq!(
        serde_json::to_value(Composition::load(&path).unwrap()).unwrap(),
        serde_json::to_value(doc).unwrap()
    );
    std::fs::remove_file(path).unwrap();
}
#[test]
fn equal_key_times_at_different_rates_are_rejected() {
    let value = Scalar::Linear {
        keys: vec![
            Key {
                segment: None,
                time: Time::new(1, 2).unwrap(),
                value: 0.0,
            },
            Key {
                segment: None,
                time: Time::new(12, 24).unwrap(),
                value: 1.0,
            },
        ],
    };
    assert!(value.validate().is_err());
}

#[test]
fn large_adjacent_key_times_do_not_collapse_to_nan() {
    let base = i64::MAX - 10;
    let value = Scalar::Linear {
        keys: vec![
            Key {
                segment: None,
                time: Time::new(base, 1).unwrap(),
                value: 0.0,
            },
            Key {
                segment: None,
                time: Time::new(base + 2, 1).unwrap(),
                value: 4.0,
            },
        ],
    };
    assert_eq!(value.at(Time::new(base + 1, 1).unwrap()).unwrap(), 2.0);
}
#[test]
fn out_of_range_animated_values_fail_before_commit() {
    let mut doc = Document::new(demo()).unwrap();
    let node = Node::new(
        "invalid",
        Operation::Blur(veylvik_document::nodes::Blur {
            radius: Scalar::Linear {
                keys: vec![
                    Key {
                        segment: None,
                        time: Time::new(0, 1).unwrap(),
                        value: 0.0,
                    },
                    Key {
                        segment: None,
                        time: Time::new(5, 1).unwrap(),
                        value: 300.0,
                    },
                ],
            },
            filter: Default::default(),
        }),
        &[("image", doc.snapshot().output)],
    );
    assert!(
        doc.apply(
            0,
            &[Command::Put {
                node: Box::new(node)
            }]
        )
        .is_err()
    );
    assert_eq!(doc.revision(), 0);
}

#[test]
fn layout_is_optional_persistent_and_transactional() {
    let original = demo();
    let id = original.output;
    let json = serde_json::to_value(&original).unwrap();
    assert!(json["nodes"][id.to_string()].get("position").is_none());
    let restored: Composition = serde_json::from_value(json).unwrap();
    let mut document = Document::new(restored).unwrap();
    let position = Some(NodePosition { x: -120.0, y: 72.5 });
    document
        .apply(0, &[Command::Place { id, position }])
        .unwrap();
    let wire = serde_json::to_string(document.snapshot()).unwrap();
    let restored: Composition = serde_json::from_str(&wire).unwrap();
    assert_eq!(restored.nodes[&id].position, position);
    document.undo(1).unwrap();
    assert_eq!(document.snapshot().nodes[&id].position, None);
    document.redo(2).unwrap();
    assert_eq!(document.snapshot().nodes[&id].position, position);
    for invalid in [f32::NAN, f32::INFINITY, 1_000_001.0] {
        assert!(
            document
                .apply(
                    3,
                    &[
                        Command::Resize {
                            width: 32,
                            height: 32
                        },
                        Command::Place {
                            id,
                            position: Some(NodePosition { x: invalid, y: 0.0 })
                        }
                    ]
                )
                .is_err()
        );
        assert_eq!(document.revision(), 3);
        assert_eq!(document.snapshot().width, original.width);
        assert_eq!(document.snapshot().nodes[&id].position, position);
    }
    assert!(
        document
            .apply(
                3,
                &[Command::Place {
                    id: NodeId::new_v4(),
                    position
                }]
            )
            .is_err()
    );
    document
        .apply(3, &[Command::Place { id, position: None }])
        .unwrap();
    assert_eq!(document.snapshot().nodes[&id].position, None);
}

#[test]
fn atomic_create_publishes_complete_data_without_replacing_collisions() {
    let directory = std::env::temp_dir().join(format!("veylvik-publish-{}", NodeId::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("frame.png");
    atomic_create(&path, b"complete frame").unwrap();
    assert!(atomic_create(&path, b"replacement").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"complete frame");
    #[cfg(unix)]
    {
        let link = directory.join("link.png");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(atomic_create(&link, b"overwrite via symlink").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"complete frame");
    }
    assert!(!std::fs::read_dir(&directory).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")
    }));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn blank_compositions_and_frame_rate_edits_are_validated_and_atomic() {
    let composition = blank(1920, 1080, Time::new(24, 1).unwrap()).unwrap();
    assert_eq!(composition.nodes.len(), 1);
    assert!(
        matches!(composition.nodes[&composition.output].operation, Operation::Solid(veylvik_document::nodes::Solid { color }) if color == [0.0;4])
    );
    assert!(blank(0, 1080, Time::new(24, 1).unwrap()).is_err());
    assert!(blank(1920, 1080, Time::new(0, 1).unwrap()).is_err());
    let mut document = Document::new(composition).unwrap();
    let nodes = serde_json::to_value(&document.snapshot().nodes).unwrap();
    assert!(
        document
            .apply(
                0,
                &[
                    Command::Resize {
                        width: 1080,
                        height: 1920
                    },
                    Command::FrameRate {
                        frame_rate: Time {
                            ticks: 30000,
                            rate: 0
                        }
                    },
                ]
            )
            .is_err()
    );
    assert_eq!(document.revision(), 0);
    assert_eq!(document.snapshot().width, 1920);
    let command = Command::FrameRate {
        frame_rate: Time::new(30000, 1001).unwrap(),
    };
    let encoded = serde_json::to_value(&command).unwrap();
    assert_eq!(encoded["command"], "frame_rate");
    let command: Command = serde_json::from_value(encoded).unwrap();
    document
        .apply(
            0,
            &[
                Command::Resize {
                    width: 1080,
                    height: 1920,
                },
                command,
            ],
        )
        .unwrap();
    assert_eq!(document.snapshot().width, 1080);
    assert!(
        Time::at_frame(30, document.snapshot().frame_rate)
            .unwrap()
            .compare(Time::new(1001, 1000).unwrap())
            .unwrap()
            .is_eq()
    );
    assert_eq!(
        serde_json::to_value(&document.snapshot().nodes).unwrap(),
        nodes
    );
    document.undo(1).unwrap();
    assert_eq!(document.snapshot().width, 1920);
    assert_eq!(document.snapshot().frame_rate.ticks, 24);
    document.redo(2).unwrap();
    assert_eq!(document.snapshot().height, 1920);
    assert_eq!(document.snapshot().frame_rate.rate, 1001);
}

#[test]
fn frame_count_defaults_for_old_documents_and_edits_are_atomic() {
    let original = motion_demo();
    let mut wire = serde_json::to_value(&original).unwrap();
    wire.as_object_mut().unwrap().remove("frame_count");
    let old: Composition = serde_json::from_value(wire).unwrap();
    assert_eq!(old.frame_count, DEFAULT_FRAME_COUNT);
    old.validate().unwrap();
    let mut document = Document::new(original).unwrap();
    let before = serde_json::to_value(document.snapshot()).unwrap();
    for count in [0, MAX_FRAME_COUNT + 1] {
        assert!(
            document
                .apply(
                    0,
                    &[
                        Command::Resize {
                            width: 17,
                            height: 19
                        },
                        Command::FrameCount { frame_count: count }
                    ]
                )
                .is_err()
        );
        assert_eq!(document.revision(), 0);
        assert_eq!(serde_json::to_value(document.snapshot()).unwrap(), before);
    }
    let command: Command =
        serde_json::from_value(serde_json::json!({"command":"frame_count","frame_count":1}))
            .unwrap();
    document.apply(0, &[command]).unwrap();
    assert_eq!(document.snapshot().frame_count, 1);
    assert_eq!(
        serde_json::to_value(&document.snapshot().nodes).unwrap(),
        before["nodes"]
    );
    document.undo(1).unwrap();
    assert_eq!(document.snapshot().frame_count, 96);
    document.redo(2).unwrap();
    assert_eq!(document.snapshot().frame_count, 1);
    document
        .apply(
            3,
            &[Command::FrameCount {
                frame_count: MAX_FRAME_COUNT,
            }],
        )
        .unwrap();
    let decoded: Composition =
        serde_json::from_slice(&serde_json::to_vec(document.snapshot()).unwrap()).unwrap();
    assert_eq!(decoded.frame_count, MAX_FRAME_COUNT);
}
#[test]
fn a_sound_sits_on_the_timeline_and_is_no_picture() {
    use veylvik_document::*;
    let mut doc = Document::new(blank(640, 360, Time { ticks: 24, rate: 1 }).unwrap()).unwrap();
    let sound: Operation = serde_json::from_value(registry::SOUND.default_operation()).unwrap();
    let node = Node::new("Music", sound, &[]);
    let id = node.id;
    doc.apply(
        0,
        &[Command::Put {
            node: Box::new(node),
        }],
    )
    .unwrap();
    assert_eq!(registry::SOUND.output, registry::PortKind::Sound);
    // It cannot be shown, and nothing takes it as an input.
    assert!(doc.apply(1, &[Command::Output { id }]).is_err());
    let blur: Operation = serde_json::from_value(registry::BLUR.default_operation()).unwrap();
    let wrong = Node::new("Blur", blur, &[("image", id)]);
    let error = doc
        .apply(
            1,
            &[Command::Put {
                node: Box::new(wrong),
            }],
        )
        .unwrap_err();
    assert!(error.to_string().contains("takes an image"), "{error}");
    // A stop before the start is refused like footage's.
    let mut bad = doc.snapshot().nodes[&id].clone();
    let Operation::Sound(s) = &mut bad.operation else {
        panic!()
    };
    s.timing.source_in = Time { ticks: 5, rate: 1 };
    s.timing.source_out = Some(Time { ticks: 2, rate: 1 });
    assert!(
        doc.apply(
            1,
            &[Command::Put {
                node: Box::new(bad)
            }]
        )
        .is_err()
    );
}
#[test]
fn one_input_connects_and_disconnects_without_touching_the_rest() {
    use veylvik_document::*;
    let mut doc = Document::new(blank(640, 360, Time { ticks: 24, rate: 1 }).unwrap()).unwrap();
    let back = doc.snapshot().output;
    let node = |kind: &registry::NodeType, inputs: &[(&str, NodeId)]| {
        Node::new(
            kind.label,
            serde_json::from_value(kind.default_operation()).unwrap(),
            inputs,
        )
    };
    let blur = node(&registry::BLUR, &[("image", back)]);
    let merge = node(
        &registry::MERGE,
        &[("background", back), ("foreground", back)],
    );
    let (blur_id, merge_id) = (blur.id, merge.id);
    let put = |node: Node| Command::Put {
        node: Box::new(node),
    };
    doc.apply(0, &[put(blur), put(merge)]).unwrap();
    doc.apply(
        1,
        &[Command::Connect {
            id: merge_id,
            input: "foreground".into(),
            source: Some(blur_id),
        }],
    )
    .unwrap();
    let inputs = &doc.snapshot().nodes[&merge_id].inputs;
    assert_eq!(
        (inputs["background"], inputs["foreground"]),
        (back, blur_id)
    );
    // A required input cannot be left empty, a port must exist, and a loop is refused.
    let connect = |input: &str, source| Command::Connect {
        id: merge_id,
        input: input.into(),
        source,
    };
    assert!(doc.apply(2, &[connect("foreground", None)]).is_err());
    assert!(doc.apply(2, &[connect("sideways", Some(back))]).is_err());
    assert!(
        doc.apply(2, &[connect("background", Some(merge_id))])
            .is_err()
    );
    assert_eq!(
        doc.steps().0.last().map(String::as_str),
        Some("Connect Merge")
    );
}
#[test]
fn an_own_size_picture_feeds_only_what_takes_any_size() {
    use veylvik_document::*;
    let mut doc = Document::new(blank(640, 360, Time { ticks: 24, rate: 1 }).unwrap()).unwrap();
    let default = |kind: &registry::NodeType| -> Operation {
        serde_json::from_value(kind.default_operation()).unwrap()
    };
    let noise = Node::new("Noise", default(&registry::FRACTAL_NOISE), &[]);
    let own = Node::new("Small", default(&nodes::OWN_SIZE), &[("image", noise.id)]);
    let (noise_id, id) = (noise.id, own.id);
    let put = |node: Node| Command::Put {
        node: Box::new(node),
    };
    doc.apply(0, &[put(noise), put(own)]).unwrap();
    assert_eq!(nodes::OWN_SIZE.output, registry::PortKind::Picture);
    let Operation::OwnSize(size) = &doc.snapshot().nodes[&id].operation else {
        panic!()
    };
    assert_eq!((size.width, size.height), (1024, 1024));
    // It goes wherever an image does: an ordinary node, the output, and what uses it
    // at its own size (a card, a shape's texture, a scene environment).
    let blur = Node::new("Blur", default(&registry::BLUR), &[("image", id)]);
    doc.apply(1, &[put(blur), Command::Output { id }]).unwrap();
    assert!(!nodes::OWN_SIZE.takes_picture("image"));
    assert!(registry::IMAGE_CARD.takes_picture("image"));
    let card = Node::new("Card", default(&registry::IMAGE_CARD), &[("image", id)]);
    let shape = Node::new("Ball", default(&registry::SHAPE_3D), &[("texture", id)]);
    let around = Node::new(
        "Around",
        default(&registry::ENVIRONMENT_3D),
        &[("image", id)],
    );
    doc.apply(2, &[put(card), put(shape), put(around)]).unwrap();
    // Sizes are bounded.
    for (width, height) in [(0, 64), (64, 0), (16385, 64), (12288, 12288)] {
        let mut bad = doc.snapshot().nodes[&id].clone();
        bad.operation = Operation::OwnSize(nodes::OwnSize {
            width,
            height,
            fit: nodes::Fit::Fit,
        });
        assert!(doc.apply(3, &[put(bad)]).is_err(), "{width} x {height}");
    }
    let _ = noise_id;
}
