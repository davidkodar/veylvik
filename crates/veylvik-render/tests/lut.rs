use std::sync::Arc;
use veylvik_document::media::{Curve, Gamut, InputColour};
use veylvik_document::*;
use veylvik_render::*;

/// A LUT image in the layout `veylvik_io::lut` produces: N² × (N + 1), with a metadata row.
fn lut_image(n: usize, f: impl Fn([f32; 3]) -> [f32; 3]) -> Image {
    let mut pixels = Vec::new();
    for b in 0..n {
        for g in 0..n {
            for r in 0..n {
                let s = |v: usize| v as f32 / (n - 1) as f32;
                let [x, y, z] = f([s(r), s(g), s(b)]);
                pixels.push([x, y, z, 1.0]);
            }
        }
    }
    let mut meta = vec![[0.0, 0.0, 0.0, 1.0]; n * n];
    meta[1] = [1.0, 1.0, 1.0, 1.0];
    meta[2] = [n as f32, 0.0, 0.0, 1.0];
    pixels.extend(meta);
    Image::new((n * n) as u32, (n + 1) as u32, pixels).unwrap()
}
const SOURCE: [[f32; 4]; 6] = [
    [0.0, 0.0, 0.0, 1.0],
    [0.18, 0.18, 0.18, 1.0],
    [0.05, 0.4, 0.8, 1.0],
    [0.9, 0.3, 0.1, 1.0],
    [0.5, 0.25, 0.125, 0.5],
    [0.7, 0.7, 0.7, 1.0],
];
fn render(lut: Image, mix: f32, input: InputColour, output: InputColour) -> Vec<[f32; 4]> {
    let renderer = Renderer::new().unwrap();
    let mut doc = blank(SOURCE.len() as u32, 1, Time::new(24, 1).unwrap()).unwrap();
    let source = doc.output;
    doc.nodes.get_mut(&source).unwrap().operation = Operation::Image(nodes::Image {
        asset: "source".into(),
    });
    let node = Node::new(
        "LUT",
        Operation::Lut(nodes::Lut {
            asset: "look.cube".into(),
            mix: Scalar::Constant { value: mix },
            input,
            output,
        }),
        &[("image", source)],
    );
    doc.output = node.id;
    doc.nodes.insert(node.id, node);
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "source",
        Arc::new(Image::new(SOURCE.len() as u32, 1, SOURCE.to_vec()).unwrap()),
    );
    inputs.insert("look.cube", Arc::new(lut));
    let plan = Plan::compile(&doc).unwrap();
    renderer
        .render_frame(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap()
        .pixels
}
fn rec709(v: f32) -> f32 {
    if v < 0.018 {
        4.5 * v
    } else {
        1.099 * v.powf(0.45) - 0.099
    }
}
fn linear709(v: f32) -> f32 {
    if v < 0.081 {
        v / 4.5
    } else {
        ((v + 0.099) / 1.099).powf(1.0 / 0.45)
    }
}
fn close(a: [f32; 4], b: [f32; 4], tolerance: f32) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tolerance)
}

#[test]
#[ignore = "requires Vulkan"]
fn identity_inverting_and_mixed_luts_match_reference() {
    let rec = InputColour {
        curve: Curve::Rec709,
        gamut: Gamut::Rec709,
    };
    // Identity: unchanged up to float rounding through encode and decode.
    let out = render(lut_image(17, |c| c), 1.0, rec, rec);
    for (a, b) in out.iter().zip(SOURCE) {
        assert!(close(*a, b, 2e-4), "{a:?} vs {b:?}");
    }
    // Inverting in Rec.709 code values, exact at the lattice and linear between.
    let out = render(lut_image(33, |c| c.map(|v| 1.0 - v)), 1.0, rec, rec);
    for (a, b) in out.iter().zip(SOURCE) {
        let expected = [0, 1, 2].map(|k| linear709(1.0 - rec709(b[k] / b[3])) * b[3]);
        let expected = [expected[0], expected[1], expected[2], b[3]];
        assert!(close(*a, expected, 2e-3), "{a:?} vs {expected:?}");
    }
    // Mix 0 is the original exactly.
    let out = render(lut_image(17, |c| c.map(|v| 1.0 - v)), 0.0, rec, rec);
    for (a, b) in out.iter().zip(SOURCE) {
        assert!(close(*a, b, 1e-6), "{a:?} vs {b:?}");
    }
}
#[test]
#[ignore = "requires Vulkan"]
fn camera_spaces_round_trip_through_an_identity_lut() {
    // An identity LUT declared as S-Log3 / S-Gamut3.Cine both ways must give the image back:
    // the shader's encode and decode curves and gamut matrices are exact inverses.
    for (curve, gamut) in [
        (Curve::Slog3, Gamut::SGamut3Cine),
        (Curve::Logc3, Gamut::ArriWideGamut3),
        (Curve::AppleLog, Gamut::Rec2020),
        (Curve::Vlog, Gamut::VGamut),
        (Curve::Srgb, Gamut::P3D65),
    ] {
        let space = InputColour { curve, gamut };
        let out = render(lut_image(33, |c| c), 1.0, space, space);
        for (a, b) in out.iter().zip(SOURCE) {
            assert!(close(*a, b, 2e-3), "{curve:?}: {a:?} vs {b:?}");
        }
    }
}
