//! A scene lit by a point or a spot light is as bright at any frame size: the light falls off over pixels, so its strength follows the frame.
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
/// A grey card filling the frame, lit by one light of `kind` 300 pixels (of a 1920-wide
/// frame) in front of it, filmed in a frame `width` wide; the middle pixel's red.
fn lit(renderer: &Renderer, width: u32, kind: LightKind) -> f32 {
    let s = width as f32 / 1920.0;
    let mut doc = blank(width, width * 9 / 16, Time::new(24, 1).unwrap()).unwrap();
    let mut card = op(&SHAPE_3D);
    if let Operation::Shape3d(c) = &mut card {
        c.kind = ShapeKind::Plane;
        c.size_x = (4000.0 * s).into();
        c.size_y = (4000.0 * s).into();
        c.colour = [0.5, 0.5, 0.5].into();
        c.roughness = 1.0.into();
    }
    let card = add(&mut doc, card, &[]);
    let mut light = op(&LIGHT_3D);
    if let Operation::Light3d(l) = &mut light {
        l.kind = kind;
        l.intensity = 0.5.into();
        l.placement.position = [0.0, 0.0, 300.0 * s].into();
    }
    let light = add(&mut doc, light, &[]);
    let merged = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", card), ("scene_2", light)],
    );
    let mut render = op(&RENDER_SCENE);
    if let Operation::RenderScene(r) = &mut render {
        r.occlusion = 0.0.into();
    }
    doc.output = add(&mut doc, render, &[("scene", merged)]);
    let height = width * 9 / 16;
    let pixels = renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap()
        .pixels;
    pixels[(height / 2 * width + width / 2) as usize][0]
}
#[test]
#[ignore = "requires Vulkan"]
fn a_point_or_spot_light_is_as_bright_at_any_frame_size() {
    let renderer = Renderer::new().unwrap();
    renderer.set_scene_renderer(Arc::new(veylvik_render3d::Engine::new(renderer.device())));
    for kind in [LightKind::Point, LightKind::Spot] {
        let small = lit(&renderer, 480, kind);
        let large = lit(&renderer, 1920, kind);
        assert!(small > 0.02, "{kind:?} lights the card: {small}");
        assert!(
            (small / large - 1.0).abs() < 0.03,
            "{kind:?}: {small} at 480 wide, {large} at 1920"
        );
    }
}
