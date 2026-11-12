use veylvik_document::vector::{FillRule, PathCommand::*, PathMask};
use veylvik_render::*;
fn rectangles(rule: FillRule) -> PathMask {
    PathMask {
        fill_rule: rule,
        commands: vec![
            Move { to: [2.0, 2.0] },
            Line { to: [30.0, 2.0] },
            Line { to: [30.0, 30.0] },
            Line { to: [2.0, 30.0] },
            Close {},
            Move { to: [10.0, 10.0] },
            Line { to: [22.0, 10.0] },
            Line { to: [22.0, 22.0] },
            Line { to: [10.0, 22.0] },
            Close {},
        ],
    }
}
#[test]
#[ignore = "requires Vulkan"]
fn vector_masks_preserve_winding_curves_resize_and_gpu_compositing() {
    let renderer = Renderer::new().unwrap();
    let nonzero = renderer
        .render_path_mask(&rectangles(FillRule::NonZero), 32, 32)
        .unwrap();
    let pixels = renderer.read_frame(&nonzero).unwrap().pixels;
    assert_eq!(pixels[16 * 32 + 16], [1.0; 4]);
    assert_eq!(pixels[0], [0.0; 4]);
    let evenodd = renderer
        .render_path_mask(&rectangles(FillRule::EvenOdd), 32, 32)
        .unwrap();
    let pixels = renderer.read_frame(&evenodd).unwrap().pixels;
    assert_eq!(pixels[16 * 32 + 16], [0.0; 4]);
    assert_eq!(pixels[5 * 32 + 5], [1.0; 4]);
    // Resizing uses the same backend; contours outside the view are clipped.
    let clipped = renderer
        .render_path_mask(&rectangles(FillRule::EvenOdd), 13, 7)
        .unwrap();
    assert_eq!(
        renderer.read_frame(&clipped).unwrap().pixels[5 * 13 + 5],
        [1.0; 4]
    );
    let curve = PathMask {
        fill_rule: FillRule::NonZero,
        commands: vec![
            Move { to: [3.0, 28.0] },
            Cubic {
                control1: [3.0, 1.0],
                control2: [29.0, 1.0],
                to: [29.0, 28.0],
            },
            Quad {
                control: [16.0, 32.0],
                to: [3.0, 28.0],
            },
            Close {},
        ],
    };
    let frame = std::sync::Arc::new(renderer.render_path_mask(&curve, 32, 32).unwrap());
    let pixels = renderer.read_frame(&frame).unwrap().pixels;
    assert!(pixels.iter().any(|p| p[3] > 0.0 && p[3] < 1.0));
    for p in &pixels {
        assert_eq!(p[0], p[3]);
        assert_eq!(p[1], p[3]);
        assert_eq!(p[2], p[3]);
    }
    let mask = veylvik_document::Node::new(
        "path",
        veylvik_document::Operation::Image {
            asset: "path".into(),
        },
        &[],
    );
    let solid = veylvik_document::Node::new(
        "HDR",
        veylvik_document::Operation::Solid {
            color: [4.0, 2.0, 1.0, 1.0],
        },
        &[],
    );
    let output = veylvik_document::Node::new(
        "mask",
        veylvik_document::Operation::Mask,
        &[("image", solid.id), ("mask", mask.id)],
    );
    let mut doc = veylvik_document::demo();
    doc.width = 32;
    doc.height = 32;
    doc.output = output.id;
    doc.nodes = [mask, solid, output]
        .into_iter()
        .map(|n| (n.id, n))
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert_gpu("path", frame);
    let result = renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(veylvik_document::Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    for (a, b) in result.pixels.iter().zip(&pixels) {
        assert_eq!(a[0], 4.0 * b[3]);
        assert_eq!(a[3], b[3]);
    }
    assert_eq!(renderer.resource_stats().unwrap().source_uploads, 0);
    renderer.release_vector_resources().unwrap();
    // Explicit retirement leaves retained output frames intact and permits recreation.
    assert_eq!(
        renderer.read_frame(&nonzero).unwrap().pixels[16 * 32 + 16],
        [1.0; 4]
    );
    assert!(renderer.render_path_mask(&curve, 32, 32).is_ok());
    let before = renderer.resource_stats().unwrap().buffers_created;
    assert!(renderer.render_path_mask(&curve, 0, 32).is_err());
    assert!(
        renderer
            .render_path_mask(&curve, MAX_VECTOR_DIMENSION + 1, 32)
            .is_err()
    );
    let invalid = PathMask {
        fill_rule: FillRule::NonZero,
        commands: vec![Close {}],
    };
    assert!(renderer.render_path_mask(&invalid, 32, 32).is_err());
    assert_eq!(renderer.resource_stats().unwrap().buffers_created, before);
}
