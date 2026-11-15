use veylvik_document::*;
use veylvik_render::*;
fn composition(nodes: Vec<Node>, output: NodeId) -> Composition {
    Composition {
        ocio: Default::default(),
        working_space: Default::default(),
        display: Default::default(),
        groups: Default::default(),
        schema: 1,
        frame_count: 240,
        width: 13,
        height: 7,
        frame_rate: Time::new(24, 1).unwrap(),
        nodes: nodes.into_iter().map(|n| (n.id, n)).collect(),
        output,
    }
}
#[test]
fn compile_prunes_unused_nodes() {
    let a = Node::new(
        "a",
        Operation::Solid(veylvik_document::nodes::Solid { color: [1.0; 4] }),
        &[],
    );
    let b = Node::new(
        "b",
        Operation::Solid(veylvik_document::nodes::Solid { color: [0.0; 4] }),
        &[],
    );
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
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [0.2, 0.4, 0.8, 1.0],
        }),
        &[],
    );
    let fg = Node::new(
        "fg",
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [4.0, 0.0, 0.0, 0.5],
        }),
        &[],
    );
    let merge = Node::new(
        "merge",
        Operation::Merge(veylvik_document::nodes::Merge {}),
        &[("background", bg.id), ("foreground", fg.id)],
    );
    let exposure = Node::new(
        "exposure",
        Operation::Exposure(veylvik_document::nodes::Exposure {
            stops: Scalar::Linear {
                keys: vec![
                    Key {
                        segment: None,
                        time: Time::new(0, 1).unwrap(),
                        value: 0.0,
                    },
                    Key {
                        segment: None,
                        time: Time::new(1, 1).unwrap(),
                        value: 1.0,
                    },
                ],
            },
        }),
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
        Operation::Transform(veylvik_document::nodes::Transform {
            x: 0.5.into(),
            y: 0.0.into(),
        }),
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
        Operation::Blur(veylvik_document::nodes::Blur {
            radius: 1.0.into(),
            filter: Default::default(),
        }),
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
        Operation::Noise(veylvik_document::nodes::Noise {
            seed: 42,
            scale: 2.0.into(),
        }),
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
    let first = Node::new(
        "source",
        Operation::Solid(veylvik_document::nodes::Solid { color: [1.0; 4] }),
        &[],
    );
    let mut id = first.id;
    let mut nodes = vec![first];
    for _ in 0..100 {
        let node = Node::new(
            "gain",
            Operation::Exposure(veylvik_document::nodes::Exposure {
                stops: 0.125.into(),
            }),
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
        Operation::Image(veylvik_document::nodes::Image {
            asset: "first.png".into(),
        }),
        &[],
    );
    let second = Node::new(
        "second",
        Operation::Image(veylvik_document::nodes::Image {
            asset: "second.png".into(),
        }),
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
        Operation::Image(veylvik_document::nodes::Image {
            asset: "plate".into(),
        }),
        &[],
    );
    let second = Node::new(
        "same asset",
        Operation::Image(veylvik_document::nodes::Image {
            asset: "plate".into(),
        }),
        &[],
    );
    let merge = Node::new(
        "merge",
        Operation::Merge(veylvik_document::nodes::Merge {}),
        &[("background", first.id), ("foreground", second.id)],
    );
    let blur = Node::new(
        "blur",
        Operation::Blur(veylvik_document::nodes::Blur {
            radius: 1.0.into(),
            filter: Default::default(),
        }),
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

#[test]
fn lowered_blur_preserves_fanout_and_never_aliases_inputs() {
    let source = Node::new(
        "source",
        Operation::Solid(veylvik_document::nodes::Solid { color: [0.5; 4] }),
        &[],
    );
    let blur = Node::new(
        "blur",
        Operation::Blur(veylvik_document::nodes::Blur {
            radius: 3.0.into(),
            filter: Default::default(),
        }),
        &[("image", source.id)],
    );
    let merge = Node::new(
        "merge",
        Operation::Merge(veylvik_document::nodes::Merge {}),
        &[("background", source.id), ("foreground", blur.id)],
    );
    let id = merge.id;
    let plan = Plan::compile(&composition(vec![source, blur, merge], id)).unwrap();
    assert_eq!(plan.node_count(), 3);
    assert_eq!(plan.pass_count(), 4);
    let passes = plan.passes();
    assert_eq!(
        passes.iter().map(|p| p.kernel).collect::<Vec<_>>(),
        [
            Kernel::Solid,
            Kernel::BlurHorizontal,
            Kernel::BlurVertical,
            Kernel::Merge
        ]
    );
    for pass in &passes {
        assert!(!pass.input_slots.contains(&pass.output_slot));
    }
    assert_eq!(passes[3].input_slots[0], passes[0].output_slot);
    assert_eq!(passes[3].input_slots[1], passes[2].output_slot);
    assert_eq!(plan.image_slots(), 3);
}

#[test]
#[ignore = "requires Vulkan"]
fn separable_blur_matches_direct_reference_and_pool_is_bounded() {
    use std::sync::Arc;
    let mut renderer = Renderer::new().unwrap();
    let source = Node::new(
        "image",
        Operation::Image(veylvik_document::nodes::Image {
            asset: "plate".into(),
        }),
        &[],
    );
    let blur = Node::new(
        "blur",
        Operation::Blur(veylvik_document::nodes::Blur {
            radius: 0.0.into(),
            filter: Default::default(),
        }),
        &[("image", source.id)],
    );
    let id = blur.id;
    let mut doc = composition(vec![source, blur], id);
    let pixels: Vec<[f32; 4]> = (0..91)
        .map(|i| {
            let alpha = (i % 5) as f32 / 4.0;
            [i as f32 * 0.1 * alpha, -0.5 * alpha, 0.25 * alpha, alpha]
        })
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "plate",
        Arc::new(Image::new(13, 7, pixels.clone()).unwrap()),
    );
    let request = RenderRequest::full(Time::new(0, 1).unwrap());
    let mut created = None;
    for radius in [0.0f32, 0.5, 1.0, 3.0, 32.0] {
        doc.nodes.get_mut(&id).unwrap().operation =
            Operation::Blur(veylvik_document::nodes::Blur {
                radius: radius.into(),
                filter: Default::default(),
            });
        let plan = Plan::compile(&doc).unwrap();
        renderer.budget_bytes = plan.working_bytes() + 91 * 16;
        let actual = renderer.render_frame(&plan, request, &inputs).unwrap();
        let radius = radius.round_ties_even() as i32;
        for y in 0..7i32 {
            for x in 0..13i32 {
                let mut expected = [0.0f64; 4];
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        let (sx, sy) = (x + dx, y + dy);
                        if (0..13).contains(&sx) && (0..7).contains(&sy) {
                            for (out, value) in
                                expected.iter_mut().zip(pixels[(sy * 13 + sx) as usize])
                            {
                                *out += value as f64 / (2 * radius + 1).pow(2) as f64;
                            }
                        }
                    }
                }
                for (value, expected) in actual.pixels[(y * 13 + x) as usize].iter().zip(expected) {
                    assert!(
                        (*value as f64 - expected).abs() < 2e-6,
                        "radius={radius}: {value} != {expected}"
                    );
                }
            }
        }
        let stats = renderer.resource_stats().unwrap();
        assert_eq!(stats.resident_bytes, stats.idle_bytes);
        assert!(stats.resident_bytes <= renderer.budget_bytes);
        if let Some((passes, count)) = created
            && passes == plan.pass_count()
        {
            assert_eq!(stats.buffers_created, count);
        }
        // Identity blur has fewer passes; every warmed plan still reuses all buffers.
        renderer.render_frame(&plan, request, &inputs).unwrap();
        assert_eq!(
            renderer.resource_stats().unwrap().buffers_created,
            stats.buffers_created
        );
        created = Some((plan.pass_count(), stats.buffers_created));
    }
    assert!(renderer.resource_stats().unwrap().buffers_reused > 0);
    renderer.trim_resources().unwrap();
    assert_eq!(renderer.resource_stats().unwrap().resident_bytes, 0);
    renderer.pool_limit_bytes = 0;
    let plan = Plan::compile(&doc).unwrap();
    renderer.render_frame(&plan, request, &inputs).unwrap();
    assert_eq!(renderer.resource_stats().unwrap().resident_bytes, 0);
    renderer.pool_limit_bytes = 128 * 1024 * 1024;
    renderer.render_frame(&plan, request, &inputs).unwrap();
    renderer.budget_bytes = 1;
    assert!(renderer.render_frame(&plan, request, &inputs).is_err());
    assert_eq!(renderer.resource_stats().unwrap().resident_bytes, 0);
    renderer.budget_bytes = 512 * 1024 * 1024;
    renderer.render_frame(&plan, request, &inputs).unwrap();
}

