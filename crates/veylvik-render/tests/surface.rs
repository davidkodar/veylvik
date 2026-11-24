use std::sync::Arc;
use veylvik_document::{Node, Operation, Time, surface::Look};
use veylvik_render::{Image, ImageInputs, Plan, RenderRequest, Renderer};
fn scene() -> (veylvik_document::Composition, ImageInputs) {
    let mut doc = veylvik_document::blank(64, 48, Time::new(24, 1).unwrap()).unwrap();
    let bg = doc.output;
    doc.nodes.get_mut(&bg).unwrap().operation = Operation::Image(veylvik_document::nodes::Image {
        asset: "plate".into(),
    });
    let fg = Node::new(
        "Layer",
        Operation::Image(veylvik_document::nodes::Image {
            asset: "layer".into(),
        }),
        &[],
    );
    let blend = Node::new(
        "Surface blend",
        Look::Painted.operation(),
        &[("background", bg), ("foreground", fg.id)],
    );
    doc.output = blend.id;
    doc.nodes.insert(fg.id, fg);
    doc.nodes.insert(blend.id, blend);
    let mut plate = vec![[0.5, 0.5, 0.5, 1.]; 64 * 48];
    let mut layer = vec![[0.; 4]; 64 * 48];
    for y in 0..48 {
        for x in 0..64 {
            if x < 20 {
                plate[y * 64 + x] = [0.25, 0.25, 0.25, 1.];
            }
            if x == 32 {
                plate[y * 64 + x] = [0.01, 0.01, 0.01, 1.];
            }
            if (8..56).contains(&x) && (8..40).contains(&y) {
                layer[y * 64 + x] = [0.8, 0.4, 0.1, 1.];
            }
        }
    }
    let mut inputs = ImageInputs::default();
    inputs.insert("plate", Arc::new(Image::new(64, 48, plate).unwrap()));
    inputs.insert("layer", Arc::new(Image::new(64, 48, layer).unwrap()));
    (doc, inputs)
}
fn set(doc: &mut veylvik_document::Composition, name: &str, value: f32) {
    *doc.nodes
        .get_mut(&doc.output)
        .unwrap()
        .operation
        .scalar_mut(name)
        .unwrap() = value.into();
}
fn neutral(doc: &mut veylvik_document::Composition) {
    for name in [
        "lighting",
        "detail",
        "wear",
        "distortion",
        "softness",
        "grain",
    ] {
        set(doc, name, 0.);
    }
}
#[test]
#[ignore = "requires Vulkan"]
fn surface_blending_preserves_clean_plate_and_transfers_light_detail_and_cracks() {
    let renderer = Renderer::new().unwrap();
    let (mut doc, inputs) = scene();
    let render = |doc: &veylvik_document::Composition| {
        renderer
            .render_frame(
                &Plan::compile(doc).unwrap(),
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap()
            .pixels
    };
    neutral(&mut doc);
    let clean = render(&doc);
    let mut merge = doc.clone();
    merge.nodes.get_mut(&merge.output).unwrap().operation =
        Operation::Merge(veylvik_document::nodes::Merge {});
    assert_eq!(
        clean,
        render(&merge),
        "neutral settings must exactly match source-over"
    );
    set(&mut doc, "lighting", 1.);
    let lit = render(&doc);
    assert!((lit[24 * 64 + 12][0] - 0.4).abs() < 1e-5);
    assert!((lit[24 * 64 + 44][0] - 0.8).abs() < 1e-5);
    neutral(&mut doc);
    set(&mut doc, "detail", 1.);
    let detailed = render(&doc);
    assert!(detailed[24 * 64 + 32][0] < clean[24 * 64 + 32][0] * 0.1);
    neutral(&mut doc);
    set(&mut doc, "wear", 1.);
    set(&mut doc, "threshold", 0.3);
    let worn = render(&doc);
    assert!(
        (worn[24 * 64 + 32][0] - 0.01).abs() < 1e-5,
        "dark crack must show the plate"
    );
    assert_eq!(worn[24 * 64 + 44], clean[24 * 64 + 44]);
    for frame in [&lit, &detailed, &worn] {
        assert_eq!(
            frame[2 * 64 + 32],
            clean[2 * 64 + 32],
            "outside layer must be unchanged"
        );
    }
    set(&mut doc, "opacity", 0.);
    let hidden = render(&doc);
    let plate = renderer
        .render_frame(
            &Plan::compile_output(&doc, doc.nodes[&doc.output].inputs["background"]).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert_eq!(hidden, plate.pixels);
}
#[test]
#[ignore = "requires Vulkan"]
fn surface_grain_is_deterministic_animated_and_alpha_safe() {
    let renderer = Renderer::new().unwrap();
    let (mut doc, mut inputs) = scene();
    neutral(&mut doc);
    inputs.insert(
        "plate",
        Arc::new(Image::new(64, 48, vec![[0.; 4]; 64 * 48]).unwrap()),
    );
    inputs.insert(
        "layer",
        Arc::new(Image::new(64, 48, vec![[1.5, 0.2, 0.1, 0.5]; 64 * 48]).unwrap()),
    );
    set(&mut doc, "lighting", 1.);
    set(&mut doc, "detail", 1.);
    set(&mut doc, "wear", 1.);
    let plan = Plan::compile(&doc).unwrap();
    let base = renderer
        .render_frame(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert_eq!(
        base.pixels[24 * 64 + 24],
        [1.5, 0.2, 0.1, 0.5],
        "transparent plate must not shade HDR foreground"
    );
    set(&mut doc, "grain", 0.1);
    let plan = Plan::compile(&doc).unwrap();
    let render = |time| {
        renderer
            .render_frame(&plan, RenderRequest::full(time), &inputs)
            .unwrap()
            .pixels
    };
    let a = render(Time::new(0, 1).unwrap());
    assert_eq!(a, render(Time::new(0, 24).unwrap()));
    let b = render(Time::new(1, 24).unwrap());
    assert_ne!(a, b);
    assert!(
        a.iter()
            .chain(&b)
            .all(|p| p[3] == 0.5 && p.iter().all(|v| v.is_finite()))
    );
    let a_gpu = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    let b_gpu = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(1, 24).unwrap()),
            &inputs,
        )
        .unwrap();
    assert!(!Arc::ptr_eq(&a_gpu, &b_gpu));
}
#[test]
#[ignore = "requires Vulkan"]
fn surface_softness_and_distortion_use_transparent_borders() {
    let renderer = Renderer::new().unwrap();
    let (mut doc, inputs) = scene();
    neutral(&mut doc);
    set(&mut doc, "softness", 2.);
    set(&mut doc, "distortion", 3.);
    let frame = renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert!(
        frame
            .pixels
            .iter()
            .all(|p| p.iter().all(|v| v.is_finite()) && (0. ..=1.).contains(&p[3]))
    );
    assert_eq!(frame.pixels[0], [0.25, 0.25, 0.25, 1.]);
    assert!(
        frame.pixels[24 * 64 + 7][0] > 0.25,
        "softness should extend a premultiplied edge"
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn mural_example_has_visible_integration_and_shared_camera_motion() {
    let renderer = Renderer::new().unwrap();
    let scene = veylvik_presets::showcase::build("surface_paint").unwrap();
    let after = Plan::compile(&scene.composition).unwrap();
    let before = Plan::compile_output(&scene.composition, scene.before).unwrap();
    let start = renderer.render(&after, Time::new(0, 1).unwrap()).unwrap();
    let middle = renderer.render(&after, Time::new(2, 1).unwrap()).unwrap();
    let end = renderer.render(&after, Time::new(4, 1).unwrap()).unwrap();
    let untreated = renderer.render(&before, Time::new(2, 1).unwrap()).unwrap();
    assert_eq!(start, end);
    assert_ne!(start, middle);
    assert!(
        middle
            .iter()
            .zip(&untreated)
            .filter(|(a, b)| (0..3).any(|i| (a[i] - b[i]).abs() > 0.01))
            .count()
            > 2000
    );
    assert!(
        middle
            .iter()
            .all(|p| p.iter().all(|v| v.is_finite()) && (0. ..=1.).contains(&p[3]))
    );
}
