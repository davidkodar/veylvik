use veylvik_document::{nodes::*, *};
use veylvik_scene::{evaluate, math, mesh};

const W: u32 = 1920;
const H: u32 = 1080;
fn at(ticks: i64) -> Time {
    Time { ticks, rate: 1 }
}
fn scene() -> Composition {
    blank(W, H, Time::new(24, 1).unwrap()).unwrap()
}
fn default<T: serde::de::DeserializeOwned>(kind: &registry::NodeType) -> T {
    let op: Operation = serde_json::from_value(kind.default_operation()).unwrap();
    serde_json::from_value(serde_json::to_value(&op).unwrap()["parameters"].clone())
        .or_else(|_| serde_json::from_value(serde_json::to_value(&op).unwrap()))
        .unwrap_or_else(|_| panic!("default {}", kind.type_id))
}
fn add(doc: &mut Composition, label: &str, op: Operation, inputs: &[(&str, NodeId)]) -> NodeId {
    let node = Node::new(label, op, inputs);
    let id = node.id;
    doc.nodes.insert(id, node);
    id
}
fn op(kind: &registry::NodeType) -> Operation {
    serde_json::from_value(kind.default_operation()).unwrap()
}
fn close(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 0.05 && (a[1] - b[1]).abs() < 0.05
}

#[test]
fn the_home_view_sees_the_2d_frame_and_cameras_move_from_it() {
    let mut doc = scene();
    let image = doc.output;
    let card = add(&mut doc, "card", op(&IMAGE_CARD), &[("image", image)]);
    let camera = add(&mut doc, "camera", op(&CAMERA_3D), &[]);
    let merged = add(
        &mut doc,
        "merge",
        op(&MERGE_SCENES),
        &[("scene", card), ("scene_2", camera)],
    );
    let s = evaluate(&doc, merged, at(0)).unwrap();
    assert_eq!((s.objects.len(), s.cameras.len()), (1, 1));
    // A default camera equals the home view; both map the card's corners to the frame's.
    for index in [0, 1, 9] {
        let cam = s.camera(index, W, H);
        let card = &s.objects[0];
        for (corner, pixel) in [
            ([-960.0, -540.0, 0.0], [0.0, 0.0]),
            ([960.0, 540.0, 0.0], [1920.0, 1080.0]),
        ] {
            let (p, _) = cam.project(math::point(&card.world, corner), W, H).unwrap();
            assert!(close(p, pixel), "{index}: {p:?} vs {pixel:?}");
        }
    }
    // Pull the camera back by the home distance: the frame shrinks to half size.
    let distance = veylvik_scene::home_distance(W, 50.0, 36.0);
    let Operation::Camera3d(c) = &mut doc.nodes.get_mut(&camera).unwrap().operation else {
        panic!()
    };
    c.placement.position = [0.0, 0.0, distance].into();
    let s = evaluate(&doc, merged, at(0)).unwrap();
    let (p, depth) = s.camera(1, W, H).project([0.0, 0.0, 0.0], W, H).unwrap();
    assert!(close(p, [480.0, 270.0]) && (depth - 2.0 * distance).abs() < 0.5);
    // Behind the camera projects to nothing.
    assert!(
        s.camera(1, W, H)
            .project([960.0, 540.0, 3.0 * distance], W, H)
            .is_none()
    );
}

#[test]
fn objects_place_turn_and_animate_about_their_own_centre() {
    let mut doc = scene();
    let mut shape = op(&SHAPE_3D);
    if let Operation::Shape3d(s) = &mut shape {
        s.placement
            .position
            .set_key_at(at(0), [0.0, 0.0, 0.0])
            .unwrap();
        s.placement
            .position
            .set_key_at(at(2), [200.0, -100.0, 50.0])
            .unwrap();
        s.placement.rotation = [0.0, 90.0, 0.0].into();
    }
    let id = add(&mut doc, "shape", shape, &[]);
    let s = evaluate(&doc, id, at(1)).unwrap();
    let o = &s.objects[0];
    assert_eq!(math::point(&o.world, [0.0; 3]), [1060.0, 490.0, 25.0]);
    // 90 degrees about Y turns +x towards -z.
    let x = math::direction(&o.world, [1.0, 0.0, 0.0]);
    assert!(x[0].abs() < 1e-5 && (x[2] + 1.0).abs() < 1e-5, "{x:?}");
    assert!(!mesh(o).unwrap().indices.is_empty());

    // 3D text turns about its own centre, so a half turn keeps it in place.
    let text = add(&mut doc, "title", op(&registry::TEXT), &[]);
    let mut extruded = op(&TEXT_3D);
    if let Operation::Text3d(t) = &mut extruded {
        t.placement.rotation = [0.0, 180.0, 0.0].into();
    }
    let solid = add(&mut doc, "3d", extruded, &[("outline", text)]);
    let s = evaluate(&doc, solid, at(0)).unwrap();
    let Some(veylvik_scene::Geometry::Text { outline, .. }) =
        s.objects.first().map(|o| &o.geometry)
    else {
        panic!()
    };
    let [x0, y0, x1, y1] = veylvik_text::bounds(outline);
    let centre = [(x0 + x1) * 0.5, (y0 + y1) * 0.5, 0.0];
    let moved = math::point(&s.objects[0].world, centre);
    assert!((moved[0] - centre[0]).abs() < 1e-2 && (moved[1] - centre[1]).abs() < 1e-2);
    assert!(mesh(&s.objects[0]).unwrap().indices.len() > 100);
}

