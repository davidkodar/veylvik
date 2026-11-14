use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;
fn source() -> (Composition, ImageInputs, Vec<[f32; 4]>) {
    let mut doc = blank(96, 64, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation =
        Operation::Image(veylvik_document::nodes::Image {
            asset: "coverage".into(),
        });
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
        Operation::MetallicShine(veylvik_document::nodes::MetallicShine {
            x: 48.0.into(),
            y: 32.0.into(),
            width: 20.0.into(),
            angle: 0.0.into(),
            intensity: 2.0.into(),
        }),
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
            Operation::Material(veylvik_document::nodes::Material {
                smooth_edges: true,
                brushing: Default::default(),
                optics: None,
                studio: Default::default(),
                kind,
                scale: 90.0.into(),
                bevel: 4.0.into(),
                roughness: 0.3.into(),
                lighting: 0.0.into(),
                grain: 0.6.into(),
                emission: 1.0.into(),
            }),
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
        doc.nodes.get_mut(&doc.output).unwrap().operation =
            Operation::Atmosphere(veylvik_document::nodes::Atmosphere {
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
            });
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

#[test]
#[ignore = "requires Vulkan"]
fn replacing_backdrop_reveals_bricks_and_preserves_opaque_animated_text() {
    use presets::background::{self, Placement};
    let scene = presets::showcase::build("gold_text").unwrap();
    let original = &scene.composition;
    let text = original.nodes[&original.output].inputs["foreground"];
    let instance = background::insert("brick_red", original, Placement::Automatic, None).unwrap();
    let doc = Document::new(original.clone())
        .unwrap()
        .prepare(0, &instance.commands)
        .unwrap();
    let renderer = Renderer::new().unwrap();
    let time = Time::new(2, 1).unwrap();
    let result = renderer
        .render(&Plan::compile(&doc).unwrap(), time)
        .unwrap();
    let old = renderer
        .render(&Plan::compile(original).unwrap(), time)
        .unwrap();
    let subject = renderer
        .render(&Plan::compile_output(original, text).unwrap(), time)
        .unwrap();
    let mut exposed = 0;
    let mut preserved = 0;
    for ((new, old), subject) in result.iter().zip(old).zip(subject) {
        if subject[3] == 0.0 && (new[0] - old[0]).abs() > 0.02 {
            exposed += 1;
        }
        if subject[3] == 1.0 {
            assert_eq!(*new, subject);
            preserved += 1;
        }
    }
    assert!(exposed > 500000, "brick background must be visible");
    assert!(preserved > 10000, "gold letters must remain above bricks");
}

#[test]
#[ignore = "requires Vulkan"]
fn library_spheres_are_finite_antialiased_and_material_specific() {
    let renderer = Renderer::new().unwrap();
    let gold = renderer
        .material_swatch(MaterialKind::Gold, 0.15, 0.65)
        .unwrap();
    let silver = renderer
        .material_swatch(MaterialKind::Silver, 0.15, 0.65)
        .unwrap();
    assert_eq!(gold.len(), 256 * 256);
    assert_eq!(gold[0], [0.; 4]);
    assert_eq!(gold[124 * 256 + 128][3], 1.);
    assert!(
        gold.iter()
            .all(|p| p.iter().all(|v| v.is_finite()) && (0. ..=1.).contains(&p[3]))
    );
    assert!(gold.iter().any(|p| p[3] > 0. && p[3] < 1.));
    assert!(
        gold.iter()
            .zip(&silver)
            .any(|(a, b)| (a[2] - b[2]).abs() > 0.2)
    );
    assert!(
        renderer
            .material_swatch(MaterialKind::Gold, f32::NAN, 0.5)
            .is_err()
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn polished_metal_has_hdr_highlights_and_roughness_softens_them() {
    let renderer = Renderer::new().unwrap();
    for kind in [
        MaterialKind::Gold,
        MaterialKind::Silver,
        MaterialKind::Aluminium,
        MaterialKind::Bronze,
    ] {
        let polished = renderer.material_swatch(kind, 0.08, 0.65).unwrap();
        let rough = renderer.material_swatch(kind, 0.85, 0.65).unwrap();
        let peak = |pixels: &[[f32; 4]]| {
            pixels
                .iter()
                .filter(|p| p[3] > 0.99)
                .map(|p| p[0].max(p[1]).max(p[2]))
                .fold(0.0_f32, f32::max)
        };
        assert!(
            peak(&polished) > 1.0,
            "polished metal retains HDR highlights"
        );
        assert!(
            peak(&rough) < peak(&polished),
            "roughness softens specular peaks"
        );
        assert!(
            polished.iter().any(|p| p[3] > 0.99 && p[0] < 0.15),
            "dark reflections provide metal contrast"
        );
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn area_lights_and_optics_control_radiance_without_changing_coverage() {
    let renderer = Renderer::new().unwrap();
    let (mut doc, inputs, original) = source();
    let material: Operation = serde_json::from_value(
        veylvik_document::registry::TYPES
            .iter()
            .find(|d| d.type_id == "material")
            .unwrap()
            .default_operation(),
    )
    .unwrap();
    let node = Node::new("lit material", material, &[("image", doc.output)]);
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
    for kind in [
        MaterialKind::Gold,
        MaterialKind::LeatherBrown,
        MaterialKind::OakLight,
    ] {
        let Operation::Material(m) = &mut doc.nodes.get_mut(&doc.output).unwrap().operation else {
            panic!()
        };
        m.kind = kind;
        m.studio = Default::default();
        m.studio.ambient = 0.;
        m.studio.environment = 0.;
        m.studio.lights = vec![veylvik_document::optical::AreaLight {
            position: [0., 0., 4.],
            rotation: [0.; 3],
            size: [8., 8.],
            ..Default::default()
        }];
        let lit = render(&doc);
        assert!(lit.iter().any(|p| p[0] > 0.01));
        let Operation::Material(m) = &mut doc.nodes.get_mut(&doc.output).unwrap().operation else {
            panic!()
        };
        m.studio.lights[0].intensity *= 2.;
        let brighter = render(&doc);
        for (a, b) in lit.iter().zip(&brighter) {
            for c in 0..3 {
                assert!((a[c] * 2. - b[c]).abs() < 1e-4);
            }
        }
        let Operation::Material(m) = &mut doc.nodes.get_mut(&doc.output).unwrap().operation else {
            panic!()
        };
        m.optics = Some(veylvik_document::optical::SurfaceOptics {
            metallic: true,
            reflectance: [0.; 3],
            ior: 1.,
        });
        let no_reflection = render(&doc);
        let energy = |frame: &Vec<[f32; 4]>| frame.iter().map(|p| p[0] + p[1] + p[2]).sum::<f32>();
        assert!(energy(&no_reflection) < energy(&brighter) * 0.5);
        let Operation::Material(m) = &mut doc.nodes.get_mut(&doc.output).unwrap().operation else {
            panic!()
        };
        m.optics = None;
        m.studio.lights[0].enabled = false;
        let dark = render(&doc);
        for (p, source) in dark.iter().zip(&original) {
            assert_eq!(p[3], source[3]);
            assert_eq!(&p[..3], &[0.; 3]);
        }
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn environment_prefilter_preserves_constant_radiance() {
    let renderer = Renderer::new().unwrap();
    let source = vec![[2.0, 1.0, 0.5, 1.0]; 32 * 16];
    let result = renderer
        .prefilter_environment(32, 16, &source, false)
        .unwrap();
    assert_eq!(result.len(), 701096);
    for p in &result {
        for c in 0..4 {
            assert!((p[c] - source[0][c]).abs() < 0.003, "{p:?}");
        }
    }
    let neutral = renderer
        .prefilter_environment(32, 16, &source, true)
        .unwrap();
    for p in &neutral {
        assert!((p[0] - p[1]).abs() < 1e-6 && (p[1] - p[2]).abs() < 1e-6);
    }
    assert!(renderer.prefilter_environment(32, 16, &[], false).is_err());
}

#[test]
#[ignore = "requires Vulkan"]
fn studio_environment_rotation_changes_reflections_and_zero_disables_them() {
    let renderer = Renderer::new().unwrap();
    let (mut doc, inputs, source) = source();
    let material: Operation = serde_json::from_value(
        veylvik_document::registry::TYPES
            .iter()
            .find(|d| d.type_id == "material")
            .unwrap()
            .default_operation(),
    )
    .unwrap();
    let node = Node::new("environment", material, &[("image", doc.output)]);
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
    let Operation::Material(m) = &mut doc.nodes.get_mut(&doc.output).unwrap().operation else {
        panic!()
    };
    m.studio.lights.clear();
    m.studio.ambient = 0.;
    m.studio.environment = 1.;
    let before = render(&doc);
    let Operation::Material(m) = &mut doc.nodes.get_mut(&doc.output).unwrap().operation else {
        panic!()
    };
    m.studio.environment_rotation += 90.;
    let rotated = render(&doc);
    assert!(
        before
            .iter()
            .zip(&rotated)
            .any(|(a, b)| (a[0] - b[0]).abs() > 0.02)
    );
    let Operation::Material(m) = &mut doc.nodes.get_mut(&doc.output).unwrap().operation else {
        panic!()
    };
    m.studio.environment = 0.;
    let dark = render(&doc);
    for (p, s) in dark.iter().zip(&source) {
        assert_eq!(p[3], s[3]);
        assert_eq!(p[..3], [0.; 3]);
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn brushed_aluminium_changes_directional_reflections_without_changing_silhouette() {
    let renderer = Renderer::new().unwrap();
    let smooth = renderer
        .material_swatch(MaterialKind::Aluminium, 0.28, 0.0)
        .unwrap();
    let brushed = renderer
        .material_swatch(MaterialKind::Aluminium, 0.28, 0.65)
        .unwrap();
    let mut difference = 0.0;
    for (a, b) in smooth.iter().zip(&brushed) {
        assert!(b.iter().all(|v| v.is_finite()));
        assert_eq!(a[3], b[3]);
        difference += (a[0] - b[0]).abs();
    }
    assert!(difference / brushed.len() as f32 > 0.01);
    let extreme = renderer
        .material_swatch(MaterialKind::Aluminium, 0.03, 1.0)
        .unwrap();
    assert!(extreme.iter().flatten().all(|v| v.is_finite()));
}

#[test]
#[ignore = "requires Vulkan"]
fn every_metal_finish_renders_and_brushing_is_independent_of_metal_kind() {
    use veylvik_document::optical::{Brushing, MetalFinish};
    let renderer = Renderer::new().unwrap();
    let (mut doc, inputs, source) = source();
    let op: Operation = serde_json::from_value(
        registry::TYPES
            .iter()
            .find(|n| n.type_id == "material")
            .unwrap()
            .default_operation(),
    )
    .unwrap();
    let node = Node::new("finish", op, &[("image", doc.output)]);
    doc.output = node.id;
    doc.nodes.insert(node.id, node);
    for kind in [
        MaterialKind::Gold,
        MaterialKind::Silver,
        MaterialKind::Bronze,
        MaterialKind::Aluminium,
    ] {
        for finish in MetalFinish::ALL {
            let Operation::Material(m) = &mut doc.nodes.get_mut(&doc.output).unwrap().operation
            else {
                panic!()
            };
            m.kind = kind;
            m.apply_finish(finish);
            let pixels = renderer
                .render_frame(
                    &Plan::compile(&doc).unwrap(),
                    RenderRequest::full(Time::new(0, 1).unwrap()),
                    &inputs,
                )
                .unwrap()
                .pixels;
            for (pixel, original) in pixels.iter().zip(&source) {
                assert!(pixel.iter().all(|v| v.is_finite()));
                assert_eq!(pixel[3], original[3]);
            }
            if finish == MetalFinish::Brushed {
                let Operation::Material(m) = &mut doc.nodes.get_mut(&doc.output).unwrap().operation
                else {
                    panic!()
                };
                m.brushing = Brushing::Smooth;
                let smooth = renderer
                    .render_frame(
                        &Plan::compile(&doc).unwrap(),
                        RenderRequest::full(Time::new(0, 1).unwrap()),
                        &inputs,
                    )
                    .unwrap()
                    .pixels;
                assert!(
                    pixels
                        .iter()
                        .zip(smooth)
                        .any(|(a, b)| (a[0] - b[0]).abs() > 0.001),
                    "brushing changes reflections for {kind:?}"
                );
            }
        }
    }
}
