//! A Figure in a 3D scene: it stands where its settings put it, walks along as its distance grows, and particles emitted from it take its shape.
use std::sync::Arc;
use veylvik_document::{nodes::*, *};
use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer};

const W: u32 = 320;
const H: u32 = 180;
const TALL: f32 = 140.0;
fn op(kind: &registry::NodeType) -> Operation {
    serde_json::from_value(kind.default_operation()).unwrap()
}
fn add(doc: &mut Composition, op: Operation, inputs: &[(&str, NodeId)]) -> NodeId {
    let node = Node::new("n", op, inputs);
    let id = node.id;
    doc.nodes.insert(id, node);
    id
}
fn renderer() -> Renderer {
    let renderer = Renderer::new().unwrap();
    renderer.set_scene_renderer(Arc::new(veylvik_render3d::Engine::new(renderer.device())));
    renderer
}
/// A figure (walked `distance` pixels) with a light; the scene, the figure's id and the document.
fn scene(distance: f32) -> (Composition, NodeId, NodeId) {
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let mut figure = op(&FIGURE_3D);
    if let Operation::Figure3d(f) = &mut figure {
        f.height = TALL.into();
        f.distance = distance.into();
        f.detail = 60.0.into();
    }
    let figure = add(&mut doc, figure, &[]);
    let mut light = op(&LIGHT_3D);
    if let Operation::Light3d(l) = &mut light {
        l.placement.position = [-200.0, -200.0, 400.0].into();
    }
    let light = add(&mut doc, light, &[]);
    let merged = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", figure), ("scene_2", light)],
    );
    (doc, figure, merged)
}
fn render(renderer: &Renderer, doc: &Composition) -> Vec<[f32; 4]> {
    renderer
        .render_frame(
            &Plan::compile(doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap()
        .pixels
}
/// The box round what is drawn: left, top, right, bottom.
fn bounds(pixels: &[[f32; 4]]) -> [u32; 4] {
    let mut b = [W, H, 0, 0];
    for (i, p) in pixels.iter().enumerate() {
        if p[3] > 0.2 {
            let (x, y) = (i as u32 % W, i as u32 / W);
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        }
    }
    b
}
#[test]
#[ignore = "requires Vulkan"]
fn a_figure_stands_where_it_is_put_walks_along_and_particles_take_its_shape() {
    let renderer = renderer();
    let filmed = |distance: f32| {
        let (mut doc, _, merged) = scene(distance);
        doc.output = add(&mut doc, op(&RENDER_SCENE), &[("scene", merged)]);
        render(&renderer, &doc)
    };
    let here = filmed(0.0);
    let [l, t, r, b] = bounds(&here);
    // Head to foot about its height, standing on the frame's middle plus half of it.
    let tall = (b - t) as f32;
    assert!(
        (tall - TALL).abs() < 0.12 * TALL,
        "{tall} tall, box {:?}",
        [l, t, r, b]
    );
    assert!(
        (b as f32 - (H as f32 * 0.5 + TALL * 0.5)).abs() < 6.0,
        "feet on the ground line: {b}"
    );
    assert!(
        ((r - l) as f32) < TALL * 0.6,
        "a person, not a block: {} wide",
        r - l
    );
    // Lit: brighter on the side the light comes from than in shadow.
    assert!(here.iter().any(|p| p[3] > 0.9 && p[0] > 0.3), "lit");
    // Walked on by half its height, the body is that much further along.
    let on = bounds(&filmed(TALL * 0.5));
    let moved = (on[0] + on[2]) as f32 * 0.5 - (l + r) as f32 * 0.5;
    assert!((moved - TALL * 0.5).abs() < 0.15 * TALL, "moved {moved}");
    // Particles born all over it make a body of the same size in the same place.
    let (mut doc, figure, merged) = scene(0.0);
    let mut particles = op(&PARTICLES);
    if let Operation::Particles(p) = &mut particles {
        p.emit_from = Some(figure);
        p.rate = 200_000.0.into();
        p.life = 0.1.into();
        p.start = (-0.1).into();
        p.speed = 0.0.into();
        p.gravity = 0.0.into();
        p.size = 0.8.into();
        p.fade = 0.0.into();
    }
    doc.output = add(&mut doc, particles, &[("scene", merged)]);
    let dust = bounds(&render(&renderer, &doc));
    for k in 0..4 {
        assert!(
            (dust[k] as i32 - [l, t, r, b][k] as i32).abs() <= 6,
            "particles {dust:?} against the body {:?}",
            [l, t, r, b]
        );
    }
}
