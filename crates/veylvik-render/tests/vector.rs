use veylvik_document::vector::{FillRule, PathCommand::*, PathMask};
use veylvik_render::*;
fn rectangles(rule: FillRule) -> PathMask {
    PathMask {
        keys: vec![],
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
        keys: vec![],
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
        veylvik_document::Operation::Image(veylvik_document::nodes::Image {
            asset: "path".into(),
        }),
        &[],
    );
    let solid = veylvik_document::Node::new(
        "HDR",
        veylvik_document::Operation::Solid(veylvik_document::nodes::Solid {
            color: [4.0, 2.0, 1.0, 1.0],
        }),
        &[],
    );
    let output = veylvik_document::Node::new(
        "mask",
        veylvik_document::Operation::Mask(veylvik_document::nodes::Mask {}),
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
        keys: vec![],
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
        Operation::Path(veylvik_document::nodes::Path {
            geometry: geometry.clone(),
        }),
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
    let Operation::Path(veylvik_document::nodes::Path { geometry }) =
        &mut doc.nodes.get_mut(&doc.output).unwrap().operation
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
    let solid = Node::new(
        "small preview",
        Operation::Solid(veylvik_document::nodes::Solid { color: [0.0; 4] }),
        &[],
    );
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
        Operation::Path(veylvik_document::nodes::Path {
            geometry: rectangles(FillRule::EvenOdd),
        }),
        &[],
    );
    let exposure = Node::new(
        "animated",
        Operation::Exposure(veylvik_document::nodes::Exposure {
            stops: Scalar::Linear {
                keys: vec![
                    Key {
                        segment: None,
                        time: Time::new(0, 1).unwrap(),
                        value: 0.0,
                    },
                    Key {
                        segment: None,
                        time: Time::new(1, 1).unwrap(),
                        value: 1.0,
                    },
                ],
            },
        }),
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
    doc.nodes.get_mut(&path.id).unwrap().operation =
        Operation::Path(veylvik_document::nodes::Path {
            geometry: rectangles(FillRule::NonZero),
        });
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
    let solid = Node::new(
        "solid",
        Operation::Solid(veylvik_document::nodes::Solid { color: [0.0; 4] }),
        &[],
    );
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

#[test]
#[ignore = "requires Vulkan"]
fn animated_path_pixels_and_caches_follow_sampled_shape() {
    use veylvik_document::*;
    let mut geometry = PathMask {
        keys: vec![],
        fill_rule: FillRule::NonZero,
        commands: vec![
            Move { to: [2.0, 2.0] },
            Line { to: [10.0, 2.0] },
            Line { to: [10.0, 10.0] },
            Line { to: [2.0, 10.0] },
            Close {},
        ],
    };
    let first = geometry.clone();
    let mut last = first.clone();
    for c in &mut last.commands {
        if let Move { to } | Line { to } = c {
            to[0] += 16.0;
        }
    }
    geometry.set_key(Time::new(0, 1).unwrap(), &first).unwrap();
    geometry.set_key(Time::new(1, 1).unwrap(), &last).unwrap();
    let node = Node::new(
        "Animated mask",
        Operation::Path(veylvik_document::nodes::Path { geometry }),
        &[],
    );
    let mut doc = blank(32, 32, Time::new(24, 1).unwrap()).unwrap();
    doc.output = node.id;
    doc.nodes = [(node.id, node)].into();
    let plan = Plan::compile(&doc).unwrap();
    let inputs = ImageInputs::default();
    let renderer = Renderer::new().unwrap();
    let a = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    let middle = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(1, 2).unwrap()),
            &inputs,
        )
        .unwrap();
    let b = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(1, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert_eq!(
        renderer.read_frame(&a).unwrap().pixels[4 * 32 + 4],
        [1.0; 4]
    );
    assert_eq!(
        renderer.read_frame(&middle).unwrap().pixels[4 * 32 + 4],
        [0.0; 4]
    );
    assert_eq!(
        renderer.read_frame(&middle).unwrap().pixels[4 * 32 + 12],
        [1.0; 4]
    );
    assert_eq!(
        renderer.read_frame(&b).unwrap().pixels[4 * 32 + 20],
        [1.0; 4]
    );
    let again = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(1, 2).unwrap()),
            &inputs,
        )
        .unwrap();
    assert!(std::sync::Arc::ptr_eq(&middle, &again));
    let held = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(2, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert!(std::sync::Arc::ptr_eq(&b, &held));
}

