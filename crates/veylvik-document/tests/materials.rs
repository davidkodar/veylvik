use veylvik_document::{
    presets::{builtin, material, text_style},
    *,
};

fn material_of(doc: &Composition, id: NodeId) -> nodes::Material {
    match &doc.nodes[&id].operation {
        Operation::Material(m) => m.clone(),
        _ => panic!("not a material"),
    }
}

fn titled() -> (Document, NodeId) {
    let mut scene = demo();
    let text = Node::new(
        "Title",
        serde_json::from_value(registry::TEXT.default_operation()).unwrap(),
        &[],
    );
    let id = text.id;
    scene.nodes.insert(id, text);
    scene.output = id;
    (Document::new(scene).unwrap(), id)
}

fn black_marble() -> material::Saved {
    let mut m = material::Source::Builtin("marble").material().unwrap();
    m.colours = Some([[0.02, 0.02, 0.025], [0.8, 0.55, 0.15], [0.; 3]]);
    m.roughness = 0.08.into();
    material::Saved::new("  Black marble  ", &m).unwrap()
}

#[test]
fn saved_materials_round_trip_and_reject_bad_input() {
    let saved = black_marble();
    assert_eq!(saved.label, "Black marble");
    let json = serde_json::to_string(&saved).unwrap();
    let back: material::Saved = serde_json::from_str(&json).unwrap();
    back.validate().unwrap();
    assert_eq!(back.material.colours, saved.material.colours);

    let mut bad = saved.clone();
    bad.schema = 2;
    assert!(bad.validate().is_err());
    let mut bad = saved.clone();
    bad.label = "\n".into();
    assert!(bad.validate().is_err());
    let mut bad = saved.clone();
    bad.label = "x".repeat(material::MAX_LABEL + 1);
    assert!(bad.validate().is_err());
    let mut bad = saved.clone();
    bad.material.colours = Some([[1.5, 0., 0.], [0.; 3], [0.; 3]]);
    assert!(bad.validate().is_err());
    let mut bad = saved;
    bad.material.roughness = 7.0.into();
    assert!(bad.validate().is_err(), "registered bounds apply");
}

#[test]
fn projects_without_colours_still_open() {
    let m = material::Source::Builtin("gold_text").material().unwrap();
    let mut json = serde_json::to_value(&m).unwrap();
    assert!(json["colours"].is_null());
    json.as_object_mut().unwrap().remove("colours");
    let back: nodes::Material = serde_json::from_value(json).unwrap();
    assert_eq!(back.colours, None);
    for kind in MaterialKind::ALL {
        assert!(kind.colour_roles().len() <= 3);
        optical::validate_palette(&kind.palette()).unwrap();
    }
}

#[test]
fn saved_material_restyles_text_but_keeps_its_size_and_motion() {
    let (mut owner, id) = titled();
    let commands = text_style::apply(owner.snapshot(), id, Some("gold_text"), None).unwrap();
    owner.apply(0, &commands).unwrap();
    let node = text_style::current(owner.snapshot(), id).unwrap().unwrap();
    let before = material_of(owner.snapshot(), node);
    let saved = black_marble();
    let count = owner.snapshot().nodes.len();
    let commands = text_style::apply(owner.snapshot(), id, Some(&saved), None).unwrap();
    owner.apply(1, &commands).unwrap();
    assert_eq!(owner.snapshot().nodes.len(), count, "replaced, not stacked");
    let after = material_of(owner.snapshot(), node);
    assert_eq!(after.kind, MaterialKind::Marble);
    assert_eq!(after.colours, saved.material.colours);
    assert_eq!(after.roughness.at(Time::new(0, 1).unwrap()).unwrap(), 0.08);
    assert_eq!(
        serde_json::to_value(&after.bevel).unwrap(),
        serde_json::to_value(&before.bevel).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&after.lighting).unwrap(),
        serde_json::to_value(&before.lighting).unwrap()
    );
    assert_eq!(owner.snapshot().nodes[&node].label, "Text · Black marble");

    // A built-in afterwards resets the custom colours, which belong to marble.
    let commands = text_style::apply(owner.snapshot(), id, Some("silver_text"), None).unwrap();
    owner.apply(2, &commands).unwrap();
    let silver = material_of(owner.snapshot(), node);
    assert_eq!(silver.kind, MaterialKind::Silver);
    assert_eq!(silver.colours, None);
}

