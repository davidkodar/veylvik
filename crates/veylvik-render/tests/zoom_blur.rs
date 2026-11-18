use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

const W: u32 = 96;
const H: u32 = 64;
/// A white dot at (72, 32) on black, blurred towards the frame's middle by `amount`.
fn blurred(amount: f32) -> Vec<[f32; 4]> {
    let renderer = Renderer::new().unwrap();
    let mut dot = vec![[0.0, 0.0, 0.0, 1.0]; (W * H) as usize];
    dot[(32 * W + 72) as usize] = [1.0, 1.0, 1.0, 1.0];
    let mut inputs = ImageInputs::default();
    inputs.insert("dot", Arc::new(Image::new(W, H, dot).unwrap()));
    let image = Node::new(
        "dot",
        Operation::Image(nodes::Image {
            asset: "dot".into(),
        }),
        &[],
    );
    let blur = Node::new(
        "blur",
        Operation::ZoomBlur(nodes::ZoomBlur {
            amount: amount.into(),
            x: 48.0.into(),
            y: 32.0.into(),
        }),
        &[("image", image.id)],
    );
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    doc.frame_count = 1;
    doc.output = blur.id;
    doc.nodes = [image, blur].into_iter().map(|n| (n.id, n)).collect();
    let plan = Plan::compile(&doc).unwrap();
    let frame = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    renderer.read_frame(&frame).unwrap().pixels
}
#[test]
#[ignore = "requires Vulkan"]
fn a_dot_streaks_outwards_from_the_point_and_keeps_its_light() {
    let lit = |pixels: &[[f32; 4]]| -> Vec<u32> {
        (0..W)
            .filter(|x| pixels[(32 * W + x) as usize][0] > 0.01)
            .collect()
    };
    // Without an amount nothing changes.
    assert_eq!(lit(&blurred(0.0)), vec![72]);
    // With a quarter: pixels out to where a quarter less of their distance is the dot,
    // 24 / 0.75 = 32 px from the middle, are lit; nothing nearer than the dot.
    let streak = lit(&blurred(0.25));
    assert!(
        *streak.first().unwrap() >= 71 && (78..=81).contains(streak.last().unwrap()),
        "{streak:?}"
    );
    // The streak lies on the line through the middle: the rows beside it stay dark.
    let pixels = blurred(0.25);
    assert!((0..W).all(|x| pixels[(20 * W + x) as usize][0] < 1e-4));
    let total: f32 = pixels.iter().map(|p| p[0]).sum();
    assert!((0.7..1.4).contains(&total), "{total}");
}
