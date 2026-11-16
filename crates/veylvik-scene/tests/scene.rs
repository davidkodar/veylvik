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