#[test]
#[ignore = "requires Vulkan"]
fn preview_scheduler_recovers_from_invalid_input_and_preserves_request_identity() {
    use std::{
        sync::{Arc, mpsc},
        time::Duration,
    };
    let (tx, rx) = mpsc::channel();
    let scheduler = PreviewScheduler::with_wake(move || {
        tx.send(()).unwrap();
    })
    .unwrap();
    let node = Node::new(
        "image",
        Operation::Image(veylvik_document::nodes::Image {
            asset: "plate".into(),
        }),
        &[],
    );
    let id = node.id;
    let plan = Arc::new(Plan::compile(&composition(vec![node], id)).unwrap());
    let request = RenderRequest::full(Time::new(7, 24).unwrap());
    let first = scheduler
        .submit(42, plan.clone(), request, ImageInputs::default())
        .unwrap();
    rx.recv_timeout(Duration::from_secs(30)).unwrap();
    let result = scheduler.try_take().unwrap().unwrap();
    assert_eq!(result.ticket, first);
    assert!(
        result
            .frame
            .err()
            .unwrap()
            .to_string()
            .contains("missing image input")
    );
    let mut inputs = ImageInputs::default();
    inputs.insert("plate", Arc::new(Image::new(1, 1, vec![[0.5; 4]]).unwrap()));
    let second = scheduler.submit(42, plan, request, inputs).unwrap();
    assert!(second.generation > first.generation);
    rx.recv_timeout(Duration::from_secs(30)).unwrap();
    let result = scheduler.try_take().unwrap().unwrap();
    assert_eq!(result.ticket, second);
    assert_eq!(result.request.time.ticks, request.time.ticks);
    assert_eq!(result.request.time.rate, request.time.rate);
    assert_eq!(result.frame.unwrap().pixels[0], [0.5; 4]);
    assert!(result.resources.unwrap().buffers_created > 0);
}

