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
                time: Time::new(0, 24).unwrap(),
                value: 0.0,
            },
            Key {
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
                time: Time::new(1, 2).unwrap(),
                value: 0.0,
            },
            Key {
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
                time: Time::new(base, 1).unwrap(),
                value: 0.0,
            },
            Key {
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
                        time: Time::new(0, 1).unwrap(),
                        value: 0.0,
                    },
                    Key {
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
