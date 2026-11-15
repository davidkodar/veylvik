use serde_json::json;
use veylvik_document::{
    groups::{Control, Group},
    *,
};
fn fixture() -> (Composition, Group, NodeId) {
    let mut doc = blank(16, 8, Time::new(24, 1).unwrap()).unwrap();
    let effect = recipes::Recipe::Glow.build(&doc, doc.output).unwrap();
    let bright = effect.roles["brightness"];
    let members = effect.nodes.iter().map(|n| n.id).collect();
    for node in effect.nodes {
        doc.nodes.insert(node.id, node);
    }
    doc.output = effect.root;
    let mut group = Group::capture(&doc, members, doc.output, "Glow").unwrap();
    let mut control = Control::new(&doc, bright, "stops", "Intensity").unwrap();
    control.range = Some([-4.0, 4.0]);
    group.controls.insert("intensity".into(), control);
    (doc, group, bright)
}
#[test]
fn groups_round_trip_keep_pixels_and_share_atomic_control_history() {
    let (doc, group, bright) = fixture();
    let old = serde_json::to_value(&doc).unwrap();
    assert!(old.get("groups").is_none());
    let mut owner = Document::new(doc.clone()).unwrap();
    owner
        .apply(
            0,
            &[Command::PutGroup {
                group: Box::new(group.clone()),
            }],
        )
        .unwrap();
    let saved = serde_json::to_vec(owner.snapshot()).unwrap();
    let loaded: Composition = serde_json::from_slice(&saved).unwrap();
    loaded.validate().unwrap();
    assert_eq!(serde_json::to_value(&loaded.nodes).unwrap(), old["nodes"]);
    let curve = json!({"kind":"smooth","keys":[{"time":{"ticks":0,"rate":24},"value":0.0},{"time":{"ticks":48,"rate":24},"value":2.0}]});
    let command = Command::SetControl {
        id: group.id,
        control: "intensity".into(),
        value: curve.clone(),
    };
    let decoded: Command = serde_json::from_value(serde_json::to_value(&command).unwrap()).unwrap();
    owner.apply(1, &[decoded]).unwrap();
    assert_eq!(
        owner.snapshot().groups[&group.id].controls["intensity"]
            .value(owner.snapshot())
            .unwrap(),
        curve
    );
    assert_eq!(
        owner.snapshot().nodes[&bright]
            .operation
            .scalar("stops")
            .unwrap()
            .at(Time::new(1, 1).unwrap())
            .unwrap(),
        1.0
    );
    owner.undo(2).unwrap();
    assert_eq!(
        owner.snapshot().nodes[&bright]
            .operation
            .scalar("stops")
            .unwrap()
            .at(Time::new(0, 1).unwrap())
            .unwrap(),
        1.0
    );
    owner.redo(3).unwrap();
    let animated_nodes = serde_json::to_value(&owner.snapshot().nodes).unwrap();
    owner
        .apply(4, &[Command::Ungroup { id: group.id }])
        .unwrap();
    assert!(owner.snapshot().groups.is_empty());
    assert_eq!(
        serde_json::to_value(&owner.snapshot().nodes).unwrap(),
        animated_nodes
    );
}
#[test]
fn all_edit_paths_respect_exposed_bounds_and_fail_without_partial_mutation() {
    let (mut doc, group, bright) = fixture();
    doc.groups.insert(group.id, group.clone());
    let mut owner = Document::new(doc).unwrap();
    let before = serde_json::to_value(owner.snapshot()).unwrap();
    for command in [
        Command::SetControl {
            id: group.id,
            control: "intensity".into(),
            value: json!({"kind":"constant","value":5}),
        },
        Command::SetParameter {
            id: bright,
            parameter: "stops".into(),
            value: json!({"kind":"constant","value":5}),
        },
        Command::SetKey {
            id: bright,
            parameter: "stops".into(),
            time: Time::new(0, 1).unwrap(),
            value: 5.0,
        },
        Command::Remove { id: bright },
        Command::Output { id: bright },
    ] {
        assert!(
            owner
                .apply(
                    0,
                    &[
                        Command::Resize {
                            width: 32,
                            height: 32
                        },
                        command
                    ]
                )
                .is_err()
        );
        assert_eq!(serde_json::to_value(owner.snapshot()).unwrap(), before);
        assert_eq!(owner.revision(), 0);
        assert_eq!(owner.history().undo_steps, 0);
    }
}
#[test]
fn shared_input_reconnects_all_targets_and_cycles_are_atomic() {
    let (mut doc, group, _) = fixture();
    assert_eq!(group.inputs.len(), 1);
    let (key, input) = group.inputs.first_key_value().unwrap();
    assert_eq!(input.targets.len(), 2);
    doc.groups.insert(group.id, group.clone());
    let other = Node::new(
        "other",
        Operation::Solid(veylvik_document::nodes::Solid { color: [1.0; 4] }),
        &[],
    );
    let original_source = group.source(&doc, key).unwrap();
    let source = other.id;
    let mut owner = Document::new(doc).unwrap();
    owner
        .apply(
            0,
            &[
                Command::Put {
                    node: Box::new(other),
                },
                Command::ConnectGroup {
                    id: group.id,
                    input: key.clone(),
                    source,
                },
            ],
        )
        .unwrap();
    for target in &input.targets {
        assert_eq!(
            owner.snapshot().nodes[&target.node].inputs[&target.port],
            source
        );
    }
    assert!(
        owner
            .apply(
                1,
                &[Command::ConnectGroup {
                    id: group.id,
                    input: key.clone(),
                    source: group.output
                }]
            )
            .is_err()
    );
    assert_eq!(owner.revision(), 1);
    let target = &input.targets[0];
    let mut node = owner.snapshot().nodes[&target.node].clone();
    node.inputs.insert(target.port.clone(), original_source);
    assert!(
        owner
            .apply(
                1,
                &[Command::Put {
                    node: Box::new(node)
                }]
            )
            .is_err()
    );
}
#[test]
fn duplicate_remaps_controls_and_preserves_external_inputs_independently() {
    let (mut doc, group, bright) = fixture();
    doc.groups.insert(group.id, group.clone());
    let source = group.source(&doc, "input_1").unwrap();
    let mut owner = Document::new(doc).unwrap();
    let (copy, commands) = group.duplicate(owner.snapshot()).unwrap();
    owner.apply(0, &commands).unwrap();
    assert!(
        group
            .duplicate(&blank(8, 8, Time::new(24, 1).unwrap()).unwrap())
            .is_err()
    );
    assert!(copy.members.is_disjoint(&group.members));
    assert_eq!(copy.source(owner.snapshot(), "input_1"), Some(source));
    owner
        .apply(
            1,
            &[Command::SetControl {
                id: copy.id,
                control: "intensity".into(),
                value: json!({"kind":"constant","value":3}),
            }],
        )
        .unwrap();
    assert_eq!(
        owner.snapshot().nodes[&bright]
            .operation
            .scalar("stops")
            .unwrap()
            .at(Time::new(0, 1).unwrap())
            .unwrap(),
        1.0
    );
    assert_eq!(
        copy.controls["intensity"].value(owner.snapshot()).unwrap(),
        json!({"kind":"constant","value":3.0})
    );
    owner.undo(2).unwrap();
    owner.undo(3).unwrap();
    assert_eq!(owner.snapshot().groups.len(), 1);
}
#[test]
fn boundaries_reject_leaks_orphans_overlaps_and_invalid_defaults() {
    let (doc, group, bright) = fixture();
    let reject = |group: Group| {
        let mut d = doc.clone();
        d.groups.insert(group.id, group);
        assert!(d.validate().is_err());
    };
    let mut bad = group.clone();
    bad.inputs.clear();
    reject(bad);
    let mut bad = group.clone();
    bad.controls.get_mut("intensity").unwrap().default = json!({"kind":"constant","value":99});
    reject(bad);
    let mut bad = group.clone();
    bad.controls.get_mut("intensity").unwrap().range = Some([4.0, -4.0]);
    reject(bad);
    let mut bad = group.clone();
    bad.output = bright;
    reject(bad);
    let mut d = doc.clone();
    d.groups.insert(group.id, group.clone());
    let mut overlap = group.clone();
    overlap.id = NodeId::new_v4();
    d.groups.insert(overlap.id, overlap);
    assert!(d.validate().is_err());
    d.groups.remove(&group.id);
    let leak = Node::new(
        "leak",
        Operation::Blur(veylvik_document::nodes::Blur {
            radius: 1.0.into(),
            filter: Default::default(),
        }),
        &[("image", bright)],
    );
    d.nodes.insert(leak.id, leak);
    assert!(d.validate().is_err());
}

#[test]
fn branch_copy_preserves_complete_groups_and_recipe_preflight_matches_commit() {
    let doc = blank(8, 8, Time::new(24, 1).unwrap()).unwrap();
    let mut owner = Document::new(doc).unwrap();
    let effect = recipes::Recipe::Glow
        .build(owner.snapshot(), owner.snapshot().output)
        .unwrap();
    let output = effect.root;
    let commands = effect.into_grouped_commands(owner.snapshot()).unwrap();
    let prepared = owner.prepare(0, &commands).unwrap();
    assert_eq!(prepared.groups.len(), 1);
    owner.apply(0, &commands).unwrap();
    assert_eq!(
        serde_json::to_value(owner.snapshot()).unwrap(),
        serde_json::to_value(prepared).unwrap()
    );
    let copy = editing::copy_branch(owner.snapshot(), output).unwrap();
    assert_eq!(copy.groups.len(), 1);
    let group = copy.groups[0].clone();
    assert_eq!(group.output, copy.root);
    for control in group.controls.values() {
        assert!(copy.nodes.contains_key(&control.node));
    }
    owner.apply(1, &copy.into_commands()).unwrap();
    assert_eq!(owner.snapshot().groups.len(), 2);
    owner.snapshot().validate().unwrap();
}
