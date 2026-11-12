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
    let mut a = Node::new("a", Operation::Exposure { stops: 0.0.into() }, &[]);
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
        Operation::Blur {
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
                        value: 100.0,
                    },
                ],
            },
        },
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
        matches!(composition.nodes[&composition.output].operation, Operation::Solid { color } if color == [0.0;4])
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
