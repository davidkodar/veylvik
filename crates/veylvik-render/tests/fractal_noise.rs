use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

fn hash(mut x: u32) -> u32 {
    x = (x ^ (x >> 16)).wrapping_mul(0x7feb352d);
    x = (x ^ (x >> 15)).wrapping_mul(0x846ca68b);
    x ^ (x >> 16)
}
fn reference(position: [f64; 3], seed: u32) -> f64 {
    let base = position.map(|v| v.floor() as i32);
    let weight = position.map(|v| {
        let t = v - v.floor();
        6.0 * t.powi(5) - 15.0 * t.powi(4) + 10.0 * t.powi(3)
    });
    let mut total = 0.0;
    for z in 0..2 {
        for y in 0..2 {
            for x in 0..2 {
                let lattice = [base[0] + x, base[1] + y, base[2] + z].map(|v| v as u32);
                let value = hash(lattice[0] ^ hash(lattice[1] ^ hash(lattice[2] ^ seed)));
                let w: f64 = [x, y, z]
                    .into_iter()
                    .zip(weight)
                    .map(|(i, w)| if i == 0 { 1.0 - w } else { w })
                    .product();
                total += (value & 0xffffff) as f64 / 16777215.0 * w;
            }
        }
    }
    total
}
#[test]
#[ignore = "requires Vulkan"]
fn fractal_noise_matches_lattice_reference_and_animated_cache_identity() {
    let renderer = Renderer::new().unwrap();
    let inputs = ImageInputs::default();
    for seed in [0, u32::MAX] {
        for (scale, octaves, roughness) in [
            (8.0, 1.0, 0.0),
            (4.0, 3.0, 0.5),
            (1.0, 8.0, 1.0),
            (8192.0, 2.6, 0.25),
        ] {
            let node = Node::new(
                "noise",
                Operation::FractalNoise(veylvik_document::nodes::FractalNoise {
                    seed,
                    scale: scale.into(),
                    octaves: octaves.into(),
                    roughness: roughness.into(),
                    evolution: Scalar::Linear {
                        keys: vec![
                            Key {
                                segment: None,
                                time: Time::new(0, 1).unwrap(),
                                value: -1.25,
                            },
                            Key {
                                segment: None,
                                time: Time::new(2, 1).unwrap(),
                                value: 0.75,
                            },
                        ],
                    },
                }),
                &[],
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
                width: 13,
                height: 9,
                frame_rate: Time::new(24, 1).unwrap(),
                output: node.id,
                nodes: [(node.id, node)].into(),
            };
            let plan = Plan::compile(&doc).unwrap();
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
                for (i, pixel) in actual.pixels.iter().enumerate() {
                    let mut pos = [
                        (i % 13) as f64 / scale as f64,
                        (i / 13) as f64 / scale as f64,
                        -1.25 + tick as f64,
                    ];
                    let mut total = 0.0;
                    let mut sum = 0.0;
                    let mut amplitude = 1.0;
                    for octave in 0..octaves.round() as u32 {
                        total += amplitude * reference(pos, hash(seed.wrapping_add(octave)));
                        sum += amplitude;
                        amplitude *= roughness as f64;
                        pos = pos.map(|v| v * 2.0);
                    }
                    assert!(
                        (pixel[0] as f64 - total / sum).abs() < 2e-6,
                        "{seed} {scale} {octaves} {tick} {i}: {pixel:?} != {}",
                        total / sum
                    );
                    assert_eq!(pixel[0], pixel[1]);
                    assert_eq!(pixel[1], pixel[2]);
                    assert_eq!(pixel[3], 1.0);
                }
                previous = Some(frame);
            }
        }
    }
}
