//! An area light with Feather fades towards its panel's edges, so its reflection in a glossy surface is soft-edged; without it the panel reflects evenly to its edge.
use std::sync::Arc;
use veylvik_document::{nodes::*, *};
use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer};

const W: u32 = 480;
const H: u32 = 270;
fn op(kind: &registry::NodeType) -> Operation {
    serde_json::from_value(kind.default_operation()).unwrap()
}
fn add(doc: &mut Composition, op: Operation, inputs: &[(&str, NodeId)]) -> NodeId {
    let node = Node::new("n", op, inputs);
    let id = node.id;
    doc.nodes.insert(id, node);
    id
}
/// The middle row's red across a glossy dark wall that mirrors a square panel in front
/// of it, the panel feathered by `feather`.
fn row(renderer: &Renderer, feather: f32) -> Vec<f32> {
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let mut wall = op(&SHAPE_3D);
    if let Operation::Shape3d(p) = &mut wall {
        p.kind = ShapeKind::Plane;
        p.size_x = 4000.0.into();
        p.size_y = 4000.0.into();
        p.colour = [0.02, 0.02, 0.02].into();
        p.roughness = 0.08.into();
    }
    let wall = add(&mut doc, wall, &[]);
    let mut panel = op(&LIGHT_3D);
    if let Operation::Light3d(l) = &mut panel {
        l.kind = LightKind::Area;
        l.shadows = false;
        l.width = 300.0.into();
        l.height = 300.0.into();
        l.intensity = 12.0.into();
        l.feather = feather.into();
        l.placement.position = [0.0, 0.0, 300.0].into();
    }
    let panel = add(&mut doc, panel, &[]);
    let mut dark = op(&ENVIRONMENT_3D);
    if let Operation::Environment3d(e) = &mut dark {
        e.intensity = 0.0.into();
    }
    let dark = add(&mut doc, dark, &[]);
    let merged = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", wall), ("scene_2", panel), ("scene_3", dark)],
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
    (0..W)
        .map(|x| pixels[(H / 2 * W + x) as usize][0])
        .collect()
}
#[test]
#[ignore = "requires Vulkan"]
fn a_feathered_panel_reflects_with_soft_edges() {
    let renderer = Renderer::new().unwrap();
    renderer.set_scene_renderer(Arc::new(veylvik_render3d::Engine::new(renderer.device())));
    let hard = row(&renderer, 0.0);
    let soft = row(&renderer, 1.0);
    let middle = (W / 2) as usize;
    assert!(hard[middle] > 0.05, "the panel shows: {}", hard[middle]);
    assert!(
        (soft[middle] / hard[middle] - 1.0).abs() < 0.1,
        "same in the middle: {} {}",
        soft[middle],
        hard[middle]
    );
    // How far the even reflection reaches, and how bright each is most of the way out.
    let reach = (middle..W as usize)
        .take_while(|&x| hard[x] > 0.5 * hard[middle])
        .count();
    assert!(reach > 10, "{reach}");
    let out = middle + reach * 7 / 10;
    assert!(
        hard[out] > 0.8 * hard[middle],
        "even to its edge: {} of {}",
        hard[out],
        hard[middle]
    );
    assert!(
        soft[out] < 0.5 * soft[middle],
        "fading towards it: {} of {}",
        soft[out],
        soft[middle]
    );
}
