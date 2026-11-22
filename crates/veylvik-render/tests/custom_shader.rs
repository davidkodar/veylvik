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
        p9: 0.0.into(),
        p10: 0.0.into(),
        p11: 0.0.into(),
        p12: 0.0.into(),
        p13: 0.0.into(),
        p14: 0.0.into(),
        p15: 0.0.into(),
        p16: 0.0.into(),
        follow: None,
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
        published: Default::default(),
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
fn sixteen_control_values_reach_the_shader_and_old_files_read_the_new_ones_as_zero() {
    // Five colours and a slider: the last colour and the slider sit in the second eight.
    let code = "// colour a 0 0 0
// colour b 0 0 0
// colour c 0 0 0
// colour d 0 0 0
// colour e 0 0 0
// slider last 0 9 0
fn shade(at: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(u.c.b, u.d.g, u.e.r + u.e.b, u.last);
}";
    let mut node = shader(code, [0.0; 8]);
    node.p9 = 0.25.into(); // c blue
    node.p11 = 0.5.into(); // d green
    node.p13 = 1.0.into(); // e red
    node.p15 = 2.0.into(); // e blue
    node.p16 = 7.0.into(); // last
    let (doc, inputs) = scene(node, false);
    assert_eq!(render(&doc, &inputs, 0)[0], [0.25, 0.5, 3.0, 7.0]);
    // A file from before the second eight.
    let constant = |v: f32| serde_json::json!({"kind": "constant", "value": v});
    let mut old = serde_json::json!({"type": "shader",
        "code": "fn shade(at: vec2<f32>) -> vec4<f32> { return vec4<f32>(0.0); }"});
    for name in ["p1", "p2", "p3", "p4", "p5", "p6", "p7", "p8"] {
        old[name] = constant(1.0);
    }
    let Operation::Shader(read) = serde_json::from_value(old).unwrap() else {
        panic!("a shader");
    };
    assert!(matches!(read.p16, Scalar::Constant { value } if value == 0.0));
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

#[test]
#[ignore = "requires Vulkan"]
fn a_shader_that_does_not_read_time_is_made_once_and_one_that_does_is_made_again() {
    let renderer = Renderer::new().unwrap();
    let at = |ticks| RenderRequest::full(Time { ticks, rate: 24 });
    let hits = || renderer.try_diagnostics().unwrap().unwrap().frames.hits;
    let pixels = |frame: &GpuFrame| renderer.read_frame(frame).unwrap().pixels;
    // The same picture at every time: asked again at another time, it comes from the cache.
    let steady =
        "fn shade(at: vec2<f32>) -> vec4<f32> { return vec4<f32>(at.x / size.x, 0.25, 0.5, 1.0); }";
    let (doc, inputs) = scene(shader(steady, [0.0; 8]), false);
    let plan = Plan::compile(&doc).unwrap();
    let first = renderer.render_gpu_cached(&plan, at(0), &inputs).unwrap();
    let before = hits();
    let later = renderer.render_gpu_cached(&plan, at(31), &inputs).unwrap();
    assert_eq!(
        hits(),
        before + 1,
        "a shader that reads no time was made again"
    );
    assert!(pixels(&first) == pixels(&later));
    // The word in a comment, or inside another word, is not reading the time.
    let comment = "// time passes\nfn shade(at: vec2<f32>) -> vec4<f32> { let timeless = 0.5; return vec4<f32>(timeless, 0.0, 0.0, 1.0); }";
    assert!(!veylvik_shader::reads_time(comment));
    // One that reads the time gives another picture at another time, never a kept one.
    let moving = "fn shade(at: vec2<f32>) -> vec4<f32> { return vec4<f32>(time, 0.25, 0.5, 1.0); }";
    assert!(veylvik_shader::reads_time(moving));
    let (doc, inputs) = scene(shader(moving, [0.0; 8]), false);
    let plan = Plan::compile(&doc).unwrap();
    let early = renderer.render_gpu_cached(&plan, at(0), &inputs).unwrap();
    let before = hits();
    let late = renderer.render_gpu_cached(&plan, at(24), &inputs).unwrap();
    assert_eq!(
        hits(),
        before,
        "a shader that reads the time was served from the cache"
    );
    assert!((pixels(&late)[0][0] - 1.0).abs() < 1e-6 && pixels(&early)[0][0] == 0.0);
    // Two shaders with the same settings and other text are two pictures.
    let other = "fn shade(at: vec2<f32>) -> vec4<f32> { return vec4<f32>(0.75, 0.25, 0.5, 1.0); }";
    let (doc, inputs) = scene(shader(other, [0.0; 8]), false);
    let plan = Plan::compile(&doc).unwrap();
    let before = hits();
    let changed = renderer.render_gpu_cached(&plan, at(0), &inputs).unwrap();
    assert_eq!(
        hits(),
        before,
        "another shader's picture was served for this one"
    );
    assert_eq!(pixels(&changed)[0][0], 0.75);
}
