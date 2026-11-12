use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;
#[test]
#[ignore = "requires Vulkan"]
fn effect_recipes_match_impulse_glow_and_outline_references() {
    let renderer = Renderer::new().unwrap();
    let mut doc = blank(25, 19, Time::new(24, 1).unwrap()).unwrap();
    let input = doc.output;
    doc.nodes.get_mut(&input).unwrap().operation = Operation::Image {
        asset: "point".into(),
    };
    let color = [0.25, 0.5, 0.75, 1.0];
    let mut pixels = vec![[0.0; 4]; 25 * 19];
    pixels[9 * 25 + 12] = color;
    let mut inputs = ImageInputs::default();
    inputs.insert("point", Arc::new(Image::new(25, 19, pixels).unwrap()));
    for recipe in recipes::Recipe::ALL {
        let effect = recipe.build(&doc, input).unwrap();
        let root = effect.root;
        let mut document = Document::new(doc.clone()).unwrap();
        document.apply(0, &effect.into_commands()).unwrap();
        let plan = Plan::compile_output(document.snapshot(), root).unwrap();
        let frame = renderer
            .render_frame(
                &plan,
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap();
        for (index, pixel) in frame.pixels.iter().enumerate() {
            let (dx, dy) = (
                (index as i32 % 25 - 12).abs(),
                (index as i32 / 25 - 9).abs(),
            );
            let center = dx == 0 && dy == 0;
            let expected = match recipe {
                recipes::Recipe::Outline => {
                    if center {
                        color
                    } else if dx <= 4 && dy <= 4 {
                        [1.0; 4]
                    } else {
                        [0.0; 4]
                    }
                }
                recipes::Recipe::Glow => {
                    let coverage = if dx <= 8 && dy <= 8 { 1.0 / 289.0 } else { 0.0 };
                    let mut expected = [0.0; 4];
                    for channel in 0..3 {
                        expected[channel] = color[channel] * coverage * 2.0
                            + if center { color[channel] } else { 0.0 };
                    }
                    expected[3] = if center { 1.0 } else { coverage };
                    expected
                }
            };
            for channel in 0..4 {
                assert!(
                    (pixel[channel] - expected[channel]).abs() < 2e-6,
                    "{recipe:?} {index}: {pixel:?} vs {expected:?}"
                );
            }
        }
    }
}
