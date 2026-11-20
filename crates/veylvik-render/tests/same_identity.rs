//! The rule the frame cache stands on, checked for every node type there is: two moments with the same identity must be the same picture.
use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

const W: u32 = 64;
const H: u32 = 36;

#[test]
#[ignore = "requires Vulkan"]
fn the_same_identity_is_the_same_picture_for_every_node_type() {
    let mut renderer = Renderer::new().unwrap();
    renderer.budget_bytes = 1 << 30;
    let rate = Time { ticks: 24, rate: 1 };
    // A picture with colour, soft alpha and nothing at its edges, for every image input.
    let pixels: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            let (x, y) = ((i % W) as f32 / W as f32, (i / W) as f32 / H as f32);
            let a = ((x - 0.5).abs().max((y - 0.5).abs()) < 0.4) as u32 as f32 * (0.4 + 0.6 * x);
            [x * a, y * a, (1.0 - x) * a, a]
        })
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert("src", Arc::new(Image::new(W, H, pixels).unwrap()));
    let (mut steady, mut moving, mut skipped) = (Vec::new(), Vec::new(), Vec::new());
    // Kept apart from "moving": the identity changed although the picture did not, which is
    // work that could have been spared, never a wrong picture.
    let mut spared = Vec::new();
    for kind in registry::TYPES {
        let mut doc = blank(W, H, rate).unwrap();
        doc.frame_count = 48;
        let source = Node::new(
            "src",
            Operation::Image(nodes::Image {
                asset: "src".into(),
            }),
            &[],
        );
        let Ok(operation) = serde_json::from_value::<Operation>(kind.default_operation()) else {
            skipped.push(kind.type_id);
            continue;
        };
        // Only what the node cannot do without: its optional ports take lights, scenes and
        // maps that an image cannot stand in for.
        let wired: Vec<(&str, NodeId)> = kind
            .inputs
            .iter()
            .filter(|port| !kind.optional.contains(port))
            .map(|port| (*port, source.id))
            .collect();
        let node = Node::new(kind.type_id, operation, &wired);
        doc.output = node.id;
        doc.nodes.insert(source.id, source);
        doc.nodes.insert(node.id, node);
        // Types that are not a picture on their own (lights, scenes, sound), or that need a
        // file or a model that is not here, are not this test's.
        let Ok(plan) = Plan::compile(&doc) else {
            skipped.push(kind.type_id);
            continue;
        };
        let at = |frame| RenderRequest::full(Time::at_frame(frame, rate).unwrap());
        let look = |frame| -> Option<(u64, Vec<[f32; 4]>)> {
            renderer.clear_frame_cache().ok()?;
            let identity = renderer.identity(&plan, at(frame), &inputs).ok()?;
            let picture = renderer
                .render_frame(&plan, at(frame), &inputs)
                .ok()?
                .pixels;
            Some((identity, picture))
        };
        let (Some(early), Some(late)) = (look(3), look(17)) else {
            skipped.push(kind.type_id);
            continue;
        };
        if early.0 == late.0 {
            assert!(
                early.1 == late.1,
                "{}: the same identity at two times, but two pictures: the cache would show the wrong one",
                kind.type_id
            );
            steady.push(kind.type_id);
        } else if early.1 == late.1 {
            spared.push(kind.type_id);
        } else {
            moving.push(kind.type_id);
        }
    }
    println!(
        "the same at every time ({}): {}",
        steady.len(),
        steady.join(", ")
    );
    println!(
        "changing with time ({}): {}",
        moving.len(),
        moving.join(", ")
    );
    println!(
        "made again though unchanged ({}): {}",
        spared.len(),
        spared.join(", ")
    );
    println!(
        "not a picture alone, or needing a file ({}): {}",
        skipped.len(),
        skipped.join(", ")
    );
    // A node type may be left out only for a reason written here; a new type is checked, or
    // someone decides why not.
    let excused = [
        // No picture of their own: parts of a scene, lights, sound.
        "light_rig",
        "sound",
        "shape_3d",
        "text_3d",
        "image_card",
        "transform_3d",
        "merge_scenes",
        "camera_3d",
        "light_3d",
        "environment_3d",
        "point_cloud",
        "model_3d",
        // Need a file, a model, a scene or a configuration that this test does not have.
        "lut",
        "render_scene",
        "image",
        "footage",
        "ocio",
        // Needs a track to follow.
        "planar_warp",
        // Takes no plain still inside it; it has a test of its own below.
        "own_size",
    ];
    let unexcused: Vec<_> = skipped
        .iter()
        .filter(|kind| !excused.contains(*kind))
        .collect();
    assert!(
        unexcused.is_empty(),
        "node types this test could not check: {unexcused:?}"
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn only_a_picture_whose_every_input_has_a_name_is_known_in_another_session() {
    let rate = Time { ticks: 24, rate: 1 };
    let pixels = || vec![[0.25_f32, 0.5, 0.75, 1.0]; (W * H) as usize];
    let mut doc = blank(W, H, rate).unwrap();
    let still = Node::new(
        "still",
        Operation::Image(nodes::Image {
            asset: "still".into(),
        }),
        &[],
    );
    doc.output = still.id;
    doc.nodes.insert(still.id, still);
    let plan = Plan::compile(&doc).unwrap();
    let request = RenderRequest::full(Time::at_frame(0, rate).unwrap());
    let lasting = |image: Image| {
        // A session of its own: its own renderer, its own count of identities.
        let renderer = Renderer::new().unwrap();
        let mut inputs = ImageInputs::default();
        inputs.insert("still", Arc::new(image));
        renderer.lasting_identity(&plan, request, &inputs).unwrap()
    };
    // Known only by this session's count: two different stills could meet under one
    // number in two sessions, so such a picture has no lasting identity at all.
    assert_eq!(lasting(Image::new(W, H, pixels()).unwrap()), None);
    // Named by its maker, the same name is the same picture in every session, and
    // another name is another picture.
    let named = lasting(Image::new(W, H, pixels()).unwrap().named(41));
    assert!(named.is_some());
    assert_eq!(
        lasting(Image::new(W, H, pixels()).unwrap().named(41)),
        named
    );
    assert_ne!(
        lasting(Image::new(W, H, pixels()).unwrap().named(42)),
        named
    );
    // A node's own data is named by its content.
    assert_eq!(
        lasting(Image::data(W, H, pixels()).unwrap()),
        lasting(Image::data(W, H, pixels()).unwrap())
    );
    assert!(lasting(Image::data(W, H, pixels()).unwrap()).is_some());
}

#[test]
#[ignore = "requires Vulkan"]
fn a_picture_made_apart_is_part_of_what_the_whole_is_made_of() {
    let renderer = Renderer::new().unwrap();
    let rate = Time { ticks: 24, rate: 1 };
    let of = |kind: &str| {
        registry::TYPES
            .iter()
            .find(|t| t.type_id == kind)
            .unwrap()
            .default_operation()
    };
    // A solid colour made at a size of its own, then shown in the frame.
    let composition = |red: f32| {
        let mut doc = blank(W, H, rate).unwrap();
        doc.frame_count = 48;
        let mut colour = of("solid");
        colour["color"] = serde_json::json!([red, 0.25, 0.5, 1.0]);
        let inner = Node::new("inner", serde_json::from_value(colour).unwrap(), &[]);
        let sized = Node::new(
            "sized",
            serde_json::from_value(of("own_size")).unwrap(),
            &[("image", inner.id)],
        );
        doc.output = sized.id;
        doc.nodes.insert(inner.id, inner);
        doc.nodes.insert(sized.id, sized);
        Plan::compile(&doc).unwrap()
    };
    let inputs = ImageInputs::default();
    let at = |frame| RenderRequest::full(Time::at_frame(frame, rate).unwrap());
    let look = |plan: &Plan, frame| {
        (
            renderer.identities(plan, at(frame), &inputs).unwrap(),
            renderer
                .render_gpu(plan, at(frame), &inputs)
                .and_then(|f| renderer.read_frame(&f))
                .unwrap()
                .pixels,
        )
    };
    let dim = composition(0.2);
    let (early, late) = (look(&dim, 3), look(&dim, 17));
    // Nothing in it moves: one identity, one picture, and one that holds between sessions.
    assert_eq!(early.0, late.0);
    assert!(early.1 == late.1);
    assert!(early.0.1.is_some());
    // What is inside changes and nothing outside does: it is another picture, and is
    // known as another.
    let bright = look(&composition(0.9), 3);
    assert!(bright.1 != early.1);
    assert_ne!(bright.0.0, early.0.0);
    assert_ne!(bright.0.1, early.0.1);
}
