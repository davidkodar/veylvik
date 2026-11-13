use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

#[test]
#[ignore = "requires Vulkan"]
fn blend_modes_match_straight_colour_reference_with_animated_opacity() {
    let renderer = Renderer::new().unwrap();
    let background = Node::new(
        "background",
        Operation::Image(veylvik_document::nodes::Image { asset: "b".into() }),
        &[],
    );
    let foreground = Node::new(
        "foreground",
        Operation::Image(veylvik_document::nodes::Image { asset: "f".into() }),
        &[],
    );
    // Transparent, partial, opaque, HDR and negative channels, including very small alpha.
    let colours = [
        [0.2, 0.7, 0.4],
        [1.8, -0.2, 0.9],
        [0.0, 0.0, 0.0],
        [0.9, 0.1, 0.6],
    ];
    let alphas = [0.0, 1.0e-6, 0.25, 0.8, 1.0];
    let mut pairs = Vec::new();
    for (i, b) in colours.iter().enumerate() {
        for ba in alphas {
            for fa in alphas {
                pairs.push((*b, ba, colours[(i + 1) % colours.len()], fa));
            }
        }
    }
    let premul = |c: [f32; 3], a: f32| [c[0] * a, c[1] * a, c[2] * a, a];
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "b",
        Arc::new(
            Image::new(
                100,
                1,
                pairs.iter().map(|(b, ba, _, _)| premul(*b, *ba)).collect(),
            )
            .unwrap(),
        ),
    );
    inputs.insert(
        "f",
        Arc::new(
            Image::new(
                100,
                1,
                pairs.iter().map(|(_, _, f, fa)| premul(*f, *fa)).collect(),
            )
            .unwrap(),
        ),
    );
    let mut blend = Node::new(
        "blend",
        Operation::Blend(veylvik_document::nodes::Blend {
            mode: BlendMode::Normal,
            opacity: Scalar::Linear {
                keys: vec![
                    Key {
                        segment: None,
                        time: Time::new(0, 1).unwrap(),
                        value: 0.0,
                    },
                    Key {
                        segment: None,
                        time: Time::new(2, 1).unwrap(),
                        value: 1.0,
                    },
                ],
            },
        }),
        &[("background", background.id), ("foreground", foreground.id)],
    );
    let mut previous = None;
    for mode in BlendMode::ALL {
        if let Operation::Blend(veylvik_document::nodes::Blend { mode: value, .. }) =
            &mut blend.operation
        {
            *value = mode;
        }
        let composition = Composition {
            groups: Default::default(),
            schema: 1,
            frame_count: 240,
            width: 100,
            height: 1,
            frame_rate: Time::new(24, 1).unwrap(),
            output: blend.id,
            nodes: [background.clone(), foreground.clone(), blend.clone()]
                .into_iter()
                .map(|n| (n.id, n))
                .collect(),
        };
        let plan = Plan::compile(&composition).unwrap();
        for tick in [0, 1, 2] {
            let request = RenderRequest::full(Time::new(tick, 1).unwrap());
            let frame = renderer.render_gpu_cached(&plan, request, &inputs).unwrap();
            if let Some(previous) = previous.take() {
                assert!(!Arc::ptr_eq(&frame, &previous));
            }
            assert!(Arc::ptr_eq(
                &frame,
                &renderer.render_gpu_cached(&plan, request, &inputs).unwrap()
            ));
            let actual = renderer.read_frame(&frame).unwrap();
            for (pixel, (b, ba, f, fa)) in actual.pixels.iter().zip(&pairs) {
                let ba = *ba as f64;
                let fa = *fa as f64 * tick as f64 / 2.0;
                let mut expected = [0.0; 4];
                expected[3] = fa + ba * (1.0 - fa);
                for c in 0..3 {
                    let b = b[c] as f64;
                    let f = f[c] as f64;
                    let mixed = match mode {
                        BlendMode::Normal => f,
                        BlendMode::Add => b + f,
                        BlendMode::Multiply => b * f,
                        BlendMode::Screen => 1.0 - (1.0 - b) * (1.0 - f),
                        BlendMode::Darken => b.min(f),
                        BlendMode::Lighten => b.max(f),
                        BlendMode::Difference => (b - f).abs(),
                    };
                    expected[c] = fa * (1.0 - ba) * f + fa * ba * mixed + (1.0 - fa) * ba * b;
                }
                for (actual, expected) in pixel.iter().zip(expected) {
                    assert!(
                        (*actual as f64 - expected).abs() < 1e-6,
                        "{mode:?} opacity={tick}/2: {pixel:?} != {expected}"
                    );
                }
            }
            previous = Some(frame);
        }
    }
}