#[test]
#[ignore = "requires Vulkan"]
fn a_shape_kept_over_its_own_box_is_the_shape_drawn_over_the_whole_frame() {
    use veylvik_document::*;
    let renderer = Renderer::new().unwrap();
    let (width, height) = (160u32, 96u32);
    // Curved, off to one side, at fractions of a pixel, and partly outside the frame.
    let shapes = [
        PathMask {
            keys: vec![],
            fill_rule: FillRule::NonZero,
            commands: vec![
                Move { to: [101.3, 20.7] },
                Cubic {
                    control1: [140.2, 10.1],
                    control2: [150.9, 60.4],
                    to: [120.5, 70.2],
                },
                Quad {
                    control: [90.6, 80.3],
                    to: [84.4, 40.9],
                },
                Close {},
            ],
        },
        PathMask {
            keys: vec![],
            fill_rule: FillRule::EvenOdd,
            commands: vec![
                Move { to: [-20.5, 70.25] },
                Line { to: [30.75, 60.5] },
                Line { to: [40.25, 120.0] },
                Line { to: [-10.0, 110.5] },
                Close {},
            ],
        },
    ];
    for geometry in shapes {
        let node = Node::new(
            "shape",
            Operation::Path(nodes::Path {
                geometry: geometry.clone(),
            }),
            &[],
        );
        let mut doc = blank(width, height, Time::new(24, 1).unwrap()).unwrap();
        doc.output = node.id;
        doc.nodes = [(node.id, node)].into();
        let shown = renderer
            .render_frame(
                &Plan::compile(&doc).unwrap(),
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &ImageInputs::default(),
            )
            .unwrap()
            .pixels;
        let whole = renderer
            .read_frame(&renderer.render_path_mask(&geometry, width, height).unwrap())
            .unwrap()
            .pixels;
        assert!(whole.iter().any(|p| p[3] > 0.0) && whole.iter().any(|p| p[3] == 0.0));
        let differing = shown.iter().zip(&whole).filter(|(a, b)| a != b).count();
        assert_eq!(
            differing, 0,
            "{differing} pixels differ from the shape drawn over the whole frame"
        );
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn a_drawing_of_many_shapes_is_drawn_once_and_found_again() {
    use veylvik_document::*;
    let renderer = Renderer::new().unwrap();
    // Twenty-four squares merged into one picture: more than sixteen, which once was
    // all that could be kept, so each was drawn again for every frame.
    let mut doc = blank(256, 144, Time::new(24, 1).unwrap()).unwrap();
    let mut below = doc.output;
    for n in 0..24u32 {
        let (x, y) = (8.0 + (n % 8) as f32 * 30.0, 8.0 + (n / 8) as f32 * 40.0);
        let shape = Node::new(
            &format!("square {n}"),
            Operation::Path(nodes::Path {
                geometry: PathMask {
                    keys: vec![],
                    fill_rule: FillRule::NonZero,
                    commands: vec![
                        Move { to: [x, y] },
                        Line { to: [x + 20.0, y] },
                        Line {
                            to: [x + 20.0, y + 25.0],
                        },
                        Line { to: [x, y + 25.0] },
                        Close {},
                    ],
                },
            }),
            &[],
        );
        let merged = Node::new(
            &format!("with {n}"),
            Operation::Merge(nodes::Merge {}),
            &[("background", below), ("foreground", shape.id)],
        );
        below = merged.id;
        doc.nodes.insert(shape.id, shape);
        doc.nodes.insert(merged.id, merged);
    }
    doc.output = below;
    let plan = Plan::compile(&doc).unwrap();
    let frame = |n: i64| {
        renderer
            .render_frame(
                &plan,
                RenderRequest::full(Time::new(n, 24).unwrap()),
                &ImageInputs::default(),
            )
            .unwrap()
            .pixels
    };
    let first = frame(0);
    let drawn = renderer.vector_cache_stats().unwrap();
    assert_eq!((drawn.misses, drawn.entries), (24, 24));
    // Kept over their own boxes they take a small part of what 24 whole frames would.
    assert!(
        drawn.bytes < 24 * 256 * 144 * 16 / 8,
        "{} bytes kept",
        drawn.bytes
    );
    assert!(frame(1) == first && frame(2) == first);
    let again = renderer.vector_cache_stats().unwrap();
    assert_eq!(again.misses, 24, "shapes were drawn again");
    assert_eq!((again.hits, again.evictions), (48, 0));
}
