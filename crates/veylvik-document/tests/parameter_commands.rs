use serde_json::json;
use veylvik_document::*;
fn parameter(id: NodeId, name: &str, value: serde_json::Value) -> Command {
    Command::SetParameter {
        id,
        parameter: name.into(),
        value,
    }
}
fn key(id: NodeId, name: &str, ticks: i64, rate: u32, value: f32) -> Command {
    Command::SetKey {
        id,
        parameter: name.into(),
        time: Time { ticks, rate },
        value,
    }
}
#[test]
fn focused_commands_preserve_nodes_and_share_atomic_history() {
    let mut document = Document::new(motion_demo()).unwrap();
    let id = document.snapshot().output;
    let original = document.snapshot().nodes[&id].clone();
    let commands = [
        parameter(
            id,
            "stops",
            json!({"kind":"smooth","keys":[{"time":{"ticks":0,"rate":24},"value":0},{"time":{"ticks":48,"rate":24},"value":1}]}),
        ),
        key(id, "stops", 0, 1, -1.0),
        key(id, "stops", 1, 1, 2.0),
    ];
    let wire = serde_json::to_value(&commands).unwrap();
    assert_eq!(wire[0]["command"], "set_parameter");
    assert_eq!(wire[1]["command"], "set_key");
    let decoded: Vec<Command> = serde_json::from_value(wire).unwrap();
    let prepared = document.prepare(0, &decoded).unwrap();
    assert_eq!(document.revision(), 0);
    document.apply(0, &decoded).unwrap();
    assert_eq!(document.history().undo_steps, 1);
    assert_eq!(
        serde_json::to_value(document.snapshot()).unwrap(),
        serde_json::to_value(prepared).unwrap()
    );
    let changed = &document.snapshot().nodes[&id];
    assert_eq!(changed.inputs, original.inputs);
    assert_eq!(changed.position, original.position);
    assert_eq!(changed.label, original.label);
    assert_eq!(document.snapshot().output, id);
    let scalar = changed.operation.scalar("stops").unwrap();
    assert_eq!(scalar.interpolation(), Some(Interpolation::Smooth));
    assert_eq!(scalar.keys().unwrap().len(), 3);
    assert_eq!(scalar.keys().unwrap()[0].time.rate, 24);
    assert_eq!(scalar.at(Time::new(1, 1).unwrap()).unwrap(), 2.0);
    assert!(document.apply(0, &decoded).is_err());
    document.undo(1).unwrap();
    assert_eq!(
        serde_json::to_value(&document.snapshot().nodes[&id]).unwrap(),
        serde_json::to_value(original).unwrap()
    );
    document.redo(2).unwrap();
    assert_eq!(
        document.snapshot().nodes[&id]
            .operation
            .scalar("stops")
            .unwrap()
            .keys()
            .unwrap()
            .len(),
        3
    );
}
#[test]
fn invalid_parameter_and_key_batches_leave_the_whole_document_unchanged() {
    let mut document = Document::new(motion_demo()).unwrap();
    let id = document.snapshot().output;
    let solid = document
        .snapshot()
        .nodes
        .values()
        .find(|node| matches!(node.operation, Operation::Solid { .. }))
        .unwrap()
        .id;
    let before = serde_json::to_vec(document.snapshot()).unwrap();
    for invalid in [
        parameter(id, "type", json!("solid")),
        parameter(id, "inputs", json!({})),
        parameter(id, "stops", json!(2)),
        parameter(id, "stops", json!({"kind":"constant","value":100})),
        parameter(solid, "color", json!([1, 2, 3])),
        parameter(solid, "color", json!([1, 2, 3, 2])),
        parameter(NodeId::nil(), "stops", json!({"kind":"constant","value":1})),
        key(solid, "color", 0, 1, 0.5),
        key(id, "stops", 0, 0, 1.0),
        key(id, "stops", 0, 1, f32::NAN),
        key(id, "stops", 0, 1, 100.0),
    ] {
        assert!(
            document
                .apply(
                    0,
                    &[
                        Command::Resize {
                            width: 17,
                            height: 13
                        },
                        invalid
                    ]
                )
                .is_err()
        );
        assert_eq!(document.revision(), 0);
        assert_eq!(document.history().undo_steps, 0);
        assert_eq!(serde_json::to_vec(document.snapshot()).unwrap(), before);
    }
    document
        .apply(
            0,
            &[parameter(solid, "color", json!([-2.0, 3.0, 0.5, 0.25]))],
        )
        .unwrap();
    assert!(
        matches!(document.snapshot().nodes[&solid].operation, Operation::Solid { color } if color == [-2.0,3.0,0.5,0.25])
    );
    document.apply(1, &[key(id, "stops", 1, 2, 1.5)]).unwrap();
    assert_eq!(
        document.snapshot().nodes[&id]
            .operation
            .scalar("stops")
            .unwrap()
            .interpolation(),
        Some(Interpolation::Linear)
    );
}

#[test]
fn removing_keys_uses_exact_times_preserves_modes_and_is_atomic() {
    for kind in ["linear", "step", "smooth"] {
        let mut document = Document::new(motion_demo()).unwrap();
        let id = document.snapshot().output;
        document
            .apply(
                0,
                &[parameter(
                    id,
                    "stops",
                    json!({"kind":kind,"keys":[
                        {"time":{"ticks":0,"rate":24},"value":-1},
                        {"time":{"ticks":24,"rate":24},"value":2}
                    ]}),
                )],
            )
            .unwrap();
        let remove = |ticks, rate| Command::RemoveKey {
            id,
            parameter: "stops".into(),
            time: Time { ticks, rate },
        };
        let before = serde_json::to_vec(document.snapshot()).unwrap();
        for invalid in [remove(1, 0), remove(1, 2)] {
            assert!(document.apply(1, &[remove(0, 1), invalid]).is_err());
            assert_eq!(document.revision(), 1);
            assert_eq!(serde_json::to_vec(document.snapshot()).unwrap(), before);
        }
        let encoded = serde_json::to_value(remove(1, 1)).unwrap();
        assert_eq!(encoded["command"], "remove_key");
        document
            .apply(1, &[serde_json::from_value(encoded).unwrap()])
            .unwrap();
        let scalar = document.snapshot().nodes[&id]
            .operation
            .scalar("stops")
            .unwrap();
        assert_eq!(serde_json::to_value(scalar).unwrap()["kind"], kind);
        assert_eq!(scalar.keys().unwrap()[0].time.rate, 24);
        document.apply(2, &[remove(0, 1)]).unwrap();
        assert_eq!(
            serde_json::to_value(
                document.snapshot().nodes[&id]
                    .operation
                    .scalar("stops")
                    .unwrap()
            )
            .unwrap(),
            json!({"kind":"constant","value":-1.0})
        );
        assert!(document.apply(3, &[remove(0, 1)]).is_err());
        document.undo(3).unwrap();
        document.undo(4).unwrap();
        assert_eq!(serde_json::to_vec(document.snapshot()).unwrap(), before);
        document.redo(5).unwrap();
        assert_eq!(
            document.snapshot().nodes[&id]
                .operation
                .scalar("stops")
                .unwrap()
                .keys()
                .unwrap()
                .len(),
            1
        );
    }
}
