use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;
#[test]
#[ignore = "requires Vulkan"]
fn channel_masks_preserve_coverage_and_convert_hdr_without_alpha_division() {
    let renderer = Renderer::new().unwrap();
    let source = vec![
        [0.0, 0.0, 0.0, 0.0],
        [0.25, 0.5, 0.75, 1.0],
        [0.125, 0.25, 0.375, 0.5],
        [-4.0, 2.0, 0.25, 0.5],
        [1e-8, 2e-8, 3e-8, 1e-7],
        [f32::MAX, f32::MAX, f32::MAX, 1.0],
        [-f32::MAX, -f32::MAX, -f32::MAX, 1.0],
        [f32::MAX, -f32::MAX, f32::MAX, 0.5],
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
    ];
    let width = source.len() as u32;
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "source",
        Arc::new(Image::new(width, 1, source.clone()).unwrap()),
    );
    let image = Node::new(
        "source",
        Operation::Image(veylvik_document::nodes::Image {
            asset: "source".into(),
        }),
        &[],
    );
    let mut previous = None;
    for channel in MaskChannel::ALL {
        let mask = Node::new(
            "mask",
            Operation::ChannelMask(veylvik_document::nodes::ChannelMask { channel }),
            &[("image", image.id)],
        );
        let doc = Composition {
            display: Default::default(),
            groups: Default::default(),
            schema: 1,
            frame_count: 240,
            width,
            height: 1,
            frame_rate: Time::new(24, 1).unwrap(),
            output: mask.id,
            nodes: [image.clone(), mask]
                .into_iter()
                .map(|n| (n.id, n))
                .collect(),
        };
        let plan = Plan::compile(&doc).unwrap();
        let request = RenderRequest::full(Time::new(0, 1).unwrap());
        let frame = renderer.render_gpu_cached(&plan, request, &inputs).unwrap();
        if let Some(old) = previous.take() {
            assert!(!Arc::ptr_eq(&frame, &old));
        }
        assert!(Arc::ptr_eq(
            &frame,
            &renderer.render_gpu_cached(&plan, request, &inputs).unwrap()
        ));
        let actual = renderer.read_frame(&frame).unwrap();
        for (pixel, source) in actual.pixels.iter().zip(&source) {
            let c = source.map(f64::from);
            let expected = match channel {
                MaskChannel::Luminance => c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722,
                MaskChannel::Red => c[0],
                MaskChannel::Green => c[1],
                MaskChannel::Blue => c[2],
                MaskChannel::Alpha => c[3],
            }
            .clamp(0.0, c[3]);
            for value in pixel {
                assert!(
                    (*value as f64 - expected).abs() < 1e-7,
                    "{channel:?} {source:?}: {pixel:?} != {expected}"
                );
            }
            assert!(pixel[3] <= source[3]);
        }
        previous = Some(frame);
    }
}
