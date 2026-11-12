use veylvik_document::*;
use veylvik_render::*;
#[test]
#[ignore = "requires Vulkan"]
fn text_renders_premultiplied_and_cache_tracks_content_font_and_animation() {
    let renderer = Renderer::new().unwrap();
    let mut doc = demo();
    doc.width = 160;
    doc.height = 100;
    let mut op: Operation = serde_json::from_value(registry::TEXT.default_operation()).unwrap();
    if let Operation::Text {
        content,
        size,
        x,
        y,
        color,
        ..
    } = &mut op
    {
        *content = "Åffi".into();
        *size = 48.0.into();
        *x = 80.0.into();
        *y = 65.0.into();
        *color = [2.0, 0.5, 0.25, 0.5];
    }
    let node = Node::new("Editable title", op, &[]);
    doc.output = node.id;
    doc.nodes = [(node.id, node)].into();
    doc.groups.clear();
    let request = RenderRequest::full(Time::new(0, 1).unwrap());
    let inputs = ImageInputs::default();
    let plan = Plan::compile(&doc).unwrap();
    let frame = renderer.render_gpu_cached(&plan, request, &inputs).unwrap();
    let pixels = renderer.read_frame(&frame).unwrap().pixels;
    assert!(pixels.iter().any(|p| p[3] == 0.5));
    assert!(pixels.iter().any(|p| p[3] > 0.0 && p[3] < 0.5));
    for p in &pixels {
        assert_eq!(p[0], 2.0 * p[3]);
        assert_eq!(p[1], 0.5 * p[3]);
    }
    assert!(std::sync::Arc::ptr_eq(
        &frame,
        &renderer.render_gpu_cached(&plan, request, &inputs).unwrap()
    ));
    if let Operation::Text { content, .. } = &mut doc.nodes.get_mut(&doc.output).unwrap().operation
    {
        *content = "New".into();
    }
    let changed = renderer
        .render_gpu_cached(&Plan::compile(&doc).unwrap(), request, &inputs)
        .unwrap();
    assert_ne!(pixels, renderer.read_frame(&changed).unwrap().pixels);
    if let Operation::Text { font, x, .. } = &mut doc.nodes.get_mut(&doc.output).unwrap().operation
    {
        **font = TextFont::Italic {};
        *x = Scalar::Linear {
            keys: vec![
                Key {
                    time: Time::new(0, 1).unwrap(),
                    value: 60.0,
                    segment: None,
                },
                Key {
                    time: Time::new(1, 1).unwrap(),
                    value: 100.0,
                    segment: None,
                },
            ],
        };
    }
    let animated = Plan::compile(&doc).unwrap();
    let first = renderer
        .render_gpu_cached(&animated, request, &inputs)
        .unwrap();
    let second = renderer
        .render_gpu_cached(
            &animated,
            RenderRequest::full(Time::new(1, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert_ne!(
        renderer.read_frame(&first).unwrap().pixels,
        renderer.read_frame(&second).unwrap().pixels
    );
    assert!(std::sync::Arc::ptr_eq(
        &first,
        &renderer
            .render_gpu_cached(&animated, request, &inputs)
            .unwrap()
    ));
    let mut large = doc.clone();
    large.width = 3840;
    large.height = 2160;
    let full = renderer
        .render_frame(&Plan::compile(&large).unwrap(), request, &inputs)
        .unwrap();
    assert_eq!(full.pixels.len(), 3840 * 2160);
    assert!(full.pixels.iter().any(|p| p[3] > 0.0));
    if let Operation::Text { content, .. } = &mut doc.nodes.get_mut(&doc.output).unwrap().operation
    {
        content.clear();
    }
    let blank = renderer
        .render_frame(&Plan::compile(&doc).unwrap(), request, &inputs)
        .unwrap();
    assert!(blank.pixels.iter().all(|p| *p == [0.0; 4]));
}

#[test]
#[ignore = "requires Vulkan"]
fn procedural_text_motion_and_compound_bypass_render_correctly() {
    let renderer = Renderer::new().unwrap();
    let mut doc = blank(160, 100, Time::new(24, 1).unwrap()).unwrap();
    let mut op: Operation = serde_json::from_value(registry::TEXT.default_operation()).unwrap();
    if let Operation::Text {
        content,
        size,
        x,
        y,
        typography,
        ..
    } = &mut op
    {
        *content = "Hi".into();
        *size = 40.0.into();
        *x = 80.0.into();
        *y = 65.0.into();
        typography.motion = text::TextMotion::Scale;
    }
    let title = Node::new("Title", op, &[]);
    let id = title.id;
    doc.nodes.insert(id, title);
    doc.output = id;
    let inputs = ImageInputs::default();
    let plan = Plan::compile(&doc).unwrap();
    let frame = |plan: &Plan, t| {
        renderer
            .read_frame(
                &renderer
                    .render_gpu_cached(plan, RenderRequest::full(Time::new(t, 1).unwrap()), &inputs)
                    .unwrap(),
            )
            .unwrap()
            .pixels
    };
    let start = frame(&plan, 0);
    let end = frame(&plan, 2);
    assert!(start.iter().all(|p| p[3] == 0.0));
    assert!(end.iter().any(|p| p[3] > 0.0));
    assert_eq!(frame(&plan, 0), start);
    let commands = presets::text_style::shadow(&doc, id).unwrap();
    let owner = Document::new(doc).unwrap();
    let mut shadow = owner.prepare(0, &commands).unwrap();
    assert_ne!(frame(&Plan::compile(&shadow).unwrap(), 2), end);
    shadow.nodes.get_mut(&shadow.output).unwrap().bypass = true;
    assert_eq!(frame(&Plan::compile(&shadow).unwrap(), 2), end);
}
