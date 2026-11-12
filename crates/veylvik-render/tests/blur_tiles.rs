use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

#[test]
#[ignore = "requires Vulkan"]
fn blur_matches_reference_across_row_tiles_partial_edges_and_radius_paths() {
    let renderer = Renderer::new().unwrap();
    for (width, height) in [(1u32, 1u32), (83, 17)] {
        let mut doc = blank(width, height, Time::new(24, 1).unwrap()).unwrap();
        let image = doc.output;
        doc.nodes.get_mut(&image).unwrap().operation = Operation::Image {
            asset: "plate".into(),
        };
        let blur = Node::new(
            "Blur",
            Operation::Blur { radius: 1.0.into() },
            &[("image", image)],
        );
        doc.output = blur.id;
        doc.nodes.insert(blur.id, blur);
        let pixels: Vec<_> = (0..width * height)
            .map(|index| {
                let alpha = (index % 7) as f32 / 6.0;
                [
                    ((index * 11) % 37) as f32 * 0.125 * alpha,
                    -0.75 * alpha,
                    0.5 * alpha,
                    alpha,
                ]
            })
            .collect();
        let mut inputs = ImageInputs::default();
        inputs.insert(
            "plate",
            Arc::new(Image::new(width, height, pixels.clone()).unwrap()),
        );
        for radius in [0.5f32, 1.0, 2.5, 3.0, 8.0, 31.5, 32.0] {
            doc.nodes.get_mut(&doc.output).unwrap().operation = Operation::Blur {
                radius: radius.into(),
            };
            let frame = renderer
                .render_frame(
                    &Plan::compile(&doc).unwrap(),
                    RenderRequest::full(Time::new(0, 1).unwrap()),
                    &inputs,
                )
                .unwrap();
            let radius = radius.round_ties_even() as i32;
            let mut expected: Vec<_> = pixels.iter().map(|p| p.map(f64::from)).collect();
            for (dx, dy) in [(1, 0), (0, 1)] {
                let before = expected.clone();
                for y in 0..height as i32 {
                    for x in 0..width as i32 {
                        let mut sum = [0.0; 4];
                        for offset in -radius..=radius {
                            let (sx, sy) = (x + dx * offset, y + dy * offset);
                            if sx >= 0 && sy >= 0 && sx < width as i32 && sy < height as i32 {
                                for (total, value) in sum
                                    .iter_mut()
                                    .zip(before[(sy as u32 * width + sx as u32) as usize])
                                {
                                    *total += value;
                                }
                            }
                        }
                        expected[(y as u32 * width + x as u32) as usize] =
                            sum.map(|value| value / (2 * radius + 1) as f64);
                    }
                }
            }
            for (index, (pixel, expected)) in frame.pixels.iter().zip(expected).enumerate() {
                for (value, expected) in pixel.iter().zip(expected) {
                    assert!(
                        (*value as f64 - expected).abs() < 2e-6,
                        "{width}x{height}, radius={radius}, pixel={index}: {pixel:?} != {expected}"
                    );
                }
            }
        }
    }
}
