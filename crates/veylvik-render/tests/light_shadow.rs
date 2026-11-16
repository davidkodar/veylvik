use std::sync::Arc;
use veylvik_document::{nodes, *};
use veylvik_render::*;
#[test]
#[ignore = "requires Vulkan"]
fn light_shadow_moves_opposite_light_and_invalidates_cached_lighting() {
    let renderer = Renderer::new().unwrap();
    let mut doc = blank(96, 64, Time::new(24, 1).unwrap()).unwrap();
    let source = doc.output;
    doc.nodes.get_mut(&source).unwrap().operation = Operation::Image(nodes::Image {
        asset: "shape".into(),
    });
    let mut images = ImageInputs::default();
    let pixels = (0..96 * 64)
        .map(|i| {
            if (40..56).contains(&(i % 96)) && (24..40).contains(&(i / 96)) {
                [1.; 4]
            } else {
                [0.; 4]
            }
        })
        .collect();
    images.insert("shape", Arc::new(Image::new(96, 64, pixels).unwrap()));
    let mut op: Operation = serde_json::from_value(nodes::MATERIAL.default_operation()).unwrap();
    let Operation::Material(m) = &mut op else {
        panic!()
    };
    m.studio.lights = vec![optical::AreaLight {
        enabled: true,
        position: [2., 0., 2.].into(),
        size: [0.01, 0.01],
        rotation: [0.; 3].into(),
        ..Default::default()
    }];
    let material = Node::new("Material", op, &[("image", source)]);
    let mid = material.id;
    doc.nodes.insert(mid, material);
    let shadow = Node::new(
        "Shadow",
        Operation::Shadow(nodes::Shadow {
            follow_lighting: true,
            distance: 10.0.into(),
            softness: 0.0.into(),
            strength: 0.65.into(),
            x: 0.0.into(),
            y: 0.0.into(),
            light: 0.0.into(),
        }),
        &[("image", source), ("lighting", mid)],
    );
    doc.output = shadow.id;
    doc.nodes.insert(shadow.id, shadow);
    let render = |d: &Composition| {
        let f = renderer
            .render_gpu_cached(
                &Plan::compile(d).unwrap(),
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &images,
            )
            .unwrap();
        renderer.read_frame(&f).unwrap().pixels
    };
    let centroid = |pixels: &Vec<[f32; 4]>| {
        let total: f32 = pixels.iter().map(|p| p[3]).sum();
        pixels
            .iter()
            .enumerate()
            .map(|(i, p)| (i % 96) as f32 * p[3])
            .sum::<f32>()
            / total
    };
    let left = render(&doc);
    assert!((centroid(&left) - 37.5).abs() < 0.1);
    assert!(
        left.iter()
            .all(|p| p[0] == 0. && p[1] == 0. && p[2] == 0. && p[3] <= 0.65001)
    );
    let Operation::Material(m) = &mut doc.nodes.get_mut(&mid).unwrap().operation else {
        panic!()
    };
    m.studio.lights[0].position = [-2., 0., 2.].into();
    let right = render(&doc);
    assert!((centroid(&right) - 57.5).abs() < 0.1);
    let Operation::Material(m) = &mut doc.nodes.get_mut(&mid).unwrap().operation else {
        panic!()
    };
    m.studio.lights[0].enabled = false;
    assert!(render(&doc).iter().all(|p| p[3] == 0.));
    let Operation::Shadow(s) = &mut doc.nodes.get_mut(&doc.output).unwrap().operation else {
        panic!()
    };
    s.follow_lighting = false;
    s.x = 5.0.into();
    assert!((centroid(&render(&doc)) - 52.5).abs() < 0.1);
}

#[test]
#[ignore = "requires Vulkan"]
fn contact_shadows_are_dense_at_the_edge_fade_and_lean() {
    use veylvik_document::{nodes, *};
    let renderer = veylvik_render::Renderer::new().unwrap();
    // An opaque 20x20 square in a 96x96 frame.
    let mut doc = blank(96, 96, Time::new(24, 1).unwrap()).unwrap();
    let square: Vec<[f32; 4]> = (0..96 * 96)
        .map(|i| {
            if (38..58).contains(&(i % 96)) && (38..58).contains(&(i / 96)) {
                [1.0; 4]
            } else {
                [0.0; 4]
            }
        })
        .collect();
    let mut inputs = veylvik_render::ImageInputs::default();
    inputs.insert(
        "square",
        std::sync::Arc::new(veylvik_render::Image::new(96, 96, square).unwrap()),
    );
    let image = Node::new(
        "square",
        Operation::Image(nodes::Image {
            asset: "square".into(),
        }),
        &[],
    );
    let source = image.id;
    doc.nodes.insert(source, image);
    let render = |contact: nodes::ContactShadow| {
        let mut d = doc.clone();
        let node = Node::new(
            "contact",
            Operation::ContactShadow(contact),
            &[("image", source)],
        );
        d.output = node.id;
        d.nodes.insert(node.id, node);
        renderer
            .render_frame(
                &veylvik_render::Plan::compile(&d).unwrap(),
                veylvik_render::RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap()
            .pixels
    };
    let contact = |lean: f32, strength: f32| nodes::ContactShadow {
        reach: 16.0.into(),
        strength: strength.into(),
        lean: lean.into(),
        angle: 90.0.into(),
    };
    let alpha = |p: &[[f32; 4]], x: usize, y: usize| p[y * 96 + x][3];
    let even = render(contact(0.0, 1.0));
    assert!(
        even.iter()
            .all(|p| p[0] == 0.0 && p[1] == 0.0 && p[2] == 0.0)
    );
    // Dense at the edge, fading over the reach, gone beyond it.
    let (edge, mid, far) = (
        alpha(&even, 59, 48),
        alpha(&even, 66, 48),
        alpha(&even, 90, 48),
    );
    assert!(
        edge > 0.6 && mid > 0.05 && mid < edge && far < 0.01,
        "{edge} {mid} {far}"
    );
    // Lean 90 degrees (down) darkens below more than above.
    let leaning = render(contact(4.0, 1.0));
    assert!(alpha(&leaning, 48, 61) > alpha(&leaning, 48, 34) + 0.1);
    assert!(render(contact(0.0, 0.0)).iter().all(|p| p[3] == 0.0));
}
