use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

/// Independent CPU model of the documented colour-difference key with despill.
fn chroma_reference(
    pixel: [f32; 4],
    key: [f32; 3],
    tolerance: f32,
    softness: f32,
    despill: f32,
) -> [f32; 4] {
    let a = pixel[3];
    if a <= 0.0 {
        return pixel;
    }
    let mut rgb = [0, 1, 2].map(|c| pixel[c] / a);
    let k = if key[1] >= key[0] && key[1] >= key[2] {
        1
    } else if key[2] >= key[0] {
        2
    } else {
        0
    };
    let lean = |c: [f32; 3]| {
        let others: Vec<f32> = (0..3).filter(|&i| i != k).map(|i| c[i]).collect();
        (c[k] - others[0].max(others[1])) / c[k].max(1e-6)
    };
    let positive = rgb.map(|v| v.max(0.0));
    let mut alpha = 1.0;
    if positive[0] * 0.2126 + positive[1] * 0.7152 + positive[2] * 0.0722 > 0.002 {
        let ratio = lean(positive) / lean(key).max(1e-6);
        let edge = 1.0 - tolerance;
        alpha = if softness > 0.0 {
            ((edge - ratio) / softness).clamp(0.0, 1.0)
        } else if ratio >= edge {
            0.0
        } else {
            1.0
        };
    }
    if despill > 0.0 {
        let others: Vec<f32> = (0..3).filter(|&i| i != k).map(|i| rgb[i]).collect();
        let limit = 0.5 * (others[0] + others[1]);
        if rgb[k] > limit {
            rgb[k] += (limit - rgb[k]) * despill;
        }
    }
    let out = alpha * a;
    [rgb[0] * out, rgb[1] * out, rgb[2] * out, out]
}

fn render(renderer: &Renderer, source: &[[f32; 4]], operation: Operation) -> Vec<[f32; 4]> {
    let mut doc = blank(source.len() as u32, 1, Time::new(24, 1).unwrap()).unwrap();
    let input = doc.output;
    doc.nodes.get_mut(&input).unwrap().operation =
        Operation::Image(veylvik_document::nodes::Image {
            asset: "input".into(),
        });
    let node = Node::new("Key", operation, &[("image", input)]);
    doc.output = node.id;
    doc.nodes.insert(node.id, node);
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "input",
        Arc::new(Image::new(doc.width, 1, source.to_vec()).unwrap()),
    );
    renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap()
        .pixels
}

#[test]
#[ignore = "requires Vulkan"]
fn chroma_key_removes_lit_shadowed_and_hdr_screen_keeps_subject_and_despills_edges() {
    let renderer = Renderer::new().unwrap();
    let source = vec![
        [0.08, 0.45, 0.12, 1.0],      // lit green screen
        [0.016, 0.09, 0.024, 1.0],    // the same screen in shadow
        [0.8, 4.5, 1.2, 1.0],         // HDR hotspot on the screen
        [0.6, 0.4, 0.3, 1.0],         // skin
        [0.5, 0.5, 0.5, 1.0],         // grey
        [0.0005, 0.001, 0.0005, 1.0], // near black
        [0.3, 0.4, 0.25, 1.0],        // greenish edge: kept, despilled
        [0.04, 0.2, 0.06, 0.5],       // semi-transparent screen pixel (premultiplied)
    ];
    let c = |v: f32| Scalar::Constant { value: v };
    for (key, tolerance, softness, despill) in [
        ([0.0, 1.0, 0.0], 0.5, 0.25, 1.0),
        ([0.08, 0.45, 0.12], 0.3, 0.1, 0.5),
        ([0.0, 1.0, 0.0], 0.5, 0.0, 0.0),
        ([0.05, 0.12, 0.45], 0.5, 0.25, 1.0),
    ] {
        let operation = Operation::ChromaKey(veylvik_document::nodes::ChromaKey {
            key_color: [key[0], key[1], key[2], 1.0],
            tolerance: c(tolerance),
            softness: c(softness),
            despill: c(despill),
        });
        operation.validate().unwrap();
        let pixels = render(&renderer, &source, operation);
        for (actual, pixel) in pixels.iter().zip(&source) {
            let expected = chroma_reference(*pixel, key, tolerance, softness, despill);
            for channel in 0..4 {
                assert!(
                    (actual[channel] - expected[channel]).abs()
                        <= 1e-5 * expected[channel].abs().max(1.0),
                    "key {key:?} pixel {pixel:?}: {actual:?} != {expected:?}"
                );
            }
        }
        if key == [0.0, 1.0, 0.0] && softness > 0.0 {
            // Screen gone everywhere, subject untouched in alpha, fringe despilled.
            for screen in [0, 1, 2, 7] {
                assert_eq!(pixels[screen][3], 0.0, "screen pixel {screen}");
            }
            for subject in [3, 4, 5, 6] {
                assert_eq!(pixels[subject][3], 1.0, "subject pixel {subject}");
            }
            assert!((pixels[6][1] - 0.275).abs() < 1e-6);
        }
    }
    let invalid = Operation::ChromaKey(veylvik_document::nodes::ChromaKey {
        key_color: [0.0, 0.0, 0.0, 1.0],
        tolerance: c(0.5),
        softness: c(0.2),
        despill: c(1.0),
    });
    assert!(invalid.validate().is_err());
}

#[test]
#[ignore = "requires Vulkan"]
fn luma_key_ramps_between_thresholds_and_inverts_when_swapped() {
    let renderer = Renderer::new().unwrap();
    let source: Vec<[f32; 4]> = [0.0, 0.05, 0.1, 0.15, 0.2, 3.0]
        .iter()
        .map(|v| [*v, *v, *v, 1.0])
        .collect();
    let c = |v: f32| Scalar::Constant { value: v };
    for (low, high) in [(0.05, 0.15), (0.15, 0.05), (0.1, 0.1)] {
        let pixels = render(
            &renderer,
            &source,
            Operation::LumaKey(veylvik_document::nodes::LumaKey {
                low: c(low),
                high: c(high),
            }),
        );
        for (actual, pixel) in pixels.iter().zip(&source) {
            let l = pixel[0];
            let alpha = if high != low {
                ((l - low) / (high - low)).clamp(0.0, 1.0)
            } else if l >= high {
                1.0
            } else {
                0.0
            };
            for channel in 0..4 {
                assert!(
                    (actual[channel] - pixel[channel] * alpha).abs() < 1e-6,
                    "low {low} high {high} at {l}: {actual:?}"
                );
            }
        }
    }
}
