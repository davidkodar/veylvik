use std::sync::Arc;
use veylvik_document::{media::Placement, *};
use veylvik_render::*;
#[test]
#[ignore = "requires Vulkan"]
fn placement_samples_full_source_before_clipping_and_preserves_alpha() {
    let renderer = Renderer::new().unwrap();
    // A larger, wide source: its right-hand pixels lie beyond the old canvas crop.
    let pixels: Vec<_> = (0..32).map(|i| [i as f32 / 64.0, 0.0, 0.0, 0.5]).collect();
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "source",
        Arc::new(Image::new(8, 4, pixels.clone()).unwrap()),
    );
    for (mode, scale, offset) in [
        (Placement::Legacy, [1.0, 1.0], [0.0, 0.0]),
        (Placement::Original, [1.0, 1.0], [-2.0, 0.0]),
        (Placement::Fit, [0.5, 0.5], [0.0, 1.0]),
        (Placement::Fill, [1.0, 1.0], [-2.0, 0.0]),
        (Placement::Stretch, [0.5, 1.0], [0.0, 0.0]),
    ] {
        let mut doc = blank(4, 4, Time::new(24, 1).unwrap()).unwrap();
        doc.nodes.get_mut(&doc.output).unwrap().operation =
            Operation::Footage(veylvik_document::nodes::Footage {
                asset: "source".into(),
                timing: Default::default(),
                placement: mode,
                colour: Default::default(),
                layer: None,
                fallback: String::new(),
            });
        let plan = Plan::compile(&doc).unwrap();
        let result = renderer
            .render_frame(
                &plan,
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap();
        for y in 0..4 {
            for x in 0..4 {
                let px = (x as f32 + 0.5 - offset[0]) / scale[0];
                let py = (y as f32 + 0.5 - offset[1]) / scale[1];
                let expected = if !(0.0..8.0).contains(&px) || !(0.0..4.0).contains(&py) {
                    [0.0; 4]
                } else {
                    // The source is a linear ramp; bilinear interpolation has this analytic value.
                    [
                        (py - 0.5).clamp(0.0, 3.0) * 8.0 / 64.0 + (px - 0.5).clamp(0.0, 7.0) / 64.0,
                        0.0,
                        0.0,
                        0.5,
                    ]
                };
                for (actual, expected) in result.pixels[(y * 4 + x) as usize].iter().zip(expected) {
                    assert!(
                        (actual - expected).abs() < 1e-6,
                        "{mode:?} {x},{y}: {actual} != {expected}"
                    );
                }
            }
        }
    }
    // Upscaling a small source must fill the canvas, not leave native-size padding.
    inputs.insert(
        "small",
        Arc::new(Image::new(1, 1, vec![[0.25, 0.0, 0.0, 0.5]]).unwrap()),
    );
    let mut doc = blank(4, 4, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation =
        Operation::Footage(veylvik_document::nodes::Footage {
            asset: "small".into(),
            timing: Default::default(),
            placement: Placement::Fit,
            colour: Default::default(),
            layer: None,
            fallback: String::new(),
        });
    let result = renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert!(result.pixels.iter().all(|p| *p == [0.25, 0.0, 0.0, 0.5]));
}
#[test]
fn old_footage_defaults_to_legacy_placement() {
    let operation: Operation = serde_json::from_value(serde_json::json!({
        "type":"footage", "asset":"clip.mp4", "timing":{"start":{"ticks":0,"rate":1},"source_in":{"ticks":0,"rate":1},"source_out":null}
    })).unwrap();
    assert!(matches!(
        operation,
        Operation::Footage(veylvik_document::nodes::Footage {
            placement: Placement::Legacy,
            ..
        })
    ));
}

/// Bars an odd number of pixels wide: the picture starts on a whole pixel (the half
/// rounded down, as editors place it), so every pixel is the source's own, not a blend.
#[test]
#[ignore = "requires Vulkan"]
fn a_fitted_picture_sits_on_whole_pixels() {
    let renderer = Renderer::new().unwrap();
    // 15 x 20, each pixel its own value; the frame is 48 x 20, so Fit keeps the size and
    // leaves 33 px of bars: 16 on the left, 17 on the right.
    let pixels: Vec<[f32; 4]> = (0..15 * 20)
        .map(|i| [(i % 15) as f32 / 16.0, (i / 15) as f32 / 32.0, 0.25, 1.0])
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "portrait",
        Arc::new(Image::new(15, 20, pixels.clone()).unwrap()),
    );
    // Original size centres the same way. (Fill scales the picture, so its pixels are
    // blends anyway; it rounds its position the same way.)
    for mode in [Placement::Fit, Placement::Original] {
        let mut doc = blank(48, 20, Time::new(24, 1).unwrap()).unwrap();
        doc.nodes.get_mut(&doc.output).unwrap().operation =
            Operation::Footage(veylvik_document::nodes::Footage {
                asset: "portrait".into(),
                timing: Default::default(),
                placement: mode,
                colour: Default::default(),
                layer: None,
                fallback: String::new(),
            });
        let plan = Plan::compile(&doc).unwrap();
        let out = renderer
            .render_frame(
                &plan,
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap()
            .pixels;
        for y in 0..20 {
            for x in 0..48 {
                let got = out[y * 48 + x];
                let want = if (16..31).contains(&x) {
                    pixels[y * 15 + x - 16]
                } else {
                    [0.0; 4]
                };
                assert_eq!(got, want, "{mode:?} at {x},{y}");
            }
        }
    }
}
