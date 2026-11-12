use veylvik_document::*;
use veylvik_render::*;

fn layered(count: u128) -> Composition {
    let mut doc = blank(13, 7, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.clear();
    let mut output = NodeId::from_u128(1);
    for index in 1..=count {
        let mut source = Node::new(
            "Layer",
            Operation::Solid {
                color: [index as f32 / 64.0, 0.1, 0.2, 0.25],
            },
            &[],
        );
        source.id = NodeId::from_u128(index);
        if index > 1 {
            let mut merge = Node::new(
                "Over",
                Operation::Merge {},
                &[("background", output), ("foreground", source.id)],
            );
            merge.id = NodeId::from_u128(count + index);
            output = merge.id;
            doc.nodes.insert(merge.id, merge);
        }
        doc.nodes.insert(source.id, source);
    }
    doc.output = output;
    doc
}

#[test]
fn branch_local_schedule_bounds_layer_memory_and_handles_deep_documents() {
    // Every source sorts before every merge. A UUID-ready queue retains all 32
    // layers; branch-local evaluation needs just two inputs and one output.
    let doc = layered(32);
    let plan = Plan::compile(&doc).unwrap();
    assert_eq!(plan.node_count(), 63);
    assert_eq!(plan.pass_count(), 63);
    assert_eq!(plan.image_slots(), 3);
    for pass in plan.passes() {
        assert!(!pass.input_slots.contains(&pass.output_slot));
    }
    assert_eq!(Plan::compile(&layered(2048)).unwrap().image_slots(), 3);
    let mut energy = procedural_demo();
    energy.width = 1920;
    energy.height = 1080;
    assert!(Plan::compile(&energy).unwrap().gpu_working_bytes() < 384 * 1024 * 1024);
}

#[test]
#[ignore = "requires Vulkan"]
fn layered_schedule_matches_source_over_reference_and_ignores_uuid_order() {
    let renderer = Renderer::new().unwrap();
    let doc = layered(32);
    let request = RenderRequest::full(Time::new(0, 1).unwrap());
    let inputs = ImageInputs::default();
    let original = renderer
        .render_gpu_cached(&Plan::compile(&doc).unwrap(), request, &inputs)
        .unwrap();
    let pixels = renderer.read_frame(&original).unwrap().pixels;
    let mut expected = [0.0f32; 4];
    for index in 1..=32 {
        for (channel, value) in [index as f32 / 64.0, 0.1, 0.2].into_iter().enumerate() {
            expected[channel] = value * 0.25 + expected[channel] * 0.75;
        }
        expected[3] = 0.25 + expected[3] * 0.75;
    }
    for pixel in &pixels {
        for channel in 0..4 {
            assert!((pixel[channel] - expected[channel]).abs() < 1e-6);
        }
    }
    let remap: std::collections::BTreeMap<_, _> =
        doc.nodes.keys().map(|id| (*id, NodeId::new_v4())).collect();
    let mut reordered = doc.clone();
    reordered.output = remap[&doc.output];
    reordered.nodes = doc
        .nodes
        .values()
        .cloned()
        .map(|mut node| {
            node.id = remap[&node.id];
            for source in node.inputs.values_mut() {
                *source = remap[source];
            }
            (node.id, node)
        })
        .collect();
    let next = renderer
        .render_gpu_cached(&Plan::compile(&reordered).unwrap(), request, &inputs)
        .unwrap();
    assert!(std::sync::Arc::ptr_eq(&original, &next));
    assert_eq!(renderer.read_frame(&next).unwrap().pixels, pixels);
}
