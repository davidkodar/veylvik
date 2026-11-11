use veylvik_document::*;
use veylvik_render::*;
fn composition(nodes: Vec<Node>, output: NodeId) -> Composition {
    Composition {
        schema: 1,
        width: 13,
        height: 7,
        frame_rate: Time::new(24, 1).unwrap(),
        nodes: nodes.into_iter().map(|n| (n.id, n)).collect(),
        output,
    }
}
#[test]
fn compile_prunes_unused_nodes() {
    let a = Node::new("a", Operation::Solid { color: [1.0; 4] }, &[]);
    let b = Node::new("b", Operation::Solid { color: [0.0; 4] }, &[]);
    let id = a.id;
    let plan = Plan::compile(&composition(vec![a, b], id)).unwrap();
    assert_eq!(plan.node_count(), 1);
}
#[test]
fn display_boundary_unpremultiplies_and_preserves_alpha() {
    assert_eq!(
        to_srgba8(&[[0.5, 0.0, 0.0, 0.5], [8.0, 0.0, 0.0, 0.0]]),
        vec![255, 0, 0, 128, 0, 0, 0, 0]
    );
}
/// Explicitly run with --ignored. Absence of Vulkan is a failure, never a skipped pass.
#[test]
#[ignore = "requires Vulkan; run cargo test -p veylvik-render -- --ignored"]
fn gpu_matches_analytic_references_and_enforces_budget() {
    let mut renderer = Renderer::new().expect("Vulkan adapter required");
    eprintln!(
        "GPU test adapter: {} {:?}",
        renderer.adapter.name, renderer.adapter.backend
    );
    let bg = Node::new(
        "bg",
        Operation::Solid {
            color: [0.2, 0.4, 0.8, 1.0],
        },
        &[],
    );
    let fg = Node::new(
        "fg",
        Operation::Solid {
            color: [4.0, 0.0, 0.0, 0.5],
        },
        &[],
    );
    let merge = Node::new(
        "merge",
        Operation::Merge,
        &[("background", bg.id), ("foreground", fg.id)],
    );
    let exposure = Node::new(
        "exposure",
        Operation::Exposure {
            stops: Scalar::Linear {
                keys: vec![
                    Key {
                        time: Time::new(0, 1).unwrap(),
                        value: 0.0,
                    },
                    Key {
                        time: Time::new(1, 1).unwrap(),
                        value: 1.0,
                    },
                ],
            },
        },
        &[("image", merge.id)],
    );
    let id = exposure.id;
    let mut doc = composition(vec![bg, fg, merge, exposure], id);
    let plan = Plan::compile(&doc).unwrap();
    for (time, gain) in [(0, 1.0), (1, 2.0)] {
        let pixels = renderer.render(&plan, Time::new(time, 1).unwrap()).unwrap();
        assert_eq!(pixels.len(), 91);
        for pixel in pixels {
            for (actual, expected) in
                pixel
                    .into_iter()
                    .zip([2.1 * gain, 0.2 * gain, 0.4 * gain, 1.0])
            {
                assert!(
                    (actual - expected).abs() < 0.00001,
                    "{actual} != {expected}"
                );
            }
        }
    }
    let transform = Node::new(
        "move",
        Operation::Transform {
            x: 0.5.into(),
            y: 0.0.into(),
        },
        &[("image", id)],
    );
    doc.output = transform.id;
    doc.nodes.insert(transform.id, transform);
    let pixels = renderer
        .render(&Plan::compile(&doc).unwrap(), Time::new(0, 1).unwrap())
        .unwrap();
    assert!((pixels[0][0] - 1.05).abs() < 0.00001);
    assert_eq!(pixels[0][3], 0.5);
    assert_eq!(pixels[1][3], 1.0);
    let blur = Node::new(
        "blur",
        Operation::Blur { radius: 1.0.into() },
        &[("image", id)],
    );
    doc.output = blur.id;
    doc.nodes.insert(blur.id, blur);
    let pixels = renderer
        .render(&Plan::compile(&doc).unwrap(), Time::new(0, 1).unwrap())
        .unwrap();
    assert!((pixels[0][3] - 4.0 / 9.0).abs() < 0.00001);
    assert_eq!(pixels[14][3], 1.0);
    let source = Node::new(
        "noise",
        Operation::Noise {
            seed: 42,
            scale: 2.0.into(),
        },
        &[],
    );
    let id = source.id;
    let doc = composition(vec![source], id);
    let plan = Plan::compile(&doc).unwrap();
    let pixels = renderer.render(&plan, Time::new(0, 1).unwrap()).unwrap();
    for y in 0..7u32 {
        for x in 0..13u32 {
            let mut h = x / 2 + (y / 2) * 65537 + 42;
            h = (h ^ (h >> 16)).wrapping_mul(0x7feb352d);
            h = (h ^ (h >> 15)).wrapping_mul(0x846ca68b);
            h ^= h >> 16;
            let value = (h & 65535) as f32 / 65535.0;
            for (actual, expected) in pixels[(y * 13 + x) as usize].into_iter().zip([
                0.1 * value,
                0.5 * value,
                value,
                value,
            ]) {
                assert!((actual - expected).abs() < 0.00001);
            }
        }
    }
    assert_eq!(
        pixels,
        renderer.render(&plan, Time::new(0, 1).unwrap()).unwrap()
    );
    renderer.budget_bytes = 1;
    assert!(
        renderer
            .render(&plan, Time::new(0, 1).unwrap())
            .unwrap_err()
            .to_string()
            .contains("budget")
    );
}

#[test]
fn long_chains_need_only_two_image_slots() {
    let first = Node::new("source", Operation::Solid { color: [1.0; 4] }, &[]);
    let mut id = first.id;
    let mut nodes = vec![first];
    for _ in 0..100 {
        let node = Node::new(
            "gain",
            Operation::Exposure { stops: 0.0.into() },
            &[("image", id)],
        );
        id = node.id;
        nodes.push(node);
    }
    let plan = Plan::compile(&composition(nodes, id)).unwrap();
    assert_eq!(plan.node_count(), 101);
    assert_eq!(plan.image_slots(), 2);
}
