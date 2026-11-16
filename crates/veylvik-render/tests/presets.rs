use veylvik_document::{
    presets::{Preset, builtin},
    *,
};
use veylvik_render::*;
#[test]
#[ignore = "requires Vulkan"]
fn starter_effects_render_identically_after_portable_round_trip_and_shimmer_animates() {
    let renderer = Renderer::new().unwrap();
    let inputs = ImageInputs::default();
    // A small gradient gives distortion and edge operations measurable detail.
    let mut base = blank(96, 54, Time::new(24, 1).unwrap()).unwrap();
    base.nodes.get_mut(&base.output).unwrap().operation =
        Operation::Gradient(veylvik_document::nodes::Gradient {
            mode: GradientMode::Linear,
            start_x: 0.0.into(),
            start_y: 0.0.into(),
            end_x: 96.0.into(),
            end_y: 54.0.into(),
        });
    let mut flat = blank(48, 32, Time::new(24, 1).unwrap()).unwrap();
    flat.nodes.get_mut(&flat.output).unwrap().operation =
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [0.2, 0.3, 0.4, 1.0],
        });
    let neon = builtin::build("neon_outline")
        .unwrap()
        .instantiate(&flat, &[("input_1".into(), flat.output)].into(), None)
        .unwrap();
    let mut flat_owner = Document::new(flat).unwrap();
    flat_owner.apply(0, &neon.commands).unwrap();
    let flat_plan = Plan::compile_output(flat_owner.snapshot(), neon.output).unwrap();
    let flat_frame = renderer
        .render_gpu_cached(
            &flat_plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    for pixel in renderer.read_frame(&flat_frame).unwrap().pixels {
        for (actual, expected) in pixel.into_iter().zip([0.2, 0.3, 0.4, 1.0]) {
            assert!(
                (actual - expected).abs() < 1e-6,
                "false edge on uniform input"
            );
        }
    }
    for starter in builtin::ALL {
        let preset = builtin::build(starter.id).unwrap();
        let instance = preset
            .instantiate(
                &base,
                &preset
                    .group
                    .inputs
                    .keys()
                    .map(|key| (key.clone(), base.output))
                    .collect(),
                None,
            )
            .unwrap();
        let mut owner = Document::new(base.clone()).unwrap();
        owner.apply(0, &instance.commands).unwrap();
        let captured = Preset::capture(owner.snapshot(), instance.group).unwrap();
        let wire = serde_json::to_vec(&captured).unwrap();
        let restored: Preset = serde_json::from_slice(&wire).unwrap();
        let copy = restored
            .instantiate(
                owner.snapshot(),
                &restored
                    .group
                    .inputs
                    .keys()
                    .map(|key| (key.clone(), base.output))
                    .collect(),
                None,
            )
            .unwrap();
        owner.apply(1, &copy.commands).unwrap();
        let original = Plan::compile_output(owner.snapshot(), instance.output).unwrap();
        let duplicate = Plan::compile_output(owner.snapshot(), copy.output).unwrap();
        let mut frames = Vec::new();
        for tick in [0, 48, 96] {
            let request = RenderRequest::full(Time::new(tick, 24).unwrap());
            let a = renderer
                .render_gpu_cached(&original, request, &inputs)
                .unwrap();
            let a = renderer.read_frame(&a).unwrap();
            let b = renderer
                .render_gpu_cached(&duplicate, request, &inputs)
                .unwrap();
            assert_eq!(a.pixels, renderer.read_frame(&b).unwrap().pixels);
            assert!(a.pixels.iter().flatten().all(|v| v.is_finite()));
            // Paper Burn removes the burnt paper by design; every other starter
            // must keep an opaque frame opaque.
            assert!(
                starter.id == "paper_burn" || a.pixels.iter().all(|p| (p[3] - 1.0).abs() < 1e-6),
                "{} exposed frame edges",
                starter.id
            );
            frames.push(a.pixels);
        }
        if starter.id == "liquid_shimmer" {
            assert_ne!(frames[0], frames[1]);
            assert_ne!(frames[1], frames[2]);
            assert_eq!(frames[0], frames[2]);
        }
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn showcase_scenes_have_visible_effects_motion_and_seamless_endpoints() {
    let renderer = Renderer::new().unwrap();
    for entry in builtin::ALL {
        let scene = presets::showcase::build(entry.id).unwrap();
        let after = Plan::compile(&scene.composition).unwrap();
        let before = Plan::compile_output(&scene.composition, scene.before).unwrap();
        let start = renderer.render(&after, Time::new(0, 1).unwrap()).unwrap();
        let sample = if entry.id == "smooth_slow_motion" {
            Time::new(37, 24).unwrap()
        } else {
            Time::new(2, 1).unwrap()
        };
        let middle = renderer.render(&after, sample).unwrap();
        let end = renderer.render(&after, Time::new(4, 1).unwrap()).unwrap();
        let untreated = renderer.render(&before, sample).unwrap();
        assert!(start == end, "{} loop seam", entry.id);
        assert_ne!(start, middle, "{} has no motion", entry.id);
        let changed = middle
            .iter()
            .zip(&untreated)
            .filter(|(a, b)| (0..3).any(|i| (a[i] - b[i]).abs() > 0.01))
            .count();
        // Glints intentionally affect only small highlight points, not the whole face.
        let minimum = if entry.id == "sun_glints" { 300 } else { 2000 };
        assert!(
            changed > minimum,
            "{} effect is not visible ({changed} pixels)",
            entry.id
        );
        assert!(middle.iter().flatten().all(|v| v.is_finite()));
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn particles_emit_on_transparency_and_render_deterministically_after_seeking() {
    let renderer = Renderer::new().unwrap();
    for entry in builtin::ALL
        .iter()
        .filter(|e| builtin::category(e.id) == "Particles")
    {
        let base = blank(960, 540, Time::new(24, 1).unwrap()).unwrap();
        let instance = builtin::build(entry.id)
            .unwrap()
            .instantiate(&base, &[("input_1".into(), base.output)].into(), None)
            .unwrap();
        let owner = Document::new(base).unwrap();
        let composition = owner.prepare(0, &instance.commands).unwrap();
        let plan = Plan::compile_output(&composition, instance.output).unwrap();
        let a = renderer.render(&plan, Time::new(17, 24).unwrap()).unwrap();
        let later = renderer.render(&plan, Time::new(73, 24).unwrap()).unwrap();
        let revisited = renderer.render(&plan, Time::new(17, 24).unwrap()).unwrap();
        assert_eq!(a, revisited, "{} changes with seek order", entry.id);
        assert_ne!(a, later);
        assert!(
            a.iter().filter(|p| p[3] > 0.01).count() > 100,
            "{} emits nothing",
            entry.id
        );
        assert!(
            a.iter().any(|p| p[3] == 0.0),
            "{} destroys transparency",
            entry.id
        );
        assert!(a.iter().all(|p| p[3] >= 0.0 && p[3] <= 1.0));
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn curated_recipes_and_flow_demo_loop_and_remain_visible() {
    let renderer = Renderer::new().unwrap();
    for id in [
        "weathered_bronze",
        "brushed_light_sweep",
        "sky_lit_gold",
        "chrome_shine_sweep",
        "glint_reveal",
        "heat_to_embers",
        "rust_spreading",
        "paper_burn",
        "wet_to_dry",
        "chipping_paint",
        "smooth_slow_motion",
    ] {
        let scene = presets::showcase::build(id).unwrap();
        let plan = Plan::compile(&scene.composition).unwrap();
        let before = Plan::compile_output(&scene.composition, scene.before).unwrap();
        let sample = Time::new(37, 24).unwrap();
        let start = renderer.render(&plan, Time::new(0, 1).unwrap()).unwrap();
        let middle = renderer.render(&plan, sample).unwrap();
        let end = renderer.render(&plan, Time::new(4, 1).unwrap()).unwrap();
        let plain = renderer.render(&before, sample).unwrap();
        assert_eq!(start, end, "{id} loop seam");
        assert_ne!(start, middle, "{id} did not animate");
        let changed = middle
            .iter()
            .zip(&plain)
            .filter(|(a, b)| (0..3).any(|i| (a[i] - b[i]).abs() > 0.01))
            .count();
        assert!(changed > 500, "{id}: only {changed} changed pixels");
        assert!(
            middle
                .iter()
                .all(|p| p.iter().all(|v| v.is_finite()) && (p[3] - 1.).abs() < 1e-6)
        );
    }
}
