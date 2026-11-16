use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::{Image, ImageInputs, Plan, RenderRequest, Renderer};

#[test]
#[ignore = "requires Vulkan"]
fn gpu_grain_measures_back_to_its_settings() {
    let renderer = Renderer::new().unwrap();
    let (w, h) = (384u32, 288u32);
    let rate = Time::new(24, 1).unwrap();
    let mut doc = blank(w, h, rate).unwrap();
    // Three flat bands: shadows, midtones, highlights.
    let pixels: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            let v = [0.01, 0.18, 0.8][((i / w) * 3 / h) as usize];
            [v, v, v, 1.0]
        })
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert("bands", Arc::new(Image::new(w, h, pixels).unwrap()));
    let image = Node::new(
        "Bands",
        Operation::Image(nodes::Image {
            asset: "bands".into(),
        }),
        &[],
    );
    let mut grain: Operation =
        serde_json::from_value(nodes::MATCH_GRAIN.default_operation()).unwrap();
    let Operation::MatchGrain(g) = &mut grain else {
        unreachable!()
    };
    g.size = 3.0.into();
    g.shadows = 0.02.into();
    g.midtones = 0.012.into();
    g.highlights = 0.008.into();
    g.red = 0.9.into();
    g.blue = 1.4.into();
    g.colour = 0.5.into();
    let node = Node::new("Grain", grain, &[("image", image.id)]);
    doc.output = node.id;
    doc.nodes.insert(image.id, image);
    doc.nodes.insert(node.id, node);
    let plan = Plan::compile(&doc).unwrap();
    let frame = |f: i64| {
        renderer
            .render_frame(
                &plan,
                RenderRequest::full(Time::at_frame(f, rate).unwrap()),
                &inputs,
            )
            .unwrap()
            .pixels
    };
    let first = frame(0);
    let m = veylvik_track::grain::measure(&first, w, h).unwrap();
    eprintln!("{m:?}");
    assert_eq!(m.size, 3);
    assert!((m.midtones - 0.012).abs() < 0.0015, "{m:?}");
    assert!(
        (m.shadows - 0.02).abs() < 0.003 && (m.highlights - 0.008).abs() < 0.0015,
        "{m:?}"
    );
    assert!(
        (m.red - 0.9).abs() < 0.1 && (m.blue - 1.4).abs() < 0.15,
        "{m:?}"
    );
    assert!((m.colour - 0.5).abs() < 0.1, "{m:?}");
    // Fresh grain every frame, and none where the image is transparent.
    let second = frame(1);
    let same = first
        .iter()
        .zip(&second)
        .filter(|(a, b)| a[1] == b[1])
        .count();
    assert!(
        same < (w * h / 100) as usize,
        "grain changes between frames"
    );
}
