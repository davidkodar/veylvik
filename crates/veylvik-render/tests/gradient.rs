use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

fn operation(mode: GradientMode, start: [f32; 2], end: [f32; 2]) -> Operation {
    Operation::Gradient(veylvik_document::nodes::Gradient {
        mode,
        start_x: start[0].into(),
        start_y: start[1].into(),
        end_x: end[0].into(),
        end_y: end[1].into(),
    })
}
fn reference(mode: GradientMode, start: [f32; 2], end: [f32; 2], x: u32, y: u32) -> f64 {
    let dx = end[0] as f64 - start[0] as f64;
    let dy = end[1] as f64 - start[1] as f64;
    let length = dx.hypot(dy);
    if length < 1e-6 {
        return 0.0;
    }
    let px = x as f64 - start[0] as f64;
    let py = y as f64 - start[1] as f64;
    match mode {
        GradientMode::Linear => ((px * dx + py * dy) / (length * length)).clamp(0.0, 1.0),
        GradientMode::Radial => (px.hypot(py) / length).clamp(0.0, 1.0),
    }
}
#[test]
#[ignore = "requires Vulkan"]
fn gradients_match_geometry_reference_animate_and_compose_with_color_and_masks() {
    let renderer = Renderer::new().unwrap();
    let mut doc = blank(17, 9, Time::new(24, 1).unwrap()).unwrap();
    let id = doc.output;
    let inputs = ImageInputs::default();
    let time = Time::new(0, 1).unwrap();
    for mode in GradientMode::ALL {
        for (start, end) in [
            ([0.0, 0.0], [16.0, 0.0]),
            ([8.0, 4.0], [8.0, 8.0]),
            ([-2.0, 1.0], [7.0, 5.0]),
            ([7.0, 5.0], [-2.0, 1.0]),
            ([3.0, 4.0], [3.0, 4.0]),
            ([0.0, 0.0], [1e-8, 0.0]),
            ([-1_000_000.0; 2], [1_000_000.0; 2]),
        ] {
            doc.nodes.get_mut(&id).unwrap().operation = operation(mode, start, end);
            let plan = Plan::compile(&doc).unwrap();
            let frame = renderer
                .render_gpu_cached(&plan, RenderRequest::full(time), &inputs)
                .unwrap();
            let again = renderer
                .render_gpu_cached(&plan, RenderRequest::full(time), &inputs)
                .unwrap();
            assert!(Arc::ptr_eq(&frame, &again));
            let image = renderer.read_frame(&frame).unwrap();
            for (index, pixel) in image.pixels.iter().enumerate() {
                let expected = reference(mode, start, end, index as u32 % 17, index as u32 / 17);
                for channel in &pixel[..3] {
                    assert!(
                        (*channel as f64 - expected).abs() < 2e-6,
                        "{mode:?} {start:?} {end:?} {index}: {pixel:?} vs {expected}"
                    );
                }
                assert_eq!(pixel[3], 1.0);
            }
        }
    }
    let mut gradient = operation(GradientMode::Linear, [0.0, 4.0], [16.0, 4.0]);
    gradient
        .scalar_mut("start_x")
        .unwrap()
        .set_key_at(time, 0.0)
        .unwrap();
    gradient
        .scalar_mut("start_x")
        .unwrap()
        .set_key_at(Time::new(1, 1).unwrap(), 8.0)
        .unwrap();
    doc.nodes.get_mut(&id).unwrap().operation = gradient;
    let color = Node::new(
        "Color",
        Operation::Colorize(veylvik_document::nodes::Colorize {
            low: [2.0, 0.0, 0.0, 0.5],
            high: [0.0, 0.0, 4.0, 0.25],
        }),
        &[("image", id)],
    );
    doc.output = color.id;
    doc.nodes.insert(color.id, color);
    let plan = Plan::compile(&doc).unwrap();
    let mut previous = None;
    for ticks in [0, 1, 2] {
        let time = Time::new(ticks, 2).unwrap();
        let frame = renderer
            .render_gpu_cached(&plan, RenderRequest::full(time), &inputs)
            .unwrap();
        if let Some(previous) = previous {
            assert!(!Arc::ptr_eq(&previous, &frame));
        }
        let image = renderer.read_frame(&frame).unwrap();
        for (index, pixel) in image.pixels.iter().enumerate() {
            let t = reference(
                GradientMode::Linear,
                [ticks as f32 * 4.0, 4.0],
                [16.0, 4.0],
                index as u32 % 17,
                index as u32 / 17,
            );
            let expected = [1.0 - t, 0.0, t, 0.5 * (1.0 - t) + 0.25 * t];
            for i in 0..4 {
                assert!((pixel[i] as f64 - expected[i]).abs() < 2e-6);
            }
        }
        previous = Some(frame);
    }
    let mask = Node::new(
        "Coverage",
        Operation::ChannelMask(veylvik_document::nodes::ChannelMask {
            channel: MaskChannel::Luminance,
        }),
        &[("image", id)],
    );
    doc.output = mask.id;
    doc.nodes.insert(mask.id, mask);
    let frame = renderer
        .render_gpu_cached(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(time),
            &inputs,
        )
        .unwrap();
    let image = renderer.read_frame(&frame).unwrap();
    for (index, pixel) in image.pixels.iter().enumerate() {
        let expected = (index % 17) as f32 / 16.0;
        for channel in pixel {
            assert!((channel - expected).abs() < 2e-6);
        }
    }
}
