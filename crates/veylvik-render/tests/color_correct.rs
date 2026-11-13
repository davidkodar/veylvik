use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

/// CPU reference of the documented grade, in f64.
fn reference(pixel: [f32; 4], g: [f64; 7]) -> [f64; 4] {
    let [temperature, tint, lift, gain, contrast, gamma, saturation] = g;
    let alpha = f64::from(pixel[3]);
    if alpha <= 0.0 {
        return pixel.map(f64::from);
    }
    let signed_pow = |v: f64, e: f64| v.signum() * v.abs().powf(e);
    let mut rgb = [0, 1, 2].map(|c| f64::from(pixel[c]) / alpha);
    let balance = [
        (0.5 * temperature).exp2(),
        (-0.5 * tint).exp2(),
        (-0.5 * temperature).exp2(),
    ];
    for c in 0..3 {
        rgb[c] *= balance[c];
        rgb[c] = rgb[c] * gain + lift;
        rgb[c] = 0.18 * signed_pow(rgb[c] / 0.18, contrast);
        rgb[c] = signed_pow(rgb[c], 1.0 / gamma);
    }
    let y = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
    let rgb = rgb.map(|v| y + (v - y) * saturation);
    [rgb[0] * alpha, rgb[1] * alpha, rgb[2] * alpha, alpha]
}

#[test]
fn colour_correct_defaults_are_registered_and_bounded() {
    let operation: Operation =
        serde_json::from_value(registry::COLOR_CORRECT.default_operation()).unwrap();
    operation.validate().unwrap();
    for (name, value) in [
        ("contrast", 1.0),
        ("gamma", 1.0),
        ("gain", 1.0),
        ("saturation", 1.0),
        ("lift", 0.0),
    ] {
        assert!(
            matches!(operation.scalar(name), Some(Scalar::Constant { value: v }) if *v == value),
            "{name}"
        );
    }
    let mut bad = operation.clone();
    *bad.scalar_mut("gamma").unwrap() = Scalar::Constant { value: 0.0 };
    assert!(bad.validate().is_err());
}

#[test]
#[ignore = "requires Vulkan"]
fn colour_correct_matches_reference_with_hdr_negative_and_transparent_pixels() {
    let renderer = Renderer::new().unwrap();
    let source = vec![
        [0.0; 4],
        [0.18, 0.18, 0.18, 1.0],
        [0.25, 0.5, 0.75, 1.0],
        [4.0, 2.0, 0.5, 1.0],
        [-0.2, 0.3, 0.1, 1.0],
        [0.1, 0.2, 0.05, 0.5],
        [12.0, 0.01, 3.0, 0.25],
    ];
    let mut doc = blank(source.len() as u32, 1, Time::new(24, 1).unwrap()).unwrap();
    let source_id = doc.output;
    doc.nodes.get_mut(&source_id).unwrap().operation =
        Operation::Image(veylvik_document::nodes::Image {
            asset: "input".into(),
        });
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "input",
        Arc::new(Image::new(doc.width, 1, source.clone()).unwrap()),
    );
    let defaults = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let mut grades = vec![defaults];
    // Each stage alone, then all of them together.
    for (index, value) in [
        (0, 0.4),
        (1, -0.3),
        (2, 0.05),
        (3, 1.7),
        (4, 1.3),
        (5, 2.2),
        (6, 0.4),
    ] {
        let mut grade = defaults;
        grade[index] = value;
        grades.push(grade);
    }
    grades.push([-0.25, 0.2, -0.02, 1.4, 0.8, 0.7, 1.6]);
    for grade in grades {
        let c = |v: f64| Scalar::Constant { value: v as f32 };
        let node = Node::new(
            "Grade",
            Operation::ColorCorrect(veylvik_document::nodes::ColorCorrect {
                temperature: c(grade[0]),
                tint: c(grade[1]),
                lift: c(grade[2]),
                gain: c(grade[3]),
                contrast: c(grade[4]),
                gamma: c(grade[5]),
                saturation: c(grade[6]),
            }),
            &[("image", source_id)],
        );
        if doc.output != source_id {
            doc.nodes.remove(&doc.output);
        }
        doc.output = node.id;
        doc.nodes.insert(node.id, node);
        let plan = Plan::compile(&doc).unwrap();
        let frame = renderer
            .render_frame(
                &plan,
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap();
        for (actual, pixel) in frame.pixels.iter().zip(&source) {
            if grade == defaults {
                assert_eq!(actual, pixel, "defaults must be an exact identity");
                continue;
            }
            let expected = reference(*pixel, grade);
            for channel in 0..4 {
                let tolerance = 1e-4 * expected[channel].abs().max(1.0);
                assert!(
                    (f64::from(actual[channel]) - expected[channel]).abs() <= tolerance,
                    "grade {grade:?} pixel {pixel:?} channel {channel}: {} != {}",
                    actual[channel],
                    expected[channel]
                );
            }
        }
    }
}