#[test]
fn transforms_carry_everything_and_bypass_and_ports_behave() {
    let mut doc = scene();
    let light = add(&mut doc, "light", op(&LIGHT_3D), &[]);
    let shape = add(&mut doc, "shape", op(&SHAPE_3D), &[]);
    let merged = add(
        &mut doc,
        "merge",
        op(&MERGE_SCENES),
        &[("scene", light), ("scene_2", shape)],
    );
    let mut turn = op(&TRANSFORM_3D);
    if let Operation::Transform3d(t) = &mut turn {
        t.placement.rotation = [0.0, 90.0, 0.0].into();
        t.placement.position = [100.0, 0.0, 0.0].into();
    }
    let turned = add(&mut doc, "turn", turn, &[("scene", merged)]);
    let s = evaluate(&doc, turned, at(0)).unwrap();
    // The light pointed along -z; turned 90 degrees about Y it points along -x.
    let d = s.lights[0].direction();
    assert!((d[0] + 1.0).abs() < 1e-5, "{d:?}");
    // The shape at the frame centre turns about it, then moves 100 px right.
    assert_eq!(
        math::point(&s.objects[0].world, [0.0; 3]).map(|v| v.round()),
        [1060.0, 540.0, 0.0]
    );
    // A bypassed transform passes its scene through; a bypassed shape vanishes.
    doc.nodes.get_mut(&turned).unwrap().bypass = true;
    doc.nodes.get_mut(&shape).unwrap().bypass = true;
    let s = evaluate(&doc, turned, at(0)).unwrap();
    assert_eq!((s.objects.len(), s.lights.len()), (0, 1));

    // Ports are typed: a scene port refuses an image, an image port refuses a scene,
    // and 3D text only extrudes a Text or Path.
    let mut bad = scene();
    let image = bad.output;
    let wrong = add(&mut bad, "wrong", op(&TRANSFORM_3D), &[("scene", image)]);
    bad.output = wrong;
    assert!(Document::new(bad).is_err());
    let mut bad = scene();
    let shape = add(&mut bad, "shape", op(&SHAPE_3D), &[]);
    let blur = add(&mut bad, "blur", op(&registry::BLUR), &[("image", shape)]);
    bad.output = blur;
    assert!(Document::new(bad).is_err());
    let mut bad = scene();
    let image = bad.output;
    let solid = add(&mut bad, "3d", op(&TEXT_3D), &[("outline", image)]);
    let render = add(&mut bad, "render", op(&RENDER_SCENE), &[("scene", solid)]);
    bad.output = render;
    assert!(Document::new(bad).is_err());
    let _ = default::<Placement>;
}

#[test]
fn placement_and_colour_are_keyable_through_commands() {
    let mut doc = scene();
    let shape = add(&mut doc, "shape", op(&SHAPE_3D), &[]);
    let render = add(&mut doc, "render", op(&RENDER_SCENE), &[("scene", shape)]);
    doc.output = render;
    let mut document = Document::new(doc).unwrap();
    let key = |track: &str, ticks, value| Command::SetTrackKey {
        id: shape,
        track: track.into(),
        time: at(ticks),
        value,
    };
    document
        .apply(
            0,
            &[
                key("placement.rotation", 0, [0.0; 3]),
                key("placement.rotation", 4, [0.0, 360.0, 0.0]),
                key("colour", 0, [1.0, 0.0, 0.0]),
            ],
        )
        .unwrap();
    let s = evaluate(document.snapshot(), shape, at(2)).unwrap();
    let x = math::direction(&s.objects[0].world, [1.0, 0.0, 0.0]);
    assert!((x[0] + 1.0).abs() < 1e-4, "half way round: {x:?}");
    assert_eq!(s.objects[0].surface.colour, [1.0, 0.0, 0.0]);
    assert!(
        document
            .apply(1, &[key("colour", 1, [2.0, 0.0, 0.0])])
            .is_err(),
        "colours stay 0-1"
    );
    let keyed: Vec<String> = document.snapshot().nodes[&shape]
        .operation
        .keyed_properties()
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    assert_eq!(keyed, ["placement.rotation", "colour"]);
}

