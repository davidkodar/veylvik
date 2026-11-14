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
