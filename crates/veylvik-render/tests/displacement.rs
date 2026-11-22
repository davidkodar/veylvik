use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

#[test]
#[ignore = "requires Vulkan"]
fn displacement_matches_reference_for_alpha_hdr_borders_animation_and_cache() {
    let renderer = Renderer::new().unwrap();
    let (width, height) = (13u32, 9u32);
    let source: Vec<[f32; 4]> = (0..width * height)
        .map(|i| {
            let alpha = (i % 5) as f32 / 4.0;
            [i as f32 / 17.0 * alpha, -0.5 * alpha, 2.0 * alpha, alpha]
        })
        .collect();
    let map_values = [
        [0.5, 0.5, 0.0, 1.0],
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [0.125, 0.375, 0.0, 0.5],
        [0.0, 0.0, 0.0, 0.0],
        [9.0, -7.0, 0.0, 0.25],
        [0.75e-6, 0.25e-6, 0.0, 1e-6],
    ];
    let map: Vec<_> = (0..width * height)
        .map(|i| map_values[i as usize % map_values.len()])
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "source",
        Arc::new(Image::new(width, height, source.clone()).unwrap()),
    );
    inputs.insert(
        "map",
        Arc::new(Image::new(width, height, map.clone()).unwrap()),
    );
    let image = Node::new(
        "image",
        Operation::Image(veylvik_document::nodes::Image {
            asset: "source".into(),
        }),
        &[],
    );
    let field = Node::new(
        "map",
        Operation::Image(veylvik_document::nodes::Image {
            asset: "map".into(),
        }),
        &[],
    );
    let effect = Node::new(
        "displace",
        Operation::Displace(veylvik_document::nodes::Displace {
            x: Scalar::Linear {
                keys: vec![
                    Key {
                        segment: None,
                        time: Time::new(0, 1).unwrap(),
                        value: -3.5,
                    },
                    Key {
                        segment: None,
                        time: Time::new(2, 1).unwrap(),
                        value: 3.5,
                    },
                ],
            },
            y: 1.25.into(),
        }),
        &[("image", image.id), ("map", field.id)],
    );
    let doc = Composition {
        published: Default::default(),
        markers: Default::default(),
        backdrops: Default::default(),
        ocio: Default::default(),
        working_space: Default::default(),
        display: Default::default(),
        groups: Default::default(),
        schema: 1,
        frame_count: 240,
        width,
        height,
        frame_rate: Time::new(24, 1).unwrap(),
        output: effect.id,
        nodes: [image, field, effect]
            .into_iter()
            .map(|n| (n.id, n))
            .collect(),
    };
    let plan = Plan::compile(&doc).unwrap();
    assert_eq!(plan.pass_count(), 3);
    let mut previous = None;
    for tick in [0, 1, 2] {
        let request = RenderRequest::full(Time::new(tick, 1).unwrap());
        let frame = renderer.render_gpu_cached(&plan, request, &inputs).unwrap();
        assert!(Arc::ptr_eq(
            &frame,
            &renderer.render_gpu_cached(&plan, request, &inputs).unwrap()
        ));
        if let Some(old) = previous.take() {
            assert!(!Arc::ptr_eq(&frame, &old));
        }
        let actual = renderer.read_frame(&frame).unwrap();
        for y in 0..height {
            for x in 0..width {
                let i = (y * width + x) as usize;
                let m = map[i];
                // Independent straight-colour reference, with transparent maps neutral.
                let offset = |c: usize, strength: f64| {
                    if m[3] == 0.0 {
                        0.0
                    } else {
                        ((m[c] as f64 / m[3] as f64).clamp(0.0, 1.0) * 2.0 - 1.0)
                            * m[3] as f64
                            * strength
                    }
                };
                let sx = x as f64 - offset(0, -3.5 + 3.5 * tick as f64);
                let sy = y as f64 - offset(1, 1.25);
                let mut expected = [0.0f64; 4];
                for dy in 0..2 {
                    for dx in 0..2 {
                        let px = sx.floor() as i32 + dx;
                        let py = sy.floor() as i32 + dy;
                        if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                            continue;
                        }
                        let weight = (if dx == 0 {
                            1.0 - sx.fract().rem_euclid(1.0)
                        } else {
                            sx.fract().rem_euclid(1.0)
                        }) * (if dy == 0 {
                            1.0 - sy.fract().rem_euclid(1.0)
                        } else {
                            sy.fract().rem_euclid(1.0)
                        });
                        for c in 0..4 {
                            expected[c] +=
                                source[(py as u32 * width + px as u32) as usize][c] as f64 * weight;
                        }
                    }
                }
                for (got, expected) in actual.pixels[i].iter().zip(expected) {
                    assert!(
                        (*got as f64 - expected).abs() < 1e-5,
                        "tick {tick} pixel {x},{y}: {:?} vs {expected}",
                        actual.pixels[i]
                    );
                }
            }
        }
        previous = Some(frame);
    }
    // Source identity changes must invalidate the cached distortion result.
    inputs.insert(
        "map",
        Arc::new(
            Image::new(
                width,
                height,
                vec![[0.5, 0.5, 0.0, 1.0]; (width * height) as usize],
            )
            .unwrap(),
        ),
    );
    let neutral = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(2, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert!(!Arc::ptr_eq(&neutral, previous.as_ref().unwrap()));
    assert_eq!(renderer.read_frame(&neutral).unwrap().pixels, source);
}
