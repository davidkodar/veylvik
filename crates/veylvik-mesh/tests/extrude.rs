use std::collections::HashMap;
use veylvik_document::vector::{FillRule, PathCommand};
use veylvik_mesh::{Mesh, extrude};

fn polygon(points: &[[f32; 2]]) -> Vec<PathCommand> {
    let mut commands = vec![PathCommand::Move { to: points[0] }];
    commands.extend(points[1..].iter().map(|&to| PathCommand::Line { to }));
    commands.push(PathCommand::Close {});
    commands
}
fn square(x: f32, y: f32, size: f32, clockwise: bool) -> Vec<PathCommand> {
    let mut points = vec![[x, y], [x + size, y], [x + size, y + size], [x, y + size]];
    if !clockwise {
        points.reverse();
    }
    polygon(&points)
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
/// Signed volume, and whether every triangle faces along its vertex normals.
fn check(mesh: &Mesh) -> (f32, bool) {
    let mut volume = 0.0;
    let mut facing = Vec::new();
    for t in mesh.indices.as_chunks::<3>().0 {
        let [a, b, c] = t.map(|i| mesh.vertices[i as usize]);
        volume += dot(a.position, cross(b.position, c.position)) / 6.0;
        let n = cross(sub(b.position, a.position), sub(c.position, a.position));
        if dot(n, n) > 1e-8 {
            facing.push(dot(n, a.normal) > 0.0);
        }
    }
    let consistent = facing.iter().all(|f| *f == facing[0]);
    (volume, consistent)
}
/// Every edge (by position) is shared by exactly two triangles.
fn watertight(mesh: &Mesh) -> bool {
    let key = |i: u32| {
        let p = mesh.vertices[i as usize].position;
        p.map(|v| (v * 1000.0).round() as i64)
    };
    let mut edges: HashMap<([i64; 3], [i64; 3]), i32> = HashMap::new();
    for t in mesh.indices.as_chunks::<3>().0 {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (a, b) = (key(a), key(b));
            *edges
                .entry(if a < b { (a, b) } else { (b, a) })
                .or_default() += 1;
        }
    }
    edges.values().all(|&n| n == 2)
}

#[test]
fn a_square_becomes_a_closed_box_with_outward_normals() {
    for clockwise in [true, false] {
        let mesh = extrude(
            &square(0.0, 0.0, 10.0, clockwise),
            FillRule::NonZero,
            4.0,
            0.1,
        )
        .unwrap();
        assert!(watertight(&mesh));
        let (volume, consistent) = check(&mesh);
        assert!(consistent, "triangles face their normals");
        assert!((volume.abs() - 400.0).abs() < 1e-2, "{volume}");
        // Side walls face away from the solid.
        for v in &mesh.vertices[mesh.vertices.len() - 16..] {
            let outward = [v.position[0] - 5.0, v.position[1] - 5.0];
            assert!(v.normal[0] * outward[0] + v.normal[1] * outward[1] > 0.0);
            assert_eq!(v.normal[2], 0.0);
        }
    }
}

#[test]
fn holes_get_walls_facing_into_the_hole_under_either_fill_rule() {
    for (rule, hole_clockwise) in [(FillRule::NonZero, false), (FillRule::EvenOdd, true)] {
        let mut ring = square(0.0, 0.0, 30.0, true);
        ring.extend(square(10.0, 10.0, 10.0, hole_clockwise));
        let mesh = extrude(&ring, rule, 2.0, 0.1).unwrap();
        assert!(watertight(&mesh));
        let (volume, consistent) = check(&mesh);
        assert!(consistent);
        assert!(
            (volume.abs() - (900.0 - 100.0) * 2.0).abs() < 1e-1,
            "{volume}"
        );
        // A wall vertex on the hole's left edge faces towards the hole's centre.
        let inner = mesh.vertices[mesh.cap_indices..]
            .iter()
            .chain(&mesh.vertices[..])
            .find(|v| v.normal[2] == 0.0 && v.position[0] == 10.0 && v.position[1] == 15.0);
        if let Some(v) = inner {
            assert!(v.normal[0] > 0.0);
        }
    }
}

