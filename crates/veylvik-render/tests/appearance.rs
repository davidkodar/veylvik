use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;
fn source() -> (Composition, ImageInputs, Vec<[f32; 4]>) {
    let mut doc = blank(96, 64, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation = Operation::Image {
        asset: "coverage".into(),
    };
    let pixels: Vec<_> = (0..96 * 64)
        .map(|i| {
            let alpha = if i % 96 < 8 {
                0.0
            } else if i % 96 < 16 {
                0.25
            } else {
                1.0
            };
            [0.2 * alpha, 0.1 * alpha, 0.05 * alpha, alpha]
        })
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "coverage",
        Arc::new(Image::new(96, 64, pixels.clone()).unwrap()),
    );
    (doc, inputs, pixels)
}
#[test]
#[ignore = "requires Vulkan"]
fn shine_preserves_coverage_and_matches_specular_profile() {
    let renderer = Renderer::new().unwrap();
    let (mut doc, inputs, source) = source();
    let node = Node::new(
        "shine",
        Operation::MetallicShine {
            x: 48.0.into(),
            y: 32.0.into(),
            width: 20.0.into(),
            angle: 0.0.into(),
            intensity: 2.0.into(),
        },
        &[("image", doc.output)],
    );
    doc.output = node.id;
    doc.nodes.insert(node.id, node);
    let render = |doc: &Composition| {
        renderer
            .render_frame(
                &Plan::compile(doc).unwrap(),
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap()
            .pixels
    };
    let result = render(&doc);
    for (i, (p, original)) in result.iter().zip(&source).enumerate() {
        assert_eq!(p[3], original[3]);
        let d = ((i % 96) as f32 - 48.0).abs() / 20.0;
        let profile = 0.32 * (1.0 - d).max(0.0).powi(3) + 0.68 * (1.0 - d / 0.16).max(0.0).powi(2);
        for c in 0..3 {
            assert!(
                (p[c] - (original[c] + [1.0, 0.94, 0.82][c] * profile * 2.0 * original[3])).abs()
                    < 0.00001
            );
        }
    }
    *doc.nodes
        .get_mut(&doc.output)
        .unwrap()
        .operation
        .scalar_mut("intensity")
        .unwrap() = 0.0.into();
    assert_eq!(render(&doc), source);
}
#[test]
#[ignore = "requires Vulkan"]
fn materials_are_distinct_finite_and_preserve_partial_alpha() {
    let renderer = Renderer::new().unwrap();
    let (mut doc, inputs, source) = source();
    let input = doc.output;
    let mut frames = Vec::new();
    for kind in MaterialKind::ALL {
        let node = Node::new(
            "surface",
            Operation::Material {
                kind,
                scale: 90.0.into(),
                bevel: 4.0.into(),
                roughness: 0.3.into(),
                lighting: 0.0.into(),
                grain: 0.6.into(),
                emission: 1.0.into(),
            },
            &[("image", input)],
        );
        doc.output = node.id;
        doc.nodes.insert(node.id, node);
        let frame = renderer
            .render_frame(
                &Plan::compile(&doc).unwrap(),
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap()
            .pixels;
        for (p, s) in frame.iter().zip(&source) {
            assert_eq!(p[3], s[3]);
            assert!(p.iter().all(|v| v.is_finite()));
            if s[3] == 0.0 {
                assert_eq!(*p, [0.0; 4]);
            }
        }
        assert!(frames.iter().all(|previous| previous != &frame));
        frames.push(frame);
    }
}
#[test]
#[ignore = "requires Vulkan"]
fn atmospheres_loop_without_state_preserve_clear_space_and_have_distinct_density() {
    let renderer = Renderer::new().unwrap();
    let mut previous = Vec::new();
    for kind in AtmosphereKind::ALL {
        let mut doc = blank(128, 96, Time::new(24, 1).unwrap()).unwrap();
        doc.nodes.get_mut(&doc.output).unwrap().operation = Operation::Atmosphere {
            kind,
            seed: 397,
            x: 64.0.into(),
            y: 90.0.into(),
            width: 100.0.into(),
            height: 80.0.into(),
            evolution: 0.0.into(),
            density: 3.0.into(),
            detail: 1.3.into(),
            lighting: 1.0.into(),
        };
        let render = |doc: &Composition| {
            renderer
                .render(&Plan::compile(doc).unwrap(), Time::new(0, 1).unwrap())
                .unwrap()
        };
        let start = render(&doc);
        assert!(start.iter().any(|p| p[3] > 0.01), "{kind:?}");
        assert!(start.contains(&[0.0; 4]));
        assert!(
            start
                .iter()
                .all(|p| p.iter().all(|v| v.is_finite()) && (0.0..=1.0).contains(&p[3]))
        );
        assert!(previous.iter().all(|p| p != &start));
        previous.push(start.clone());
        *doc.nodes
            .get_mut(&doc.output)
            .unwrap()
            .operation
            .scalar_mut("evolution")
            .unwrap() = 4.0.into();
        assert_eq!(start, render(&doc));
        *doc.nodes
            .get_mut(&doc.output)
            .unwrap()
            .operation
            .scalar_mut("evolution")
            .unwrap() = 3.9999.into();
        let near = render(&doc);
        let delta: f32 = start
            .iter()
            .zip(near)
            .flat_map(|(a, b)| a.iter().zip(b).map(|(a, b)| (a - b).abs()))
            .sum();
        assert!(delta / (start.len() as f32) < 0.01, "{kind:?} loop seam");
        *doc.nodes
            .get_mut(&doc.output)
            .unwrap()
            .operation
            .scalar_mut("evolution")
            .unwrap() = 2.0.into();
        assert_ne!(start, render(&doc));
        *doc.nodes
            .get_mut(&doc.output)
            .unwrap()
            .operation
            .scalar_mut("density")
            .unwrap() = 0.0.into();
        assert!(render(&doc).iter().all(|p| *p == [0.0; 4]));
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn surface_shine_and_atmosphere_presets_render_on_native_uhd_canvas() {
    let renderer = Renderer::new().unwrap();
    for id in [
        "gold_text",
        "metallic_shine",
        "light_smoke",
        "surface_reflection",
        "frosted_glass_material",
        "leather_tan",
        "concrete",
    ] {
        let base = blank(3840, 2160, Time::new(30, 1).unwrap()).unwrap();
        let preset = presets::builtin::build(id).unwrap();
        let instance = preset
            .instantiate_sized(
                &base,
                &preset
                    .group
                    .inputs
                    .keys()
                    .map(|key| (key.clone(), base.output))
                    .collect(),
                None,
                presets::resolution::Sizing::Fit,
            )
            .unwrap();
        let doc = Document::new(base)
            .unwrap()
            .prepare(0, &instance.commands)
            .unwrap();
        let frame = renderer
            .render_gpu_cached(
                &Plan::compile_output(&doc, instance.output).unwrap(),
                RenderRequest::full(Time::new(2, 1).unwrap()),
                &ImageInputs::default(),
            )
            .unwrap();
        assert_eq!((frame.region().width, frame.region().height), (3840, 2160));
    }
}
