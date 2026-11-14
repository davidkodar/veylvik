use veylvik_document::*;
use veylvik_render::*;
#[test]
#[ignore = "requires Vulkan"]
fn bezier_exposure_and_mixed_hold_match_analytic_values_and_cache_updates() {
    let source = Node::new(
        "source",
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [0.25, 0.5, 0.75, 1.0],
        }),
        &[],
    );
    let curve = Scalar::Linear {
        keys: vec![
            Key {
                time: Time::new(0, 1).unwrap(),
                value: 0.0,
                segment: Some(Segment::Bezier {
                    outgoing: Handle {
                        time: 1.0 / 3.0,
                        value: 2.0,
                    },
                    incoming: Handle {
                        time: 1.0 / 3.0,
                        value: 1.0,
                    },
                }),
            },
            Key {
                time: Time::new(2, 1).unwrap(),
                value: 1.0,
                segment: Some(Segment::Hold),
            },
            Key {
                time: Time::new(4, 1).unwrap(),
                value: 3.0,
                segment: None,
            },
        ],
    };
    let output = Node::new(
        "animated",
        Operation::Exposure(veylvik_document::nodes::Exposure { stops: curve }),
        &[("image", source.id)],
    );
    let id = output.id;
    let mut doc = Composition {
        ocio: Default::default(),
        working_space: Default::default(),
        display: Default::default(),
        groups: Default::default(),
        schema: 1,
        width: 13,
        height: 7,
        frame_rate: Time::new(24, 1).unwrap(),
        frame_count: 97,
        output: id,
        nodes: [(source.id, source), (id, output)].into(),
    };
    let renderer = Renderer::new().unwrap();
    let inputs = ImageInputs::default();
    let plan = Plan::compile(&doc).unwrap();
    for (ticks, expected) in [(1, 1.625f32), (3, 1.0), (4, 3.0)] {
        let frame = renderer
            .render_gpu_cached(
                &plan,
                RenderRequest::full(Time::new(ticks, 1).unwrap()),
                &inputs,
            )
            .unwrap();
        let pixels = renderer.read_frame(&frame).unwrap().pixels;
        for p in pixels {
            for (a, b) in p.into_iter().zip([
                0.25 * expected.exp2(),
                0.5 * expected.exp2(),
                0.75 * expected.exp2(),
                1.0,
            ]) {
                assert!((a - b).abs() < 2e-5, "{a} != {b}");
            }
        }
    }
    doc.nodes
        .get_mut(&id)
        .unwrap()
        .operation
        .scalar_mut("stops")
        .unwrap()
        .keys_mut()
        .unwrap()[0]
        .segment = Some(Segment::Linear);
    assert!(!plan.matches_composition(&doc, id).unwrap());
    let updated = Plan::compile(&doc).unwrap();
    let frame = renderer
        .render_gpu_cached(
            &updated,
            RenderRequest::full(Time::new(1, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert!(
        (renderer.read_frame(&frame).unwrap().pixels[0][0] - 0.25 * 0.5f32.exp2()).abs() < 1e-6
    );
}
