//! A surface with Emission shows its colour by its own light: in a scene with no light at all it is its colour times the emission, and unlit it is black; a library look shades the surface instead.
use std::sync::Arc;
use veylvik_document::{nodes::*, *};
use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer};

fn op(kind: &registry::NodeType) -> Operation {
    serde_json::from_value(kind.default_operation()).unwrap()
}
fn add(doc: &mut Composition, op: Operation, inputs: &[(&str, NodeId)]) -> NodeId {
    let node = Node::new("n", op, inputs);
    let id = node.id;
    doc.nodes.insert(id, node);
    id
}
/// The middle pixel of a plane facing the camera, in a scene whose only light is a dark
/// environment, with the plane's emission at `glow`; plain or with a library look.
fn middle(renderer: &Renderer, glow: f32, with_look: bool) -> [f32; 4] {
    let mut doc = blank(320, 180, Time::new(24, 1).unwrap()).unwrap();
    let mut plane = op(&SHAPE_3D);
    if let Operation::Shape3d(p) = &mut plane {
        p.kind = ShapeKind::Plane;
        p.size_x = 600.0.into();
        p.size_y = 600.0.into();
        p.colour = [0.2, 0.5, 0.4].into();
        p.emission = glow.into();
        if with_look {
            let Operation::Material(m) = op(&MATERIAL) else {
                panic!("a material")
            };
            p.look = Some(Box::new(m));
        }
    }
    let plane = add(&mut doc, plane, &[]);
    let mut dark = op(&ENVIRONMENT_3D);
    if let Operation::Environment3d(e) = &mut dark {
        e.intensity = 0.0.into();
    }
    let dark = add(&mut doc, dark, &[]);
    let merged = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", plane), ("scene_2", dark)],
    );
    let mut render = op(&RENDER_SCENE);
    if let Operation::RenderScene(r) = &mut render {
        r.occlusion = 0.0.into();
    }
    doc.output = add(&mut doc, render, &[("scene", merged)]);
    let pixels = renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap()
        .pixels;
    pixels[90 * 320 + 160]
}
#[test]
#[ignore = "requires Vulkan"]
fn emission_shows_the_colour_without_any_light() {
    let renderer = Renderer::new().unwrap();
    renderer.set_scene_renderer(Arc::new(veylvik_render3d::Engine::new(renderer.device())));
    let dark = middle(&renderer, 0.0, false);
    assert!(dark[1] < 0.01, "no light, no emission: {dark:?}");
    let lit = middle(&renderer, 2.0, false);
    for (k, want) in [0.4, 1.0, 0.8].into_iter().enumerate() {
        assert!((lit[k] - want).abs() < 0.03, "channel {k}: {lit:?}");
    }
    // A library look shades the surface itself: Emission is not used with one.
    assert!(middle(&renderer, 2.0, true)[1] < 0.01);
}