#[test]
fn the_free_view_orbits_walks_and_turns_into_camera_keys() {
    use veylvik_scene::FreeView;
    let home = FreeView::home(W, H);
    let near = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 0.05 && (a[1] - b[1]).abs() < 0.05;
    // Home sees the frame exactly, like the home camera.
    let (corner, _) = home.camera().project([0.0, 0.0, 0.0], W, H).unwrap();
    assert!(near(corner, [0.0, 0.0]), "{corner:?}");
    // Orbiting keeps the target in the middle of the picture.
    let mut view = home;
    view.orbit(35.0, 20.0);
    let (centre, depth) = view.camera().project(view.target, W, H).unwrap();
    assert!(near(centre, [960.0, 540.0]) && (depth - view.distance).abs() < 0.5);
    // Panning by a tenth of the view moves the old target a tenth of the frame.
    let before = view.target;
    view.pan(0.1, 0.0);
    let (p, _) = view.camera().project(before, W, H).unwrap();
    assert!((p[0] - (960.0 + 192.0)).abs() < 0.5, "{p:?}");
    // Round trip through a camera.
    let back = FreeView::from_camera(&view.camera(), view.distance);
    for (a, b) in back.target.iter().zip(view.target) {
        assert!((a - b).abs() < 0.05);
    }
    // Walking forward moves the eye along the view without turning it.
    let eye = view.eye();
    view.walk(0.0, 0.0, 100.0, 0.0, 0.0);
    let moved = view.eye();
    let travelled =
        ((moved[0] - eye[0]).powi(2) + (moved[1] - eye[1]).powi(2) + (moved[2] - eye[2]).powi(2))
            .sqrt();
    assert!((travelled - 100.0).abs() < 0.01);
    // A Camera node given the view's placement films exactly the same picture.
    let (position, rotation) = view.placement(W, H);
    let mut doc = scene();
    let mut camera = op(&CAMERA_3D);
    if let Operation::Camera3d(c) = &mut camera {
        c.placement.position = position.into();
        c.placement.rotation = rotation.into();
    }
    let id = add(&mut doc, "camera", camera, &[]);
    let filmed = evaluate(&doc, id, at(0)).unwrap().camera(1, W, H);
    for p in [
        [0.0, 0.0, 0.0],
        [500.0, 300.0, -200.0],
        [1800.0, 900.0, 100.0],
    ] {
        let (a, b) = (view.camera().project(p, W, H), filmed.project(p, W, H));
        match (a, b) {
            (Some((a, _)), Some((b, _))) => assert!(near(a, b), "{a:?} vs {b:?}"),
            (a, b) => assert_eq!(a.is_some(), b.is_some()),
        }
    }
}

#[test]
fn the_view_aims_and_frames() {
    use veylvik_scene::FreeView;
    let mut view = FreeView::home(W, H);
    view.look_at([0.0, 0.0, 1000.0], [960.0, 540.0, 0.0]);
    let (p, _) = view.camera().project([960.0, 540.0, 0.0], W, H).unwrap();
    assert!(
        (p[0] - 960.0).abs() < 0.1 && (p[1] - 540.0).abs() < 0.1,
        "{p:?}"
    );
    view.frame([100.0, 100.0, 100.0], 300.0);
    // The sphere's edge stays inside the picture.
    let r = view.camera();
    let right = math::direction(&r.world, [1.0, 0.0, 0.0]);
    let edge = [
        100.0 + right[0] * 300.0,
        100.0 + right[1] * 300.0,
        100.0 + right[2] * 300.0,
    ];
    let (p, _) = r.project(edge, W, H).unwrap();
    assert!(p[0] > 960.0 && p[0] < 1920.0, "{p:?}");
}

