use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

const WIDTH: u32 = 64;
const SUBJECT: [f32; 3] = [0.6, 0.3, 0.2];
const BACKDROP: [f32; 3] = [0.2, 0.5, 0.8];
/// The true edge: subject left of x = 30, backdrop right of it, x = 30 half of each.
fn image() -> Vec<[f32; 4]> {
    (0..WIDTH * 4)
        .map(|i| {
            let x = i % WIDTH;
            let t = if x < 30 {
                1.0
            } else if x == 30 {
                0.5
            } else {
                0.0
            };
            let c = |k: usize| SUBJECT[k] * t + BACKDROP[k] * (1.0 - t);
            [c(0), c(1), c(2), 1.0]
        })
        .collect()
}
fn render(matte: Vec<[f32; 4]>, decontaminate: f32) -> Vec<[f32; 4]> {
    let renderer = Renderer::new().unwrap();
    let mut doc = blank(WIDTH, 4, Time::new(24, 1).unwrap()).unwrap();
    let image_id = doc.output;
    doc.nodes.get_mut(&image_id).unwrap().operation = Operation::Image(nodes::Image {
        asset: "image".into(),
    });
    let matte_node = Node::new(
        "Matte",
        Operation::Image(nodes::Image {
            asset: "matte".into(),
        }),
        &[],
    );
    let matte_id = matte_node.id;
    doc.nodes.insert(matte_id, matte_node);
    let c = |v: f32| Scalar::Constant { value: v };
    let refine = Node::new(
        "Refine",
        Operation::RefineMatte(nodes::RefineMatte {
            radius: c(6.0),
            detail: c(0.7),
            decontaminate: c(decontaminate),
        }),
        &[("image", image_id), ("matte", matte_id)],
    );
    doc.output = refine.id;
    doc.nodes.insert(refine.id, refine);
    let mut inputs = ImageInputs::default();
    inputs.insert("image", Arc::new(Image::new(WIDTH, 4, image()).unwrap()));
    inputs.insert("matte", Arc::new(Image::new(WIDTH, 4, matte).unwrap()));
    let plan = Plan::compile(&doc).unwrap();
    renderer
        .render_frame(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap()
        .pixels
}
fn row(pixels: &[[f32; 4]]) -> &[[f32; 4]] {
    &pixels[WIDTH as usize..2 * WIDTH as usize]
}

#[test]
#[ignore = "requires Vulkan"]
fn a_soft_misplaced_matte_snaps_to_the_true_edge_without_background_colour() {
    // The coarse matte fades over x = 27..33, centred three pixels beyond the true edge.
    let matte = (0..WIDTH * 4)
        .map(|i| {
            let a = ((33.0 - (i % WIDTH) as f32) / 6.0).clamp(0.0, 1.0);
            [a; 4]
        })
        .collect();
    let out = render(matte, 1.0);
    let row = row(&out);
    let alpha: Vec<f32> = row.iter().map(|p| p[3]).collect();
    for (x, a) in alpha.iter().enumerate().take(30) {
        assert!(*a > 0.99, "subject pixel {x} must be opaque: {alpha:?}");
    }
    assert!(
        (0.2..0.8).contains(&alpha[30]),
        "mixed pixel is partial: {alpha:?}"
    );
    for (x, a) in alpha.iter().enumerate().skip(31) {
        assert!(*a < 0.01, "backdrop pixel {x} must be clear: {alpha:?}");
    }
    // The mixed pixel's colour is the subject's, not half backdrop.
    let straight: Vec<f32> = (0..3).map(|k| row[30][k] / row[30][3]).collect();
    for k in 0..3 {
        assert!(
            (straight[k] - SUBJECT[k]).abs() < 0.08,
            "decontaminated {straight:?} vs subject {SUBJECT:?}"
        );
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn an_opaque_matte_returns_the_image_exactly() {
    let out = render(vec![[1.0; 4]; (WIDTH * 4) as usize], 1.0);
    for (actual, expected) in out.iter().zip(image()) {
        for k in 0..4 {
            assert!(
                (actual[k] - expected[k]).abs() < 1e-5,
                "{actual:?} {expected:?}"
            );
        }
    }
    // An empty matte stays empty.
    let out = render(vec![[0.0; 4]; (WIDTH * 4) as usize], 1.0);
    assert!(out.iter().all(|p| p.iter().all(|v| v.abs() < 1e-6)));
}
