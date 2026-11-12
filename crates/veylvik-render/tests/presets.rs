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
    base.nodes.get_mut(&base.output).unwrap().operation = Operation::Gradient {
        mode: GradientMode::Linear,
        start_x: 0.0.into(),
        start_y: 0.0.into(),
        end_x: 96.0.into(),
        end_y: 54.0.into(),
    };
    let mut flat = blank(48, 32, Time::new(24, 1).unwrap()).unwrap();
    flat.nodes.get_mut(&flat.output).unwrap().operation = Operation::Solid {
        color: [0.2, 0.3, 0.4, 1.0],
    };
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
            .instantiate(&base, &[("input_1".into(), base.output)].into(), None)
            .unwrap();
        let mut owner = Document::new(base.clone()).unwrap();
        owner.apply(0, &instance.commands).unwrap();
        let captured = Preset::capture(owner.snapshot(), instance.group).unwrap();
        let wire = serde_json::to_vec(&captured).unwrap();
        let restored: Preset = serde_json::from_slice(&wire).unwrap();
        let copy = restored
            .instantiate(
                owner.snapshot(),
                &[("input_1".into(), base.output)].into(),
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
            assert!(
                a.pixels.iter().all(|p| (p[3] - 1.0).abs() < 1e-6),
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
        let middle = renderer.render(&after, Time::new(2, 1).unwrap()).unwrap();
        let end = renderer.render(&after, Time::new(4, 1).unwrap()).unwrap();
        let untreated = renderer.render(&before, Time::new(2, 1).unwrap()).unwrap();
        assert_eq!(start, end, "{} loop seam", entry.id);
        assert_ne!(start, middle, "{} has no motion", entry.id);
        let changed = middle
            .iter()
            .zip(&untreated)
            .filter(|(a, b)| (0..3).any(|i| (a[i] - b[i]).abs() > 0.01))
            .count();
        assert!(changed > 2000, "{} effect is not visible", entry.id);
        assert!(middle.iter().flatten().all(|v| v.is_finite()));
    }
}
