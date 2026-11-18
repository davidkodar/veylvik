use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

const W: u32 = 96;
const H: u32 = 64;

/// A dim grey field with one very bright warm pixel, through Bloom.
fn render(threshold: f32, intensity: f32) -> Vec<[f32; 4]> {
    let renderer = Renderer::new().unwrap();
    let mut pixels = vec![[0.2f32, 0.2, 0.2, 0.5]; (W * H) as usize];
    pixels[(32 * W + 48) as usize] = [40.0, 20.0, 4.0, 1.0];
    let mut inputs = ImageInputs::default();
    inputs.insert("source", Arc::new(Image::new(W, H, pixels).unwrap()));
    let image = Node::new(
        "image",
        Operation::Image(nodes::Image {
            asset: "source".into(),
        }),
        &[],
    );
    let bloom = Node::new(
        "bloom",
        Operation::Bloom(nodes::Bloom {
            threshold: threshold.into(),
            knee: 0.5.into(),
            intensity: intensity.into(),
            radius: 24.0.into(),
            streak: 0.0.into(),
            tint: [1.0; 4],
        }),
        &[("image", image.id)],
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
        output: bloom.id,
        nodes: [image, bloom].into_iter().map(|n| (n.id, n)).collect(),
    };
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
fn bright_light_glows_in_its_colour_and_dim_light_does_not() {
    let out = render(1.0, 1.0);
    let at = |x: u32, y: u32| out[(y * W + x) as usize];
    // The glow falls off with distance, on every side, in the light's warm colour.
    let mut previous = f32::INFINITY;
    for d in [2u32, 5, 9, 14, 20] {
        let p = at(48 + d, 32);
        let glow = p[0] - 0.2;
        assert!(glow > 0.0 && glow < previous, "falls off at {d}: {glow}");
        assert!(
            p[0] - 0.2 > (p[1] - 0.2) * 1.8 && p[1] > p[2],
            "warm at {d}: {p:?}"
        );
        assert!((at(48 - d, 32)[0] - p[0]).abs() < 1e-3 && (at(48, 32 + d)[0] - p[0]).abs() < 1e-3);
        previous = glow;
    }
    // Beyond the radius nothing changes, and alpha never does.
    assert_eq!(at(4, 4), [0.2, 0.2, 0.2, 0.5]);
    assert!(out.iter().all(|p| p[3] == 0.5 || p[3] == 1.0));
    // The dim field is under the threshold: only the light glows; with a threshold
    // above the light, or no intensity, the image is untouched.
    for (threshold, intensity) in [(64.0, 1.0), (1.0, 0.0)] {
        let same = render(threshold, intensity);
        assert_eq!(same[(32 * W + 60) as usize], [0.2, 0.2, 0.2, 0.5]);
    }
}
