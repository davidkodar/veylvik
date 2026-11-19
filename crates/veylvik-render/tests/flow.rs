use std::sync::Arc;
use veylvik_document::{nodes, *};
use veylvik_render::{flow::FlowSettings, *};
fn frame(renderer: &Renderer, width: u32, height: u32, pixels: Vec<[f32; 4]>) -> GpuFrame {
    let mut doc = blank(width, height, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation = Operation::Image(nodes::Image {
        asset: "frame".into(),
    });
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "frame",
        Arc::new(Image::new(width, height, pixels).unwrap()),
    );
    renderer
        .render_gpu(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap()
}
fn texture(shift: i32) -> Vec<[f32; 4]> {
    (0..96 * 64)
        .map(|i| {
            let x = (i % 96) - shift;
            let y = (i / 96) as f32;
            if !(0..96).contains(&x) {
                return [0.; 4];
            }
            let x = x as f32;
            let v = 0.4
                + 0.17 * (x * 0.31).sin()
                + 0.13 * (y * 0.27).cos()
                + 0.1 * (x * 0.17 + y * 0.21).sin();
            [v * 2., v * 0.5, v * 0.25, 0.5]
        })
        .collect()
}
fn settings() -> FlowSettings {
    FlowSettings {
        fraction: 0.5,
        analysis_edge: 512,
        confidence: 0.1,
        diagnostic: false,
    }
}
#[test]
#[ignore = "requires Vulkan"]
fn bidirectional_flow_interpolates_translation_and_caches_analysis() {
    let renderer = Renderer::new().unwrap();
    let a = frame(&renderer, 96, 64, texture(0));
    let b = frame(&renderer, 96, 64, texture(6));
    let output = renderer
        .interpolate_flow(&a, &b, settings(), || false)
        .unwrap()
        .unwrap();
    let pixels = renderer.read_frame(&output).unwrap().pixels;
    let expected = texture(3);
    let nearest = texture(6);
    let mut actual_error = 0.;
    let mut nearest_error = 0.;
    for y in 12..52 {
        for x in 16..80 {
            let i = y * 96 + x;
            actual_error += (pixels[i][0] - expected[i][0]).abs();
            nearest_error += (nearest[i][0] - expected[i][0]).abs();
        }
    }
    assert!(
        actual_error < nearest_error * 0.8,
        "flow error {actual_error}, nearest {nearest_error}, report {:?}",
        output.flow_report()
    );
    assert!(
        pixels
            .iter()
            .all(|p| p.iter().all(|v| v.is_finite()) && p[3] >= 0. && p[3] <= 0.5 + 1e-6)
    );
    let again = renderer
        .interpolate_flow(
            &a,
            &b,
            FlowSettings {
                fraction: 0.25,
                ..settings()
            },
            || false,
        )
        .unwrap()
        .unwrap();
    assert!(again.flow_report().unwrap().cache_hit);
    let endpoint = renderer
        .interpolate_flow(
            &a,
            &b,
            FlowSettings {
                fraction: 0.,
                ..settings()
            },
            || false,
        )
        .unwrap()
        .unwrap();
    drop(endpoint);
    assert_eq!(
        renderer.read_frame(&a).unwrap().pixels,
        texture(0),
        "A cloned frame must not retire its shared allocation early"
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn cuts_fall_back_without_ghosts_and_cancelled_work_is_not_published() {
    let renderer = Renderer::new().unwrap();
    let a = frame(&renderer, 64, 64, vec![[0., 0., 0., 1.]; 4096]);
    let b = frame(&renderer, 64, 64, vec![[1.; 4]; 4096]);
    let output = renderer
        .interpolate_flow(&a, &b, settings(), || false)
        .unwrap()
        .unwrap();
    assert!(output.flow_report().unwrap().scene_cut);
    assert_eq!(
        renderer.read_frame(&output).unwrap().pixels,
        vec![[1.; 4]; 4096]
    );
    let held = renderer.resource_stats().unwrap().held_bytes;
    let mut checks = 0;
    assert!(
        renderer
            .interpolate_flow(&b, &a, settings(), || {
                checks += 1;
                checks > 6
            })
            .unwrap()
            .is_none()
    );
    assert_eq!(renderer.resource_stats().unwrap().held_bytes, held);
}

#[test]
#[ignore = "requires Vulkan"]
fn confidence_handles_endpoints_occlusion_and_cache_clear() {
    let renderer = Renderer::new().unwrap();
    let a = frame(&renderer, 96, 64, texture(0));
    let mut covered = texture(6);
    for y in 16..48 {
        for x in 32..64 {
            covered[y * 96 + x] = [0., 0., 0., 1.];
        }
    }
    let b = frame(&renderer, 96, 64, covered.clone());
    let output = renderer
        .interpolate_flow(&a, &b, settings(), || false)
        .unwrap()
        .unwrap();
    let pixels = renderer.read_frame(&output).unwrap().pixels;
    // The newly covered central region has no correspondence. Do not invent a translucent ghost.
    let fallback = (24..40)
        .flat_map(|y| (40..56).map(move |x| y * 96 + x))
        .filter(|&i| pixels[i] == covered[i])
        .count();
    assert!(
        fallback > 180,
        "Only {fallback}/256 occluded pixels used the source fallback"
    );
    let diagnostic = renderer
        .interpolate_flow(
            &a,
            &b,
            FlowSettings {
                fraction: 0.,
                diagnostic: true,
                ..settings()
            },
            || false,
        )
        .unwrap()
        .unwrap();
    assert!(diagnostic.flow_report().is_some());
    assert!(
        renderer
            .read_frame(&diagnostic)
            .unwrap()
            .pixels
            .iter()
            .all(|p| p[3] == 1. && (p[2] - 0.15).abs() < 1e-6)
    );
    renderer.clear_frame_cache().unwrap();
    let rebuilt = renderer
        .interpolate_flow(&a, &b, settings(), || false)
        .unwrap()
        .unwrap();
    assert!(!rebuilt.flow_report().unwrap().cache_hit);
}

#[test]
#[ignore = "requires Vulkan"]
fn temporal_node_uses_source_cadence_and_reuses_source_frames() {
    let renderer = Renderer::new().unwrap();
    let mut doc = blank(96, 64, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation = Operation::Image(nodes::Image {
        asset: "source".into(),
    });
    let mut inputs = ImageInputs::default();
    inputs.insert("source", Arc::new(Image::new(96, 64, texture(0)).unwrap()));
    let motion = Node::new(
        "Motion",
        Operation::Transform(nodes::Transform {
            x: Scalar::Linear {
                keys: [(0, 0.), (1, 6.)]
                    .map(|(ticks, value)| Key {
                        time: Time { ticks, rate: 24 },
                        value,
                        segment: None,
                    })
                    .into(),
            },
            y: 0.0.into(),
        }),
        &[("image", doc.output)],
    );
    let flow = Node::new(
        "Flow",
        serde_json::from_value(registry::OPTICAL_FLOW.default_operation()).unwrap(),
        &[("image", motion.id)],
    );
    doc.output = flow.id;
    doc.nodes.insert(motion.id, motion);
    doc.nodes.insert(flow.id, flow);
    let plan = Plan::compile(&doc).unwrap();
    let request = RenderRequest::full(Time::new(1, 24).unwrap());
    let first = renderer
        .render_gpu_temporal(&plan, request, &inputs, true, || false)
        .unwrap()
        .unwrap();
    assert!(!first.flow_report().unwrap().cache_hit);
    let second = renderer
        .render_gpu_temporal(&plan, request, &inputs, true, || false)
        .unwrap()
        .unwrap();
    assert!(second.flow_report().unwrap().cache_hit);
    assert_eq!(
        renderer.read_frame(&first).unwrap().pixels,
        renderer.read_frame(&second).unwrap().pixels
    );
    let exact = renderer
        .render_gpu_temporal(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
            true,
            || false,
        )
        .unwrap()
        .unwrap();
    assert_eq!(renderer.read_frame(&exact).unwrap().pixels, texture(0));
    if let Operation::OpticalFlow(flow) = &mut doc.nodes.get_mut(&doc.output).unwrap().operation {
        flow.diagnostic = true;
    }
    let diagnostic = renderer
        .render_gpu_temporal(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
            true,
            || false,
        )
        .unwrap()
        .unwrap();
    assert!(diagnostic.flow_report().is_some());
}
#[test]
#[ignore = "requires Vulkan"]
fn a_picture_of_the_next_frame_is_carried_back_along_the_motion() {
    let renderer = Renderer::new().unwrap();
    let a = frame(&renderer, 96, 64, texture(0));
    let b = frame(&renderer, 96, 64, texture(6));
    // A matte that belongs to frame b: opaque right of column 46 (40 in frame a, moved 6).
    let payload: Vec<f32> = (0..96 * 64)
        .map(|i| if i % 96 >= 46 { 1.0 } else { 0.0 })
        .collect();
    let carried = renderer.carry_along_motion(&a, &b, &payload, 512).unwrap();
    // Seen from frame a, the edge is back at column 40, and the motion is trusted there.
    for y in 16..48 {
        let row = |x: usize| carried[y * 96 + x];
        assert!(
            row(36)[0] < 0.1 && row(44)[0] > 0.9,
            "{:?} {:?}",
            row(36),
            row(44)
        );
        assert!(row(40)[1] > 0.5, "trust {:?}", row(40));
    }
    // Sizes must agree.
    assert!(
        renderer
            .carry_along_motion(&a, &b, &payload[1..], 512)
            .is_err()
    );
}
