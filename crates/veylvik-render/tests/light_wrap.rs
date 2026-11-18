use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

const W: u32 = 96;
const H: u32 = 64;
/// A dark opaque square over transparency, wrapped by `background`.
fn wrapped(background: [f32; 4], amount: f32) -> Vec<[f32; 4]> {
    let renderer = Renderer::new().unwrap();
    let inside = |x: u32, y: u32| (32..64).contains(&x) && (16..48).contains(&y);
    let square: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            if inside(i % W, i / W) {
                [0.05, 0.05, 0.05, 1.0]
            } else {
                [0.0; 4]
            }
        })
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert("square", Arc::new(Image::new(W, H, square).unwrap()));
    inputs.insert(
        "back",
        Arc::new(Image::new(W, H, vec![background; (W * H) as usize]).unwrap()),
    );
    let image = |asset: &str| {
        Node::new(
            asset,
            Operation::Image(nodes::Image {
                asset: asset.into(),
            }),
            &[],
        )
    };
    let (front, back) = (image("square"), image("back"));
    let wrap = Node::new(
        "wrap",
        Operation::LightWrap(nodes::LightWrap {
            radius: 8.0.into(),
            amount: amount.into(),
        }),
        &[("image", front.id), ("background", back.id)],
    );
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    doc.frame_count = 1;
    doc.output = wrap.id;
    doc.nodes = [front, back, wrap].into_iter().map(|n| (n.id, n)).collect();
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
fn a_bright_background_lights_the_foregrounds_edges_only() {
    let at = |pixels: &[[f32; 4]], x: u32, y: u32| pixels[(y * W + x) as usize];
    let bright = wrapped([2.0, 1.5, 1.0, 1.0], 1.0);
    // The rim takes the background's colour; the middle, far from any edge, does not.
    let (rim, middle) = (at(&bright, 33, 32), at(&bright, 48, 32));
    assert!(rim[0] > 0.5 && rim[0] > rim[2] * 1.5, "{rim:?}");
    assert!((middle[0] - 0.05).abs() < 0.01, "{middle:?}");
    // Alpha is the square's own, and nothing appears outside it.
    assert_eq!((rim[3], middle[3]), (1.0, 1.0));
    assert_eq!(at(&bright, 20, 32), [0.0; 4]);
    // A black background, or no amount, leaves the foreground as it was.
    for plain in [
        wrapped([0.0, 0.0, 0.0, 1.0], 1.0),
        wrapped([2.0, 1.5, 1.0, 1.0], 0.0),
    ] {
        assert!((at(&plain, 33, 32)[0] - 0.05).abs() < 1e-4);
    }
}
