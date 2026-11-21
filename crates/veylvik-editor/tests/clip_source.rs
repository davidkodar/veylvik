use std::sync::Arc;
use veylvik_document::Time;
use veylvik_editor::ClipSource;
use veylvik_render::{HostPixels, Plan, RenderRequest, Renderer};

fn assert_shared<T: Send + Sync>() {}

#[test]
#[ignore = "requires Vulkan"]
fn a_composition_gives_a_host_the_same_frame_whoever_else_is_asking() {
    assert_shared::<ClipSource>();
    let mut renderer = Renderer::new().unwrap();
    renderer.budget_bytes = 2 << 30;
    let renderer = Arc::new(renderer);
    let composition = veylvik_document::motion_demo();
    let (width, height) = (composition.width, composition.height);
    let at = |frame| Time::at_frame(frame, composition.frame_rate).unwrap();
    let source = ClipSource::new(
        renderer.clone(),
        composition.clone(),
        std::env::temp_dir(),
        None,
    )
    .unwrap();
    assert_eq!(source.format().unwrap().0, [width, height]);
    let buffer = |w: u32, h: u32| {
        renderer.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("the host's frame"),
            size: u64::from(w) * u64::from(h) * 16,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    };
    // What the host finds in its buffer, read back as it stands.
    let read = |b: &wgpu::Buffer, w: u32, h: u32| {
        let frame = renderer
            .frame_from_host(b, w, h, &HostPixels::rec709(false))
            .unwrap();
        renderer.read_frame(&frame).unwrap().pixels
    };
    let ours = HostPixels::rec709(false);
    let full = buffer(width, height);
    // Full size is the composition's own render, pixel for pixel.
    source
        .render(at(12), [width, height], &full, &ours)
        .unwrap();
    let first = read(&full, width, height);
    let plan = Plan::compile(&composition).unwrap();
    let own = renderer
        .render_frame(&plan, RenderRequest::full(at(12)), &Default::default())
        .unwrap()
        .pixels;
    assert!(
        first == own,
        "full size differs from the composition's own render"
    );
    assert!(
        first.iter().any(|p| p[3] > 0.0),
        "the test picture is empty"
    );
    // Other work in between, at another time and another size, changes nothing.
    let (half_w, half_h) = (width / 2, height / 2);
    let half = buffer(half_w, half_h);
    source
        .render(at(40), [half_w, half_h], &half, &ours)
        .unwrap();
    source
        .render(at(12), [half_w, half_h], &half, &ours)
        .unwrap();
    let small = read(&half, half_w, half_h);
    source
        .render(at(12), [width, height], &full, &ours)
        .unwrap();
    assert!(
        read(&full, width, height) == first,
        "the same frame came out differently the second time"
    );
    // The half-size picture is the same picture: as bright on average.
    let mean = |pixels: &[[f32; 4]]| {
        pixels
            .iter()
            .map(|p| f64::from(p[0] + p[1] + p[2] + p[3]))
            .sum::<f64>()
            / pixels.len() as f64
    };
    assert!(
        (mean(&small) - mean(&first)).abs() < 0.02 * mean(&first).max(0.01),
        "{} against {}",
        mean(&small),
        mean(&first)
    );
    // Two threads asking at once each get their own frame, the same as when asked alone.
    let (one, two) = (buffer(width, height), buffer(width, height));
    std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            source
                .render(at(12), [width, height], &one, &ours)
                .map_err(|e| e.to_string())
        });
        let b = scope.spawn(|| {
            source
                .render(at(40), [width, height], &two, &ours)
                .map_err(|e| e.to_string())
        });
        a.join().unwrap().unwrap();
        b.join().unwrap().unwrap();
    });
    assert!(read(&one, width, height) == first);
    source
        .render(at(40), [width, height], &full, &ours)
        .unwrap();
    assert!(read(&two, width, height) == read(&full, width, height));
    // A changed composition gives the changed picture from then on.
    let mut changed = composition.clone();
    changed.width = width / 2;
    changed.height = height / 2;
    source.set_composition(changed).unwrap();
    assert_eq!(source.format().unwrap().0, [width / 2, height / 2]);
}

#[test]
#[ignore = "requires Vulkan"]
fn a_clip_source_brings_its_own_3d_renderer_and_compiles_what_it_uses() {
    use veylvik_document::{Node, Operation, registry};
    let of = |kind: &str| -> Operation {
        serde_json::from_value(
            registry::TYPES
                .iter()
                .find(|t| t.type_id == kind)
                .unwrap()
                .default_operation(),
        )
        .unwrap()
    };
    // A renderer as a host makes it: no editor, no 3D renderer registered.
    let renderer = Arc::new(Renderer::new().unwrap());
    let mut composition = veylvik_document::blank(64, 36, Time::new(24, 1).unwrap()).unwrap();
    let shape = Node::new("cube", of("shape_3d"), &[]);
    let film = Node::new("film", of("render_scene"), &[("scene", shape.id)]);
    // And an ordinary 2D effect over the film (a blur: an untouched exposure is no pass).
    let graded = Node::new("graded", of("blur"), &[("image", film.id)]);
    composition.output = graded.id;
    composition.nodes.insert(shape.id, shape);
    composition.nodes.insert(film.id, film);
    composition.nodes.insert(graded.id, graded);
    let source =
        ClipSource::new(renderer.clone(), composition, std::env::temp_dir(), None).unwrap();
    assert!(source.compile().unwrap() > 0);
    let target = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("the host's frame"),
        size: 64 * 36 * 16,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    source
        .render(
            Time::new(0, 1).unwrap(),
            [64, 36],
            &target,
            &HostPixels::rec709(false),
        )
        .expect("a Render scene renders through a clip source with no editor");
    let pixels = renderer
        .read_frame(
            &renderer
                .frame_from_host(&target, 64, 36, &HostPixels::rec709(false))
                .unwrap(),
        )
        .unwrap()
        .pixels;
    assert!(
        pixels.iter().any(|p| p[3] > 0.0),
        "the cube is not in the picture"
    );
}
