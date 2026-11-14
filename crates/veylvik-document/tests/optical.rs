use veylvik_document::{
    MaterialKind, Operation,
    optical::{StudioRig, SurfaceOptics},
};
#[test]
fn legacy_material_gets_valid_optics_and_lighting() {
    let mut value = veylvik_document::registry::TYPES
        .iter()
        .find(|n| n.type_id == "material")
        .unwrap()
        .default_operation();
    value.as_object_mut().unwrap().remove("optics");
    value.as_object_mut().unwrap().remove("studio");
    let op: Operation = serde_json::from_value(value).unwrap();
    op.validate().unwrap();
    let Operation::Material(material) = op else {
        panic!()
    };
    assert!(material.optics.is_none());
    assert_eq!(material.studio, StudioRig::default());
}
#[test]
fn optical_defaults_and_invalid_rigs() {
    for kind in MaterialKind::ALL {
        kind.optics().validate().unwrap();
    }
    assert!(MaterialKind::Gold.optics().f0()[0] > MaterialKind::Gold.optics().f0()[2]);
    assert!(!MaterialKind::LeatherBrown.optics().metallic);
    assert!((MaterialKind::Wood.optics().f0()[0] - 0.04).abs() < 1e-6);
    let bad = SurfaceOptics {
        ior: f32::NAN,
        ..MaterialKind::Wood.optics()
    };
    assert!(bad.validate().is_err());
    let mut rig = StudioRig::default();
    rig.lights[0].size[0] = 0.;
    assert!(rig.validate().is_err());
    rig = StudioRig::default();
    rig.lights[0].rotation[0] = f32::INFINITY;
    assert!(rig.validate().is_err());
    rig = StudioRig::default();
    rig.lights = vec![Default::default(); 9];
    assert!(rig.validate().is_err());
    rig = StudioRig::default();
    rig.lights.clear();
    rig.validate().unwrap();
    let restored: StudioRig = serde_json::from_str(&serde_json::to_string(&rig).unwrap()).unwrap();
    assert_eq!(rig, restored);
}

#[test]
fn legacy_studio_rig_loads_environment_defaults_and_rejects_nonfinite_settings() {
    let mut value = serde_json::to_value(StudioRig::default()).unwrap();
    value.as_object_mut().unwrap().remove("environment");
    value
        .as_object_mut()
        .unwrap()
        .remove("environment_rotation");
    let mut rig: StudioRig = serde_json::from_value(value).unwrap();
    assert_eq!(rig.environment, StudioRig::default().environment);
    rig.validate().unwrap();
    rig.environment_rotation = f32::NAN;
    assert!(rig.validate().is_err());
}

#[test]
fn metal_finishes_roundtrip_and_preserve_authored_lighting() {
    use veylvik_document::optical::MetalFinish;
    for kind in [
        MaterialKind::Gold,
        MaterialKind::Silver,
        MaterialKind::Bronze,
        MaterialKind::Aluminium,
    ] {
        let mut op: Operation = serde_json::from_value(
            veylvik_document::registry::TYPES
                .iter()
                .find(|n| n.type_id == "material")
                .unwrap()
                .default_operation(),
        )
        .unwrap();
        let Operation::Material(ref mut material) = op else {
            panic!()
        };
        material.kind = kind;
        material.studio.environment_rotation = 72.;
        material.bevel = 17.0.into();
        for finish in MetalFinish::ALL {
            material.apply_finish(finish);
            assert_eq!(material.finish(), Some(finish));
            assert_eq!(material.studio.environment_rotation, 72.);
            assert_eq!(
                material
                    .bevel
                    .at(veylvik_document::Time::new(0, 1).unwrap())
                    .unwrap(),
                17.
            );
            let copy: veylvik_document::nodes::Material =
                serde_json::from_value(serde_json::to_value(&material).unwrap()).unwrap();
            assert_eq!(copy.finish(), Some(finish));
        }
        material.roughness = 0.123.into();
        assert_eq!(material.finish(), None);
    }
}

#[test]
fn legacy_aluminium_keeps_brushing_and_all_metals_have_named_starters() {
    for id in ["gold_text", "silver_text", "bronze_text", "aluminium_text"] {
        let preset = veylvik_document::presets::builtin::build(id).unwrap();
        let material = preset
            .nodes
            .values()
            .find_map(|n| {
                if let Operation::Material(m) = &n.operation {
                    Some(m)
                } else {
                    None
                }
            })
            .unwrap();
        assert!(material.finish().is_some());
        let mut old = serde_json::to_value(material).unwrap();
        old.as_object_mut().unwrap().remove("brushing");
        let restored: veylvik_document::nodes::Material = serde_json::from_value(old).unwrap();
        assert_eq!(restored.is_brushed(), id == "aluminium_text");
    }
}
