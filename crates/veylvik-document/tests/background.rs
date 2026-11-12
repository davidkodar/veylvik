use veylvik_document::{
    presets::{
        background::{self, Placement},
        builtin, showcase,
    },
    *,
};
#[test]
fn every_starter_has_replaceable_backdrop_and_repeated_replacement_undoes() {
    for entry in builtin::ALL {
        let scene = showcase::build(entry.id).unwrap();
        let old = background::automatic(&scene.composition)
            .unwrap_or_else(|| panic!("{} missing backdrop", entry.id));
        assert!(scene.composition.nodes[&old].scene_background);
        let instance =
            background::insert("brick_red", &scene.composition, Placement::Automatic, None)
                .unwrap();
        let mut owner = Document::new(scene.composition.clone()).unwrap();
        owner
            .apply(0, &instance.commands)
            .unwrap_or_else(|e| panic!("{}: {e}", entry.id));
        let doc = owner.snapshot();
        assert_eq!(background::automatic(doc), Some(instance.output));
        assert!(!background::reachable(doc).contains(&old));
        assert!(doc.nodes.contains_key(&old));
        let again = background::insert("marble", doc, Placement::Automatic, None).unwrap();
        owner.apply(owner.revision(), &again.commands).unwrap();
        assert_eq!(background::automatic(owner.snapshot()), Some(again.output));
        owner.undo(owner.revision()).unwrap();
        owner.undo(owner.revision()).unwrap();
        assert_eq!(
            serde_json::to_value(owner.snapshot()).unwrap(),
            serde_json::to_value(&scene.composition).unwrap()
        );
    }
}
#[test]
fn legacy_backdrop_detection_and_professional_behind_mode_are_distinct() {
    let mut scene = showcase::build("gold_text").unwrap().composition;
    for node in scene.nodes.values_mut() {
        node.scene_background = false;
    }
    let old = background::automatic(&scene).unwrap();
    let replaced = background::insert("brick_red", &scene, Placement::Automatic, None).unwrap();
    let owner = Document::new(scene.clone()).unwrap();
    let doc = owner.prepare(0, &replaced.commands).unwrap();
    assert!(!background::reachable(&doc).contains(&old));
    assert_eq!(doc.output, scene.output);
    let behind = background::insert("brick_red", &scene, Placement::Behind, None).unwrap();
    let doc = owner.prepare(0, &behind.commands).unwrap();
    assert_ne!(doc.output, scene.output);
    assert!(background::reachable(&doc).contains(&old));
    assert_eq!(background::automatic(&doc), Some(old));
    assert_eq!(doc.nodes[&doc.output].inputs["foreground"], scene.output);
}
#[test]
fn ambiguous_and_flattened_scenes_require_explicit_choice() {
    let mut doc = blank(96, 64, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation = Operation::Image {
        asset: "photo.png".into(),
    };
    assert_eq!(background::automatic(&doc), None);
    assert!(background::insert("brick_red", &doc, Placement::Automatic, None).is_err());
    let explicit =
        background::insert("brick_red", &doc, Placement::Replace(doc.output), None).unwrap();
    let replaced = Document::new(doc)
        .unwrap()
        .prepare(0, &explicit.commands)
        .unwrap();
    assert_eq!(replaced.output, explicit.output);
    let mut doc = showcase::build("gold_text").unwrap().composition;
    doc.nodes.get_mut(&doc.output).unwrap().scene_background = true;
    assert_eq!(background::automatic(&doc), None);
}
