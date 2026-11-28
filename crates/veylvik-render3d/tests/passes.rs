//! Render scene's render passes: Cryptomatte objects named by their labels with exact shares of each pixel, normals and positions of the surfaces met, and motion in pixels to the next frame and from the last.
use std::sync::Arc;
use veylvik_document::track::{Curve3, Curve3Mode, Key3};
use veylvik_document::{nodes::*, *};
use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer, cryptomatte};

const W: u32 = 160;
const H: u32 = 90;
fn op(kind: &registry::NodeType) -> Operation {
    serde_json::from_value(kind.default_operation()).unwrap()
}
fn add(doc: &mut Composition, label: &str, op: Operation, inputs: &[(&str, NodeId)]) -> NodeId {
    let node = Node::new(label, op, inputs);
    let id = node.id;
    doc.nodes.insert(id, node);
    id
}
fn renderer() -> Renderer {
    let renderer = Renderer::new().unwrap();
    renderer.set_scene_renderer(Arc::new(veylvik_render3d::Engine::new(renderer.device())));
    renderer
}
fn sphere(position: Track3) -> Operation {
    let mut o = op(&SHAPE_3D);
    if let Operation::Shape3d(s) = &mut o {
        s.kind = ShapeKind::Sphere;
        for size in [&mut s.size_x, &mut s.size_y, &mut s.size_z] {
            *size = 50.0.into();
        }
        s.detail = 48.0.into();
        s.placement.position = position;
    }
    o
}
/// A ball on the left gliding 80 units to the right over the second, a still ball on the
/// right, filmed from the home view: the pass asked for, at frame 12.
fn balls(pass: ScenePass) -> Vec<[f32; 4]> {
    let renderer = renderer();
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let gliding = Track3::Animated(Curve3 {
        interpolation: Curve3Mode::Linear,
        keys: vec![
            Key3 {
                time: Time::new(0, 1).unwrap(),
                value: [-80.0, 0.0, 0.0],
            },
            Key3 {
                time: Time::new(1, 1).unwrap(),
                value: [0.0, 0.0, 0.0],
            },
        ],
    });
    let left = add(&mut doc, "Left ball", sphere(gliding), &[]);
    let right = add(&mut doc, "Right ball", sphere([40.0, 0.0, 0.0].into()), &[]);
    let scene = add(
        &mut doc,
        "Scene",
        op(&MERGE_SCENES),
        &[("scene", left), ("scene_2", right)],
    );
    let mut render = op(&RENDER_SCENE);
    if let Operation::RenderScene(r) = &mut render {
        r.camera = 0.0.into();
        r.pass = pass;
    }
    doc.output = add(&mut doc, "Render", render, &[("scene", scene)]);
    renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(12, 24).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap()
        .pixels
}
fn at(pixels: &[[f32; 4]], x: u32, y: u32) -> [f32; 4] {
    pixels[(y * W + x) as usize]
}
#[test]
#[ignore = "requires Vulkan"]
fn objects_are_named_by_their_labels_with_their_share_of_each_pixel() {
    let pixels = balls(ScenePass::Objects);
    let (left, right) = (cryptomatte::id("Left ball"), cryptomatte::id("Right ball"));
    // The left ball is at x -40 at frame 12: its middle is pixel 40, the right's 120.
    let inside = at(&pixels, 40, 45);
    assert_eq!((inside[0].to_bits(), inside[1]), (left, 1.0), "{inside:?}");
    assert_eq!(inside[3], 0.0, "nothing else covers its middle: {inside:?}");
    let other = at(&pixels, 120, 45);
    assert_eq!((other[0].to_bits(), other[1]), (right, 1.0), "{other:?}");
    assert_eq!(at(&pixels, 80, 5), [0.0; 4], "the sky is no object");
    // Along the rim a pixel is shared with nothing: shares are sixteenths below one.
    let mut edges = 0;
    for p in &pixels {
        let share = p[1];
        if share > 0.0 && share < 1.0 {
            edges += 1;
            assert_eq!((share * 16.0).fract(), 0.0, "{p:?}");
            assert!([left, right].contains(&p[0].to_bits()), "{p:?}");
        }
    }
    assert!(
        edges > 40,
        "the balls' rims are partly covered pixels: {edges}"
    );
    let more = balls(ScenePass::ObjectsMore);
    assert!(
        more.iter().all(|p| p[1] == 0.0),
        "no pixel has a third object"
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn normals_face_the_viewer_at_a_balls_middle_and_up_at_its_top() {
    let pixels = balls(ScenePass::Normals);
    let points = balls(ScenePass::Position);
    // The scene's frame starts at the picture's top left: the right ball's middle is
    // at (120, 45, 0). A ball's normal points from its middle through the surface.
    for (x, y) in [(120, 45), (110, 40), (128, 52)] {
        let (n, p) = (at(&pixels, x, y), at(&points, x, y));
        let out = [p[0] - 120.0, p[1] - 45.0, p[2]];
        let length = (out[0] * out[0] + out[1] * out[1] + out[2] * out[2]).sqrt();
        let along = (n[0] * out[0] + n[1] * out[1] + n[2] * out[2]) / length;
        assert!(along > 0.995 && n[3] == 1.0, "{x},{y}: {n:?} {p:?}");
        let unit = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!((unit - 1.0).abs() < 1e-3, "{n:?}");
    }
    // Near the top of the ball (y down: up is -y) the surface faces up.
    let top = (0..45)
        .map(|y| at(&pixels, 120, y))
        .find(|p| p[3] > 0.0)
        .unwrap();
    assert!(top[1] < -0.7, "{top:?}");
}
#[test]
#[ignore = "requires Vulkan"]
fn positions_are_where_the_surface_is_in_the_scene() {
    let pixels = balls(ScenePass::Position);
    for (x, y) in [(120, 45), (105, 45), (120, 30), (40, 45)] {
        let p = at(&pixels, x, y);
        // On the ball's surface (48 facets round): 25 from its middle (the left one is at x 40 at frame 12).
        let middle = if x < 80 { 40.0 } else { 120.0 };
        let d = ((p[0] - middle).powi(2) + (p[1] - 45.0).powi(2) + p[2].powi(2)).sqrt();
        assert!(
            (d - 25.0).abs() < 0.2 && p[2] > 0.0 && p[3] == 1.0,
            "{x},{y}: {p:?}"
        );
    }
    assert_eq!(at(&pixels, 80, 5), [0.0; 4]);
}
#[test]
#[ignore = "requires Vulkan"]
fn motion_is_the_pixels_a_surface_moves_to_the_next_frame_and_from_the_last() {
    let pixels = balls(ScenePass::Motion);
    // 80 units a second at 24 frames a second, a little more on screen at its front.
    let step = 80.0 / 24.0;
    let gliding = at(&pixels, 40, 45);
    assert!(
        gliding[0] > step && gliding[0] < step * 1.2,
        "forward {gliding:?}"
    );
    assert!(
        (gliding[2] + gliding[0]).abs() < 0.05,
        "back as far: {gliding:?}"
    );
    assert!(
        gliding[1].abs() < 0.05 && gliding[3].abs() < 0.05,
        "{gliding:?}"
    );
    let still = at(&pixels, 120, 45);
    assert!(still.iter().all(|v| v.abs() < 1e-3), "{still:?}");
}
