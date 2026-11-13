use std::collections::BTreeSet;
use veylvik_document::{
    Document, Node, NodeId, NodePosition, Operation, Time, blank, clipboard::NodeClipboard,
    recipes::Recipe,
};

#[test]
fn same_project_paste_remaps_internal_edges_keeps_external_edges_and_undoes_atomically() {
    let mut doc = blank(320, 180, Time::new(24, 1).unwrap()).unwrap();
    let source = doc.output;
    let mut first = Node::new(
        "Exposure",
        Operation::Exposure(veylvik_document::nodes::Exposure { stops: 1.0.into() }),
        &[("image", source)],
    );
    first.position = Some(NodePosition { x: 100.0, y: 200.0 });
    first
        .operation
        .scalar_mut("stops")
        .unwrap()
        .set_key_at(Time::new(1, 1).unwrap(), 2.0)
        .unwrap();
    let a = first.id;
    doc.nodes.insert(a, first);
    let second = Node::new(
        "Merge",
        Operation::Merge(veylvik_document::nodes::Merge {}),
        &[("background", source), ("foreground", a)],
    );
    let b = second.id;
    doc.nodes.insert(b, second);
    let session = NodeId::new_v4();
    let clip = NodeClipboard::capture(&doc, session, [a, b].into(), b).unwrap();
    let clip = NodeClipboard::decode(&clip.encode().unwrap()).unwrap();
    let copy = clip.paste(&doc, session).unwrap();
    assert_eq!(copy.nodes.len(), 2);
    let aa = copy.mapping[&a];
    let bb = copy.mapping[&b];
    assert_eq!(copy.nodes[&aa].inputs["image"], source);
    assert_eq!(copy.nodes[&bb].inputs["foreground"], aa);
    assert_eq!(copy.nodes[&bb].inputs["background"], source);
    assert_eq!(
        serde_json::to_value(&copy.nodes[&aa].operation).unwrap(),
        serde_json::to_value(&doc.nodes[&a].operation).unwrap()
    );
    assert_eq!(copy.nodes[&aa].position.unwrap().x, 100.0);
    let original = serde_json::to_value(&doc).unwrap();
    let mut owner = Document::new(doc).unwrap();
    owner.apply(0, &copy.into_commands()).unwrap();
    assert_eq!(owner.snapshot().output, source);
    assert_eq!(owner.history().undo_steps, 1);
    owner.undo(1).unwrap();
    assert_eq!(serde_json::to_value(owner.snapshot()).unwrap(), original);
}
#[test]
fn other_project_and_deleted_sources_receive_shared_explicit_transparent_inputs() {
    let mut doc = blank(8, 8, Time::new(24, 1).unwrap()).unwrap();
    let source = doc.output;
    let node = Node::new(
        "Merge",
        Operation::Merge(veylvik_document::nodes::Merge {}),
        &[("background", source), ("foreground", source)],
    );
    let id = node.id;
    doc.nodes.insert(id, node);
    let session = NodeId::new_v4();
    let clip = NodeClipboard::capture(&doc, session, [id].into(), id).unwrap();
    for (target, destination_session) in [
        (doc.clone(), NodeId::new_v4()),
        (blank(8, 8, Time::new(24, 1).unwrap()).unwrap(), session),
    ] {
        let copy = clip.paste(&target, destination_session).unwrap();
        assert_eq!(copy.nodes.len(), 2);
        let root = &copy.nodes[&copy.root];
        let boundary = root.inputs["background"];
        assert_eq!(boundary, root.inputs["foreground"]);
        assert_ne!(boundary, source);
        assert!(copy.nodes[&boundary].label.starts_with("Input: "));
        assert!(
            matches!(copy.nodes[&boundary].operation,Operation::Solid(veylvik_document::nodes::Solid {color}) if color==[0.0;4])
        );
        let owner = Document::new(target).unwrap();
        owner.prepare(0, &copy.into_commands()).unwrap();
    }
}
#[test]
fn complete_groups_and_controls_survive_copy_without_sharing_ids() {
    let doc = blank(320, 180, Time::new(24, 1).unwrap()).unwrap();
    let effect = Recipe::Glow.build(&doc, doc.output).unwrap();
    let root = effect.root;
    let commands = effect.into_grouped_commands(&doc).unwrap();
    let doc = Document::new(doc).unwrap().prepare(0, &commands).unwrap();
    let group = doc.groups.values().next().unwrap();
    let session = NodeId::new_v4();
    let clip = NodeClipboard::capture(&doc, session, group.members.clone(), root).unwrap();
    let copy = clip.paste(&doc, session).unwrap();
    assert_eq!(copy.groups.len(), 1);
    assert_ne!(copy.groups[0].id, group.id);
    for (name, control) in &group.controls {
        assert_eq!(
            copy.groups[0].controls[name].node,
            copy.mapping[&control.node]
        );
    }
    let owner = Document::new(doc).unwrap();
    owner.prepare(0, &copy.into_commands()).unwrap();
}
#[test]
fn malformed_clipboards_are_rejected_before_any_edit() {
    let mut doc = blank(8, 8, Time::new(24, 1).unwrap()).unwrap();
    let node = Node::new(
        "Exposure",
        Operation::Exposure(veylvik_document::nodes::Exposure { stops: 1.0.into() }),
        &[("image", doc.output)],
    );
    let id = node.id;
    doc.nodes.insert(id, node);
    let clip = NodeClipboard::capture(&doc, NodeId::new_v4(), [id].into(), id).unwrap();
    assert!(NodeClipboard::decode("plain user text").is_err());
    assert!(
        NodeClipboard::decode(&"x".repeat(veylvik_document::limits::MAX_DOCUMENT_BYTES + 1))
            .is_err()
    );
    let mut bad = clip.clone();
    bad.veylvik_nodes = 2;
    assert!(bad.validate().is_err());
    let mut bad = clip.clone();
    bad.selected = BTreeSet::new();
    assert!(bad.validate().is_err());
    let mut bad = clip.clone();
    bad.fragment.nodes.get_mut(&doc.output).unwrap().operation =
        Operation::Solid(veylvik_document::nodes::Solid { color: [1.0; 4] });
    assert!(bad.validate().is_err());
    let mut bad = clip;
    bad.fragment
        .nodes
        .get_mut(&id)
        .unwrap()
        .inputs
        .insert("image".into(), id);
    assert!(bad.validate().is_err());
}
#[test]
fn paste_limits_are_checked_and_image_references_are_only_data() {
    let mut doc = blank(8, 8, Time::new(24, 1).unwrap()).unwrap();
    let image = Node::new(
        "Image",
        Operation::Image(veylvik_document::nodes::Image {
            asset: "missing/source.png".into(),
        }),
        &[],
    );
    let id = image.id;
    doc.nodes.insert(id, image);
    let session = NodeId::new_v4();
    let clip = NodeClipboard::capture(&doc, session, [id].into(), id).unwrap();
    let mut clip = clip;
    clip.source_base = Some("/tmp/source-project".into());
    let copy = clip.paste(&doc, NodeId::new_v4()).unwrap();
    assert!(
        matches!(&copy.nodes[&copy.root].operation,Operation::Image(veylvik_document::nodes::Image {asset}) if asset=="/tmp/source-project/missing/source.png")
    );
    while doc.nodes.len() < veylvik_document::limits::MAX_NODES {
        let node = Node::new(
            "Source",
            Operation::Solid(veylvik_document::nodes::Solid { color: [0.0; 4] }),
            &[],
        );
        doc.nodes.insert(node.id, node);
    }
    assert!(clip.paste(&doc, session).is_err());
}
