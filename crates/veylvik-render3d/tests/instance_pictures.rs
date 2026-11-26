//! Copies of 3D objects at particles that are born on an emitter picture appear only where the picture is, and those flying to a target picture end on it.
use std::sync::Arc;
use veylvik_document::{nodes::*, *};
use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer};

const W: u32 = 320;
const H: u32 = 180;
fn op(kind: &registry::NodeType) -> Operation {
    serde_json::from_value(kind.default_operation()).unwrap()
}
fn add(doc: &mut Composition, op: Operation, inputs: &[(&str, NodeId)]) -> NodeId {
    let node = Node::new("n", op, inputs);
    let id = node.id;
    doc.nodes.insert(id, node);
    id
}
fn rectangle(doc: &mut Composition, x: f32, y: f32) -> NodeId {
    let mut r = op(&RECTANGLE);
    if let Operation::Rectangle(r) = &mut r {
        r.x = x.into();
        r.y = y.into();
        r.width = 60.0.into();
        r.height = 50.0.into();
        r.color = [1.0, 1.0, 1.0, 1.0];
    }
    add(doc, r, &[])
}
fn render(renderer: &Renderer, doc: &Composition, seconds: (i64, u32)) -> Vec<[f32; 4]> {
    renderer
        .render_frame(
            &Plan::compile(doc).unwrap(),
            RenderRequest::full(Time::new(seconds.0, seconds.1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap()
        .pixels
}
/// The box round what is drawn (left, top, right, bottom) and how many pixels.
fn bounds(pixels: &[[f32; 4]]) -> ([i32; 4], usize) {
    let mut b = [W as i32, H as i32, -1, -1];
    let mut n = 0;
    for (i, p) in pixels.iter().enumerate() {
        if p[3] > 0.3 {
            let (x, y) = ((i as u32 % W) as i32, (i as u32 / W) as i32);
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
            n += 1;
        }
    }
    (b, n)
}
/// Copies of a small cube at `particles` (with its pictures wired by `wire`), filmed.
fn copies(
    renderer: &Renderer,
    seconds: (i64, u32),
    make: impl Fn(&mut Composition, &mut Particles) -> Vec<(&'static str, NodeId)>,
) -> Vec<[f32; 4]> {
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let Operation::Particles(mut p) = op(&PARTICLES) else {
        unreachable!()
    };
    p.speed = 0.0.into();
    p.gravity = 0.0.into();
    p.variation = 0.0.into();
    let wires = make(&mut doc, &mut p);
    let particles = add(&mut doc, Operation::Particles(p), &wires);
    let mut cube = op(&SHAPE_3D);
    if let Operation::Shape3d(c) = &mut cube {
        c.size_x = 3.0.into();
        c.size_y = 3.0.into();
        c.size_z = 3.0.into();
        c.emission = 1.0.into();
    }
    let cube = add(&mut doc, cube, &[]);
    let copies = add(
        &mut doc,
        op(&PARTICLE_INSTANCES),
        &[("particles", particles), ("scene", cube)],
    );
    let mut render = op(&RENDER_SCENE);
    if let Operation::RenderScene(r) = &mut render {
        r.occlusion = 0.0.into();
    }
    doc.output = add(&mut doc, render, &[("scene", copies)]);
    self::render(renderer, &doc, seconds)
}
#[test]
#[ignore = "requires Vulkan"]
fn copies_are_born_on_an_emitter_and_land_on_a_target() {
    let renderer = Renderer::new().unwrap();
    renderer.set_scene_renderer(Arc::new(veylvik_render3d::Engine::new(renderer.device())));
    // Where each picture is, from the picture itself.
    let place = |x: f32, y: f32| {
        let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
        doc.output = rectangle(&mut doc, x, y);
        bounds(&render(&renderer, &doc, (0, 1))).0
    };
    let left = place(80.0, 90.0);
    let right = place(240.0, 90.0);
    // Within a few pixels: a copy is a 3-pixel cube, and near the end of a flight a copy
    // is not quite there yet.
    let inside = |b: [i32; 4], of: [i32; 4]| {
        b[0] >= of[0] - 6 && b[1] >= of[1] - 6 && b[2] <= of[2] + 6 && b[3] <= of[3] + 6
    };
    // Born on the emitter: copies only over the rectangle on the left, and plenty of them.
    let (born, n) = bounds(&copies(&renderer, (1, 1), |doc, p| {
        p.density = 4000.0.into();
        p.life = 2.0.into();
        p.start = (-2.0).into();
        vec![("emitter", rectangle(doc, 80.0, 90.0))]
    }));
    assert!(n > 200, "copies on the emitter: {n}");
    assert!(
        inside(born, left),
        "copies {born:?} on the emitter {left:?}"
    );
    // Flying to the target: born in a box on the left, at the end of their life on the right.
    let (landed, n) = bounds(&copies(&renderer, (23, 24), |doc, p| {
        p.rate = 2000.0.into();
        p.start = 0.0.into();
        p.duration = 0.05.into();
        p.life = 1.0.into();
        p.homing = 1.0.into();
        p.x = 40.0.into();
        p.y = 90.0.into();
        p.width = 20.0.into();
        p.height = 20.0.into();
        vec![("target", rectangle(doc, 240.0, 90.0))]
    }));
    assert!(n > 50, "copies at the target: {n}");
    assert!(
        inside(landed, right),
        "copies {landed:?} at the target {right:?}"
    );
}
