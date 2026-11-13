use veylvik_document::*;
use veylvik_render::*;
fn scene(width: u32, height: u32, phase: f32) -> Composition {
    let mut doc = blank(width, height, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation =
        Operation::Fire(veylvik_document::nodes::Fire {
            seed: 297,
            x: (width as f32 * 0.5).into(),
            y: (height as f32 * 0.85).into(),
            width: (width as f32 * 0.7).into(),
            height: (height as f32 * 0.7).into(),
            evolution: phase.into(),
            intensity: 1.1.into(),
            turbulence: 1.4.into(),
        });
    doc
}
#[test]
#[ignore = "requires Vulkan"]
fn fire_is_transparent_deterministic_periodic_and_native_uhd() {
    let renderer = Renderer::new().unwrap();
    let render = |phase| {
        renderer
            .render(
                &Plan::compile(&scene(160, 90, phase)).unwrap(),
                Time::new(0, 1).unwrap(),
            )
            .unwrap()
    };
    let first = render(0.0);
    assert_eq!(first, render(4.0));
    assert_ne!(first, render(1.0));
    assert_eq!(first, render(0.0));
    assert!(first.iter().any(|p| p[3] > 0.5));
    assert!(first.contains(&[0.0; 4]));
    assert!(
        first
            .iter()
            .all(|p| p.iter().all(|v| v.is_finite()) && (0.0..=1.0).contains(&p[3]))
    );
    let near = render(3.9999);
    let delta: f32 = first
        .iter()
        .zip(near)
        .flat_map(|(a, b)| a.iter().zip(b).map(|(a, b)| (a - b).abs()))
        .sum();
    assert!(delta / (first.len() as f32) < 0.001);
    let doc = scene(3840, 2160, 1.0);
    let frame = renderer
        .render_gpu_cached(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap();
    assert_eq!((frame.region().width, frame.region().height), (3840, 2160));
    let pixels = renderer.read_frame(&frame).unwrap().pixels;
    assert!(pixels[(1800 * 3840 + 1920) as usize][3] > 0.1);
    assert_eq!(pixels[0], [0.0; 4]);
}
#[test]
#[ignore = "requires Vulkan"]
fn adapted_particle_preset_renders_at_native_uhd() {
    use presets::{builtin, resolution::Sizing};
    let renderer = Renderer::new().unwrap();
    let base = blank(3840, 2160, Time::new(30, 1).unwrap()).unwrap();
    let inserted = builtin::build("embers")
        .unwrap()
        .instantiate_sized(
            &base,
            &[("input_1".into(), base.output)].into(),
            None,
            Sizing::Fit,
        )
        .unwrap();
    let owner = Document::new(base).unwrap();
    let doc = owner.prepare(0, &inserted.commands).unwrap();
    let frame = renderer
        .render_gpu_cached(
            &Plan::compile_output(&doc, inserted.output).unwrap(),
            RenderRequest::full(Time::new(2, 1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap();
    assert_eq!((frame.region().width, frame.region().height), (3840, 2160));
    assert!(
        renderer
            .read_frame(&frame)
            .unwrap()
            .pixels
            .iter()
            .filter(|p| p[3] > 0.01)
            .count()
            > 1000
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn dci_4k_exceeds_webgpu_default_binding_size_but_renders_on_capable_adapters() {
    let renderer = Renderer::new().unwrap();
    let doc = scene(4096, 2160, 0.0);
    let frame = renderer
        .render_gpu_cached(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap();
    assert_eq!((frame.region().width, frame.region().height), (4096, 2160));
}
