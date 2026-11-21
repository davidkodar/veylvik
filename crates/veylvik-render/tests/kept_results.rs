//! A node's result that does not change from frame to frame is kept and stands for its passes in the next frame, and every frame is bit for bit the frame a fresh renderer makes.
use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

const W: u32 = 96;
const H: u32 = 64;

fn of(kind: &str, set: serde_json::Value) -> Operation {
    let mut value = registry::TYPES
        .iter()
        .find(|t| t.type_id == kind)
        .unwrap()
        .default_operation();
    for (key, field) in set.as_object().unwrap() {
        value[key] = field.clone();
    }
    serde_json::from_value(value).unwrap()
}
/// A still picture graded, blurred, graded and graded again. The last grade changes
/// every frame; with `changing_first` the first does too, so everything after it changes
/// though its settings stay the same.
fn composition(changing_first: bool) -> (Composition, ImageInputs) {
    let pixels: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            let (x, y) = ((i % W) as f32 / W as f32, (i / W) as f32 / H as f32);
            [x, y, 1.0 - x, 1.0]
        })
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert("still", Arc::new(Image::new(W, H, pixels).unwrap()));
    let still = Node::new(
        "still",
        of("image", serde_json::json!({"asset": "still"})),
        &[],
    );
    // Brighter every frame.
    let changing = || {
        of(
            "exposure",
            serde_json::json!({"stops": {"kind": "linear", "keys": [
                {"time": {"ticks": 0, "rate": 1}, "value": -2.0},
                {"time": {"ticks": 1, "rate": 1}, "value": 1.0}
            ]}}),
        )
    };
    let steady = || {
        of(
            "exposure",
            serde_json::json!({"stops": {"kind": "constant", "value": 0.5}}),
        )
    };
    let first = Node::new(
        "first grade",
        if changing_first { changing() } else { steady() },
        &[("image", still.id)],
    );
    let blurred = Node::new(
        "blurred",
        of(
            "blur",
            serde_json::json!({"radius": {"kind": "constant", "value": 6.0}}),
        ),
        &[("image", first.id)],
    );
    let graded = Node::new("graded", steady(), &[("image", blurred.id)]);
    let last = Node::new("last grade", changing(), &[("image", graded.id)]);
    let mut composition = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    composition.output = last.id;
    for node in [still, first, blurred, graded, last] {
        composition.nodes.insert(node.id, node);
    }
    (composition, inputs)
}

#[test]
#[ignore = "requires Vulkan"]
fn an_unchanging_result_stands_for_its_passes_and_every_frame_is_the_same() {
    for changing_first in [false, true] {
        frames(changing_first);
    }
}
fn frames(changing_first: bool) {
    let (composition, inputs) = composition(changing_first);
    let plan = Plan::compile(&composition).unwrap();
    let rate = Time::new(24, 1).unwrap();
    let playing = Renderer::new().unwrap();
    for frame in 0..8 {
        let at = Time::at_frame(frame, rate).unwrap();
        let shown = playing
            .render_frame(&plan, RenderRequest::full(at), &inputs)
            .unwrap()
            .pixels;
        // A renderer that has kept nothing makes the same frame, bit for bit.
        let fresh = Renderer::new()
            .unwrap()
            .render_frame(&plan, RenderRequest::full(at), &inputs)
            .unwrap()
            .pixels;
        let bits =
            |p: &[[f32; 4]]| -> Vec<u32> { p.iter().flatten().map(|v| v.to_bits()).collect() };
        assert_eq!(
            bits(&shown),
            bits(&fresh),
            "frame {frame} differs (changing first: {changing_first})"
        );
    }
    let (kept, _, hits) = playing.kept_results().unwrap();
    // Kept after the second frame (unchanged since the first), used from the third on:
    // the graded blur, or with the first grade changing only the still picture (what is
    // made of a changing result changes with it; the frames above prove nothing stale
    // is used).
    assert!(kept >= 1, "nothing was kept");
    assert!(hits >= 5, "kept results stood for passes only {hits} times");
}
