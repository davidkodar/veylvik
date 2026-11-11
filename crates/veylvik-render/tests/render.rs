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

#[test]
fn image_contract_and_regions_reject_invalid_data() {
    assert!(Image::new(2, 1, vec![[0.0; 4]]).is_err());
    assert!(Image::new(1, 1, vec![[f32::NAN, 0.0, 0.0, 1.0]]).is_err());
    assert!(Image::new(1, 1, vec![[1.0, 0.0, 0.0, 0.0]]).is_err());
    assert!(Image::new(1, 1, vec![[1.0, 0.0, 0.0, 1.1]]).is_err());
    assert!(Image::new(1, 1, vec![[-0.2, 4.0, 0.0, 0.5]]).is_ok());
    for region in [
        Region {
            x: u32::MAX,
            y: 0,
            width: 2,
            height: 1,
        },
        Region {
            x: 0,
            y: 0,
            width: 0,
            height: 1,
        },
        Region {
            x: 0,
            y: 0,
            width: 14,
            height: 1,
        },
    ] {
        assert!(region.validate(13, 7).is_err());
    }
}
#[test]
fn alternate_root_only_requires_its_own_sources() {
    let first = Node::new(
        "first",
        Operation::Image {
            asset: "first.png".into(),
        },
        &[],
    );
    let second = Node::new(
        "second",
        Operation::Image {
            asset: "second.png".into(),
        },
        &[],
    );
    let preview = first.id;
    let output = second.id;
    let doc = composition(vec![first, second], output);
    let plan = Plan::compile_output(&doc, preview).unwrap();
    assert_eq!(
        plan.required_assets().into_iter().collect::<Vec<_>>(),
        ["first.png"]
    );
    assert_eq!(doc.output, output);
}
#[test]
#[ignore = "requires Vulkan; run cargo test -p veylvik-render -- --ignored"]
fn gpu_image_injection_preview_and_crop_match_full_frame() {
    use std::sync::Arc;
    let mut renderer = Renderer::new().expect("Vulkan required");
    let first = Node::new(
        "first",
        Operation::Image {
            asset: "plate".into(),
        },
        &[],
    );
    let second = Node::new(
        "same asset",
        Operation::Image {
            asset: "plate".into(),
        },
        &[],
    );
    let merge = Node::new(
        "merge",
        Operation::Merge,
        &[("background", first.id), ("foreground", second.id)],
    );
    let blur = Node::new(
        "blur",
        Operation::Blur { radius: 1.0.into() },
        &[("image", merge.id)],
    );
    let preview = first.id;
    let merged = merge.id;
    let output = blur.id;
    let doc = composition(vec![first, second, merge, blur], output);
    let plan = Plan::compile(&doc).unwrap();
    let time = Time::new(0, 1).unwrap();
    let source = Arc::new(
        Image::new(
            2,
            2,
            vec![
                [2.0, 0.0, 0.0, 0.5],
                [0.0, 0.5, 0.0, 0.5],
                [0.0, 0.0, 0.5, 0.5],
                [0.0; 4],
            ],
        )
        .unwrap(),
    );
    let mut inputs = ImageInputs::default();
    inputs.insert("plate", source);
    let frame = renderer
        .render_frame(
            &Plan::compile_output(&doc, preview).unwrap(),
            RenderRequest::full(time),
            &inputs,
        )
        .unwrap();
    assert_eq!(frame.pixels[0], [2.0, 0.0, 0.0, 0.5]);
    assert_eq!(frame.pixels[1], [0.0, 0.5, 0.0, 0.5]);
    assert_eq!(frame.pixels[13], [0.0, 0.0, 0.5, 0.5]);
    assert_eq!(frame.pixels[2], [0.0; 4]);
    let merged_frame = renderer
        .render_frame(
            &Plan::compile_output(&doc, merged).unwrap(),
            RenderRequest::full(time),
            &inputs,
        )
        .unwrap();
    assert_eq!(merged_frame.pixels[0], [3.0, 0.0, 0.0, 0.75]);
    let full = renderer
        .render_frame(&plan, RenderRequest::full(time), &inputs)
        .unwrap();
    let region = Region {
        x: 1,
        y: 0,
        width: 3,
        height: 2,
    };
    let cropped = renderer
        .render_frame(
            &plan,
            RenderRequest {
                time,
                region: Some(region),
            },
            &inputs,
        )
        .unwrap();
    assert_eq!(cropped.region, region);
    assert_eq!(
        cropped.pixels,
        [&full.pixels[1..4], &full.pixels[14..17]].concat()
    );
    assert!((full.pixels[0][0] - 3.0 / 9.0).abs() < 1e-6);
    assert!(
        renderer
            .render(&plan, time)
            .unwrap_err()
            .to_string()
            .contains("missing image input")
    );
    inputs.insert(
        "plate",
        Arc::new(Image::new(1, 1, vec![[0.0, 0.0, 1.0, 1.0]]).unwrap()),
    );
    let changed = renderer
        .render_frame(
            &Plan::compile_output(&doc, preview).unwrap(),
            RenderRequest::full(time),
            &inputs,
        )
        .unwrap();
    assert_eq!(changed.pixels[0], [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(changed.pixels[1], [0.0; 4]);
    renderer.budget_bytes = plan.working_bytes();
    assert!(
        renderer
            .render_frame(&plan, RenderRequest::full(time), &inputs)
            .err()
            .unwrap()
            .to_string()
            .contains("budget")
    );
}
