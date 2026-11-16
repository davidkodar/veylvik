use std::sync::Arc;
use veylvik_document::{nodes::*, *};
use veylvik_render::{Image, ImageInputs, Plan, RenderRequest, Renderer};

const W: u32 = 160;
const H: u32 = 90;
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
fn render(renderer: &Renderer, doc: &Composition, inputs: &ImageInputs) -> Vec<[f32; 4]> {
    renderer
        .render_frame(
            &Plan::compile(doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            inputs,
        )
        .unwrap()
        .pixels
}
fn gradient() -> (Composition, ImageInputs, Vec<[f32; 4]>, NodeId) {
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let pixels: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            let (x, y) = ((i % W) as f32 / W as f32, (i / W) as f32 / H as f32);
            let a = if (i % W) < 20 { 0.5 } else { 1.0 };
            [x * a, y * a, 0.3 * a, a]
        })
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "picture",
        Arc::new(Image::new(W, H, pixels.clone()).unwrap()),
    );
    let image = add(
        &mut doc,
        Operation::Image(nodes::Image {
            asset: "picture".into(),
        }),
        &[],
    );
    (doc, inputs, pixels, image)
}
#[test]
#[ignore = "requires Vulkan"]
fn an_untouched_card_renders_exactly_like_its_image() {
    let renderer = renderer();
    let (mut doc, inputs, pixels, image) = gradient();
    let card = add(&mut doc, op(&IMAGE_CARD), &[("image", image)]);
    let render_node = add(&mut doc, op(&RENDER_SCENE), &[("scene", card)]);
    doc.output = render_node;
    let out = render(&renderer, &doc, &inputs);
    let worst = out
        .iter()
        .zip(&pixels)
        .map(|(a, b)| (0..4).map(|c| (a[c] - b[c]).abs()).fold(0.0, f32::max))
        .fold(0.0, f32::max);
    // Half-float rendering keeps about three significant digits.
    assert!(worst < 2e-3, "worst channel difference {worst}");
    // Pulling the camera back by the home distance halves the card.
    let camera = add(&mut doc, op(&CAMERA_3D), &[]);
    if let Operation::Camera3d(c) = &mut doc.nodes.get_mut(&camera).unwrap().operation {
        c.placement.position = [0.0, 0.0, veylvik_scene::home_distance(W, 50.0, 36.0)].into();
    }
    let merged = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", card), ("scene_2", camera)],
    );
    doc.nodes.get_mut(&render_node).unwrap().inputs = [("scene".into(), merged)].into();
    let far = render(&renderer, &doc, &inputs);
    let at = |p: &[[f32; 4]], x: u32, y: u32| p[(y * W + x) as usize];
    assert_eq!(at(&far, 5, 5)[3], 0.0, "outside the shrunken card");
    let centre = at(&far, W / 2, H / 2);
    assert!(
        (centre[0] - 0.5).abs() < 0.02 && centre[3] > 0.99,
        "{centre:?}"
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn shapes_render_lit_and_solid_with_clean_edges() {
    let renderer = renderer();
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let mut cube = op(&SHAPE_3D);
    if let Operation::Shape3d(s) = &mut cube {
        s.size_x = 60.0.into();
        s.size_y = 60.0.into();
        s.size_z = 60.0.into();
        s.colour = [0.8, 0.05, 0.05].into();
        s.placement.rotation = [25.0, 35.0, 0.0].into();
    }
    let cube = add(&mut doc, cube, &[]);
    let mut light = op(&LIGHT_3D);
    if let Operation::Light3d(l) = &mut light {
        l.placement.position = [-200.0, -200.0, 400.0].into();
        l.colour = [1.0; 3].into();
    }
    let light = add(&mut doc, light, &[]);
    let merged = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", cube), ("scene_2", light)],
    );
    let render_node = add(&mut doc, op(&RENDER_SCENE), &[("scene", merged)]);
    doc.output = render_node;
    let out = render(&renderer, &doc, &Default::default());
    let at = |x: u32, y: u32| out[(y * W + x) as usize];
    let centre = at(W / 2, H / 2);
    assert!(
        centre[3] > 0.999 && centre[0] > centre[1] * 3.0,
        "red and solid: {centre:?}"
    );
    assert_eq!(at(2, 2)[3], 0.0, "empty around the cube");
    assert!(
        out.iter()
            .all(|p| p.iter().all(|v| v.is_finite()) && p[3] <= 1.0001)
    );
    // Antialiased: some edge pixels are partly covered.
    assert!(out.iter().any(|p| p[3] > 0.05 && p[3] < 0.95));
    // Different faces catch different light.
    let shades: std::collections::BTreeSet<i32> = out
        .iter()
        .filter(|p| p[3] > 0.999)
        .map(|p| (p[0] * 20.0) as i32)
        .collect();
    assert!(shades.len() >= 3, "{shades:?}");
}

#[test]
#[ignore = "requires Vulkan"]
fn render_scene_without_a_3d_renderer_says_so() {
    let renderer = Renderer::new().unwrap();
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let shape = add(&mut doc, op(&SHAPE_3D), &[]);
    let render_node = add(&mut doc, op(&RENDER_SCENE), &[("scene", shape)]);
    doc.output = render_node;
    let Err(error) = renderer.render_frame(
        &Plan::compile(&doc).unwrap(),
        RenderRequest::full(Time::new(0, 1).unwrap()),
        &Default::default(),
    ) else {
        panic!("rendered without a 3D renderer")
    };
    let error = error.to_string();
    assert!(error.contains("3D renderer"), "{error}");
}
