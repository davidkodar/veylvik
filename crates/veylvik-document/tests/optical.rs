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

#[test]
fn shot_light_finds_the_bright_side_and_its_colour() {
    use veylvik_document::optical::{ShotLight, StudioRig};
    let (w, h) = (192u32, 108u32);
    // Dim blue-grey frame with a warm sun near the top right.
    let pixels: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            let (x, y) = ((i % w) as f32, (i / w) as f32);
            if ((x - 160.0).powi(2) + (y - 20.0).powi(2)).sqrt() < 10.0 {
                [8.0, 5.0, 2.0, 1.0]
            } else {
                [0.05, 0.06, 0.08, 1.0]
            }
        })
        .collect();
    let shot = ShotLight::analyse(&pixels, w, h).unwrap();
    assert!((shot.position[0] - 160.0).abs() < 8.0 && (shot.position[1] - 20.0).abs() < 8.0);
    assert!(
        shot.colour[0] == 1.0 && shot.colour[2] < 0.4,
        "warm sun: {:?}",
        shot.colour
    );
    assert!(shot.contrast > 5.0);
    let mut rig = StudioRig::default();
    rig.match_shot(&shot, w, h);
    let key = &rig.lights[1];
    assert!(
        key.enabled && key.position[0] > 0.0 && key.position[1] < 0.0,
        "up and right"
    );
    rig.validate().unwrap();
    // Flat grey: a soft key above, environment follows brightness.
    let flat = vec![[0.18, 0.18, 0.18, 1.0]; (w * h) as usize];
    let shot = ShotLight::analyse(&flat, w, h).unwrap();
    rig.match_shot(&shot, w, h);
    assert_eq!(rig.lights[1].position, [0.0, -3.0, 2.5]);
    assert!((rig.environment - 0.65).abs() < 0.01);
    assert!(ShotLight::analyse(&vec![[0.0; 4]; (w * h) as usize], w, h).is_none());
}

fn moving_track() -> veylvik_document::planar::Track {
    use veylvik_document::planar::{Sample, Track};
    let quad = |x: f64| [[x, 20.0], [x + 20.0, 20.0], [x + 20.0, 40.0], [x, 40.0]];
    let sample = |ticks: i64, x: f64| Sample {
        time: veylvik_document::Time { ticks, rate: 24 },
        corners: quad(x),
        features: vec![],
        rms_pixels: 0.0,
        motion: None,
    };
    Track {
        reference_time: veylvik_document::Time { ticks: 0, rate: 24 },
        reference: quad(10.0),
        region: None,
        samples: vec![sample(0, 10.0), sample(24, 150.0)],
    }
}

#[test]
fn following_lights_ride_the_track_and_keep_aiming_at_the_subject() {
    use veylvik_document::optical::{StudioRig, aim, scene_point};
    let track = moving_track();
    track.validate().unwrap();
    let mut rig = StudioRig::default();
    rig.lights[1].enabled = true;
    rig.lights[1].follow = true;
    rig.lights[1].position = [0.5, 0.0, 2.0];
    let fixed = rig.lights[0].clone();
    let at = |ticks| {
        rig.followed(
            Some(&track),
            veylvik_document::Time { ticks, rate: 24 },
            192,
            108,
        )
        .unwrap()
    };
    let (start, end) = (at(0), at(24));
    // Start: tracked centre (20, 30); end: (160, 30); each plus the 0.5-unit offset.
    let expect = |x| {
        let p = scene_point([x, 30.0], 192, 108);
        [p[0] + 0.5, p[1], 2.0]
    };
    for (rig, x) in [(&start, 20.0), (&end, 160.0)] {
        let light = &rig.lights[1];
        for (a, b) in light.position.iter().zip(expect(x)) {
            assert!(
                (a - b).abs() < 1e-3,
                "{:?} vs {:?}",
                light.position,
                expect(x)
            );
        }
        assert_eq!(light.rotation, aim(light.position));
        assert_eq!(
            rig.lights[0].position, fixed.position,
            "non-following lights stay put"
        );
    }
    assert!(end.lights[1].position[0] > start.lights[1].position[0]);
    // Without a track nothing moves.
    let still = rig
        .followed(
            None,
            veylvik_document::Time {
                ticks: 24,
                rate: 24,
            },
            192,
            108,
        )
        .unwrap();
    assert_eq!(still.lights[1].position, rig.lights[1].position);
}

#[test]
fn the_look_slider_runs_polished_to_matte_and_reads_back() {
    use veylvik_document::presets::material::Source;
    let mut gold = Source::Builtin("gold_text").material().unwrap();
    let mut last = -1.0;
    for step in 0..=10 {
        let look = step as f32 / 10.0;
        gold.set_look(look);
        assert!(
            (gold.look().unwrap() - look).abs() < 1e-4,
            "round trip at {look}"
        );
        let rough = gold
            .roughness
            .at(veylvik_document::Time::new(0, 1).unwrap())
            .unwrap();
        assert!(rough > last, "roughness rises with the look");
        last = rough;
    }
    veylvik_document::presets::material::Saved::new("Look", &gold).unwrap();
    // Animated roughness has no single look.
    gold.roughness = serde_json::from_value(serde_json::json!({"kind":"linear","keys":[
        {"time":{"ticks":0,"rate":1},"value":0.1},{"time":{"ticks":1,"rate":1},"value":0.5}]}))
    .unwrap();
    assert!(gold.look().is_none());
    // Wood keeps its grain: only metals tie grain to the finish.
    let mut wood = Source::Builtin("walnut").material().unwrap();
    let grain = serde_json::to_value(&wood.grain).unwrap();
    wood.set_look(0.0);
    assert_eq!(serde_json::to_value(&wood.grain).unwrap(), grain);
}
