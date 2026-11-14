use std::sync::Arc;
use veylvik_document::{nodes, *};
use veylvik_render::*;
#[test]
#[ignore = "requires Vulkan"]
fn shutter_preserves_hdr_alpha_and_cancels_without_partial_output() {
    let renderer = Renderer::new().unwrap();
    let mut doc = blank(32, 16, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation = Operation::Image(nodes::Image {
        asset: "shape".into(),
    });
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "shape",
        Arc::new(
            Image::new(
                32,
                16,
                (0..512)
                    .map(|i| {
                        if (14..18).contains(&(i % 32)) {
                            [2.0, 0.5, 0.25, 0.5]
                        } else {
                            [0.; 4]
                        }
                    })
                    .collect(),
            )
            .unwrap(),
        ),
    );
    let shutter = Shutter {
        angle: 360.,
        phase: 0.,
        samples: 8,
        frame_rate: doc.frame_rate,
    };
    let request = RenderRequest::full(Time::new(0, 1).unwrap());
    let plan = Plan::compile(&doc).unwrap();
    let original = renderer
        .render_frame(&plan, request, &inputs)
        .unwrap()
        .pixels;
    let frame = renderer
        .render_gpu_shutter(&plan, request, &inputs, shutter, || false)
        .unwrap()
        .unwrap();
    assert_eq!(renderer.read_frame(&frame).unwrap().pixels, original);
    drop(frame);
    let moving = Node::new(
        "Move",
        Operation::Transform(nodes::Transform {
            x: Scalar::Linear {
                keys: vec![
                    Key {
                        time: Time::new(-1, 24).unwrap(),
                        value: -16.,
                        segment: None,
                    },
                    Key {
                        time: Time::new(1, 24).unwrap(),
                        value: 16.,
                        segment: None,
                    },
                ],
            },
            y: 0.0.into(),
        }),
        &[("image", doc.output)],
    );
    doc.output = moving.id;
    doc.nodes.insert(moving.id, moving);
    let plan = Plan::compile(&doc).unwrap();
    let frame = renderer
        .render_gpu_shutter(&plan, request, &inputs, shutter, || false)
        .unwrap()
        .unwrap();
    let pixels = renderer.read_frame(&frame).unwrap().pixels;
    let total: f32 = pixels.iter().map(|p| p[3]).sum();
    assert!(
        (total - 32.).abs() < 0.0001,
        "motion preserves alpha coverage"
    );
    assert!(
        pixels[8][3] > 0. && pixels[23][3] > 0.,
        "temporal motion extends the shape"
    );
    assert!(
        pixels[15][3] < 0.5,
        "moving shape spends only part of the exposure here"
    );
    assert!(
        pixels.iter().all(|p| (p[0] - 4. * p[3]).abs() < 0.00001),
        "premultiplied HDR colour must not darken at edges"
    );
    drop(frame);
    let held = renderer.resource_stats().unwrap().held_bytes;
    let mut checks = 0;
    let result = renderer
        .render_gpu_shutter(&plan, request, &inputs, shutter, || {
            checks += 1;
            checks >= 5
        })
        .unwrap();
    assert!(result.is_none());
    assert_eq!(renderer.resource_stats().unwrap().held_bytes, held);
    let zero = renderer
        .render_gpu_shutter(
            &plan,
            request,
            &inputs,
            Shutter {
                angle: 0.,
                ..shutter
            },
            || false,
        )
        .unwrap()
        .unwrap();
    assert_eq!(renderer.read_frame(&zero).unwrap().pixels, original);
}
