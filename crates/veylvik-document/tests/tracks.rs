use veylvik_document::optical::{AreaLight, StudioRig};
use veylvik_document::presets::material::Source;
use veylvik_document::*;

fn at(ticks: i64) -> Time {
    Time { ticks, rate: 1 }
}

#[test]
fn colour_and_vector_tracks_key_blend_and_fall_back_to_constants() {
    let mut track = Track3::from([0.0, 0.5, 1.0]);
    assert_eq!(
        serde_json::to_value(&track).unwrap(),
        serde_json::json!([0.0, 0.5, 1.0])
    );
    track.set_key_at(at(0), [0.0, 0.0, 0.0]).unwrap();
    track.set_key_at(at(2), [1.0, 0.5, 0.25]).unwrap();
    assert_eq!(track.at(at(1)).unwrap(), [0.5, 0.25, 0.125]);
    assert_eq!(
        track.at(at(5)).unwrap(),
        [1.0, 0.5, 0.25],
        "holds after the last key"
    );
    let json = serde_json::to_string(&track).unwrap();
    assert_eq!(serde_json::from_str::<Track3>(&json).unwrap(), track);
    assert!(track.set_key_at(at(1), [f32::NAN, 0.0, 0.0]).is_err());
    track.remove_key_at(at(0)).unwrap();
    track.remove_key_at(at(2)).unwrap();
    assert_eq!(track.constant(), Some([1.0, 0.5, 0.25]));
    assert!(track.remove_key_at(at(2)).is_err());
}

#[test]
fn rigs_and_colours_written_before_tracks_still_load() {
    // A light as saved before keys existed: plain arrays and a plain intensity.
    let old = serde_json::json!({"environment":0.65,"environment_rotation":-45.0,"ambient":0.0,"lights":[
        {"enabled":true,"position":[1.0,2.0,3.0],"rotation":[0.0,10.0,0.0],"size":[2.0,3.0],
         "colour":[1.0,0.9,0.8],"intensity":4.5}]});
    let rig: StudioRig = serde_json::from_value(old).unwrap();
    rig.validate().unwrap();
    let light = rig.lights[0].at(at(0)).unwrap();
    assert_eq!((light.position, light.intensity), ([1.0, 2.0, 3.0], 4.5));
    let mut material = Source::Builtin("marble").material().unwrap();
    let json = serde_json::json!([[0.02, 0.02, 0.025], [0.8, 0.55, 0.15], [0.0, 0.0, 0.0]]);
    material.colours = Some(serde_json::from_value(json).unwrap());
    assert_eq!(
        optical::palette_at(material.colours.as_ref().unwrap(), at(3)).unwrap()[1],
        [0.8, 0.55, 0.15]
    );
}

#[test]
fn keys_on_colours_and_lights_go_through_undoable_commands() {
    let mut scene = blank(64, 64, Time { ticks: 24, rate: 1 }).unwrap();
    let mut material = Source::Builtin("marble").material().unwrap();
    material.studio.lights = vec![AreaLight::default()];
    let node = Node::new(
        "m",
        Operation::Material(material),
        &[("image", scene.output)],
    );
    let id = node.id;
    scene.nodes.insert(id, node);
    scene.output = id;
    let mut document = Document::new(scene).unwrap();
    let track_key = |track: &str, ticks, value| Command::SetTrackKey {
        id,
        track: track.into(),
        time: at(ticks),
        value,
    };
    let revision = document
        .apply(
            0,
            &[
                // The family's own colours become keyable.
                track_key("colours.1", 0, [0.9, 0.1, 0.1]),
                track_key("colours.1", 2, [0.1, 0.1, 0.9]),
                track_key("studio.lights.0.position", 0, [-3.0, 0.0, 2.0]),
                track_key("studio.lights.0.position", 2, [3.0, 0.0, 2.0]),
                Command::SetKey {
                    id,
                    parameter: "studio.lights.0.intensity".into(),
                    time: at(2),
                    value: 8.0,
                },
            ],
        )
        .unwrap();
    let Operation::Material(m) = &document.snapshot().nodes[&id].operation else {
        panic!()
    };
    let colours = m.colours.as_ref().unwrap();
    assert_eq!(colours[1].at(at(1)).unwrap(), [0.5, 0.1, 0.5]);
    assert_eq!(
        colours[0].constant(),
        Some(MaterialKind::Marble.palette()[0])
    );
    let light = m.studio.at(at(1)).unwrap().lights[0];
    assert_eq!(light.position, [0.0, 0.0, 2.0]);
    assert_eq!(m.studio.at(at(2)).unwrap().lights[0].intensity, 8.0);
    // Out-of-range colours, unknown tracks and vectors given to set_key are refused.
    for bad in [
        track_key("colours.0", 1, [1.5, 0.0, 0.0]),
        track_key("colours.3", 1, [0.5; 3]),
        track_key("studio.lights.4.position", 1, [0.0; 3]),
        track_key("studio.lights.0.intensity", 1, [1.0; 3]),
    ] {
        assert!(document.apply(revision, &[bad]).is_err());
    }
    let revision = document
        .apply(
            revision,
            &[Command::RemoveKey {
                id,
                parameter: "colours.1".into(),
                time: at(2),
            }],
        )
        .unwrap();
    let Operation::Material(m) = &document.snapshot().nodes[&id].operation else {
        panic!()
    };
    assert_eq!(m.colours.as_ref().unwrap()[1].keys().len(), 1);
    assert!(document.undo(revision).is_ok());
}
