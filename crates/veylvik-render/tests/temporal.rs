use std::sync::Arc;
use veylvik_document::{nodes, *};
use veylvik_render::*;
fn moving() -> (Composition, ImageInputs) {
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
                            [2., 0.5, 0.25, 0.5]
                        } else {
                            [0.; 4]
                        }
                    })
                    .collect(),
            )
            .unwrap(),
        ),
    );
    let transform = Node::new(
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
    doc.output = transform.id;
    doc.nodes.insert(transform.id, transform);
    (doc, inputs)
}
fn append(doc: &mut Composition, operation: Operation) {
    let node = Node::new("Temporal", operation, &[("image", doc.output)]);
    doc.output = node.id;
    doc.nodes.insert(node.id, node);
}
#[test]
#[ignore = "requires Vulkan"]
fn branch_shutter_matches_reference_and_remap_freezes_source() {
    let renderer = Renderer::new().unwrap();
    let (mut doc, inputs) = moving();
    let time = Time::new(0, 1).unwrap();
    let request = RenderRequest::full(time);
    let base = Plan::compile(&doc).unwrap();
    let reference = renderer
        .render_gpu_shutter(
            &base,
            request,
            &inputs,
            Shutter {
                angle: 360.,
                phase: 0.,
                samples: 8,
                frame_rate: doc.frame_rate,
            },
            || false,
        )
        .unwrap()
        .unwrap();
    let expected = renderer.read_frame(&reference).unwrap().pixels;
    drop(reference);
    append(
        &mut doc,
        Operation::MotionBlur(nodes::MotionBlur {
            angle: 360.0.into(),
            phase: 0.0.into(),
            preview_samples: 1.0.into(),
            final_samples: 8.0.into(),
        }),
    );
    let plan = Plan::compile(&doc).unwrap();
    assert_eq!(
        plan.required_assets().into_iter().collect::<Vec<_>>(),
        ["shape"]
    );
    let actual = renderer.render_frame(&plan, request, &inputs).unwrap();
    assert_eq!(actual.pixels, expected);
    let preview = renderer
        .render_gpu_temporal(&plan, request, &inputs, true, || false)
        .unwrap()
        .unwrap();
    let sharp = renderer.render_frame(&base, request, &inputs).unwrap();
    assert_eq!(renderer.read_frame(&preview).unwrap().pixels, sharp.pixels);
    drop(preview);
    append(
        &mut doc,
        Operation::TimeRemap(nodes::TimeRemap {
            rate: 0.0.into(),
            source_time: 0.0.into(),
        }),
    );
    let plan = Plan::compile(&doc).unwrap();
    assert_eq!(
        renderer
            .render_frame(
                &plan,
                RenderRequest::full(Time::new(10, 1).unwrap()),
                &inputs
            )
            .unwrap()
            .pixels,
        expected
    );
    let held = renderer.resource_stats().unwrap().held_bytes;
    let mut calls = 0;
    assert!(
        renderer
            .render_gpu_temporal(&plan, request, &inputs, false, || {
                calls += 1;
                calls > 5
            })
            .unwrap()
            .is_none()
    );
    assert_eq!(renderer.resource_stats().unwrap().held_bytes, held);
}
#[test]
fn temporal_nesting_is_bounded_and_roundtrips() {
    let (mut doc, _) = moving();
    for _ in 0..8 {
        append(
            &mut doc,
            Operation::TimeRemap(nodes::TimeRemap {
                rate: 1.0.into(),
                source_time: 0.0.into(),
            }),
        );
    }
    let restored: Composition = serde_json::from_slice(&serde_json::to_vec(&doc).unwrap()).unwrap();
    assert!(Plan::compile(&restored).is_ok());
    append(
        &mut doc,
        Operation::TimeRemap(nodes::TimeRemap {
            rate: 1.0.into(),
            source_time: 0.0.into(),
        }),
    );
    assert!(Plan::compile(&doc).is_err());
}

#[test]
fn retime_preserves_fractional_frame_boundaries() {
    for fps in [
        Time::new(24, 1).unwrap(),
        Time::new(24000, 1001).unwrap(),
        Time::new(30000, 1001).unwrap(),
    ] {
        for (rate, frame, target) in [(2., 1, 2), (0.5, 2, 1), (-1., 2, -2), (0., 19, 0)] {
            let remap = nodes::TimeRemap {
                rate: rate.into(),
                source_time: 0.0.into(),
            };
            let time = remap
                .source_at(Time::at_frame(frame, fps).unwrap())
                .unwrap();
            assert!(
                time.compare(Time::at_frame(target, fps).unwrap())
                    .unwrap()
                    .is_eq()
            );
        }
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn retimed_branch_does_not_shift_other_input_or_publish_stale_cached_frames() {
    let renderer = Renderer::new().unwrap();
    let (mut doc, inputs) = moving();
    let original = doc.output;
    append(
        &mut doc,
        Operation::TimeRemap(nodes::TimeRemap {
            rate: 0.0.into(),
            source_time: 0.0.into(),
        }),
    );
    let frozen = doc.output;
    let merge = Node::new(
        "Independent branches",
        Operation::Merge(nodes::Merge {}),
        &[("background", frozen), ("foreground", original)],
    );
    doc.output = merge.id;
    doc.nodes.insert(merge.id, merge);
    let plan = Plan::compile(&doc).unwrap();
    let at = |time| {
        renderer
            .read_frame(
                &renderer
                    .render_gpu_cached(&plan, RenderRequest::full(time), &inputs)
                    .unwrap(),
            )
            .unwrap()
            .pixels
    };
    let zero = Time::new(0, 1).unwrap();
    let later = Time::new(1, 48).unwrap();
    let first = at(zero);
    let moved = at(later);
    assert_ne!(first, moved);
    assert_eq!(moved[15][3], 0.5, "frozen branch stays centered");
    assert_eq!(moved[23][3], 0.5, "other branch moves independently");
    assert_eq!(at(zero), first, "seeking back is deterministic");
}