#[test]
fn curves_get_smooth_walls_and_corners_stay_sharp() {
    // A circle from four cubic arcs.
    let k = 0.552_284_8 * 20.0;
    let circle = vec![
        PathCommand::Move { to: [20.0, 0.0] },
        PathCommand::Cubic {
            control1: [20.0, k],
            control2: [k, 20.0],
            to: [0.0, 20.0],
        },
        PathCommand::Cubic {
            control1: [-k, 20.0],
            control2: [-20.0, k],
            to: [-20.0, 0.0],
        },
        PathCommand::Cubic {
            control1: [-20.0, -k],
            control2: [-k, -20.0],
            to: [0.0, -20.0],
        },
        PathCommand::Cubic {
            control1: [k, -20.0],
            control2: [20.0, -k],
            to: [20.0, 0.0],
        },
        PathCommand::Close {},
    ];
    let mesh = extrude(&circle, FillRule::NonZero, 5.0, 0.05).unwrap();
    assert!(watertight(&mesh));
    let (volume, consistent) = check(&mesh);
    assert!(consistent);
    let area = std::f32::consts::PI * 400.0;
    assert!((volume.abs() / 5.0 - area).abs() / area < 0.01);
    // Smooth: a wall normal is radial, not the facet's.
    for v in mesh.vertices.iter().filter(|v| v.normal[2] == 0.0) {
        let r = (v.position[0].powi(2) + v.position[1].powi(2)).sqrt();
        let radial = [v.position[0] / r, v.position[1] / r];
        assert!(v.normal[0] * radial[0] + v.normal[1] * radial[1] > 0.995);
    }
    // The square's corners keep their own face normals.
    let boxy = extrude(&square(0.0, 0.0, 10.0, true), FillRule::NonZero, 4.0, 0.1).unwrap();
    assert!(
        boxy.vertices
            .iter()
            .filter(|v| v.normal[2] == 0.0)
            .all(|v| v.normal[0].abs() == 1.0 || v.normal[1].abs() == 1.0)
    );
}

#[test]
fn real_text_extrudes_and_bad_input_is_refused() {
    let mut text: veylvik_document::Operation =
        serde_json::from_value(veylvik_document::registry::TEXT.default_operation()).unwrap();
    let veylvik_document::Operation::Text(t) = &mut text else {
        panic!()
    };
    t.content = "Ag8".into();
    let outline = veylvik_text::outline(&text, veylvik_document::Time::new(0, 1).unwrap()).unwrap();
    let mesh = extrude(&outline.commands, outline.fill_rule, 12.0, 0.25).unwrap();
    assert!(mesh.indices.len() > 300 && mesh.cap_indices < mesh.indices.len());
    let (volume, consistent) = check(&mesh);
    assert!(consistent && volume.abs() > 1.0);
    assert!(
        mesh.vertices
            .iter()
            .all(|v| v.position.iter().chain(&v.normal).all(|x| x.is_finite()))
    );
    // Flat: caps only. Bad depth or tolerance: refused.
    let flat = extrude(&outline.commands, outline.fill_rule, 0.0, 0.25).unwrap();
    assert_eq!(flat.cap_indices, flat.indices.len());
    assert!(extrude(&outline.commands, outline.fill_rule, f32::NAN, 0.25).is_err());
    assert!(extrude(&outline.commands, outline.fill_rule, 1.0, 0.0).is_err());
    assert!(
        extrude(
            &[PathCommand::Line { to: [1.0, 1.0] }],
            FillRule::NonZero,
            1.0,
            0.1
        )
        .is_err()
    );
}

#[test]
fn primitives_are_closed_solids_of_the_expected_volume() {
    use veylvik_mesh::{Primitive, primitive};
    let pi = std::f32::consts::PI;
    for (kind, size, expected) in [
        (Primitive::Cube, [10.0, 20.0, 30.0], 6000.0),
        (
            Primitive::Sphere,
            [20.0, 20.0, 20.0],
            4.0 / 3.0 * pi * 1000.0,
        ),
        (Primitive::Cylinder, [20.0, 10.0, 20.0], pi * 100.0 * 10.0),
        (Primitive::Cone, [20.0, 10.0, 20.0], pi * 100.0 * 10.0 / 3.0),
        (
            Primitive::Torus,
            [40.0, 40.0, 10.0],
            2.0 * pi * pi * 15.0 * 25.0,
        ),
    ] {
        let mesh = primitive(kind, size, 128).unwrap();
        let (volume, consistent) = check(&mesh);
        assert!(consistent, "{kind:?} faces its normals");
        assert!(
            (volume.abs() - expected).abs() / expected < 0.01,
            "{kind:?}: {volume} vs {expected}"
        );
    }
    let plane = primitive(Primitive::Plane, [10.0, 10.0, 0.0], 3).unwrap();
    assert_eq!((plane.vertices.len(), plane.indices.len()), (4, 6));
    assert!(plane.vertices.iter().all(|v| v.normal == [0.0, 0.0, 1.0]));
    assert!(primitive(Primitive::Cube, [f32::NAN, 1.0, 1.0], 3).is_err());
    // Segments are clamped, so huge requests stay bounded.
    assert!(
        primitive(Primitive::Sphere, [1.0; 3], 1_000_000)
            .unwrap()
            .indices
            .len()
            < 300_000
    );
}