#[test]
fn walked_paths_are_smoothed_unwrapped_and_thinned() {
    use veylvik_scene::smooth_walk;
    // A straight walk with shake, turning through 360 degrees.
    let samples: Vec<_> = (0..97)
        .map(|i| {
            let shake = if i % 2 == 0 { 6.0 } else { -6.0 };
            (
                Time { ticks: i, rate: 24 },
                [i as f32 * 10.0, shake, 0.0],
                [0.0, (i as f32 * 5.0).rem_euclid(360.0), 0.0],
            )
        })
        .collect();
    let out = smooth_walk(&samples, 9, 4);
    assert_eq!(out.len(), 25, "every fourth frame, last included");
    assert_eq!(out.first().unwrap().0.ticks, 0);
    assert_eq!(out.last().unwrap().0.ticks, 96);
    // Shake is gone in the middle; the ends stay put.
    assert!(
        out[5..20].iter().all(|p| p.1[1].abs() < 1.0),
        "{:?}",
        out[10].1
    );
    assert_eq!(out[0].1, samples[0].1);
    // Rotation keeps increasing past 360 instead of jumping back.
    assert!(out.windows(2).all(|w| w[1].2[1] >= w[0].2[1] - 1e-3));
    assert!((out.last().unwrap().2[1] - 480.0).abs() < 1e-3);
}

