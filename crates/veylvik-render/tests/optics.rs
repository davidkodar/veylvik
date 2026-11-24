use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;
fn scene(op: Operation) -> (Composition, ImageInputs, Vec<[f32; 4]>) {
    let mut doc = blank(96, 64, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation =
        Operation::Image(veylvik_document::nodes::Image {
            asset: "shape".into(),
        });
    let pixels: Vec<_> = (0..96 * 64)
        .map(|i| {
            let a = if i % 96 < 16 {
                0.0
            } else if i % 96 < 24 {
                0.25
            } else {
                1.0
            };
            [a * 0.8, a * 0.5, a * 0.2, a]
        })
        .collect();
    let env = Node::new(
        "environment",
        Operation::Image(veylvik_document::nodes::Image {
            asset: "environment".into(),
        }),
        &[],
    );
    let mut bindings = vec![("image", doc.output)];
    if matches!(
        op,
        Operation::SurfaceReflection(veylvik_document::nodes::SurfaceReflection { .. })
    ) {
        bindings.push(("reflection", env.id));
    }
    if matches!(op, Operation::Glass(veylvik_document::nodes::Glass { .. })) {
        bindings.push(("background", env.id));
    }
    let node = Node::new("optics", op, &bindings);
    doc.output = node.id;
    doc.nodes.insert(node.id, node);
    doc.nodes.insert(env.id, env);
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "shape",
        Arc::new(Image::new(96, 64, pixels.clone()).unwrap()),
    );
    inputs.insert(
        "environment",
        Arc::new(Image::new(96, 64, vec![[0.2, 0.4, 0.6, 1.0]; 96 * 64]).unwrap()),
    );
    (doc, inputs, pixels)
}
fn render(r: &Renderer, doc: &Composition, inputs: &ImageInputs) -> Vec<[f32; 4]> {
    r.render_frame(
        &Plan::compile(doc).unwrap(),
        RenderRequest::full(Time::new(0, 1).unwrap()),
        inputs,
    )
    .unwrap()
    .pixels
}
fn set(doc: &mut Composition, key: &str, value: f32) {
    *doc.nodes
        .get_mut(&doc.output)
        .unwrap()
        .operation
        .scalar_mut(key)
        .unwrap() = value.into();
}
#[test]
#[ignore = "requires Vulkan"]
fn reflection_preserves_alpha_and_tracks_changed_environment() {
    let r = Renderer::new().unwrap();
    let (mut doc, mut inputs, original) = scene(Operation::SurfaceReflection(
        veylvik_document::nodes::SurfaceReflection {
            kind: ReflectionKind::Metal,
            bevel: 4.0.into(),
            curvature: 0.4.into(),
            roughness: 2.0.into(),
            strength: 0.8.into(),
            rotation: 0.0.into(),
            scale: 1.0.into(),
            offset_x: 0.0.into(),
            offset_y: 0.0.into(),
        },
    ));
    let first = render(&r, &doc, &inputs);
    assert_ne!(first, original);
    for (a, b) in first.iter().zip(&original) {
        assert_eq!(a[3], b[3]);
        assert!(a.iter().all(|v| v.is_finite()));
    }
    inputs.insert(
        "environment",
        Arc::new(Image::new(96, 64, vec![[0.8, 0.1, 0.05, 1.0]; 96 * 64]).unwrap()),
    );
    assert_ne!(first, render(&r, &doc, &inputs));
    inputs.insert(
        "environment",
        Arc::new(Image::new(96, 64, vec![[0.0; 4]; 96 * 64]).unwrap()),
    );
    assert_eq!(original, render(&r, &doc, &inputs));
    set(&mut doc, "strength", 0.0);
    assert_eq!(original, render(&r, &doc, &inputs));
}
#[test]
#[ignore = "requires Vulkan"]
fn glass_unity_ior_is_background_cutout_and_frosting_is_finite() {
    let r = Renderer::new().unwrap();
    let (mut doc, mut inputs, original) = scene(Operation::Glass(veylvik_document::nodes::Glass {
        bevel: 4.0.into(),
        refraction: 0.0.into(),
        roughness: 0.0.into(),
        ior: 1.0.into(),
        transmission: 1.0.into(),
        tint: 0.0.into(),
        dispersion: 0.0.into(),
    }));
    // Zero bevel removes grazing Fresnel: unity IOR then gives exact transmission.
    set(&mut doc, "bevel", 0.0);
    for (p, source) in render(&r, &doc, &inputs).iter().zip(original) {
        for c in 0..3 {
            assert!((p[c] - [0.2, 0.4, 0.6][c] * source[3]).abs() < 1e-6);
        }
        assert_eq!(p[3], source[3]);
    }
    let checker = (0..96 * 64)
        .map(|i| {
            let c = ((i % 96 / 2 + i / 96 / 2) % 2) as f32;
            [c, c, c, 1.0]
        })
        .collect();
    inputs.insert(
        "environment",
        Arc::new(Image::new(96, 64, checker).unwrap()),
    );
    let clear = render(&r, &doc, &inputs);
    set(&mut doc, "roughness", 12.0);
    let frosted = render(&r, &doc, &inputs);
    assert_ne!(clear, frosted);
    assert!(frosted.iter().all(|p| p.iter().all(|v| v.is_finite())));
    let variance = |p: &Vec<[f32; 4]>| {
        p.iter()
            .skip(32)
            .filter(|p| p[3] == 1.0)
            .map(|p| (p[0] - 0.5).powi(2))
            .sum::<f32>()
    };
    assert!(variance(&frosted) < variance(&clear) * 0.2);
}
#[test]
#[ignore = "requires Vulkan"]
fn glints_are_repeatable_looping_and_can_be_disabled() {
    let r = Renderer::new().unwrap();
    let (mut doc, inputs, original) =
        scene(Operation::SunGlints(veylvik_document::nodes::SunGlints {
            seed: 193,
            size: 8.0.into(),
            intensity: 3.0.into(),
            threshold: 0.1.into(),
            evolution: 0.0.into(),
            density: 1.0.into(),
            angle: 15.0.into(),
        }));
    let start = render(&r, &doc, &inputs);
    assert_ne!(start, original);
    set(&mut doc, "evolution", 2.0);
    assert_ne!(start, render(&r, &doc, &inputs));
    set(&mut doc, "evolution", 4.0);
    assert_eq!(start, render(&r, &doc, &inputs));
    set(&mut doc, "intensity", 0.0);
    assert_eq!(original, render(&r, &doc, &inputs));
    set(&mut doc, "intensity", 3.0);
    set(&mut doc, "density", 0.0);
    assert_eq!(original, render(&r, &doc, &inputs));
}
#[test]
#[ignore = "requires Vulkan"]
fn patina_styles_preserve_coverage_and_zero_amounts_restore_clean_surface() {
    let r = Renderer::new().unwrap();
    let mut previous = Vec::new();
    for kind in PatinaKind::ALL {
        let (mut doc, inputs, original) =
            scene(Operation::Patina(veylvik_document::nodes::Patina {
                kind,
                seed: 71,
                amount: 0.8.into(),
                dirt: 0.3.into(),
                scale: 60.0.into(),
                wear: 0.5.into(),
                scratches: 0.2.into(),
            }));
        let result = render(&r, &doc, &inputs);
        assert_ne!(result, original);
        assert!(previous.iter().all(|p| p != &result));
        for (a, b) in result.iter().zip(&original) {
            assert_eq!(a[3], b[3]);
            assert!(a.iter().all(|v| v.is_finite()));
        }
        assert_eq!(result, render(&r, &doc, &inputs));
        previous.push(result);
        for key in ["amount", "dirt", "scratches"] {
            set(&mut doc, key, 0.0);
        }
        assert_eq!(original, render(&r, &doc, &inputs));
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn polished_gold_showcase_keeps_visible_glints_after_antialiasing() {
    let r = Renderer::new().unwrap();
    let scene = veylvik_presets::showcase::build("sun_glints").unwrap();
    let before = Plan::compile_output(&scene.composition, scene.before).unwrap();
    let after = Plan::compile(&scene.composition).unwrap();
    let time = Time::new(2, 1).unwrap();
    let base = r.render(&before, time).unwrap();
    let lit = r.render(&after, time).unwrap();
    let bright = base
        .iter()
        .filter(|p| p[0] * 0.2126 + p[1] * 0.7152 + p[2] * 0.0722 >= 0.55)
        .count();
    let changed = lit
        .iter()
        .zip(&base)
        .filter(|(a, b)| (0..3).any(|i| (a[i] - b[i]).abs() > 0.01))
        .count();
    assert!(
        changed > 300,
        "Only {changed} glint pixels despite {bright} eligible highlights"
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn glints_find_single_pixel_highlights_in_partial_cells_and_respect_threshold() {
    let r = Renderer::new().unwrap();
    for size in [1., 18., 512.] {
        let (mut doc, mut inputs, _) =
            scene(Operation::SunGlints(veylvik_document::nodes::SunGlints {
                seed: 193,
                size: size.into(),
                intensity: 3.0.into(),
                threshold: 1.0.into(),
                evolution: 0.0.into(),
                density: 1.0.into(),
                angle: 15.0.into(),
            }));
        let mut pixels = vec![[0.; 4]; 96 * 64];
        pixels[96 * 64 - 1] = [2., 2., 2., 0.5];
        inputs.insert(
            "shape",
            Arc::new(Image::new(96, 64, pixels.clone()).unwrap()),
        );
        let mut visible = false;
        for evolution in [0., 1., 2., 3.] {
            set(&mut doc, "evolution", evolution);
            let frame = render(&r, &doc, &inputs);
            visible |= frame != pixels;
            assert!(
                frame
                    .iter()
                    .all(|p| p.iter().all(|v| v.is_finite()) && (0.0..=1.).contains(&p[3]))
            );
        }
        assert!(visible, "Missed an isolated edge highlight at size {size}");
        set(&mut doc, "threshold", 3.);
        assert_eq!(render(&r, &doc, &inputs), pixels);
        inputs.insert(
            "shape",
            Arc::new(Image::new(96, 64, vec![[0.; 4]; 96 * 64]).unwrap()),
        );
        set(&mut doc, "threshold", 0.);
        assert_eq!(render(&r, &doc, &inputs), vec![[0.; 4]; 96 * 64]);
    }
}
