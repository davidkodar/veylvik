use std::collections::BTreeMap;
use veylvik_document::{
    presets::{Preset, builtin},
    *,
};
#[test]
fn starters_are_portable_and_instances_are_independent_with_one_undo() {
    for starter in builtin::ALL {
        let preset = builtin::build(starter.id).unwrap();
        preset.validate().unwrap();
        let wire = serde_json::to_vec(&preset).unwrap();
        let preset: Preset = serde_json::from_slice(&wire).unwrap();
        let mut owner = Document::new(blank(640, 360, Time::new(30, 1).unwrap()).unwrap()).unwrap();
        let bindings = preset
            .group
            .inputs
            .keys()
            .map(|key| (key.clone(), owner.snapshot().output))
            .collect();
        let instance = preset
            .instantiate(owner.snapshot(), &bindings, None)
            .unwrap();
        let output = instance.output;
        let group = instance.group;
        let original = owner.snapshot().output;
        owner.apply(0, &instance.commands).unwrap();
        assert_eq!(owner.snapshot().output, original);
        assert_eq!(owner.history().undo_steps, 1);
        let second = preset
            .instantiate(owner.snapshot(), &bindings, None)
            .unwrap();
        owner.apply(1, &second.commands).unwrap();
        assert!(
            owner.snapshot().groups[&group]
                .members
                .is_disjoint(&owner.snapshot().groups[&second.group].members)
        );
        let captured = Preset::capture(owner.snapshot(), group).unwrap();
        assert_eq!(captured.nodes.len(), preset.nodes.len());
        assert!(
            captured
                .nodes
                .values()
                .all(|n| n.inputs.values().all(|id| captured.nodes.contains_key(id)))
        );
        assert!(owner.snapshot().nodes.contains_key(&output));
        owner.undo(2).unwrap();
        owner.undo(3).unwrap();
        assert_eq!(owner.snapshot().nodes.len(), 1);
    }
}
#[test]
fn bindings_are_exact_and_external_ids_never_get_accidentally_remapped() {
    let preset = builtin::build("liquid_shimmer").unwrap();
    let mut doc = blank(8, 8, Time::new(24, 1).unwrap()).unwrap();
    let mut source = doc.nodes.remove(&doc.output).unwrap();
    source.id = *preset.nodes.keys().next().unwrap();
    doc.output = source.id;
    doc.nodes.insert(source.id, source);
    assert!(preset.instantiate(&doc, &BTreeMap::new(), None).is_err());
    let bindings = [("input_1".into(), doc.output)].into();
    let instance = preset.instantiate(&doc, &bindings, None).unwrap();
    let mut owner = Document::new(doc).unwrap();
    owner.apply(0, &instance.commands).unwrap();
    let g = &owner.snapshot().groups[&instance.group];
    assert_eq!(
        g.source(owner.snapshot(), "input_1"),
        Some(owner.snapshot().output)
    );
    assert!(
        preset
            .instantiate(
                owner.snapshot(),
                &[("extra".into(), owner.snapshot().output)].into(),
                None
            )
            .is_err()
    );
}
#[test]
fn malformed_and_path_dependent_presets_fail_validation_without_panics() {
    let preset = builtin::build("neon_outline").unwrap();
    let mut invalid = preset.clone();
    invalid.schema = 99;
    assert!(invalid.validate().is_err());
    let mut invalid = preset.clone();
    invalid.nodes.remove(&invalid.group.output);
    assert!(invalid.validate().is_err());
    let mut invalid = preset.clone();
    invalid.group.inputs.clear();
    assert!(invalid.validate().is_err());
    let mut invalid = preset.clone();
    invalid
        .nodes
        .get_mut(&invalid.group.output)
        .unwrap()
        .operation = Operation::Image {
        asset: "/tmp/private.png".into(),
    };
    assert!(invalid.validate().is_err());
    let mut invalid = preset.clone();
    invalid.group.inputs.values_mut().next().unwrap().targets[0].node = NodeId::new_v4();
    assert!(invalid.validate().is_err());
    let mut invalid = preset;
    let target = invalid.group.inputs.values().next().unwrap().targets[0].clone();
    invalid
        .nodes
        .get_mut(&target.node)
        .unwrap()
        .inputs
        .insert(target.port, NodeId::new_v4());
    assert!(invalid.validate().is_err());
}
#[test]
fn animation_keeps_exact_seconds_and_defaults_across_frame_rates() {
    let preset = builtin::build("liquid_shimmer").unwrap();
    let doc = blank(640, 360, Time::new(60, 1).unwrap()).unwrap();
    let instance = preset
        .instantiate(&doc, &[("input_1".into(), doc.output)].into(), None)
        .unwrap();
    let mut owner = Document::new(doc).unwrap();
    owner.apply(0, &instance.commands).unwrap();
    let group = &owner.snapshot().groups[&instance.group];
    let control = &group.controls["flow_animation"];
    let scalar = owner.snapshot().nodes[&control.node]
        .operation
        .scalar(&control.parameter)
        .unwrap();
    assert_eq!(scalar.at(Time::new(120, 60).unwrap()).unwrap(), 0.8);
    assert_eq!(
        control.default,
        preset.group.controls["flow_animation"].default
    );
}

#[test]
fn expanded_catalog_has_unique_discoverable_ids_and_editable_controls() {
    let mut ids = std::collections::BTreeSet::new();
    assert_eq!(builtin::ALL.len(), 28);
    for entry in builtin::ALL {
        assert!(ids.insert(entry.id));
        assert!(builtin::CATEGORIES.contains(&builtin::category(entry.id)));
        let preset = builtin::build(entry.id).unwrap();
        assert!(
            !preset.group.controls.is_empty(),
            "{} has no public controls",
            entry.id
        );
        assert_eq!(preset.group.inputs.len(), 1);
    }
    assert_eq!(
        builtin::ALL
            .iter()
            .filter(|e| builtin::category(e.id) == "Particles")
            .count(),
        5
    );
}
