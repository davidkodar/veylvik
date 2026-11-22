use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;
fn branches() -> (Composition, NodeId, NodeId) {
    let a = Node::new(
        "A",
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [0.4, 0.2, 0.8, 0.5],
        }),
        &[],
    );
    let b = Node::new("B", a.operation.clone(), &[]);
    let blur_a = Node::new(
        "soft A",
        Operation::Blur(veylvik_document::nodes::Blur {
            radius: 2.0.into(),
            filter: Default::default(),
        }),
        &[("image", a.id)],
    );
    let blur_b = Node::new("soft B", blur_a.operation.clone(), &[("image", b.id)]);
    let output = Node::new(
        "merge",
        Operation::Merge(veylvik_document::nodes::Merge {}),
        &[("background", blur_a.id), ("foreground", blur_b.id)],
    );
    let ids = (blur_a.id, blur_b.id);
    (
        Composition {
            published: Default::default(),
            markers: Default::default(),
            backdrops: Default::default(),
            ocio: Default::default(),
            working_space: Default::default(),
            display: Default::default(),
            groups: Default::default(),
            schema: 1,
            frame_count: 240,
            width: 13,
            height: 7,
            frame_rate: Time::new(24, 1).unwrap(),
            output: output.id,
            nodes: [a, b, blur_a, blur_b, output]
                .into_iter()
                .map(|n| (n.id, n))
                .collect(),
        },
        ids.0,
        ids.1,
    )
}
#[test]
fn equivalent_branches_share_passes_without_merging_distinct_inputs_or_curves() {
    let (mut doc, a, b) = branches();
    let plan = Plan::compile(&doc).unwrap();
    assert_eq!(plan.node_count(), 5);
    assert_eq!(plan.pass_count(), 4);
    let passes = plan.passes();
    assert_eq!(passes[3].input_slots[0], passes[3].input_slots[1]);
    for pass in passes {
        assert!(!pass.input_slots.contains(&pass.output_slot));
    }
    doc.nodes.get_mut(&b).unwrap().label = "different label".into();
    doc.nodes.get_mut(&b).unwrap().position = Some(NodePosition { x: 800.0, y: 100.0 });
    assert_eq!(Plan::compile(&doc).unwrap().pass_count(), 4);
    let source = doc.nodes[&b].inputs["image"];
    doc.nodes.get_mut(&source).unwrap().operation =
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [0.5, 0.2, 0.8, 0.5],
        });
    assert_eq!(Plan::compile(&doc).unwrap().pass_count(), 7);
    doc.nodes.get_mut(&source).unwrap().operation =
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [0.4, 0.2, 0.8, 0.5],
        });
    doc.nodes.get_mut(&b).unwrap().operation = Operation::Blur(veylvik_document::nodes::Blur {
        radius: Scalar::Linear {
            keys: vec![
                Key {
                    segment: None,
                    time: Time::new(0, 1).unwrap(),
                    value: 2.0,
                },
                Key {
                    segment: None,
                    time: Time::new(1, 1).unwrap(),
                    value: 3.0,
                },
            ],
        },
        filter: Default::default(),
    });
    assert_eq!(Plan::compile(&doc).unwrap().pass_count(), 6);
    doc.nodes.get_mut(&a).unwrap().operation = doc.nodes[&b].operation.clone();
    assert_eq!(Plan::compile(&doc).unwrap().pass_count(), 4);
}
#[test]
#[ignore = "requires Vulkan"]
fn duplicated_branches_match_explicit_sharing_and_keep_output_immutable() {
    let renderer = Renderer::new().unwrap();
    let (mut doc, a, b) = branches();
    let request = RenderRequest::full(Time::new(0, 1).unwrap());
    let inputs = ImageInputs::default();
    let original = renderer
        .render_gpu_cached(&Plan::compile(&doc).unwrap(), request, &inputs)
        .unwrap();
    let pixels = renderer.read_frame(&original).unwrap().pixels;
    assert!(pixels.iter().any(|p| p[3] > 0.0));
    doc.nodes
        .get_mut(&doc.output)
        .unwrap()
        .inputs
        .insert("foreground".into(), a);
    let explicit = renderer
        .render_gpu_cached(&Plan::compile(&doc).unwrap(), request, &inputs)
        .unwrap();
    assert!(Arc::ptr_eq(&original, &explicit));
    doc.nodes
        .get_mut(&doc.output)
        .unwrap()
        .inputs
        .insert("foreground".into(), b);
    doc.nodes.get_mut(&b).unwrap().operation = Operation::Blur(veylvik_document::nodes::Blur {
        radius: 3.0.into(),
        filter: Default::default(),
    });
    let changed = renderer
        .render_gpu_cached(&Plan::compile(&doc).unwrap(), request, &inputs)
        .unwrap();
    assert!(!Arc::ptr_eq(&original, &changed));
    assert_ne!(renderer.read_frame(&changed).unwrap().pixels, pixels);
    assert_eq!(renderer.read_frame(&original).unwrap().pixels, pixels);
}
