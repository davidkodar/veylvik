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
            clip_black: c(0.0),
            clip_white: c(1.0),
            shrink: c(0.0),
            soften: c(0.0),
            darks: c(0.0),
            show: veylvik_document::nodes::KeyView::Result,
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
        clip_black: c(0.0),
        clip_white: c(1.0),
        shrink: c(0.0),
        soften: c(0.0),
        darks: c(0.0),
        show: veylvik_document::nodes::KeyView::Result,
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

#[test]
#[ignore = "requires Vulkan"]
fn a_shaped_key_keeps_darks_clips_shrinks_and_shows_its_matte() {
    use veylvik_document::nodes::{ChromaKey, KeyView};
    let renderer = Renderer::new().unwrap();
    let c = |v: f32| Scalar::Constant { value: v };
    let key = |change: &dyn Fn(&mut ChromaKey)| {
        let mut key = ChromaKey {
            key_color: [0.08, 0.45, 0.12, 1.0],
            tolerance: c(0.5),
            softness: c(1.0),
            despill: c(1.0),
            clip_black: c(0.0),
            clip_white: c(1.0),
            shrink: c(0.0),
            soften: c(0.0),
            darks: c(0.0),
            show: KeyView::Result,
        };
        change(&mut key);
        Operation::ChromaKey(key)
    };
    let source = vec![
        [0.08, 0.45, 0.12, 1.0],    // lit screen
        [0.08, 0.45, 0.12, 1.0],    // lit screen
        [0.004, 0.009, 0.004, 1.0], // dark hair leaning green
        [0.30, 0.36, 0.30, 1.0],    // a veil: leans a little to the screen
        [0.6, 0.4, 0.3, 1.0],       // skin
        [0.6, 0.4, 0.3, 1.0],       // skin
        [0.6, 0.4, 0.3, 1.0],       // skin
        [0.6, 0.4, 0.3, 1.0],       // skin
    ];
    // By colour alone, the dark hair goes partly and the veil stays partly.
    let plain = render(&renderer, &source, key(&|_| {}));
    assert!(
        plain[2][3] < 0.9 && plain[3][3] > 0.05 && plain[3][3] < 0.95,
        "{plain:?}"
    );
    // Keep darks holds the hair; the screen still goes.
    let darks = render(&renderer, &source, key(&|k| k.darks = c(0.05)));
    assert_eq!(darks[2][3], 1.0);
    assert_eq!(darks[0][3], 0.0);
    // Clip black clears the veil, clip white would fill it.
    let veil = plain[3][3];
    let cleared = render(&renderer, &source, key(&|k| k.clip_black = c(veil + 0.02)));
    assert_eq!(cleared[3][3], 0.0);
    let filled = render(&renderer, &source, key(&|k| k.clip_white = c(veil - 0.02)));
    assert_eq!(filled[3][3], 1.0);
    // Shrink eats two pixels into the skin; growing gives them back to the veil's side.
    let shrunk = render(
        &renderer,
        &source,
        key(&|k| {
            k.clip_black = c(veil + 0.02);
            k.shrink = c(2.0);
        }),
    );
    assert_eq!([shrunk[4][3], shrunk[5][3], shrunk[6][3]], [0.0, 0.0, 1.0]);
    // Soften turns the hard step into a ramp.
    let soft = render(
        &renderer,
        &source,
        key(&|k| {
            k.clip_black = c(veil + 0.02);
            k.soften = c(2.0);
        }),
    );
    assert!(
        soft[3][3] > 0.0 && soft[3][3] < soft[4][3] && soft[4][3] < 1.0,
        "{soft:?}"
    );
    // The matte view: opaque grey equal to the alpha of the keyed picture.
    let matte = render(&renderer, &source, key(&|k| k.show = KeyView::Matte));
    for (grey, keyed) in matte.iter().zip(&plain) {
        assert!(
            (grey[0] - keyed[3]).abs() < 1e-6 && grey[3] == 1.0,
            "{grey:?} {keyed:?}"
        );
    }
}
