use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

fn operation(radius: f32) -> Operation {
    Operation::Morphology(veylvik_document::nodes::Morphology {
        radius: radius.into(),
    })
}
fn reference(pixels: &[[f32; 4]], width: u32, height: u32, radius: f32) -> Vec<[f32; 4]> {
    let radius = radius.round() as i32;
    (0..height as i32)
        .flat_map(|y| {
            (0..width as i32).map(move |x| {
                let mut alpha: f32 = if radius < 0 { 1.0 } else { 0.0 };
                for dy in -radius.abs()..=radius.abs() {
                    for dx in -radius.abs()..=radius.abs() {
                        let (px, py) = (x + dx, y + dy);
                        let sample =
                            if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                                0.0
                            } else {
                                pixels[(py as u32 * width + px as u32) as usize][3]
                            };
                        alpha = if radius < 0 {
                            alpha.min(sample)
                        } else {
                            alpha.max(sample)
                        };
                    }
                }
                [alpha; 4]
            })
        })
        .collect()
}
#[test]
fn morphology_validates_bounds_and_never_aliases_colored_zero_radius_input() {
    for radius in [-256.0, -0.5, 0.0, 256.0] {
        operation(radius).validate().unwrap();
    }
    for radius in [-256.01, 256.01, f32::NAN, f32::INFINITY] {
        assert!(operation(radius).validate().is_err());
    }
    let mut doc = blank(17, 9, Time::new(24, 1).unwrap()).unwrap();
    let mask = Node::new("Mask", operation(0.0), &[("image", doc.output)]);
    doc.output = mask.id;
    doc.nodes.insert(mask.id, mask);
    let plan = Plan::compile(&doc).unwrap();
    assert_eq!(plan.pass_count(), 3);
    assert_eq!(plan.passes()[1].kernel, Kernel::MorphologyHorizontal);
    assert_eq!(plan.passes()[2].kernel, Kernel::MorphologyVertical);
    for pass in plan.passes() {
        assert!(!pass.input_slots.contains(&pass.output_slot));
    }
}
#[test]
#[ignore = "requires Vulkan"]
fn morphology_matches_square_reference_and_animated_cache_and_crop() {
    let renderer = Renderer::new().unwrap();
    for (width, height) in [(17, 9), (1, 9), (17, 1), (1, 1)] {
        let source: Vec<_> = (0..width * height)
            .map(|index| {
                let alpha = ((index * 37 + 13) % 17) as f32 / 16.0;
                // Deliberately colored HDR pixels: the output depends only on alpha.
                [alpha * -8.0, alpha * 4.0, alpha * 0.25, alpha]
            })
            .collect();
        let mut doc = blank(width, height, Time::new(24, 1).unwrap()).unwrap();
        let source_id = doc.output;
        doc.nodes.get_mut(&source_id).unwrap().operation =
            Operation::Image(veylvik_document::nodes::Image {
                asset: "source".into(),
            });
        let node = Node::new("Mask", operation(0.0), &[("image", source_id)]);
        doc.output = node.id;
        doc.nodes.insert(node.id, node);
        let mut inputs = ImageInputs::default();
        inputs.insert(
            "source",
            Arc::new(Image::new(width, height, source.clone()).unwrap()),
        );
        for radius in [
            -256.0, -32.0, -3.0, -1.5, -0.5, -0.49, 0.0, 0.49, 0.5, 1.5, 3.0, 32.0, 256.0,
        ] {
            doc.nodes.get_mut(&doc.output).unwrap().operation = operation(radius);
            let plan = Plan::compile(&doc).unwrap();
            let request = RenderRequest::full(Time::new(0, 1).unwrap());
            let frame = renderer.render_gpu_cached(&plan, request, &inputs).unwrap();
            assert!(Arc::ptr_eq(
                &frame,
                &renderer.render_gpu_cached(&plan, request, &inputs).unwrap()
            ));
            assert_eq!(
                renderer.read_frame(&frame).unwrap().pixels,
                reference(&source, width, height, radius),
                "{width}x{height}, radius {radius}"
            );
        }
        let radius = doc
            .nodes
            .get_mut(&doc.output)
            .unwrap()
            .operation
            .scalar_mut("radius")
            .unwrap();
        *radius = Scalar::Linear {
            keys: vec![
                Key {
                    segment: None,
                    time: Time::new(0, 1).unwrap(),
                    value: -2.0,
                },
                Key {
                    segment: None,
                    time: Time::new(1, 1).unwrap(),
                    value: 2.0,
                },
            ],
        };
        let plan = Plan::compile(&doc).unwrap();
        let mut previous = None;
        for tick in 0..=4 {
            let frame = renderer
                .render_gpu_cached(
                    &plan,
                    RenderRequest::full(Time::new(tick, 4).unwrap()),
                    &inputs,
                )
                .unwrap();
            if let Some(previous) = previous {
                assert!(!Arc::ptr_eq(&previous, &frame));
            }
            assert_eq!(
                renderer.read_frame(&frame).unwrap().pixels,
                reference(&source, width, height, tick as f32 - 2.0)
            );
            previous = Some(frame);
        }
        let region = Region {
            x: width - 1,
            y: height - 1,
            width: 1,
            height: 1,
        };
        let frame = renderer
            .render_gpu_cached(
                &plan,
                RenderRequest {
                    time: Time::new(1, 1).unwrap(),
                    region: Some(region),
                    scene_view: None,
                    scene_reflections: false,
                },
                &inputs,
            )
            .unwrap();
        assert_eq!(
            renderer.read_frame(&frame).unwrap().pixels,
            vec![*reference(&source, width, height, 2.0).last().unwrap()]
        );
    }
}
