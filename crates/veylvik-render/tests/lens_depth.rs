use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

const W: u32 = 64;
const H: u32 = 32;

#[test]
#[ignore = "requires Vulkan"]
fn depth_drives_the_blur_without_bleeding_sharp_foreground() {
    let renderer = Renderer::new().unwrap();
    // Left: in-focus white at depth 10. Right: a far 1-pixel checkerboard at depth 1000.
    let colour: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            let v = if x < W / 2 { 1.0 } else { ((x + y) % 2) as f32 };
            [v, v, v, 1.0]
        })
        .collect();
    let depth: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            let z = if i % W < W / 2 { 10.0 } else { 1000.0 };
            [z, z, z, 1.0]
        })
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert("colour", Arc::new(Image::new(W, H, colour).unwrap()));
    inputs.insert("depth", Arc::new(Image::new(W, H, depth).unwrap()));
    let image = |asset: &str| {
        Node::new(
            asset,
            Operation::Image(nodes::Image {
                asset: asset.into(),
            }),
            &[],
        )
    };
    let (colour, depth) = (image("colour"), image("depth"));
    let blur = Node::new(
        "Lens blur",
        Operation::LensBlur(nodes::LensBlur {
            radius: 6.0.into(),
            highlights: 0.0.into(),
            threshold: 1.0.into(),
            focus: 10.0.into(),
        }),
        &[("image", colour.id), ("depth", depth.id)],
    );
    let doc = Composition {
        markers: Default::default(),
        ocio: Default::default(),
        working_space: Default::default(),
        display: Default::default(),
        groups: Default::default(),
        schema: 1,
        frame_count: 1,
        width: W,
        height: H,
        frame_rate: Time::new(24, 1).unwrap(),
        output: blur.id,
        nodes: [colour, depth, blur]
            .into_iter()
            .map(|n| (n.id, n))
            .collect(),
    };
    let plan = Plan::compile(&doc).unwrap();
    let frame = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    let out = renderer.read_frame(&frame).unwrap().pixels;
    let at = |x: u32, y: u32| out[(y * W + x) as usize][0];
    for y in 8..24 {
        // In focus: untouched, right up to the edge.
        for x in 0..W / 2 {
            assert_eq!(at(x, y), 1.0, "sharp at {x},{y}");
        }
        // Far: the checkerboard is blurred to grey, and the sharp white beside it
        // does not spread into it.
        for x in W / 2 + 1..W - 8 {
            let v = at(x, y);
            assert!(
                (0.38..=0.62).contains(&v),
                "blurred, not bled, at {x},{y}: {v}"
            );
        }
    }
}
