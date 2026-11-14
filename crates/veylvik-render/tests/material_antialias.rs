use std::sync::Arc;
use veylvik_document::{nodes, *};
use veylvik_render::*;
#[test]
#[ignore = "requires Vulkan"]
fn smooth_bevel_is_continuous_and_preserves_coverage() {
    let renderer = Renderer::new().unwrap();
    let mut doc = blank(48, 48, Time::new(24, 1).unwrap()).unwrap();
    let input = doc.output;
    doc.nodes.get_mut(&input).unwrap().operation = Operation::Image(nodes::Image {
        asset: "circle".into(),
    });
    let pixels: Vec<_> = (0..48 * 48)
        .map(|i| {
            let x = (i % 48) as f32 - 23.5;
            let y = (i / 48) as f32 - 23.5;
            let a = (17.5 - (x * x + y * y).sqrt()).clamp(0., 1.);
            [a; 4]
        })
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "circle",
        Arc::new(Image::new(48, 48, pixels.clone()).unwrap()),
    );
    let op: Operation = serde_json::from_value(nodes::MATERIAL.default_operation()).unwrap();
    let Operation::Material(m) = &op else {
        panic!()
    };
    assert!(m.smooth_edges);
    let mut old = serde_json::to_value(&op).unwrap();
    old.as_object_mut().unwrap().remove("smooth_edges");
    let Operation::Material(m) = serde_json::from_value::<Operation>(old).unwrap() else {
        panic!()
    };
    assert!(m.smooth_edges, "older projects get smoothing by default");
    let node = Node::new("Gold", op, &[("image", input)]);
    doc.output = node.id;
    doc.nodes.insert(node.id, node);
    let mut render = |smooth, radius: f32| {
        let Operation::Material(m) = &mut doc.nodes.get_mut(&doc.output).unwrap().operation else {
            panic!()
        };
        m.smooth_edges = smooth;
        m.bevel = radius.into();
        m.grain = 0.0.into();
        renderer
            .render_frame(
                &Plan::compile(&doc).unwrap(),
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap()
            .pixels
    };
    assert_eq!(
        render(false, 3.1),
        render(false, 3.2),
        "legacy mode retains integer bevel behaviour"
    );
    let a = render(true, 3.1);
    let b = render(true, 3.2);
    assert!(
        a.iter().zip(&b).any(|(a, b)| (a[0] - b[0]).abs() > 0.00001),
        "fractional bevel changes must affect shading"
    );
    for (p, mask) in a.iter().zip(&pixels) {
        assert_eq!(p[3], mask[3]);
        assert!(p.iter().all(|v| v.is_finite()));
        if mask[3] == 0. {
            assert_eq!(*p, [0.; 4]);
        }
    }
}
