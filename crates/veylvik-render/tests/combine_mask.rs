use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

#[test]
#[ignore = "requires Vulkan"]
fn mask_modes_match_soft_coverage_and_preserve_input_order_and_cache_identity() {
    let renderer = Renderer::new().unwrap();
    let (width, height) = (17, 9);
    let values = [0.0_f32, 1.0, 0.25, 0.5, 0.75, 1e-6, 1.0 - 1e-6];
    let a: Vec<_> = (0..width * height)
        .map(|i| {
            let alpha = values[i as usize % values.len()];
            [alpha * 4.0, alpha * -2.0, 0.0, alpha]
        })
        .collect();
    let b: Vec<_> = (0..width * height)
        .map(|i| {
            let alpha = values[i as usize / values.len() % values.len()];
            [0.0, alpha * 8.0, alpha * -4.0, alpha]
        })
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert("a", Arc::new(Image::new(width, height, a.clone()).unwrap()));
    inputs.insert("b", Arc::new(Image::new(width, height, b.clone()).unwrap()));
    let mut doc = blank(width, height, Time::new(24, 1).unwrap()).unwrap();
    let first = doc.output;
    doc.nodes.get_mut(&first).unwrap().operation =
        Operation::Image(veylvik_document::nodes::Image { asset: "a".into() });
    let second = Node::new(
        "B",
        Operation::Image(veylvik_document::nodes::Image { asset: "b".into() }),
        &[],
    );
    let other = second.id;
    doc.nodes.insert(other, second);
    let combine = Node::new(
        "Combine",
        Operation::CombineMask(veylvik_document::nodes::CombineMask {
            mode: MaskMode::Union,
        }),
        &[("mask", first), ("other", other)],
    );
    doc.output = combine.id;
    doc.nodes.insert(combine.id, combine);
    let mut previous = None;
    for reverse in [false, true] {
        for mode in MaskMode::ALL {
            let node = doc.nodes.get_mut(&doc.output).unwrap();
            node.operation = Operation::CombineMask(veylvik_document::nodes::CombineMask { mode });
            node.inputs
                .insert("mask".into(), if reverse { other } else { first });
            node.inputs
                .insert("other".into(), if reverse { first } else { other });
            let plan = Plan::compile(&doc).unwrap();
            let request = RenderRequest::full(Time::new(0, 1).unwrap());
            let frame = renderer.render_gpu_cached(&plan, request, &inputs).unwrap();
            if let Some(previous) = previous {
                assert!(!Arc::ptr_eq(&previous, &frame));
            }
            assert!(Arc::ptr_eq(
                &frame,
                &renderer.render_gpu_cached(&plan, request, &inputs).unwrap()
            ));
            for (index, pixel) in renderer
                .read_frame(&frame)
                .unwrap()
                .pixels
                .iter()
                .enumerate()
            {
                let (a, b) = if reverse {
                    (b[index][3] as f64, a[index][3] as f64)
                } else {
                    (a[index][3] as f64, b[index][3] as f64)
                };
                let expected = match mode {
                    MaskMode::Union => 1.0 - (1.0 - a) * (1.0 - b),
                    MaskMode::Intersect => a * b,
                    MaskMode::Subtract => a * (1.0 - b),
                    MaskMode::Exclude => a + b - 2.0 * a * b,
                };
                for channel in pixel {
                    assert!(
                        (*channel as f64 - expected).abs() < 2e-7,
                        "{mode:?} {reverse} {a} {b}: {pixel:?} vs {expected}"
                    );
                    assert!((0.0..=1.0).contains(channel));
                }
            }
            previous = Some(frame);
        }
    }
    // A hard one-pixel source grown by one, then subtracting the original,
    // must yield exactly the eight surrounding pixels, including after tinting.
    let source = Node::new(
        "Point",
        Operation::Ellipse(veylvik_document::nodes::Ellipse {
            x: 8.0.into(),
            y: 4.0.into(),
            radius_x: 0.5.into(),
            radius_y: 0.5.into(),
            feather: 1.0.into(),
        }),
        &[],
    );
    let source_id = source.id;
    doc.nodes.insert(source.id, source);
    let grown = Node::new(
        "Grow",
        Operation::Morphology(veylvik_document::nodes::Morphology { radius: 1.0.into() }),
        &[("image", source_id)],
    );
    let grown_id = grown.id;
    doc.nodes.insert(grown.id, grown);
    let outline = Node::new(
        "Outline",
        Operation::CombineMask(veylvik_document::nodes::CombineMask {
            mode: MaskMode::Subtract,
        }),
        &[("mask", grown_id), ("other", source_id)],
    );
    let outline_id = outline.id;
    doc.nodes.insert(outline.id, outline);
    let color = Node::new(
        "Tint",
        Operation::Colorize(veylvik_document::nodes::Colorize {
            low: [0.0, 0.0, 0.0, 1.0],
            high: [4.0, 0.0, 2.0, 0.5],
        }),
        &[("image", outline_id)],
    );
    doc.output = color.id;
    doc.nodes.insert(color.id, color);
    let frame = renderer
        .render_gpu_cached(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    for (index, pixel) in renderer
        .read_frame(&frame)
        .unwrap()
        .pixels
        .iter()
        .enumerate()
    {
        let (x, y) = (index as i32 % 17, index as i32 / 17);
        let ring = (x - 8).abs() <= 1 && (y - 4).abs() <= 1 && (x != 8 || y != 4);
        assert_eq!(*pixel, if ring { [2.0, 0.0, 1.0, 0.5] } else { [0.0; 4] });
    }
}