#[test]
#[ignore = "requires Vulkan"]
fn gpu_frames_remain_immutable_and_held_allocations_count_against_budget() {
    let mut renderer = Renderer::new().unwrap();
    let node = Node::new(
        "source",
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [2.0, 0.5, 0.0, 0.5],
        }),
        &[],
    );
    let id = node.id;
    let mut doc = composition(vec![node], id);
    let plan = Plan::compile(&doc).unwrap();
    let request = RenderRequest {
        time: Time::new(0, 1).unwrap(),
        region: Some(Region {
            x: 2,
            y: 1,
            width: 3,
            height: 2,
        }),
    };
    renderer.budget_bytes = plan.gpu_working_bytes();
    let first = renderer
        .render_gpu(&plan, request, &ImageInputs::default())
        .unwrap();
    assert_eq!(
        renderer.resource_stats().unwrap().held_bytes,
        first.byte_size()
    );
    assert!(
        renderer
            .render_gpu(&plan, request, &ImageInputs::default())
            .is_err()
    );
    assert!(renderer.read_frame(&first).is_err());
    renderer.budget_bytes = 512 * 1024 * 1024;
    doc.nodes.get_mut(&id).unwrap().operation = Operation::Solid(veylvik_document::nodes::Solid {
        color: [0.0, 0.0, 1.0, 1.0],
    });
    let second = renderer
        .render_gpu(
            &Plan::compile(&doc).unwrap(),
            request,
            &ImageInputs::default(),
        )
        .unwrap();
    assert_eq!(
        renderer.read_frame(&first).unwrap().pixels,
        vec![[1.0, 0.25, 0.0, 0.5]; 6]
    );
    assert_eq!(
        renderer.read_frame(&second).unwrap().pixels,
        vec![[0.0, 0.0, 1.0, 1.0]; 6]
    );
    let other = Renderer::new().unwrap();
    assert!(other.read_frame(&first).is_err());
    renderer.trim_resources().unwrap();
    assert_eq!(
        renderer.resource_stats().unwrap().resident_bytes,
        first.byte_size() + second.byte_size()
    );
    drop(first);
    assert_eq!(
        renderer.resource_stats().unwrap().held_bytes,
        second.byte_size()
    );
    drop(second);
    assert_eq!(renderer.resource_stats().unwrap().resident_bytes, 0);
    // A frame may outlive its renderer without a lock, dangling allocation, or panic.
    let frame = renderer
        .render_gpu(&plan, request, &ImageInputs::default())
        .unwrap();
    drop(renderer);
    drop(frame);
}

