use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;
fn composition(nodes: Vec<Node>, output: NodeId) -> Composition {
    Composition {
        schema: 1,
        width: 13,
        height: 7,
        frame_rate: Time::new(24, 1).unwrap(),
        nodes: nodes.into_iter().map(|n| (n.id, n)).collect(),
        output,
    }
}
fn sample(pixels: &[[f32; 4]], x: i32, y: i32) -> [f64; 4] {
    if (0..13).contains(&x) && (0..7).contains(&y) {
        pixels[(y * 13 + x) as usize].map(f64::from)
    } else {
        [0.0; 4]
    }
}
#[test]
#[ignore = "requires Vulkan"]
fn affine_matches_independent_inverse_sampling_and_degenerate_cases() {
    let renderer = Renderer::new().unwrap();
    let source = Node::new(
        "source",
        Operation::Image {
            asset: "plate".into(),
        },
        &[],
    );
    let pixels: Vec<_> = (0..91)
        .map(|i| [i as f32 * 0.01, 0.25, -0.1, 0.5])
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "plate",
        Arc::new(Image::new(13, 7, pixels.clone()).unwrap()),
    );
    for (sx, sy, angle, tx, ty) in [
        (1.0, 1.0, 0.0, 0.0, 0.0),
        (1.0, 1.0, 90.0, 0.0, 0.0),
        (-1.3, 0.7, 23.0, 0.3, -0.2),
        (2.0, 2.0, -45.0, 2.0, 1.0),
        (0.0, 1.0, 0.0, 0.0, 0.0),
        (1.0e-8, 1.0, 0.0, 0.0, 0.0),
        (1.0, 1.0, 0.0, 1_000_000.0, 0.0),
    ] {
        let operation = Operation::Affine {
            x: tx.into(),
            y: ty.into(),
            scale_x: sx.into(),
            scale_y: sy.into(),
            rotation: angle.into(),
            pivot_x: 6.0.into(),
            pivot_y: 3.0.into(),
        };
        let node = Node::new("affine", operation, &[("image", source.id)]);
        let id = node.id;
        let plan = Plan::compile(&composition(vec![source.clone(), node], id)).unwrap();
        let actual = renderer
            .render_frame(
                &plan,
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap();
        for y in 0..7 {
            for x in 0..13 {
                let expected = if sx.abs() < 1e-6 || sy.abs() < 1e-6 {
                    [0.0; 4]
                } else {
                    let (sin, cos) = (angle as f64).to_radians().sin_cos();
                    let (dx, dy) = (x as f64 - 6.0 - tx as f64, y as f64 - 3.0 - ty as f64);
                    let px = (dx * cos + dy * sin) / sx as f64 + 6.0;
                    let py = (-dx * sin + dy * cos) / sy as f64 + 3.0;
                    let (ix, iy) = (px.floor() as i32, py.floor() as i32);
                    let (fx, fy) = (px - px.floor(), py - py.floor());
                    let samples = [
                        sample(&pixels, ix, iy),
                        sample(&pixels, ix + 1, iy),
                        sample(&pixels, ix, iy + 1),
                        sample(&pixels, ix + 1, iy + 1),
                    ];
                    std::array::from_fn(|c| {
                        (samples[0][c] * (1.0 - fx) + samples[1][c] * fx) * (1.0 - fy)
                            + (samples[2][c] * (1.0 - fx) + samples[3][c] * fx) * fy
                    })
                };
                for (value, expected) in actual.pixels[y * 13 + x].iter().zip(expected) {
                    assert!(
                        (*value as f64 - expected).abs() < 2e-5,
                        "scale={sx},{sy}, angle={angle}, pixel={x},{y}: {value} != {expected}"
                    );
                }
            }
        }
    }
}
#[test]
#[ignore = "requires Vulkan"]
fn ellipse_coverage_and_alpha_mask_preserve_hdr_premultiplication() {
    let renderer = Renderer::new().unwrap();
    let solid = Node::new(
        "HDR",
        Operation::Solid {
            color: [4.0, -2.0, 1.0, 0.5],
        },
        &[],
    );
    for radius_y in [2.0, 0.0] {
        let ellipse = Node::new(
            "ellipse",
            Operation::Ellipse {
                x: 6.0.into(),
                y: 3.0.into(),
                radius_x: 4.0.into(),
                radius_y: radius_y.into(),
                feather: 2.0.into(),
            },
            &[],
        );
        let mask = Node::new(
            "mask",
            Operation::Mask {},
            &[("image", solid.id), ("mask", ellipse.id)],
        );
        let id = mask.id;
        let doc = composition(vec![solid.clone(), ellipse, mask], id);
        let pixels = renderer
            .render(&Plan::compile(&doc).unwrap(), Time::new(0, 1).unwrap())
            .unwrap();
        for y in 0..7 {
            for x in 0..13 {
                let alpha = if radius_y == 0.0 {
                    0.0
                } else {
                    let distance =
                        (((x as f64 - 6.0) / 4.0).hypot((y as f64 - 3.0) / 2.0) - 1.0) * 2.0;
                    let t = ((distance + 1.0) / 2.0).clamp(0.0, 1.0);
                    1.0 - t * t * (3.0 - 2.0 * t)
                };
                for (actual, base) in pixels[y * 13 + x].iter().zip([2.0, -1.0, 0.5, 0.5]) {
                    assert!((*actual as f64 - base * alpha).abs() < 2e-6);
                }
            }
        }
    }
    let image = Node::new(
        "mask image",
        Operation::Image {
            asset: "mask".into(),
        },
        &[],
    );
    let mask = Node::new(
        "mask",
        Operation::Mask {},
        &[("image", solid.id), ("mask", image.id)],
    );
    let id = mask.id;
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "mask",
        Arc::new(Image::new(1, 1, vec![[100.0, -25.0, 8.0, 0.25]]).unwrap()),
    );
    let pixels = renderer
        .render_frame(
            &Plan::compile(&composition(vec![solid, image, mask], id)).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap()
        .pixels;
    assert_eq!(pixels[0], [0.5, -0.25, 0.125, 0.125]);
    assert_eq!(pixels[1], [0.0; 4]);
}
