use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

#[test]
fn colorize_endpoints_validate_and_default_to_black_and_white() {
    let operation: Operation =
        serde_json::from_value(registry::COLORIZE.default_operation()).unwrap();
    let Operation::Colorize(veylvik_document::nodes::Colorize { low, high }) = operation else {
        panic!()
    };
    assert_eq!(low, [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(high, [1.0; 4]);
    for endpoint in [
        [f32::NAN, 0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0, -0.1],
        [0.0, 0.0, 0.0, 1.1],
    ] {
        assert!(
            Operation::Colorize(veylvik_document::nodes::Colorize {
                low: endpoint,
                high
            })
            .validate()
            .is_err()
        );
        assert!(
            Operation::Colorize(veylvik_document::nodes::Colorize {
                low,
                high: endpoint
            })
            .validate()
            .is_err()
        );
    }
    Operation::Colorize(veylvik_document::nodes::Colorize {
        low: [-f32::MAX, f32::MAX, 0.0, 0.5],
        high,
    })
    .validate()
    .unwrap();
}

#[test]
#[ignore = "requires Vulkan"]
fn colorize_matches_premultiplied_reference_with_hdr_transparency_and_cache_changes() {
    let renderer = Renderer::new().unwrap();
    let source = vec![
        [0.0; 4],
        [0.0, 0.0, 0.0, 1.0],
        [1.0; 4],
        [0.25, 0.5, 0.75, 1.0],
        [0.125, 0.25, 0.375, 0.5],
        [-4.0, 2.0, 0.25, 0.5],
        [1e-21, 2e-21, 3e-21, 1e-20],
        [f32::MAX, f32::MAX, f32::MAX, 1.0],
        [-f32::MAX, -f32::MAX, -f32::MAX, 1.0],
        [f32::MAX, -f32::MAX, f32::MAX, 0.5],
        [0.0, 1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
        [0.1, 0.0, 0.0, 0.1],
    ];
    let mut doc = blank(source.len() as u32, 1, Time::new(24, 1).unwrap()).unwrap();
    let source_id = doc.output;
    doc.nodes.get_mut(&source_id).unwrap().operation =
        Operation::Image(veylvik_document::nodes::Image {
            asset: "input".into(),
        });
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "input",
        Arc::new(Image::new(doc.width, 1, source.clone()).unwrap()),
    );
    let mut previous = None;
    for (low, high) in [
        ([0.0, 0.0, 0.0, 1.0], [1.0; 4]),
        ([0.1, -0.2, 4.0, 0.3], [8.0, 0.5, 0.1, 0.8]),
        ([4.0, 2.0, 1.0, 0.0], [0.2, 0.4, 0.8, 0.6]),
        ([0.1, 0.2, 0.3, 0.5], [-f32::MAX, f32::MAX, 0.0, 1.0]),
    ] {
        let colorize = Node::new(
            "Color",
            Operation::Colorize(veylvik_document::nodes::Colorize { low, high }),
            &[("image", source_id)],
        );
        if doc.output != source_id {
            doc.nodes.remove(&doc.output);
        }
        doc.output = colorize.id;
        doc.nodes.insert(colorize.id, colorize);
        let plan = Plan::compile(&doc).unwrap();
        let request = RenderRequest::full(Time::new(0, 1).unwrap());
        let frame = renderer.render_gpu_cached(&plan, request, &inputs).unwrap();
        if let Some(old) = previous.take() {
            assert!(!Arc::ptr_eq(&frame, &old));
        }
        assert!(Arc::ptr_eq(
            &frame,
            &renderer.render_gpu_cached(&plan, request, &inputs).unwrap()
        ));
        for (actual, pixel) in renderer
            .read_frame(&frame)
            .unwrap()
            .pixels
            .iter()
            .zip(&source)
        {
            let alpha = pixel[3] as f64;
            let luminance =
                pixel[0] as f64 * 0.2126 + pixel[1] as f64 * 0.7152 + pixel[2] as f64 * 0.0722;
            let t = if alpha == 0.0 {
                0.0
            } else {
                luminance.clamp(0.0, alpha) / alpha
            };
            for channel in 0..4 {
                let a = low[channel] as f64 * if channel < 3 { low[3] as f64 } else { 1.0 };
                let b = high[channel] as f64 * if channel < 3 { high[3] as f64 } else { 1.0 };
                let expected = (a * (1.0 - t) + b * t) * alpha;
                assert!(
                    (actual[channel] as f64 - expected).abs() <= 2e-6 * expected.abs().max(1e-20),
                    "{pixel:?}: {actual:?}, channel {channel}: expected {expected}"
                );
            }
            assert!(actual[3] <= pixel[3]);
        }
        previous = Some(frame);
    }
}
