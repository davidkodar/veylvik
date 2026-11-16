use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

const W: u32 = 96;
const H: u32 = 64;
const K1: f64 = -0.2;
const K2: f64 = 0.05;

/// Renders a source whose red and green are each pixel's own centre, through the node,
/// so every output pixel says where it sampled the source.
fn sampled(redistort: bool) -> Vec<[f32; 4]> {
    let renderer = Renderer::new().unwrap();
    let source: Vec<[f32; 4]> = (0..W * H)
        .map(|i| [(i % W) as f32 + 0.5, (i / W) as f32 + 0.5, 0.0, 1.0])
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert("source", Arc::new(Image::new(W, H, source).unwrap()));
    let image = Node::new(
        "image",
        Operation::Image(nodes::Image {
            asset: "source".into(),
        }),
        &[],
    );
    let lens = Node::new(
        "lens",
        Operation::LensDistortion(nodes::LensDistortion {
            redistort,
            k1: (K1 as f32).into(),
            k2: (K2 as f32).into(),
        }),
        &[("image", image.id)],
    );
    let doc = Composition {
        ocio: Default::default(),
        working_space: Default::default(),
        display: Default::default(),
        groups: Default::default(),
        schema: 1,
        frame_count: 1,
        width: W,
        height: H,
        frame_rate: Time::new(24, 1).unwrap(),
        output: lens.id,
        nodes: [image, lens].into_iter().map(|n| (n.id, n)).collect(),
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
/// The model, independently: ideal offset -> seen offset, radii over half the diagonal.
fn distort(offset: [f64; 2]) -> [f64; 2] {
    let radius = 0.5 * f64::from(W).hypot(f64::from(H));
    let r2 = (offset[0] * offset[0] + offset[1] * offset[1]) / (radius * radius);
    let s = 1.0 + K1 * r2 + K2 * r2 * r2;
    [offset[0] * s, offset[1] * s]
}
fn inside(p: [f32; 4]) -> bool {
    p[3] > 0.999 && (1.0..W as f32 - 1.0).contains(&p[0]) && (1.0..H as f32 - 1.0).contains(&p[1])
}
#[test]
#[ignore = "requires Vulkan"]
fn removing_and_putting_on_the_lens_follow_the_model() {
    let centre = [f64::from(W) * 0.5, f64::from(H) * 0.5];
    let (removed, put_on) = (sampled(false), sampled(true));
    let (mut checked, mut empty) = (0, 0);
    for y in 0..H {
        for x in 0..W {
            let i = (y * W + x) as usize;
            let offset = [
                f64::from(x) + 0.5 - centre[0],
                f64::from(y) + 0.5 - centre[1],
            ];
            // Removing: each ideal pixel shows the source where the lens saw it.
            let seen = distort(offset);
            let p = removed[i];
            if inside(p) {
                assert!(
                    (f64::from(p[0]) - centre[0] - seen[0]).abs() < 2e-3,
                    "{x},{y} {p:?}"
                );
                assert!(
                    (f64::from(p[1]) - centre[1] - seen[1]).abs() < 2e-3,
                    "{x},{y} {p:?}"
                );
                checked += 1;
            }
            // Putting on: the source point sampled is the ideal one the lens moves here.
            let q = put_on[i];
            if inside(q) {
                let back = distort([f64::from(q[0]) - centre[0], f64::from(q[1]) - centre[1]]);
                assert!((back[0] - offset[0]).abs() < 2e-3 && (back[1] - offset[1]).abs() < 2e-3);
                checked += 1;
            } else if q[3] == 0.0 {
                empty += 1;
            }
        }
    }
    assert!(
        checked > (W * H) as usize * 3 / 2,
        "most pixels checked: {checked}"
    );
    // Barrel distortion put on reaches past the ideal frame at the corners.
    assert!(empty > 0 && put_on[0][3] == 0.0);
}