#[test]
#[ignore = "requires Vulkan"]
fn gpu_validation_rejects_nonfinite_output_without_poisoning_renderer() {
    let renderer = Renderer::new().unwrap();
    let node = Node::new(
        "source",
        Operation::Image(veylvik_document::nodes::Image {
            asset: "plate".into(),
        }),
        &[],
    );
    let gain = Node::new(
        "gain",
        Operation::Exposure(veylvik_document::nodes::Exposure { stops: 32.0.into() }),
        &[("image", node.id)],
    );
    let id = gain.id;
    let doc = composition(vec![node, gain], id);
    let plan = Plan::compile(&doc).unwrap();
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "plate",
        std::sync::Arc::new(Image::new(1, 1, vec![[f32::MAX, 0.0, 0.0, 1.0]]).unwrap()),
    );
    let request = RenderRequest::full(Time::new(0, 1).unwrap());
    assert!(
        renderer
            .render_gpu(&plan, request, &inputs)
            .err()
            .unwrap()
            .to_string()
            .contains("non-finite")
    );
    assert_eq!(renderer.resource_stats().unwrap().held_bytes, 0);
    inputs.insert(
        "plate",
        std::sync::Arc::new(Image::new(1, 1, vec![[0.0, 0.0, 0.0, 1.0]]).unwrap()),
    );
    assert!(renderer.render_gpu(&plan, request, &inputs).is_ok());
}

#[test]
#[ignore = "requires Vulkan"]
fn gpu_preview_scheduler_shares_device_and_releases_cancelled_outputs() {
    use std::{
        sync::{Arc, mpsc},
        time::Duration,
    };
    let renderer = Arc::new(Renderer::new().unwrap());
    let node = Node::new(
        "solid",
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [0.25, 0.5, 1.0, 1.0],
        }),
        &[],
    );
    let id = node.id;
    let plan = Arc::new(Plan::compile(&composition(vec![node], id)).unwrap());
    let (tx, rx) = mpsc::channel();
    let scheduler = GpuPreviewScheduler::with_renderer(renderer.clone(), move || {
        tx.send(()).unwrap();
    })
    .unwrap();
    let request = RenderRequest::full(Time::new(0, 1).unwrap());
    let ticket = scheduler
        .submit(5, plan.clone(), request, ImageInputs::default())
        .unwrap();
    rx.recv_timeout(Duration::from_secs(30)).unwrap();
    let result = scheduler.try_take().unwrap().unwrap();
    assert_eq!(result.ticket, ticket);
    let frame = result.frame.unwrap();
    assert_eq!(
        renderer.read_frame(&frame).unwrap().pixels[0],
        [0.25, 0.5, 1.0, 1.0]
    );
    drop(frame);
    scheduler
        .submit(6, plan, request, ImageInputs::default())
        .unwrap();
    rx.recv_timeout(Duration::from_secs(30)).unwrap();
    scheduler.cancel().unwrap();
    assert!(scheduler.try_take().unwrap().is_none());
    // Completed frames stay in the playback cache for repeat playback; clearing it
    // must release every GPU byte the scheduler held.
    scheduler.playback_cache().clear();
    renderer.clear_frame_cache().unwrap();
    assert_eq!(renderer.resource_stats().unwrap().held_bytes, 0);
}

