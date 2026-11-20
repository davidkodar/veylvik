//! What a node says about space is checked against what it does, for every node type: nothing outside its stated extent, and nothing taken from beyond its stated reach.
use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

const W: u32 = 96;
const H: u32 = 64;
/// Where the source has content, and the part of the output that is compared.
const CONTENT: [i32; 4] = [30, 20, 66, 44];
const PROBE: [i32; 4] = [40, 26, 56, 38];

fn picture(disturbed_beyond: Option<i32>) -> Arc<Image> {
    let pixels = (0..W * H)
        .map(|i| {
            let (x, y) = ((i % W) as i32, (i / W) as i32);
            let inside = |a: [i32; 4], grow: i32| {
                x >= a[0] - grow && y >= a[1] - grow && x < a[2] + grow && y < a[3] + grow
            };
            if let Some(reach) = disturbed_beyond
                && !inside(PROBE, reach)
            {
                // Another picture altogether, away from the probe.
                return [0.9, 0.1, 0.4, 1.0];
            }
            if !inside(CONTENT, 0) {
                return [0.0; 4];
            }
            let (u, v) = (x as f32 / W as f32, y as f32 / H as f32);
            let a = 0.5 + 0.5 * u;
            [u * a, v * a, (1.0 - u) * a, a]
        })
        .collect();
    Arc::new(Image::new(W, H, pixels).unwrap())
}

#[test]
#[ignore = "requires Vulkan"]
fn what_a_node_says_about_space_is_what_it_does() {
    let mut renderer = Renderer::new().unwrap();
    renderer.budget_bytes = 1 << 30;
    let rate = Time { ticks: 24, rate: 1 };
    let time = Time::at_frame(3, rate).unwrap();
    let (mut bounded, mut local, mut tried) = (Vec::new(), Vec::new(), 0);
    for kind in registry::TYPES {
        // As it comes, and with each of its numbers at its least and at its most: a claim
        // has to hold for the settings it is made with, not only the usual ones.
        let usual = kind.default_operation();
        let mut settings = vec![("as it comes".to_string(), usual.clone())];
        for parameter in kind.parameters {
            if !matches!(parameter.kind, registry::ParameterKind::Scalar) {
                continue;
            }
            for (which, limit) in [("least", parameter.min), ("most", parameter.max)] {
                if let Some(limit) = limit {
                    let mut changed = usual.clone();
                    changed[parameter.name] =
                        serde_json::json!({"kind": "constant", "value": limit});
                    settings.push((
                        format!("{} at its {which}, {limit}", parameter.name),
                        changed,
                    ));
                }
            }
        }
        for (label, setting) in settings {
            let mut doc = blank(W, H, rate).unwrap();
            doc.frame_count = 48;
            let source = Node::new(
                "src",
                Operation::Image(nodes::Image {
                    asset: "src".into(),
                }),
                &[],
            );
            let Ok(operation) = serde_json::from_value::<Operation>(setting) else {
                continue;
            };
            let wired: Vec<(&str, NodeId)> = kind
                .inputs
                .iter()
                .filter(|port| !kind.optional.contains(port))
                .map(|port| (*port, source.id))
                .collect();
            if wired.is_empty() {
                continue;
            }
            let node = Node::new(kind.type_id, operation, &wired);
            doc.output = node.id;
            doc.nodes.insert(source.id, source);
            doc.nodes.insert(node.id, node);
            let Ok(plan) = Plan::compile(&doc) else {
                continue;
            };
            let extent = plan.extent(time, &|_| Extent::Area(CONTENT)).unwrap();
            let reach = plan
                .reach(time, "src")
                .unwrap()
                .unwrap_or(Reach::Everything);
            // A node that says nothing has nothing to be held to.
            if extent == Extent::Everything && reach == Reach::Everything {
                continue;
            }
            let render = |image: Arc<Image>| {
                renderer.clear_frame_cache().unwrap();
                let mut inputs = ImageInputs::default();
                inputs.insert("src", image);
                renderer
                    .render_frame(&plan, RenderRequest::full(time), &inputs)
                    .map(|frame| frame.pixels)
            };
            let Ok(plain) = render(picture(None)) else {
                continue;
            };
            tried += 1;
            // Extent: with content only in CONTENT, nothing may show outside what the node says.
            if extent != Extent::Everything {
                for (i, pixel) in plain.iter().enumerate() {
                    let (x, y) = ((i as u32 % W) as i32, (i as u32 / W) as i32);
                    assert!(
                        extent.contains(x, y) || *pixel == [0.0; 4],
                        "{} ({label}): says its picture stays within {extent:?}, but ({x}, {y}) is {pixel:?}",
                        kind.type_id
                    );
                }
                if !bounded.contains(&kind.type_id) {
                    bounded.push(kind.type_id);
                }
            }
            // Reach: the source changed beyond the stated reach must leave the probe as it was.
            let pixels = match reach {
                Reach::Everything => continue,
                Reach::Same => 0,
                Reach::Around(pixels) => pixels as i32,
            };
            // A reach past the frame's edges leaves nothing to disturb.
            if PROBE[0] - pixels <= 0 || PROBE[1] - pixels <= 0 {
                continue;
            }
            let disturbed = render(picture(Some(pixels))).unwrap();
            for y in PROBE[1]..PROBE[3] {
                for x in PROBE[0]..PROBE[2] {
                    let i = (y as u32 * W + x as u32) as usize;
                    assert!(
                        plain[i] == disturbed[i],
                        "{} ({label}): says it reaches {reach:?}, but ({x}, {y}) changed when the source changed beyond that",
                        kind.type_id
                    );
                }
            }
            if !local.contains(&kind.type_id) {
                local.push(kind.type_id);
            }
        }
    }
    println!("settings tried: {tried}");
    println!(
        "say where their picture is ({}): {}",
        bounded.len(),
        bounded.join(", ")
    );
    println!(
        "say how far they reach ({}): {}",
        local.len(),
        local.join(", ")
    );
    // The nodes that have said so far; the lists only grow.
    for kind in [
        "exposure",
        "color_correct",
        "colorize",
        "luma_key",
        "blur",
        "merge",
        "blend",
        "mask",
        "fade",
    ] {
        assert!(
            bounded.contains(&kind),
            "{kind} no longer says where its picture is"
        );
        assert!(
            local.contains(&kind),
            "{kind} no longer says how far it reaches"
        );
    }
    for kind in ["channel_mask", "combine_mask"] {
        assert!(
            local.contains(&kind),
            "{kind} no longer says how far it reaches"
        );
    }
}
