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
                colours: None,
                optics: None,
                studio: Default::default(),
                kind,
                scale: 90.0.into(),
                bevel: 4.0.into(),
                roughness: 0.3.into(),
                lighting: 0.0.into(),
                grain: 0.6.into(),
                emission: 1.0.into(),
                shot: 1.0.into(),
                normal_strength: 1.0.into(),
                height_depth: 4.0.into(),
                normal_directx: false,
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
            position: [0., 0., 4.].into(),
            rotation: [0.; 3].into(),
            size: [8., 8.],
            ..Default::default()
        }];
        let lit = render(&doc);
        assert!(lit.iter().any(|p| p[0] > 0.01));
        let Operation::Material(m) = &mut doc.nodes.get_mut(&doc.output).unwrap().operation else {
            panic!()
        };
        m.studio.lights[0].intensity = (m.studio.lights[0]
            .intensity
            .at(Time::new(0, 1).unwrap())
            .unwrap()
            * 2.)
            .into();
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

#[test]
#[ignore = "requires Vulkan"]
fn material_colours_repaint_the_pattern_and_default_to_the_family() {
    let renderer = Renderer::new().unwrap();
    let preset = veylvik_document::presets::builtin::build("marble").unwrap();
    let mut marble = preset
        .nodes
        .values()
        .find_map(|n| match &n.operation {
            Operation::Material(m) => Some(m.clone()),
            _ => None,
        })
        .unwrap();
    let family = renderer.material_sample(&marble).unwrap();
    marble.colours = Some(veylvik_document::optical::palette_track(
        MaterialKind::Marble.palette(),
    ));
    assert_eq!(renderer.material_sample(&marble).unwrap(), family);
    // Black marble with gold veins: same pattern, new colours.
    marble.colours = Some(veylvik_document::optical::palette_track([
        [0.02, 0.02, 0.025],
        [0.8, 0.55, 0.15],
        [0.; 3],
    ]));
    let black = renderer.material_sample(&marble).unwrap();
    let mean = |p: &[[f32; 4]]| p.iter().map(|v| v[0] + v[1] + v[2]).sum::<f32>() / p.len() as f32;
    assert!(mean(&black) < mean(&family) * 0.5);
    assert!(black.iter().all(|p| p.iter().all(|v| v.is_finite())));
    marble.colours = Some(veylvik_document::optical::palette_track([
        [2.0, 0., 0.],
        [0.; 3],
        [0.; 3],
    ]));
    assert!(
        veylvik_document::Document::new({
            let mut doc = blank(8, 8, Time::new(24, 1).unwrap()).unwrap();
            let node = Node::new("m", Operation::Material(marble), &[("image", doc.output)]);
            doc.output = node.id;
            doc.nodes.insert(node.id, node);
            doc
        })
        .is_err()
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn connected_shot_colours_reflections_and_shot_zero_is_the_studio() {
    let renderer = Renderer::new().unwrap();
    let (doc, inputs, _) = source();
    let coverage = doc.output;
    let plate = Node::new(
        "Red shot",
        Operation::Solid(nodes::Solid {
            color: [0.9, 0.05, 0.02, 1.0],
        }),
        &[],
    );
    let preset = veylvik_document::presets::builtin::build("silver_text").unwrap();
    let mut silver = preset
        .nodes
        .values()
        .find_map(|n| match &n.operation {
            Operation::Material(m) => Some(m.clone()),
            _ => None,
        })
        .unwrap();
    silver.lighting = 0.0.into();
    let render = |silver: &nodes::Material, environment: bool| {
        let mut ports = vec![("image", coverage)];
        if environment {
            ports.push(("environment", plate.id));
        }
        let node = Node::new("m", Operation::Material(silver.clone()), &ports);
        let mut d = doc.clone();
        d.nodes.insert(plate.id, plate.clone());
        d.output = node.id;
        d.nodes.insert(node.id, node);
        renderer
            .render_frame(
                &Plan::compile(&d).unwrap(),
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap()
            .pixels
    };
    let studio = render(&silver, false);
    silver.shot = 0.0.into();
    assert_eq!(
        render(&silver, true),
        studio,
        "shot 0 keeps the studio look"
    );
    silver.shot = 1.0.into();
    let shot = render(&silver, true);
    let redness = |p: &[[f32; 4]]| p.iter().map(|v| v[0] - v[2]).sum::<f32>();
    assert!(
        redness(&shot) > redness(&studio) + 1.0,
        "the red shot tints the chrome"
    );
    assert!(shot.iter().all(|p| p.iter().all(|v| v.is_finite())));
    assert_eq!(
        shot.iter().map(|p| p[3]).collect::<Vec<_>>(),
        studio.iter().map(|p| p[3]).collect::<Vec<_>>()
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn frosted_glass_is_smooth_and_keeps_the_frame_opaque() {
    let renderer = Renderer::new().unwrap();
    let (mut doc, mut inputs, _) = source();
    let checker: Vec<_> = (0..96 * 64)
        .map(|i| {
            let v = if (i % 96 / 2 + i / 96 / 2) % 2 == 0 {
                1.0
            } else {
                0.0
            };
            [v, v, v, 1.0]
        })
        .collect();
    inputs.insert("checker", Arc::new(Image::new(96, 64, checker).unwrap()));
    let background = Node::new(
        "Checker",
        Operation::Image(nodes::Image {
            asset: "checker".into(),
        }),
        &[],
    );
    let glass = Node::new(
        "Frost",
        serde_json::from_value(serde_json::json!({
            "type":"glass","bevel":{"kind":"constant","value":2.0},
            "refraction":{"kind":"constant","value":0.0},"roughness":{"kind":"constant","value":12.0},
            "ior":{"kind":"constant","value":1.5},"transmission":{"kind":"constant","value":1.0},
            "tint":{"kind":"constant","value":0.0},"dispersion":{"kind":"constant","value":0.0}}))
        .unwrap(),
        &[("image", doc.output), ("background", background.id)],
    );
    doc.output = glass.id;
    doc.nodes.insert(background.id, background);
    doc.nodes.insert(glass.id, glass);
    let pixels = renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap()
        .pixels;
    // Fully covered interior, including the frame edge: opaque and nearly flat grey
    // although the 2-pixel checker underneath alternates between 0 and 1.
    for y in 0..64 {
        for x in 24..96 {
            let p = pixels[y * 96 + x];
            assert!((p[3] - 1.0).abs() < 1e-4, "alpha at {x},{y} is {}", p[3]);
            // Bevel highlights sit within a few pixels of the canvas edge.
            if (6..58).contains(&y) && x + 1 < 90 {
                let q = pixels[y * 96 + x + 1];
                assert!(
                    (p[0] - q[0]).abs() < 0.02,
                    "grain at {x},{y}: {} vs {}",
                    p[0],
                    q[0]
                );
            }
        }
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn a_shared_light_rig_lights_every_surface_like_their_own_lights() {
    let renderer = Renderer::new().unwrap();
    let (doc, inputs, _) = source();
    let coverage = doc.output;
    let preset = veylvik_document::presets::builtin::build("gold_text").unwrap();
    let mut gold = preset
        .nodes
        .values()
        .find_map(|n| match &n.operation {
            Operation::Material(m) => Some(m.clone()),
            _ => None,
        })
        .unwrap();
    gold.lighting = 0.0.into();
    let mut studio = veylvik_document::optical::StudioRig::default();
    studio.lights[1].enabled = true;
    studio.lights[1].intensity = 6.0.into();
    let render = |own: &veylvik_document::optical::StudioRig,
                  rig: Option<&veylvik_document::optical::StudioRig>| {
        let mut d = doc.clone();
        let mut material = gold.clone();
        material.studio = own.clone();
        let mut ports = vec![("image", coverage)];
        let light = Node::new(
            "Rig",
            Operation::LightRig(nodes::LightRig {
                studio: rig.cloned().unwrap_or_default(),
            }),
            &[],
        );
        if rig.is_some() {
            ports.push(("lights", light.id));
            d.nodes.insert(light.id, light);
        }
        let node = Node::new("m", Operation::Material(material), &ports);
        d.output = node.id;
        d.nodes.insert(node.id, node);
        renderer
            .render_frame(
                &Plan::compile(&d).unwrap(),
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap()
            .pixels
    };
    let own = render(&studio, None);
    // The rig replaces the surface's own (default) lights entirely.
    let shared = render(
        &veylvik_document::optical::StudioRig::default(),
        Some(&studio),
    );
    assert_eq!(shared, own);
    let mut brighter = studio.clone();
    brighter.lights[1].intensity = 12.0.into();
    let relit = render(
        &veylvik_document::optical::StudioRig::default(),
        Some(&brighter),
    );
    let sum = |p: &[[f32; 4]]| p.iter().map(|v| v[0] + v[1] + v[2]).sum::<f32>();
    assert!(
        sum(&relit) > sum(&shared) * 1.01,
        "moving the rig's light relights the surface"
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn a_key_light_matched_to_the_shot_faces_and_lights_the_surface() {
    use veylvik_document::optical::{ShotLight, StudioRig};
    let renderer = Renderer::new().unwrap();
    let (doc, inputs, _) = source();
    let coverage = doc.output;
    let mut silver = veylvik_document::presets::material::Source::Builtin("silver_text")
        .material()
        .unwrap();
    silver.lighting = 0.0.into();
    let render = |studio: StudioRig| {
        let mut d = doc.clone();
        let mut m = silver.clone();
        m.studio = studio;
        let node = Node::new("m", Operation::Material(m), &[("image", coverage)]);
        d.output = node.id;
        d.nodes.insert(node.id, node);
        renderer
            .render_frame(
                &Plan::compile(&d).unwrap(),
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap()
            .pixels
    };
    let (w, h) = (96, 64);
    for sun in [[80.0, 10.0], [15.0, 55.0]] {
        let shot: Vec<[f32; 4]> = (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as f32, (i / w) as f32);
                if (x - sun[0]).hypot(y - sun[1]) < 6.0 {
                    [6.0, 5.0, 4.0, 1.0]
                } else {
                    [0.05; 4]
                }
            })
            .collect();
        let mut matched = StudioRig::default();
        matched
            .match_shot(
                &ShotLight::analyse(&shot, w as u32, h as u32).unwrap(),
                w as u32,
                h as u32,
                Time::new(0, 1).unwrap(),
            )
            .unwrap();
        let mut dark = matched.clone();
        dark.lights[1].enabled = false;
        let sum = |p: &[[f32; 4]]| p.iter().map(|v| v[0] + v[1] + v[2]).sum::<f32>();
        assert!(
            sum(&render(matched)) > sum(&render(dark)) * 1.02,
            "the key light at {sun:?} lights the surface"
        );
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn a_light_following_a_track_relights_over_time_without_decoding_the_footage() {
    use veylvik_document::planar::{Sample, Track};
    let renderer = Renderer::new().unwrap();
    let (mut doc, inputs, _) = source();
    let coverage = doc.output;
    let quad = |x: f64| [[x, 10.0], [x + 10.0, 10.0], [x + 10.0, 20.0], [x, 20.0]];
    let sample = |ticks: i64, x: f64| Sample {
        time: Time { ticks, rate: 24 },
        corners: quad(x),
        features: vec![],
        rms_pixels: 0.0,
        motion: None,
    };
    // Footage that is never supplied: rendering the title must not need it.
    let footage = Node::new(
        "Unused footage",
        Operation::Image(nodes::Image {
            asset: "missing.png".into(),
        }),
        &[],
    );
    let track = Node::new(
        "Lamp track",
        Operation::PlanarTrack(nodes::PlanarTrack {
            data: Box::new(Track {
                reference_time: Time { ticks: 0, rate: 24 },
                reference: quad(2.0),
                region: None,
                samples: vec![sample(0, 2.0), sample(24, 84.0)],
            }),
        }),
        &[("image", footage.id)],
    );
    let mut studio = veylvik_document::optical::StudioRig {
        environment: 0.0,
        ..Default::default()
    };
    studio.lights[1].enabled = true;
    studio.lights[1].follow = true;
    studio.lights[1].position = [0.0, 0.0, 1.0].into();
    studio.lights[1].intensity = 20.0.into();
    let rig = Node::new(
        "Rig",
        Operation::LightRig(nodes::LightRig { studio }),
        &[("track", track.id)],
    );
    let mut silver = veylvik_document::presets::material::Source::Builtin("silver_text")
        .material()
        .unwrap();
    silver.lighting = 0.0.into();
    let material = Node::new(
        "m",
        Operation::Material(silver),
        &[("image", coverage), ("lights", rig.id)],
    );
    doc.output = material.id;
    for n in [footage, track, rig, material] {
        doc.nodes.insert(n.id, n);
    }
    let plan = Plan::compile(&doc).unwrap();
    assert!(
        !plan.required_assets().contains("missing.png"),
        "the track's footage is data only"
    );
    let frame = |ticks| {
        renderer
            .render_frame(
                &plan,
                RenderRequest::full(Time::new(ticks, 24).unwrap()),
                &inputs,
            )
            .unwrap()
            .pixels
    };
    let (start, end) = (frame(0), frame(24));
    // The light moves from the left to the right of the frame: so does the bright side.
    let side = |p: &[[f32; 4]], right: bool| {
        p.iter()
            .enumerate()
            .filter(|(i, _)| (i % 96 >= 56) == right && i % 96 >= 16)
            .map(|(_, v)| v[0])
            .sum::<f32>()
    };
    assert!(side(&start, false) / side(&start, true) > side(&end, false) / side(&end, true));
}

#[test]
#[ignore = "requires Vulkan"]
fn every_catalogue_look_renders_finite_and_distinct() {
    let renderer = Renderer::new().unwrap();
    let mut seen: Vec<Vec<[f32; 4]>> = Vec::new();
    for entry in &veylvik_document::presets::catalogue::ENTRIES {
        let pixels = renderer.material_sample(&entry.material()).unwrap();
        assert!(
            pixels.iter().all(|p| p.iter().all(|v| v.is_finite())),
            "{}",
            entry.id
        );
        assert!(
            pixels
                .iter()
                .any(|p| p[3] > 0.5 && p[0] + p[1] + p[2] > 0.001),
            "{} is blank",
            entry.id
        );
        assert!(
            !seen.contains(&pixels),
            "{} looks like another entry",
            entry.id
        );
        seen.push(pixels);
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn an_image_texture_colours_the_surface_and_keeps_its_shape() {
    let renderer = Renderer::new().unwrap();
    let (mut doc, mut inputs, _) = source();
    let coverage = doc.output;
    // Left half red, right half blue.
    let texture: Vec<_> = (0..96 * 64)
        .map(|i| {
            if i % 96 < 56 {
                [0.8, 0.05, 0.05, 1.0]
            } else {
                [0.05, 0.05, 0.8, 1.0]
            }
        })
        .collect();
    inputs.insert("texture", Arc::new(Image::new(96, 64, texture).unwrap()));
    let image = Node::new(
        "Texture",
        Operation::Image(nodes::Image {
            asset: "texture".into(),
        }),
        &[],
    );
    let mut m = veylvik_document::presets::material::Source::Builtin("gloss_red_plastic")
        .material()
        .unwrap();
    m.kind = MaterialKind::Texture;
    m.colours = None;
    m.lighting = 0.0.into();
    let textured = Node::new(
        "m",
        Operation::Material(m.clone()),
        &[("image", coverage), ("texture", image.id)],
    );
    let bare = Node::new("bare", Operation::Material(m), &[("image", coverage)]);
    doc.nodes.insert(image.id, image);
    let render = |d: &Composition| {
        renderer
            .render_frame(
                &Plan::compile(d).unwrap(),
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap()
            .pixels
    };
    let mut with = doc.clone();
    with.output = textured.id;
    with.nodes.insert(textured.id, textured);
    let pixels = render(&with);
    let (left, right) = (pixels[32 * 96 + 30], pixels[32 * 96 + 80]);
    assert!(
        left[0] > left[2] * 2.0 && right[2] > right[0] * 2.0,
        "{left:?} {right:?}"
    );
    assert!(
        pixels
            .iter()
            .zip(&inputs.get("coverage").unwrap().pixels().to_vec())
            .all(|(p, c)| (p[3] - c[3]).abs() < 1e-5)
    );
    doc.output = bare.id;
    doc.nodes.insert(bare.id, bare);
    assert!(
        render(&doc).iter().all(|p| p.iter().all(|v| v.is_finite())),
        "no texture connected is still valid"
    );
    // The library swatch of a family that draws its own pattern still renders.
    renderer
        .material_swatch(MaterialKind::Gold, 0.2, 0.3)
        .unwrap();
}

#[test]
#[ignore = "requires Vulkan"]
fn glass_shows_the_shot_through_it_tinted() {
    let renderer = Renderer::new().unwrap();
    let (mut doc, inputs, _) = source();
    let coverage = doc.output;
    let shot = Node::new(
        "Green shot",
        Operation::Solid(nodes::Solid {
            color: [0.05, 0.6, 0.1, 1.0],
        }),
        &[],
    );
    let ruby = veylvik_document::presets::catalogue::get("glass_clear")
        .unwrap()
        .material();
    let glass = Node::new(
        "Glass",
        Operation::Material(ruby),
        &[("image", coverage), ("environment", shot.id)],
    );
    doc.output = glass.id;
    doc.nodes.insert(shot.id, shot);
    doc.nodes.insert(glass.id, glass);
    let pixels = renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap()
        .pixels;
    let p = pixels[32 * 96 + 60];
    assert!(
        p[1] > p[0] * 2.0 && p[1] > 0.3,
        "clear glass passes the green shot: {p:?}"
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn materials_preview_on_the_users_own_words() {
    let renderer = Renderer::new().unwrap();
    let mut text: Operation = serde_json::from_value(registry::TEXT.default_operation()).unwrap();
    let Operation::Text(t) = &mut text else {
        panic!()
    };
    t.content = "\nHello world\nsecond line".into();
    for id in ["gold_text", "glass_clear", "denim"] {
        let m = veylvik_document::presets::material::Source::Builtin(id)
            .material()
            .unwrap();
        let pixels = renderer.material_on_text(t, &m).unwrap();
        assert_eq!(pixels.len(), 256 * 128);
        assert!(pixels.iter().all(|p| p.iter().all(|v| v.is_finite())));
        let inked = pixels.iter().filter(|p| p[3] > 0.5).count();
        if id == "glass_clear" {
            assert_eq!(inked, pixels.len(), "glass is shown over its checkerboard");
        } else {
            assert!(
                inked > 1500 && inked < 256 * 128 / 2,
                "{id}: the words, centred: {inked}"
            );
            // Ink sits around the centre, not against an edge.
            let ink_x = pixels
                .iter()
                .enumerate()
                .filter(|(_, p)| p[3] > 0.5)
                .map(|(i, _)| (i % 256) as f32)
                .sum::<f32>()
                / inked as f32;
            assert!((ink_x - 128.0).abs() < 20.0, "{id} centred: {ink_x}");
        }
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn pbr_maps_tilt_roughen_and_raise_the_surface() {
    let renderer = Renderer::new().unwrap();
    let (mut doc, mut inputs, coverage_pixels) = source();
    let coverage = doc.output;
    // Maps arrive linearised like any imported 8-bit image.
    let linear = |d: f32| {
        if d <= 0.04045 {
            d / 12.92
        } else {
            ((d + 0.055) / 1.055).powf(2.4)
        }
    };
    let map = |f: &dyn Fn(usize, usize) -> [f32; 3]| {
        let pixels = (0..96 * 64)
            .map(|i| {
                let [r, g, b] = f(i % 96, i / 96);
                [linear(r), linear(g), linear(b), 1.0]
            })
            .collect();
        Arc::new(Image::new(96, 64, pixels).unwrap())
    };
    inputs.insert("flat_normal", map(&|_, _| [0.5, 0.5, 1.0]));
    // Right half tilted towards +x, top half towards +green.
    inputs.insert(
        "tilted_normal",
        map(&|x, _| {
            if x < 56 {
                [0.5, 0.5, 1.0]
            } else {
                [0.85, 0.5, 0.86]
            }
        }),
    );
    inputs.insert("green_normal", map(&|_, _| [0.5, 0.8, 0.9]));
    inputs.insert("half_grey", map(&|_, _| [0.5; 3]));
    inputs.insert("black", map(&|_, _| [0.0; 3]));
    inputs.insert("white", map(&|_, _| [1.0; 3]));
    inputs.insert(
        "ramp",
        map(&|x, _| [((x as f32) / 6.0).sin() * 0.5 + 0.5; 3]),
    );
    let mut m = veylvik_document::presets::material::Source::Builtin("gold_text")
        .material()
        .unwrap();
    m.lighting = 0.0.into();
    m.roughness = 0.5.into();
    let mut ids = std::collections::BTreeMap::new();
    for key in [
        "flat_normal",
        "tilted_normal",
        "green_normal",
        "half_grey",
        "black",
        "white",
        "ramp",
    ] {
        let node = Node::new(
            key,
            Operation::Image(nodes::Image { asset: key.into() }),
            &[],
        );
        ids.insert(key, node.id);
        doc.nodes.insert(node.id, node);
    }
    let render = |material: &veylvik_document::nodes::Material, maps: &[(&str, &str)]| {
        let mut d = doc.clone();
        let mut ports = vec![("image", coverage)];
        ports.extend(maps.iter().map(|(port, key)| (*port, ids[key])));
        let node = Node::new("m", Operation::Material(material.clone()), &ports);
        d.output = node.id;
        d.nodes.insert(node.id, node);
        renderer
            .render_frame(
                &Plan::compile(&d).unwrap(),
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap()
            .pixels
    };
    let differs = |a: &[[f32; 4]], b: &[[f32; 4]], x: std::ops::Range<usize>| {
        a.iter()
            .zip(b)
            .enumerate()
            .filter(|(i, (p, q))| {
                x.contains(&(i % 96)) && (0..3).any(|c| (p[c] - q[c]).abs() > 0.01)
            })
            .count()
    };
    let bare = render(&m, &[]);
    // Neutral maps leave the surface exactly as it was.
    for neutral in [
        render(&m, &[("normal_map", "flat_normal")]),
        render(&m, &[("roughness_map", "half_grey")]),
        render(&m, &[("height_map", "white")]),
    ] {
        assert_eq!(differs(&bare, &neutral, 0..96), 0);
    }
    // A tilted normal map changes only where it tilts.
    let tilted = render(&m, &[("normal_map", "tilted_normal")]);
    assert_eq!(differs(&bare, &tilted, 16..54), 0);
    assert!(differs(&bare, &tilted, 58..94) > 1000);
    // Strength 0 switches the map off.
    let mut off = m.clone();
    off.normal_strength = 0.0.into();
    assert_eq!(
        differs(
            &bare,
            &render(&off, &[("normal_map", "tilted_normal")]),
            0..96
        ),
        0
    );
    // OpenGL and DirectX disagree about green.
    let gl = render(&m, &[("normal_map", "green_normal")]);
    let mut dx = m.clone();
    dx.normal_directx = true;
    assert!(differs(&gl, &render(&dx, &[("normal_map", "green_normal")]), 16..96) > 1000);
    // A black roughness map polishes, a white one roughens.
    let glossy = render(&m, &[("roughness_map", "black")]);
    let rough = render(&m, &[("roughness_map", "white")]);
    assert!(differs(&glossy, &rough, 16..96) > 1000);
    assert!(differs(&bare, &glossy, 16..96) > 1000);
    // A height ramp adds relief; depth 0 removes it.
    assert!(differs(&bare, &render(&m, &[("height_map", "ramp")]), 16..96) > 1000);
    let mut flat = m.clone();
    flat.height_depth = 0.0.into();
    assert_eq!(
        differs(&bare, &render(&flat, &[("height_map", "ramp")]), 0..96),
        0
    );
    // All three together keep the coverage and stay finite.
    let all = render(
        &m,
        &[
            ("normal_map", "tilted_normal"),
            ("roughness_map", "ramp"),
            ("height_map", "ramp"),
        ],
    );
    for (p, c) in all.iter().zip(&coverage_pixels) {
        assert!((p[3] - c[3]).abs() < 1e-5 && p.iter().all(|v| v.is_finite()));
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn a_normal_map_and_the_height_map_it_came_from_shade_alike() {
    let renderer = Renderer::new().unwrap();
    let (w, h, depth) = (96u32, 64u32, 16.0f32);
    let linear = |d: f32| {
        if d <= 0.04045 {
            d / 12.92
        } else {
            ((d + 0.055) / 1.055).powf(2.4)
        }
    };
    let dome = |x: f32, y: f32| {
        let d = ((x - 48.0).powi(2) + (y - 32.0).powi(2)).sqrt() / 24.0;
        (1.0 - d * d).max(0.0)
    };
    let (mut heights, mut normals) = (Vec::new(), Vec::new());
    for i in 0..(w * h) as usize {
        let (x, y) = ((i % 96) as f32, (i / 96) as f32);
        let v = linear(dome(x, y));
        heights.push([v, v, v, 1.0]);
        // OpenGL convention: green points up the image.
        let dx = (dome(x + 1.0, y) - dome(x - 1.0, y)) * 0.5 * depth;
        let dy = (dome(x, y + 1.0) - dome(x, y - 1.0)) * 0.5 * depth;
        let length = (dx * dx + dy * dy + 1.0).sqrt();
        let encode = |v: f32| linear(v / length * 0.5 + 0.5);
        normals.push([encode(-dx), encode(dy), encode(1.0), 1.0]);
    }
    let mut inputs = ImageInputs::default();
    inputs.insert("heights", Arc::new(Image::new(w, h, heights).unwrap()));
    inputs.insert("normals", Arc::new(Image::new(w, h, normals).unwrap()));
    inputs.insert(
        "cover",
        Arc::new(Image::new(w, h, vec![[1.0; 4]; (w * h) as usize]).unwrap()),
    );
    let mut doc = blank(w, h, Time::new(24, 1).unwrap()).unwrap();
    let mut ids = std::collections::BTreeMap::new();
    for key in ["heights", "normals", "cover"] {
        let node = Node::new(
            key,
            Operation::Image(nodes::Image { asset: key.into() }),
            &[],
        );
        ids.insert(key, node.id);
        doc.nodes.insert(node.id, node);
    }
    let mut m = veylvik_document::presets::material::Source::Builtin("gloss_red_plastic")
        .material()
        .unwrap();
    m.lighting = 0.0.into();
    m.bevel = 0.0.into();
    m.grain = 0.0.into();
    m.height_depth = depth.into();
    let render = |m: &veylvik_document::nodes::Material, port: &str, key: &str| {
        let mut d = doc.clone();
        let node = Node::new(
            "m",
            Operation::Material(m.clone()),
            &[("image", ids["cover"]), (port, ids[key])],
        );
        d.output = node.id;
        d.nodes.insert(node.id, node);
        renderer
            .render_frame(
                &Plan::compile(&d).unwrap(),
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap()
            .pixels
    };
    let rms = |p: &[[f32; 4]], q: &[[f32; 4]]| {
        (p.iter()
            .zip(q)
            .map(|(a, b)| (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>())
            .sum::<f32>()
            / p.len() as f32)
            .sqrt()
    };
    let raised = render(&m, "height_map", "heights");
    let mapped = render(&m, "normal_map", "normals");
    let mut directx = m.clone();
    directx.normal_directx = true;
    let flipped = render(&directx, "normal_map", "normals");
    assert!(rms(&raised, &mapped) < 0.005, "{}", rms(&raised, &mapped));
    assert!(rms(&raised, &flipped) > 0.05, "{}", rms(&raised, &flipped));
}

#[test]
#[ignore = "requires Vulkan"]
fn layered_materials_mask_and_wear_from_the_edges() {
    let renderer = Renderer::new().unwrap();
    let (w, h) = (96u32, 64u32);
    let solid = |c: [f32; 4]| Operation::Solid(nodes::Solid { color: c });
    let mut doc = blank(w, h, Time::new(24, 1).unwrap()).unwrap();
    // A 64x40 opaque block; Under is blue, Over is red, both with its shape.
    let block: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if (16..80).contains(&x) && (12..52).contains(&y) {
                [1.0; 4]
            } else {
                [0.0; 4]
            }
        })
        .collect();
    let left: Vec<[f32; 4]> = (0..w * h)
        .map(|i| if i % w < 48 { [1.0; 4] } else { [0.0; 4] })
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert("block", Arc::new(Image::new(w, h, block).unwrap()));
    inputs.insert("left", Arc::new(Image::new(w, h, left).unwrap()));
    let image = |key: &str| {
        Node::new(
            key,
            Operation::Image(nodes::Image { asset: key.into() }),
            &[],
        )
    };
    let (shape, half) = (image("block"), image("left"));
    let (blue, red) = (
        Node::new("blue", solid([0.0, 0.0, 1.0, 1.0]), &[]),
        Node::new("red", solid([1.0, 0.0, 0.0, 1.0]), &[]),
    );
    let cut = |label: &str, colour: &Node| {
        Node::new(
            label,
            Operation::Mask(nodes::Mask {}),
            &[("image", colour.id), ("mask", shape.id)],
        )
    };
    let (under, over) = (cut("under", &blue), cut("over", &red));
    for n in [&shape, &half, &blue, &red, &under, &over] {
        doc.nodes.insert(n.id, n.clone());
    }
    let render = |layers: nodes::MaterialLayers, mask: bool| {
        let mut d = doc.clone();
        let mut ports = vec![("under", under.id), ("over", over.id)];
        if mask {
            ports.push(("mask", half.id));
        }
        let node = Node::new("layers", Operation::MaterialLayers(layers), &ports);
        d.output = node.id;
        d.nodes.insert(node.id, node);
        renderer
            .render_frame(
                &Plan::compile(&d).unwrap(),
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap()
            .pixels
    };
    let layers = |mix: f32, wear: f32| nodes::MaterialLayers {
        seed: 3,
        mix: mix.into(),
        edge_wear: wear.into(),
        wear_width: 6.0.into(),
        breakup: 0.0.into(),
        noise_size: 30.0.into(),
    };
    let at = |p: &[[f32; 4]], x: u32, y: u32| p[(y * w + x) as usize];
    let centre = (48, 32);
    // Over everywhere without wear; Under at mix 0; the mask keeps Over on the left.
    let full = render(layers(1.0, 0.0), false);
    assert!(at(&full, 20, 14)[0] > 0.99 && at(&full, centre.0, centre.1)[0] > 0.99);
    assert!(at(&render(layers(0.0, 0.0), false), 40, 30)[2] > 0.99);
    let masked = render(layers(1.0, 0.0), true);
    assert!(at(&masked, 30, 32)[0] > 0.99 && at(&masked, 70, 32)[2] > 0.99);
    // Full wear reveals Under at the edges and corners, never deep inside.
    let worn = render(layers(1.0, 1.0), false);
    assert!(at(&worn, 16, 12)[2] > 0.9, "corner {:?}", at(&worn, 16, 12));
    assert!(at(&worn, 16, 32)[2] > 0.9, "edge {:?}", at(&worn, 16, 32));
    assert!(at(&worn, centre.0, centre.1)[0] > 0.99);
    // Coverage is the shape's: nothing outside, opaque inside.
    for p in [&full, &worn, &masked] {
        assert_eq!(at(p, 4, 4)[3], 0.0);
        assert!((at(p, centre.0, centre.1)[3] - 1.0).abs() < 1e-6);
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn keyed_colours_and_lights_change_the_render_over_time() {
    let renderer = Renderer::new().unwrap();
    let (mut doc, inputs, _) = source();
    let coverage = doc.output;
    let mut m = veylvik_document::presets::material::Source::Builtin("gloss_red_plastic")
        .material()
        .unwrap();
    m.lighting = 0.0.into();
    let mut colours = veylvik_document::optical::palette_track(MaterialKind::Plastic.palette());
    colours[0] = [0.8, 0.05, 0.05].into();
    colours[0]
        .set_key_at(Time::new(0, 1).unwrap(), [0.8, 0.05, 0.05])
        .unwrap();
    colours[0]
        .set_key_at(Time::new(2, 1).unwrap(), [0.05, 0.05, 0.8])
        .unwrap();
    m.colours = Some(colours);
    let node = Node::new("m", Operation::Material(m), &[("image", coverage)]);
    doc.output = node.id;
    doc.nodes.insert(node.id, node);
    let plan = Plan::compile(&doc).unwrap();
    let frame = |ticks| {
        renderer
            .render_frame(
                &plan,
                RenderRequest::full(Time::new(ticks, 1).unwrap()),
                &inputs,
            )
            .unwrap()
            .pixels[32 * 96 + 60]
    };
    let (red, blue) = (frame(0), frame(2));
    assert!(
        red[0] > red[2] * 2.0 && blue[2] > blue[0] * 2.0,
        "{red:?} {blue:?}"
    );
    // A keyed light sweeping across moves its highlight.
    let Operation::Material(m) = &mut doc.nodes.get_mut(&doc.output).unwrap().operation else {
        panic!()
    };
    m.colours = None;
    let mut light = veylvik_document::optical::AreaLight {
        intensity: 30.0.into(),
        size: [0.5, 0.5],
        ..Default::default()
    };
    light
        .position
        .set_key_at(Time::new(0, 1).unwrap(), [-2.0, 0.0, 2.0])
        .unwrap();
    light
        .position
        .set_key_at(Time::new(2, 1).unwrap(), [2.0, 0.0, 2.0])
        .unwrap();
    light.rotation = veylvik_document::optical::aim([0.0, 0.0, 2.0]).into();
    m.studio.lights = vec![light];
    m.studio.environment = 0.0;
    let plan = Plan::compile(&doc).unwrap();
    let brightness = |ticks| {
        let pixels = renderer
            .render_frame(
                &plan,
                RenderRequest::full(Time::new(ticks, 1).unwrap()),
                &inputs,
            )
            .unwrap()
            .pixels;
        let row = &pixels[32 * 96..33 * 96];
        let total: f32 = row.iter().map(|p| p[0] + p[1] + p[2]).sum();
        row.iter()
            .enumerate()
            .map(|(x, p)| x as f32 * (p[0] + p[1] + p[2]))
            .sum::<f32>()
            / total.max(1e-6)
    };
    assert!(
        brightness(2) > brightness(0) + 1.0,
        "{} vs {}",
        brightness(0),
        brightness(2)
    );
}