#[test]
fn linear_inverse_undoes_rotation_and_scale() {
    let m = math::mul(
        &math::rotate([20.0, -35.0, 50.0]),
        &math::scale([2.0, 0.5, 3.0]),
    );
    let inv = math::linear_inverse(&m).unwrap();
    let v = [3.0, -1.0, 2.0];
    let back = math::direction(&inv, math::direction(&m, v));
    for (a, b) in back.iter().zip(v) {
        assert!((a - b).abs() < 1e-4);
    }
    assert!(math::linear_inverse(&math::scale([1.0, 0.0, 1.0])).is_none());
}
#[test]
fn an_area_light_splits_into_a_repeatable_row_of_strips() {
    use veylvik_document::nodes::LightKind;
    use veylvik_scene::{Light, math, strips};
    let light = Light {
        node: veylvik_document::NodeId::nil(),
        kind: LightKind::Area,
        shadows: false,
        world: math::translate([100.0, 50.0, 400.0]),
        colour: [1.0; 3],
        intensity: 2.0,
        cone: 40.0,
        softness: 0.2,
        size: [1200.0, 20.0],
        aim: None,
    };
    // Even strips tile the panel exactly and keep its brightness.
    let even = strips(&light, 6, 0.0, 7);
    assert_eq!(even.len(), 6);
    for (i, s) in even.iter().enumerate() {
        assert!((s.size[0] - 200.0).abs() < 1e-3 && s.size[1] == 20.0 && s.intensity == 2.0);
        let x = s.position()[0];
        assert!((x - (100.0 - 500.0 + 200.0 * i as f32)).abs() < 1e-2, "{x}");
        assert!((s.position()[2] - 400.0).abs() < 1e-3);
    }
    // Uneven strips differ from each other, stay within the panel in order, with
    // gaps, and the same seed gives the same row while another gives a different one.
    let uneven = strips(&light, 6, 1.0, 7);
    let mut edge = -600.0 - 1e-2;
    let mut covered = 0.0;
    for s in &uneven {
        let x = s.position()[0] - 100.0;
        assert!(
            x - s.size[0] * 0.5 >= edge - 0.05,
            "strips keep their order"
        );
        edge = x + s.size[0] * 0.5;
        covered += s.size[0];
        assert!((0.5..=3.5).contains(&(s.intensity)) && (10.0..=30.0).contains(&s.size[1]));
    }
    assert!(edge <= 600.0 + 0.05 && covered < 1150.0, "gaps: {covered}");
    let widths = |row: &[Light]| row.iter().map(|s| s.size[0]).collect::<Vec<_>>();
    assert!(
        widths(&uneven)
            .windows(2)
            .any(|w| (w[0] - w[1]).abs() > 20.0)
    );
    assert_eq!(widths(&uneven), widths(&strips(&light, 6, 1.0, 7)));
    assert_ne!(widths(&uneven), widths(&strips(&light, 6, 1.0, 8)));
}
#[test]
fn cameras_and_lights_aim_at_an_object_wherever_they_are_placed() {
    let mut doc = scene();
    let mut cube = op(&SHAPE_3D);
    let Operation::Shape3d(shape) = &mut cube else {
        panic!()
    };
    shape.placement.position = [300.0, -120.0, -200.0].into();
    let cube = add(&mut doc, "cube", cube, &[]);
    let camera = |doc: &mut Composition, target: Option<NodeId>| {
        let mut op = op(&CAMERA_3D);
        let Operation::Camera3d(c) = &mut op else {
            panic!()
        };
        c.placement.position = [900.0, 400.0, -700.0].into();
        c.placement.rotation = [10.0, 20.0, 30.0].into();
        c.aim_at = target;
        add(doc, "camera", op, &[])
    };
    let aimed = camera(&mut doc, Some(cube));
    let lost = camera(&mut doc, Some(NodeId::new_v4()));
    let mut row = op(&LIGHT_3D);
    let Operation::Light3d(light) = &mut row else {
        panic!()
    };
    light.kind = LightKind::Area;
    light.strips = 4.0.into();
    light.placement.position = [-800.0, -600.0, 500.0].into();
    light.aim_at = Some(cube);
    let row = add(&mut doc, "strips", row, &[]);
    let merged = add(
        &mut doc,
        "merge",
        op(&MERGE_SCENES),
        &[
            ("scene", cube),
            ("scene_2", aimed),
            ("scene_3", lost),
            ("scene_4", row),
        ],
    );
    // A transform above everything must not break the aim.
    let mut turn = op(&TRANSFORM_3D);
    let Operation::Transform3d(t) = &mut turn else {
        panic!()
    };
    t.placement.rotation = [0.0, 35.0, 0.0].into();
    t.placement.scale = [1.5, 1.5, 1.5].into();
    let root = add(&mut doc, "turn", turn, &[("scene", merged)]);
    for root in [merged, root] {
        let s = evaluate(&doc, root, at(0)).unwrap();
        let target = s.objects[0].sphere().0;
        // The aimed camera has the cube dead centre, upright; the other keeps its rotation.
        let (p, _) = s.cameras[0].project(target, W, H).unwrap();
        assert!(close(p, [960.0, 540.0]), "{p:?}");
        let side = math::direction(&s.cameras[0].world, [1.0, 0.0, 0.0]);
        if root == merged {
            assert!(side[1].abs() < 1e-4, "no roll: {side:?}");
        }
        let missed = s.cameras[1].project(target, W, H).map(|(p, _)| p);
        assert!(missed.is_none_or(|p| !close(p, [960.0, 540.0])));
        // The row turns as one: its middle points at the cube and the strips stay in line.
        assert_eq!(s.lights.len(), 4);
        let middle =
            [0, 1, 2].map(|k| (s.lights[0].position()[k] + s.lights[3].position()[k]) * 0.5);
        let to = [0, 1, 2].map(|k| target[k] - middle[k]);
        let length = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
        let facing = s.lights[1].direction();
        let along: f32 = (0..3).map(|k| facing[k] * to[k] / length).sum();
        assert!(along > 0.995, "row faces the cube: {along}");
    }
    // Aiming at where it already stands changes nothing.
    let m = math::translate([1.0, 2.0, 3.0]);
    assert_eq!(math::aimed(&m, &m, [1.0, 2.0, 3.0]), m);
}
#[test]
fn a_3d_text_arrives_letter_by_letter() {
    let mut doc = scene();
    let mut words = op(&TEXT);
    let Operation::Text(text) = &mut words else {
        panic!()
    };
    text.content = "A BC".into();
    let words = add(&mut doc, "words", words, &[]);
    let mut solid = op(&TEXT_3D);
    let Operation::Text3d(t) = &mut solid else {
        panic!()
    };
    t.letters.motion = LetterMotion::Fly;
    t.letters.start = 1.0;
    t.letters.duration = 1.0;
    t.letters.stagger = 1.0;
    t.letters.distance = 500.0;
    t.letters.ease = false;
    let solid = add(&mut doc, "solid", solid, &[("outline", words)]);
    let count =
        |doc: &Composition, seconds: i64| evaluate(doc, solid, at(seconds)).unwrap().objects;
    // Before the start nothing is there; then one letter a second; spaces are no piece.
    assert_eq!(count(&doc, 1).len(), 0);
    assert_eq!(count(&doc, 2).len(), 1);
    assert_eq!(count(&doc, 3).len(), 2);
    let all = count(&doc, 9);
    assert_eq!(all.len(), 3);
    // Halfway in, the second letter is half the distance towards the viewer.
    let halfway = evaluate(&doc, solid, Time { ticks: 5, rate: 2 })
        .unwrap()
        .objects;
    assert_eq!(halfway.len(), 2);
    let z = |o: &veylvik_scene::Object| math::point(&o.world, [0.0; 3])[2];
    assert!((z(&halfway[1]) - z(&all[1]) - 250.0).abs() < 0.01);
    assert!((z(&halfway[0]) - z(&all[0])).abs() < 0.01);
    // Without the animation it is one solid, as before.
    let Operation::Text3d(t) = &mut doc.nodes.get_mut(&solid).unwrap().operation else {
        panic!()
    };
    t.letters.motion = LetterMotion::None;
    assert_eq!(count(&doc, 0).len(), 1);
}
