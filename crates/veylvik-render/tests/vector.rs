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
        veylvik_document::Operation::Mask {},
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

#[test]
#[ignore = "requires Vulkan"]
fn path_nodes_render_cache_edit_and_preflight_without_external_assets() {
    use veylvik_document::*;
    let mut renderer = Renderer::new().unwrap();
    let geometry = rectangles(FillRule::EvenOdd);
    let node = Node::new(
        "path",
        Operation::Path {
            geometry: geometry.clone(),
        },
        &[],
    );
    let mut doc = demo();
    doc.width = 32;
    doc.height = 32;
    doc.output = node.id;
    doc.nodes = [(node.id, node)].into();
    let plan = Plan::compile(&doc).unwrap();
    assert!(plan.required_assets().is_empty());
    let request = RenderRequest::full(Time::new(0, 1).unwrap());
    let inputs = ImageInputs::default();
    let reference = renderer.render_path_mask(&geometry, 32, 32).unwrap();
    let expected = renderer.read_frame(&reference).unwrap().pixels;
    drop(reference);
    let frame = renderer.render_gpu_cached(&plan, request, &inputs).unwrap();
    assert_eq!(renderer.read_frame(&frame).unwrap().pixels, expected);
    let again = renderer.render_gpu_cached(&plan, request, &inputs).unwrap();
    assert!(std::sync::Arc::ptr_eq(&frame, &again));
    doc.nodes.get_mut(&doc.output).unwrap().label = "renamed".into();
    let renamed = renderer
        .render_gpu_cached(&Plan::compile(&doc).unwrap(), request, &inputs)
        .unwrap();
    assert!(std::sync::Arc::ptr_eq(&frame, &renamed));
    let Operation::Path { geometry } = &mut doc.nodes.get_mut(&doc.output).unwrap().operation
    else {
        unreachable!()
    };
    geometry.fill_rule = FillRule::NonZero;
    let edited = renderer
        .render_gpu_cached(&Plan::compile(&doc).unwrap(), request, &inputs)
        .unwrap();
    assert!(!std::sync::Arc::ptr_eq(&frame, &edited));
    assert_eq!(
        renderer.read_frame(&edited).unwrap().pixels[16 * 32 + 16],
        [1.0; 4]
    );
    drop((frame, again, renamed, edited));
    renderer.clear_frame_cache().unwrap();
    renderer.trim_resources().unwrap();
    renderer.budget_bytes = plan.gpu_working_bytes() - 1;
    let before = renderer.resource_stats().unwrap().buffers_created;
    assert!(renderer.render_gpu(&plan, request, &inputs).is_err());
    assert_eq!(renderer.resource_stats().unwrap().buffers_created, before);
    renderer.budget_bytes = plan.gpu_working_bytes();
    assert!(renderer.render_gpu(&plan, request, &inputs).is_ok());
    doc.width = MAX_VECTOR_DIMENSION + 1;
    assert!(Plan::compile(&doc).is_err());
    let solid = Node::new("small preview", Operation::Solid { color: [0.0; 4] }, &[]);
    let output = solid.id;
    doc.nodes.insert(output, solid);
    assert!(Plan::compile_output(&doc, output).is_ok());
}

#[test]
#[ignore = "requires Vulkan"]
fn vector_coverage_cache_survives_animation_and_yields_to_memory_pressure() {
    use veylvik_document::*;
    let mut renderer = Renderer::new().unwrap();
    let path = Node::new(
        "path",
        Operation::Path {
            geometry: rectangles(FillRule::EvenOdd),
        },
        &[],
    );
    let exposure = Node::new(
        "animated",
        Operation::Exposure {
            stops: Scalar::Linear {
                keys: vec![
                    Key {
                        time: Time::new(0, 1).unwrap(),
                        value: 0.0,
                    },
                    Key {
                        time: Time::new(1, 1).unwrap(),
                        value: 1.0,
                    },
                ],
            },
        },
        &[("image", path.id)],
    );
    let mut doc = demo();
    doc.width = 32;
    doc.height = 32;
    doc.output = exposure.id;
    doc.nodes = [path.clone(), exposure]
        .into_iter()
        .map(|n| (n.id, n))
        .collect();
    let plan = Plan::compile(&doc).unwrap();
    let inputs = ImageInputs::default();
    let first = renderer
        .render_frame(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    let stats = renderer.vector_cache_stats().unwrap();
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.entries, 1);
    let second = renderer
        .render_frame(
            &plan,
            RenderRequest::full(Time::new(1, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert_eq!(renderer.vector_cache_stats().unwrap().hits, 1);
    assert_eq!(
        second.pixels[5 * 32 + 5][0],
        first.pixels[5 * 32 + 5][0] * 2.0
    );
    // A limit equal to the declared plan workspace must not charge a cached
    // source twice or force its rerasterization.
    renderer.budget_bytes = plan.working_bytes();
    renderer
        .render_frame(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert_eq!(renderer.vector_cache_stats().unwrap().hits, 2);
    doc.nodes.get_mut(&path.id).unwrap().operation = Operation::Path {
        geometry: rectangles(FillRule::NonZero),
    };
    renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert_eq!(renderer.vector_cache_stats().unwrap().misses, 2);
    assert!(renderer.vector_cache_stats().unwrap().evictions >= 1);
    // Ordinary non-vector rendering can reclaim retained mask storage.
    let solid = Node::new("solid", Operation::Solid { color: [0.0; 4] }, &[]);
    doc.output = solid.id;
    doc.nodes = [(solid.id, solid)].into();
    let simple = Plan::compile(&doc).unwrap();
    renderer.budget_bytes = simple.gpu_working_bytes();
    renderer
        .render_gpu(
            &simple,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert_eq!(renderer.vector_cache_stats().unwrap().entries, 0);
    renderer.clear_frame_cache().unwrap();
    assert_eq!(renderer.resource_stats().unwrap().held_bytes, 0);
}
