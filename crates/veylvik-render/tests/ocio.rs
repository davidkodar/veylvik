use std::sync::Arc;
use veylvik_document::{nodes, *};
use veylvik_render::*;
#[test]
#[ignore = "requires Vulkan"]
fn ocio_gpu_matches_reference_for_hdr_negative_values_and_premultiplied_alpha() {
    let renderer = Renderer::new().unwrap();
    let config = veylvik_color::Config::load("ocio://default").unwrap();
    let display = config.displays().unwrap().remove(0);
    let view = config.views(&display).unwrap().remove(0);
    let mut doc = blank(16, 1, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation = Operation::Image(nodes::Image {
        asset: "test".into(),
    });
    let pixels: Vec<_> = (0..16)
        .map(|i| {
            let alpha = if i == 0 { 0. } else { 0.5 };
            [
                i as f32 * 0.31 * alpha,
                (-0.1 + i as f32 * 0.08) * alpha,
                0.03 * alpha,
                alpha,
            ]
        })
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert("test", Arc::new(Image::new(16, 1, pixels.clone()).unwrap()));
    let frame = renderer
        .render_gpu(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    for program in [
        config
            .processor("ACEScg", "sRGB - Display", "", "")
            .unwrap(),
        config.processor("ACEScg", "", &display, &view).unwrap(),
    ] {
        let processor = ocio::OcioProcessor::new(&renderer, &program.shader().unwrap()).unwrap();
        let output = renderer
            .read_frame(&processor.apply(&renderer, &frame).unwrap())
            .unwrap();
        for (input, output) in pixels.iter().zip(output.pixels) {
            assert_eq!(output[3], input[3]);
            if input[3] == 0. {
                assert_eq!(output, [0.; 4]);
                continue;
            }
            let reference = program
                .reference([
                    input[0] / input[3],
                    input[1] / input[3],
                    input[2] / input[3],
                    1.,
                ])
                .unwrap();
            for c in 0..3 {
                assert!(
                    (output[c] - reference[c] * input[3]).abs() < 0.001,
                    "{input:?}: GPU {output:?}, CPU {reference:?}"
                );
            }
        }
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn acescg_compositing_and_separate_view_output_are_correct() {
    let renderer = Renderer::new().unwrap();
    let config = veylvik_color::Config::load("ocio://default").unwrap();
    let mut doc = blank(8, 8, Time::new(24, 1).unwrap()).unwrap();
    let a = [0.6, 0.12, 0.02, 1.];
    let b = [0.05, 0.3, 0.7, 1.];
    doc.nodes.get_mut(&doc.output).unwrap().operation = Operation::Solid(nodes::Solid { color: a });
    let other = Node::new("Second", Operation::Solid(nodes::Solid { color: b }), &[]);
    let second = other.id;
    doc.nodes.insert(second, other);
    let blend = Node::new(
        "Screen",
        Operation::Blend(nodes::Blend {
            mode: BlendMode::Screen,
            opacity: 1.0.into(),
        }),
        &[("background", doc.output), ("foreground", second)],
    );
    doc.output = blend.id;
    doc.nodes.insert(blend.id, blend);
    doc.working_space = color_management::WorkingSpace::Acescg;
    let to = config
        .processor("Linear Rec.709 (sRGB)", "ACEScg", "", "")
        .unwrap();
    let from = config
        .processor("ACEScg", "Linear Rec.709 (sRGB)", "", "")
        .unwrap();
    let aa = to.reference(a).unwrap();
    let bb = to.reference(b).unwrap();
    let reference = from
        .reference([
            1. - (1. - aa[0]) * (1. - bb[0]),
            1. - (1. - aa[1]) * (1. - bb[1]),
            1. - (1. - aa[2]) * (1. - bb[2]),
            1.,
        ])
        .unwrap();
    let request = RenderRequest::full(Time::new(0, 1).unwrap());
    let inputs = ImageInputs::default();
    let frame = renderer
        .render_frame(&Plan::compile(&doc).unwrap(), request, &inputs)
        .unwrap();
    for (actual, expected) in frame.pixels[0].iter().zip(reference) {
        assert!(
            (actual - expected).abs() < 0.00001,
            "{actual} != {expected}"
        );
    }
    let display = config.displays().unwrap().remove(0);
    let view = config.views(&display).unwrap().remove(0);
    doc.ocio.view = Some(color_management::OcioTransform {
        display,
        view,
        ..Default::default()
    });
    let plan = Plan::compile(&doc).unwrap();
    let raw = renderer.render_gpu_cached(&plan, request, &inputs).unwrap();
    let shown = renderer
        .apply_ocio_view(&plan, &inputs, raw.clone())
        .unwrap();
    let shown = renderer.read_frame(&shown).unwrap();
    assert_ne!(shown.pixels, frame.pixels);
    assert_eq!(
        renderer
            .render_frame_export(&plan, request, &inputs)
            .unwrap()
            .pixels,
        frame.pixels,
        "Preview must not change export"
    );
    doc.ocio.output = doc.ocio.view.clone();
    let plan = Plan::compile(&doc).unwrap();
    assert_eq!(
        renderer
            .render_frame_export(&plan, request, &inputs)
            .unwrap()
            .pixels,
        shown.pixels,
        "Explicit output can match preview"
    );
    doc.ocio.view.as_mut().unwrap().config = "/tmp/veylvik-missing-config.ocio".into();
    let plan = Plan::compile(&doc).unwrap();
    assert!(
        renderer.apply_ocio_view(&plan, &inputs, raw).is_err(),
        "Never silently replace a missing config"
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn custom_config_3d_lut_search_path_and_cache_invalidation() {
    let root = std::env::temp_dir().join(format!("veylvik-ocio-{}", std::process::id()));
    std::fs::create_dir_all(root.join("luts")).unwrap();
    let path = root.join("config.ocio");
    std::fs::write(
        &path,
        r#"ocio_profile_version: 2
search_path: luts
roles:
  default: input
  scene_linear: input
file_rules:
  - !<Rule> {name: Default, colorspace: input}
displays:
  sRGB:
    - !<View> {name: Raw, colorspace: input}
colorspaces:
  - !<ColorSpace>
    name: input
    bitdepth: 32f
    isdata: false
    allocation: uniform
  - !<ColorSpace>
    name: look
    bitdepth: 32f
    isdata: false
    allocation: uniform
    to_scene_reference: !<FileTransform> {src: look.cube, interpolation: tetrahedral}
"#,
    )
    .unwrap();
    let lut = root.join("luts/look.cube");
    let write_lut = |gain: f32| {
        let mut text = String::from("LUT_3D_SIZE 2\n");
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    text.push_str(&format!("{} {} {}\n", r as f32 * gain, g, b));
                }
            }
        }
        std::fs::write(&lut, text).unwrap();
    };
    write_lut(0.5);
    let config = veylvik_color::Config::load(path.to_str().unwrap()).unwrap();
    let program = config.processor("look", "input", "", "").unwrap();
    assert_eq!(program.files(), [lut.to_string_lossy().into_owned()]);
    let renderer = Renderer::new().unwrap();
    let mut doc = blank(8, 8, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation = Operation::Solid(nodes::Solid {
        color: [0.8, 0.3, 0.2, 1.],
    });
    let node = Node::new(
        "Custom OCIO",
        Operation::Ocio(nodes::Ocio {
            transform: color_management::OcioTransform {
                config: "config.ocio".into(),
                source: "look".into(),
                destination: "input".into(),
                display: String::new(),
                view: String::new(),
            },
        }),
        &[("image", doc.output)],
    );
    doc.output = node.id;
    doc.nodes.insert(node.id, node);
    let plan = Plan::compile(&doc).unwrap();
    let mut inputs = ImageInputs::default();
    inputs.set_base_directory(&root);
    let request = RenderRequest::full(Time::new(0, 1).unwrap());
    let first = renderer
        .render_frame(&plan, request, &inputs)
        .unwrap()
        .pixels[0];
    let expected = program.reference([0.8, 0.3, 0.2, 1.]).unwrap();
    for i in 0..4 {
        assert!(
            (first[i] - expected[i]).abs() < 0.00001,
            "{first:?} != {expected:?}"
        );
    }
    write_lut(0.25);
    let second = renderer
        .render_frame(&plan, request, &inputs)
        .unwrap()
        .pixels[0];
    assert!(
        (second[0] - 0.2).abs() < 0.00001,
        "Changed LUT must invalidate cached processor: {second:?}"
    );
    std::fs::remove_dir_all(root).unwrap();
    assert!(
        renderer.render_frame(&plan, request, &inputs).is_err(),
        "Missing LUT/config must not use stale processor"
    );
}