#[test]
fn saved_material_works_as_a_background_or_foreground_layer() {
    let saved = black_marble();
    let doc = demo();
    let instance = builtin::material_layer(&saved, &doc, true, None).unwrap();
    let mut owner = Document::new(doc).unwrap();
    owner.apply(0, &instance.commands).unwrap();
    let found = owner.snapshot().nodes.values().any(
        |n| matches!(&n.operation, Operation::Material(m) if m.colours == saved.material.colours),
    );
    assert!(found);
    assert!(material::Source::Builtin("blur").preset().is_err());
}

#[test]
fn optional_inputs_may_stay_unconnected_but_required_ones_may_not() {
    let mut doc = demo();
    let shot = doc.output;
    let text = Node::new(
        "Title",
        serde_json::from_value(registry::TEXT.default_operation()).unwrap(),
        &[],
    );
    let material = material::Source::Builtin("silver_text").material().unwrap();
    let plain = Node::new(
        "Silver",
        Operation::Material(material.clone()),
        &[("image", text.id)],
    );
    let reflecting = Node::new(
        "Silver in shot",
        Operation::Material(material.clone()),
        &[("image", text.id), ("environment", shot)],
    );
    let unknown = Node::new(
        "Bad",
        Operation::Material(material.clone()),
        &[("image", text.id), ("mask", shot)],
    );
    let missing = Node::new(
        "Bad",
        Operation::Material(material),
        &[("environment", shot)],
    );
    assert!(registry::MATERIAL.is_optional("environment"));
    assert_eq!(registry::MATERIAL.required().collect::<Vec<_>>(), ["image"]);
    doc.nodes.insert(text.id, text);
    for (node, ok) in [
        (plain, true),
        (reflecting, true),
        (unknown, false),
        (missing, false),
    ] {
        let mut candidate = doc.clone();
        candidate.output = node.id;
        candidate.nodes.insert(node.id, node);
        assert_eq!(Document::new(candidate).is_ok(), ok);
    }
}

#[test]
fn light_rigs_only_connect_to_lights_inputs_and_never_render() {
    let mut doc = demo();
    let text = Node::new(
        "Title",
        serde_json::from_value(registry::TEXT.default_operation()).unwrap(),
        &[],
    );
    let rig = Node::new(
        "Key light",
        Operation::LightRig(nodes::LightRig::default()),
        &[],
    );
    let material = material::Source::Builtin("gold_text").material().unwrap();
    let lit = Node::new(
        "Gold",
        Operation::Material(material.clone()),
        &[("image", text.id), ("lights", rig.id)],
    );
    let wrong_way = Node::new(
        "Gold",
        Operation::Material(material.clone()),
        &[("image", rig.id)],
    );
    let image_as_lights = Node::new(
        "Gold",
        Operation::Material(material),
        &[("image", text.id), ("lights", text.id)],
    );
    assert_eq!(registry::LIGHT_RIG.output, registry::PortKind::Lights);
    doc.nodes.insert(text.id, text);
    doc.nodes.insert(rig.id, rig.clone());
    for (node, ok) in [(lit, true), (wrong_way, false), (image_as_lights, false)] {
        let mut candidate = doc.clone();
        candidate.output = node.id;
        candidate.nodes.insert(node.id, node);
        assert_eq!(Document::new(candidate).is_ok(), ok);
    }
    let mut rig_output = doc;
    rig_output.output = rig.id;
    assert!(Document::new(rig_output).is_err());
}

#[test]
fn a_light_rig_track_must_be_a_planar_track() {
    let mut doc = demo();
    let image = doc.output;
    let rig = Node::new(
        "Rig",
        Operation::LightRig(nodes::LightRig::default()),
        &[("track", image)],
    );
    let text = Node::new(
        "Title",
        serde_json::from_value(registry::TEXT.default_operation()).unwrap(),
        &[],
    );
    let material = Node::new(
        "Gold",
        Operation::Material(material::Source::Builtin("gold_text").material().unwrap()),
        &[("image", text.id), ("lights", rig.id)],
    );
    doc.output = material.id;
    for n in [rig, text, material] {
        doc.nodes.insert(n.id, n);
    }
    let error = Document::new(doc).err().unwrap().to_string();
    assert!(error.contains("Planar track"), "{error}");
}