#[test]
#[ignore = "requires Vulkan"]
fn output_cache_keys_evaluated_dependencies_and_evicts_within_budget() {
    use std::sync::Arc;
    let mut renderer = Renderer::new().unwrap();
    let source = Node::new(
        "source",
        Operation::Image(veylvik_document::nodes::Image {
            asset: "plate".into(),
        }),
        &[],
    );
    let source_id = source.id;
    let gain = Node::new(
        "gain",
        Operation::Exposure(veylvik_document::nodes::Exposure {
            stops: Scalar::Linear {
                keys: vec![
                    Key {
                        segment: None,
                        time: Time::new(0, 1).unwrap(),
                        value: 0.0,
                    },
                    Key {
                        segment: None,
                        time: Time::new(1, 1).unwrap(),
                        value: 1.0,
                    },
                ],
            },
        }),
        &[("image", source_id)],
    );
    let id = gain.id;
    let mut doc = composition(vec![source, gain], id);
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "plate",
        Arc::new(Image::new(1, 1, vec![[0.25, 0.0, 0.0, 1.0]]).unwrap()),
    );
    let request = RenderRequest::full(Time::new(1, 1).unwrap());
    let first = renderer
        .render_gpu_cached(&Plan::compile(&doc).unwrap(), request, &inputs)
        .unwrap();
    let allocations = renderer.resource_stats().unwrap().buffers_created;
    doc.nodes.get_mut(&id).unwrap().label = "Label does not change pixels".into();
    doc.nodes.get_mut(&id).unwrap().position =
        Some(veylvik_document::NodePosition { x: -42.0, y: 123.0 });
    let unused = Node::new(
        "unused",
        Operation::Solid(veylvik_document::nodes::Solid { color: [1.0; 4] }),
        &[],
    );
    doc.nodes.insert(unused.id, unused);
    let held = renderer
        .render_gpu_cached(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(20, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert!(Arc::ptr_eq(&first, &held));
    assert_eq!(
        renderer.resource_stats().unwrap().buffers_created,
        allocations
    );
    assert_eq!(renderer.cache_stats().unwrap().hits, 1);
    let animated = renderer
        .render_gpu_cached(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert!(!Arc::ptr_eq(&first, &animated));
    assert_eq!(renderer.read_frame(&animated).unwrap().pixels[0][0], 0.25);
    assert_eq!(renderer.read_frame(&first).unwrap().pixels[0][0], 0.5);
    inputs.insert(
        "plate",
        Arc::new(Image::new(1, 1, vec![[0.0, 0.25, 0.0, 1.0]]).unwrap()),
    );
    let changed_source = renderer
        .render_gpu_cached(&Plan::compile(&doc).unwrap(), request, &inputs)
        .unwrap();
    assert_eq!(
        renderer.read_frame(&changed_source).unwrap().pixels[0],
        [0.0, 0.5, 0.0, 1.0]
    );
    let crop = RenderRequest {
        region: Some(Region {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }),
        ..request
    };
    let cropped = renderer
        .render_gpu_cached(&Plan::compile(&doc).unwrap(), crop, &inputs)
        .unwrap();
    assert_eq!(cropped.region().width, 1);
    drop((first, held, animated, changed_source, cropped));
    renderer.cache_limit_bytes = 91 * 16;
    let plan = Plan::compile(&doc).unwrap();
    renderer.budget_bytes = plan.gpu_working_bytes() + 16;
    for ticks in [0, 1, 0, 1] {
        let frame = renderer
            .render_gpu_cached(
                &plan,
                RenderRequest::full(Time::new(ticks, 1).unwrap()),
                &inputs,
            )
            .unwrap();
        drop(frame);
        assert_eq!(renderer.cache_stats().unwrap().entries, 1);
        assert!(renderer.resource_stats().unwrap().resident_bytes <= renderer.budget_bytes);
    }
    assert!(renderer.cache_stats().unwrap().evictions >= 4);
    renderer.cache_limit_bytes = 0;
    drop(renderer.render_gpu_cached(&plan, request, &inputs).unwrap());
    assert_eq!(renderer.cache_stats().unwrap().entries, 0);
    assert_eq!(renderer.resource_stats().unwrap().held_bytes, 0);
    renderer.trim_resources().unwrap();
    assert_eq!(renderer.resource_stats().unwrap().resident_bytes, 0);
}

#[test]
#[ignore = "requires Vulkan"]
fn curve_modes_reach_gpu_and_cache_keys_follow_evaluated_values() {
    use std::sync::Arc;
    let renderer = Renderer::new().unwrap();
    let source = Node::new(
        "white",
        Operation::Solid(veylvik_document::nodes::Solid { color: [1.0; 4] }),
        &[],
    );
    let mut stops = Scalar::Linear {
        keys: vec![
            Key {
                segment: None,
                time: Time::new(0, 1).unwrap(),
                value: 0.0,
            },
            Key {
                segment: None,
                time: Time::new(1, 1).unwrap(),
                value: 8.0,
            },
        ],
    };
    let exposure = Node::new(
        "gain",
        Operation::Exposure(veylvik_document::nodes::Exposure {
            stops: stops.clone(),
        }),
        &[("image", source.id)],
    );
    let id = exposure.id;
    let mut doc = composition(vec![source, exposure], id);
    let inputs = ImageInputs::default();
    for (mode, quarter_stops) in [
        (Interpolation::Linear, 2.0_f32),
        (Interpolation::Step, 0.0),
        (Interpolation::Smooth, 1.25),
        (Interpolation::Monotone, 2.0),
    ] {
        stops.set_interpolation(mode);
        doc.nodes.get_mut(&id).unwrap().operation =
            Operation::Exposure(veylvik_document::nodes::Exposure {
                stops: stops.clone(),
            });
        let plan = Plan::compile(&doc).unwrap();
        let frame = renderer
            .render_gpu_cached(
                &plan,
                RenderRequest::full(Time::new(1, 4).unwrap()),
                &inputs,
            )
            .unwrap();
        let pixels = renderer.read_frame(&frame).unwrap();
        for pixel in pixels.pixels {
            assert!((pixel[0] - quarter_stops.exp2()).abs() < 0.00001);
            assert_eq!(pixel[3], 1.0);
        }
        if mode == Interpolation::Step {
            let held = renderer
                .render_gpu_cached(
                    &plan,
                    RenderRequest::full(Time::new(3, 4).unwrap()),
                    &inputs,
                )
                .unwrap();
            assert!(Arc::ptr_eq(&frame, &held));
            let next = renderer
                .render_gpu_cached(
                    &plan,
                    RenderRequest::full(Time::new(1, 1).unwrap()),
                    &inputs,
                )
                .unwrap();
            assert!(!Arc::ptr_eq(&frame, &next));
            assert_eq!(renderer.read_frame(&next).unwrap().pixels[0][0], 256.0);
        }
    }
}

#[test]
#[ignore = "requires Vulkan"]
fn immutable_source_uploads_reuse_contents_and_aliases_share_budget() {
    use std::sync::Arc;
    let mut renderer = Renderer::new().unwrap();
    let a = Node::new(
        "a",
        Operation::Image(veylvik_document::nodes::Image { asset: "a".into() }),
        &[],
    );
    let b = Node::new(
        "alias",
        Operation::Image(veylvik_document::nodes::Image { asset: "b".into() }),
        &[],
    );
    let merge = Node::new(
        "over",
        Operation::Merge(veylvik_document::nodes::Merge {}),
        &[("background", a.id), ("foreground", b.id)],
    );
    let exposure = Node::new(
        "animated",
        Operation::Exposure(veylvik_document::nodes::Exposure {
            stops: Scalar::Linear {
                keys: vec![
                    Key {
                        segment: None,
                        time: Time::new(0, 1).unwrap(),
                        value: 0.0,
                    },
                    Key {
                        segment: None,
                        time: Time::new(1, 1).unwrap(),
                        value: 1.0,
                    },
                ],
            },
        }),
        &[("image", merge.id)],
    );
    let output = exposure.id;
    let plan = Plan::compile(&composition(vec![a, b, merge, exposure], output)).unwrap();
    renderer.budget_bytes = plan.working_bytes() + 16; // One unique 1×1 source, despite two names.
    let source = Arc::new(Image::new(1, 1, vec![[0.25, 0.0, 0.0, 0.5]]).unwrap());
    let mut inputs = ImageInputs::default();
    inputs.insert("a", source.clone());
    inputs.insert("b", source);
    let first = renderer
        .render_frame(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert_eq!(first.pixels[0], [0.375, 0.0, 0.0, 0.75]);
    let first_stats = renderer.resource_stats().unwrap();
    assert_eq!(first_stats.source_uploads, 1);
    assert_eq!(first_stats.source_upload_bytes, 16);
    let second = renderer
        .render_frame(
            &plan,
            RenderRequest::full(Time::new(1, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert_eq!(second.pixels[0], [0.75, 0.0, 0.0, 0.75]);
    let second_stats = renderer.resource_stats().unwrap();
    assert_eq!(second_stats.source_uploads, 1);
    assert_eq!(second_stats.source_reuses, 1);
    assert_eq!(second_stats.buffers_created, first_stats.buffers_created);
    let replacement = Arc::new(Image::new(1, 1, vec![[0.0, 0.5, 0.0, 1.0]]).unwrap());
    inputs.insert("a", replacement.clone());
    inputs.insert("b", replacement);
    let third = renderer
        .render_frame(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    assert_eq!(third.pixels[0], [0.0, 0.5, 0.0, 1.0]);
    assert_eq!(renderer.resource_stats().unwrap().source_uploads, 2);
    renderer.trim_resources().unwrap();
    assert_eq!(renderer.resource_stats().unwrap().resident_bytes, 0);
    renderer
        .render_frame(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    let stats = renderer.resource_stats().unwrap();
    assert_eq!(stats.source_uploads, 3);
    assert_eq!(stats.source_upload_bytes, 48);
    assert!(stats.resident_bytes <= renderer.budget_bytes);
}

#[test]
#[ignore = "requires Vulkan"]
fn gpu_frame_inputs_preserve_crops_ownership_cache_and_budget() {
    use std::sync::Arc;
    let mut renderer = Renderer::new().unwrap();
    let source_node = Node::new(
        "source",
        Operation::Image(veylvik_document::nodes::Image {
            asset: "cpu".into(),
        }),
        &[],
    );
    let source_id = source_node.id;
    let source_plan = Plan::compile(&composition(vec![source_node], source_id)).unwrap();
    let pixels: Vec<_> = (0..91)
        .map(|i| [(i % 13) as f32 / 13.0, (i / 13) as f32 / 7.0, 2.0, 1.0])
        .collect();
    let mut cpu = ImageInputs::default();
    cpu.insert("cpu", Arc::new(Image::new(13, 7, pixels.clone()).unwrap()));
    let crop = Region {
        x: 3,
        y: 2,
        width: 4,
        height: 3,
    };
    let source = Arc::new(
        renderer
            .render_gpu(
                &source_plan,
                RenderRequest {
                    time: Time::new(0, 1).unwrap(),
                    region: Some(crop),
                },
                &cpu,
            )
            .unwrap(),
    );
    let source_uploads = renderer.resource_stats().unwrap().source_uploads;
    let input_node = Node::new(
        "gpu",
        Operation::Image(veylvik_document::nodes::Image {
            asset: "frame".into(),
        }),
        &[],
    );
    let input_id = input_node.id;
    let plan = Plan::compile(&composition(vec![input_node], input_id)).unwrap();
    let mut inputs = ImageInputs::default();
    inputs.insert_gpu("frame", source.clone());
    assert!(inputs.contains_key("frame"));
    assert!(inputs.get("frame").is_none());
    assert!(Arc::ptr_eq(inputs.get_gpu("frame").unwrap(), &source));
    assert_eq!(inputs.byte_size(), 0); // GPU retention is accounted by the renderer.
    renderer.budget_bytes = source.byte_size() + plan.working_bytes();
    let request = RenderRequest::full(Time::new(0, 1).unwrap());
    let output = renderer.render_frame(&plan, request, &inputs).unwrap();
    for y in 0..7 {
        for x in 0..13 {
            let expected = if x < 4 && y < 3 {
                pixels[(y + 2) * 13 + x + 3]
            } else {
                [0.0; 4]
            };
            assert_eq!(output.pixels[y * 13 + x], expected);
        }
    }
    let stats = renderer.resource_stats().unwrap();
    assert_eq!(stats.source_uploads, source_uploads);
    assert_eq!(stats.held_bytes, source.byte_size());
    assert!(stats.resident_bytes <= renderer.budget_bytes);
    let foreign = Renderer::new().unwrap();
    assert!(foreign.render_frame(&plan, request, &inputs).is_err());
    assert!(foreign.render_gpu_cached(&plan, request, &inputs).is_err());
    assert_eq!(foreign.resource_stats().unwrap().buffers_created, 0);
    renderer.budget_bytes = 512 * 1024 * 1024;
    let first = renderer.render_gpu_cached(&plan, request, &inputs).unwrap();
    let hit = renderer.render_gpu_cached(&plan, request, &inputs).unwrap();
    assert!(Arc::ptr_eq(&first, &hit));
    let solid = Node::new(
        "new pixels",
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [0.75, 0.25, 3.0, 1.0],
        }),
        &[],
    );
    let id = solid.id;
    let replacement = Arc::new(
        renderer
            .render_gpu(
                &Plan::compile(&composition(vec![solid], id)).unwrap(),
                RenderRequest {
                    time: request.time,
                    region: Some(crop),
                },
                &ImageInputs::default(),
            )
            .unwrap(),
    );
    inputs.insert_gpu("frame", replacement);
    let changed = renderer.render_gpu_cached(&plan, request, &inputs).unwrap();
    assert!(!Arc::ptr_eq(&first, &changed));
    assert_eq!(
        renderer.read_frame(&changed).unwrap().pixels[0],
        [0.75, 0.25, 3.0, 1.0]
    );
    assert_eq!(renderer.read_frame(&first).unwrap().pixels, output.pixels);
    inputs.insert(
        "frame",
        Arc::new(Image::new(1, 1, vec![[0.0, 0.0, 0.0, 1.0]]).unwrap()),
    );
    assert!(inputs.get_gpu("frame").is_none());
    assert_eq!(inputs.byte_size(), 16);
    assert_eq!(
        renderer
            .render_frame(&plan, request, &inputs)
            .unwrap()
            .pixels[0],
        [0.0, 0.0, 0.0, 1.0]
    );
}

/// Render `operation` over a single one-pixel light at the centre of a size x size frame.
fn render_point(operation: Operation, size: u32, light: f32) -> Vec<[f32; 4]> {
    use std::sync::Arc;
    let source = Node::new(
        "image",
        Operation::Image(veylvik_document::nodes::Image {
            asset: "plate".into(),
        }),
        &[],
    );
    let node = Node::new("filter", operation, &[("image", source.id)]);
    let id = node.id;
    let mut doc = composition(vec![source, node], id);
    doc.width = size;
    doc.height = size;
    let mut pixels = vec![[0.0f32; 4]; (size * size) as usize];
    let centre = (size / 2 * size + size / 2) as usize;
    pixels[centre] = [light, light, light, 1.0];
    let mut inputs = ImageInputs::default();
    inputs.insert("plate", Arc::new(Image::new(size, size, pixels).unwrap()));
    let renderer = Renderer::new().unwrap();
    let plan = Plan::compile(&doc).unwrap();
    renderer
        .render_frame(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap()
        .pixels
}

#[test]
#[ignore = "requires Vulkan"]
fn gaussian_blur_is_round_symmetric_and_conserves_energy() {
    let out = render_point(
        Operation::Blur(veylvik_document::nodes::Blur {
            radius: 9.0.into(),
            filter: veylvik_document::nodes::BlurFilter::Gaussian,
        }),
        41,
        1.0,
    );
    let at = |x: i32, y: i32| out[((20 + y) * 41 + 20 + x) as usize][0];
    let energy: f32 = out.iter().map(|p| p[0]).sum();
    assert!((energy - 1.0).abs() < 1e-4, "energy {energy}");
    for d in 1..=9 {
        assert!((at(d, 0) - at(-d, 0)).abs() < 1e-7 && (at(0, d) - at(d, 0)).abs() < 1e-7);
        assert!(at(d, 0) < at(d - 1, 0), "falls off from the centre");
    }
    // Separable gaussian: the diagonal follows the product of the axes.
    assert!((at(3, 3) * at(0, 0) - at(3, 0) * at(0, 3)).abs() < 1e-7);
    assert_eq!(at(10, 0), 0.0, "reach is the radius");
}

#[test]
#[ignore = "requires Vulkan"]
fn lens_blur_spreads_a_point_light_into_a_solid_disc() {
    for (radius, size) in [(8.0f32, 41u32), (30.0, 81)] {
        let out = render_point(
            Operation::LensBlur(veylvik_document::nodes::LensBlur {
                radius: radius.into(),
                highlights: 0.0.into(),
                threshold: 1.0.into(),
            }),
            size,
            1.0,
        );
        let c = (size / 2) as i32;
        let energy: f32 = out.iter().map(|p| p[0]).sum();
        assert!(
            (energy - 1.0).abs() < 0.02,
            "radius {radius}: energy {energy}"
        );
        let (mut inside, mut lit, mut outside) = (0, 0, 0.0f32);
        for y in 0..size as i32 {
            for x in 0..size as i32 {
                let d = (((x - c).pow(2) + (y - c).pow(2)) as f32).sqrt();
                let v = out[(y * size as i32 + x) as usize][0];
                if d <= radius - 3.0 {
                    inside += 1;
                    lit += usize::from(v > 0.0);
                } else if d > radius + 4.0 {
                    outside = outside.max(v);
                }
            }
        }
        assert_eq!(
            lit, inside,
            "radius {radius}: the disc is solid, not speckled"
        );
        assert_eq!(outside, 0.0, "radius {radius}: nothing beyond the disc");
        // Even disc: the centre is no brighter than the average over the disc.
        let mean = 1.0 / (std::f32::consts::PI * radius * radius);
        let centre = out[(c * size as i32 + c) as usize][0];
        assert!(
            centre < mean * 2.0,
            "radius {radius}: centre {centre} vs mean {mean}"
        );
    }
    // Highlights add energy only above the threshold.
    let lens = |light: f32| {
        render_point(
            Operation::LensBlur(veylvik_document::nodes::LensBlur {
                radius: 8.0.into(),
                highlights: 2.0.into(),
                threshold: 1.0.into(),
            }),
            41,
            light,
        )
        .iter()
        .map(|p| p[0])
        .sum::<f32>()
    };
    assert!((lens(0.5) - 0.5).abs() < 0.01);
    assert!((lens(3.0) - 3.0 * (1.0 + 2.0 * 2.0)).abs() < 0.1);
}
