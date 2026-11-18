use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

const W: u32 = 32;
const H: u32 = 16;

fn shader(code: &str, values: [f32; 8]) -> nodes::Shader {
    let v = values.map(Scalar::from);
    let [p1, p2, p3, p4, p5, p6, p7, p8] = v;
    nodes::Shader {
        code: code.into(),
        p1,
        p2,
        p3,
        p4,
        p5,
        p6,
        p7,
        p8,
    }
}
/// A composition of a ramp image (red = x, green = y, alpha 1) through `shader`.
fn scene(shader: nodes::Shader, connected: bool) -> (Composition, ImageInputs) {
    let pixels: Vec<[f32; 4]> = (0..W * H)
        .map(|i| [(i % W) as f32, (i / W) as f32, 0.0, 1.0])
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert("ramp", Arc::new(Image::new(W, H, pixels).unwrap()));
    let image = Node::new(
        "ramp",
        Operation::Image(nodes::Image {
            asset: "ramp".into(),
        }),
        &[],
    );
    let wired = [("image", image.id)];
    let node = Node::new(
        "My shader",
        Operation::Shader(shader),
        if connected { &wired[..] } else { &[] },
    );
    let doc = Composition {
        markers: Default::default(),
        backdrops: Default::default(),
        ocio: Default::default(),
        working_space: Default::default(),
        display: Default::default(),
        groups: Default::default(),
        schema: 1,
        frame_count: 48,
        width: W,
        height: H,
        frame_rate: Time::new(24, 1).unwrap(),
        output: node.id,
        nodes: [image, node].into_iter().map(|n| (n.id, n)).collect(),
    };
    (doc, inputs)
}
fn render(doc: &Composition, inputs: &ImageInputs, seconds: i64) -> Vec<[f32; 4]> {
    let renderer = Renderer::new().unwrap();
    let plan = Plan::compile(doc).unwrap();
    let frame = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(seconds, 1).unwrap()),
            inputs,
        )
        .unwrap();
    renderer.read_frame(&frame).unwrap().pixels
}
#[test]
#[ignore = "requires Vulkan"]
fn a_custom_shader_reads_its_inputs_controls_size_and_time() {
    // The default node: the example code with its declared defaults.
    let Operation::Shader(example) =
        serde_json::from_value(nodes::SHADER.default_operation()).unwrap()
    else {
        panic!("a shader");
    };
    let (doc, inputs) = scene(example, true);
    let out = render(&doc, &inputs, 0);
    let p = out[(5 * W + 10) as usize];
    // mix(c, c * tint, 0.5) with tint (1, 0.8, 0.6).
    assert!(
        (p[0] - 10.0).abs() < 1e-4 && (p[1] - 5.0 * 0.9).abs() < 1e-4 && p[3] == 1.0,
        "{p:?}"
    );
    // Controls in every slot, a loop, the frame's size and time, and sampling between
    // and beyond pixels.
    let code = "// slider gain 0 4 1
// colour add 0 0 0
// slider shift -8 8 0
// slider late 0 9 0
fn shade(at: vec2<f32>) -> vec4<f32> {
    var sum = vec4<f32>(0.0);
    for (var i = 0; i < 4; i++) { sum += image(at + vec2<f32>(u.shift + f32(i), 0.0)); }
    let c = sum / 4.0;
    return vec4<f32>(c.r * u.gain + u.add.r, size.x + time * u.late, u.add.b, c.a);
}
";
    let (doc, inputs) = scene(
        shader(code, [2.0, 0.25, 0.0, 0.5, 1.0, 3.0, 0.0, 0.0]),
        true,
    );
    let out = render(&doc, &inputs, 1);
    let p = out[(3 * W + 10) as usize];
    // Mean of x = 11..=14 is 12.5; gain 2, plus 0.25. Size 32 plus time 1 x 3.
    assert!(
        (p[0] - 25.25).abs() < 1e-3 && (p[1] - 35.0).abs() < 1e-3 && p[2] == 0.5,
        "{p:?}"
    );
    // Past the right edge the input is transparent: fewer taps, lower alpha.
    assert!((out[(3 * W + 31) as usize][3] - 0.0).abs() < 1e-6);
    assert!((out[(3 * W + 29) as usize][3] - 0.5).abs() < 1e-6);
    // Unconnected, image() is transparent everywhere, and a generator still draws.
    let (doc, inputs) = scene(
        shader(
            "fn shade(at: vec2<f32>) -> vec4<f32> { return image(at) + vec4<f32>(at / size, 0.0, 1.0); }\n",
            [0.0; 8],
        ),
        false,
    );
    let out = render(&doc, &inputs, 0);
    let p = out[(8 * W + 16) as usize];
    assert!((p[0] - 16.5 / 32.0).abs() < 1e-5 && (p[1] - 8.5 / 16.0).abs() < 1e-5 && p[3] == 1.0);
}
#[test]
#[ignore = "requires Vulkan"]
fn mistakes_are_errors_with_the_users_line_not_device_faults() {
    for (code, hints) in [
        (
            "fn shade(at: vec2<f32>) -> vec4<f32> {\n    return image(at) * missing;\n}\n",
            &["line 2", "missing"][..],
        ),
        (
            "fn shade(at: vec2<f32>) -> vec4<f32> {\n    let x: f32 = vec2<f32>(1.0);\n    return image(at);\n}\n",
            &["line 2"][..],
        ),
        (
            "fn shade(at: vec2<f32>) -> vec4<f32> {\n    return image(at)\n}\n",
            &["line"][..],
        ),
    ] {
        let error = veylvik_render::custom::check(code).unwrap_err();
        for hint in hints {
            assert!(error.contains(hint), "{code}\n-> {error}");
        }
        // The document accepts the text (its rules are met); the plan refuses it.
        let (doc, _) = scene(shader(code, [0.0; 8]), true);
        doc.validate().unwrap();
        let refused = Plan::compile(&doc).err().expect("refused").to_string();
        assert!(refused.contains("My shader"), "{refused}");
    }
    // The text rules are the document's: such a node cannot even be stored.
    let (doc, _) = scene(
        shader(
            "fn shade(at: vec2<f32>) -> vec4<f32> { while (true) {} return image(at); }",
            [0.0; 8],
        ),
        true,
    );
    assert!(
        doc.validate()
            .unwrap_err()
            .to_string()
            .contains("instead of while")
    );
}
