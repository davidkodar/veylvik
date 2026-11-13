use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;
fn document() -> Composition {
    let source = Node::new(
        "source",
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [4.0, -0.2, 0.4, 0.5],
        }),
        &[],
    );
    let mut output = source.id;
    let mut nodes = vec![source];
    for operation in [
        Operation::Exposure(veylvik_document::nodes::Exposure { stops: 0.0.into() }),
        Operation::Transform(veylvik_document::nodes::Transform {
            x: 0.0.into(),
            y: 0.0.into(),
        }),
        Operation::Blur(veylvik_document::nodes::Blur { radius: 0.0.into() }),
    ] {
        let node = Node::new("identity", operation, &[("image", output)]);
        output = node.id;
        nodes.push(node);
    }
    Composition {
        groups: Default::default(),
        schema: 1,
        frame_count: 240,
        width: 13,
        height: 7,
        frame_rate: Time::new(24, 1).unwrap(),
        output,
        nodes: nodes.into_iter().map(|n| (n.id, n)).collect(),
    }
}
#[test]
fn constant_identity_effects_alias_sources_before_slot_assignment() {
    let mut doc = document();
    let plan = Plan::compile(&doc).unwrap();
    assert_eq!(plan.node_count(), 4);
    assert_eq!(plan.pass_count(), 1);
    assert_eq!(plan.image_slots(), 1);
    assert_eq!(plan.passes()[0].kernel, Kernel::Solid);
    let source = plan.passes()[0].node;
    assert_eq!(
        plan.working_bytes(),
        Plan::compile_output(&doc, source).unwrap().working_bytes()
    );
    let node = doc.nodes.get_mut(&doc.output).unwrap();
    node.operation = Operation::Blur(veylvik_document::nodes::Blur {
        radius: Scalar::Linear {
            keys: vec![
                Key {
                    segment: None,
                    time: Time::new(0, 1).unwrap(),
                    value: 0.0,
                },
                Key {
                    segment: None,
                    time: Time::new(1, 1).unwrap(),
                    value: 4.0,
                },
            ],
        },
    });
    assert_eq!(Plan::compile(&doc).unwrap().pass_count(), 3);
    // Aliases used by both ports must retain the source through the merge pass.
    let merge = Node::new(
        "merge",
        Operation::Merge(veylvik_document::nodes::Merge {}),
        &[("background", source), ("foreground", doc.output)],
    );
    doc.output = merge.id;
    doc.nodes.insert(merge.id, merge);
    for pass in Plan::compile(&doc).unwrap().passes() {
        assert!(!pass.input_slots.contains(&pass.output_slot));
    }
}
#[test]
#[ignore = "requires Vulkan"]
fn identity_chains_share_cached_pixels_and_preserve_preview_and_fanout() {
    let doc = document();
    let renderer = Renderer::new().unwrap();
    let plan = Plan::compile(&doc).unwrap();
    let source = plan.passes()[0].node;
    let request = RenderRequest::full(Time::new(0, 1).unwrap());
    let inputs = ImageInputs::default();
    let first = renderer.render_gpu_cached(&plan, request, &inputs).unwrap();
    let preview = renderer
        .render_gpu_cached(
            &Plan::compile_output(&doc, source).unwrap(),
            request,
            &inputs,
        )
        .unwrap();
    assert!(Arc::ptr_eq(&first, &preview));
    let pixels = renderer.read_frame(&first).unwrap().pixels;
    assert!(pixels.iter().all(|p| *p == [2.0, -0.1, 0.2, 0.5]));
    let mut doc = doc;
    let merge = Node::new(
        "merge",
        Operation::Merge(veylvik_document::nodes::Merge {}),
        &[("background", source), ("foreground", doc.output)],
    );
    doc.output = merge.id;
    doc.nodes.insert(merge.id, merge);
    let pixels = renderer
        .render_frame(&Plan::compile(&doc).unwrap(), request, &inputs)
        .unwrap()
        .pixels;
    assert!(pixels.iter().all(|p| *p == [3.0, -0.15, 0.3, 0.75]));
}
