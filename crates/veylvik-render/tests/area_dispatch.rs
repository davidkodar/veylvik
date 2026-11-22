//! A node clear outside a box is run over that box only, and every frame is bit for bit the frame made with every pass over the whole frame: moving shapes, partly outside the frame, through every way of combining masks, masked and merged.
use veylvik_document::vector::{FillRule, PathCommand::*, PathMask};
use veylvik_document::*;
use veylvik_render::*;

const W: u32 = 160;
const H: u32 = 96;

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
/// A quadrilateral at `x`, `y`, `w` wide and `h` tall, a little slanted (antialiased edges).
fn shape(x: f32, y: f32, w: f32, h: f32) -> Operation {
    Operation::Path(nodes::Path {
        geometry: PathMask {
            keys: Vec::new(),
            fill_rule: FillRule::NonZero,
            commands: vec![
                Move { to: [x, y] },
                Line {
                    to: [x + w, y + 1.5],
                },
                Line {
                    to: [x + w - 2.0, y + h],
                },
                Line {
                    to: [x - 1.0, y + h - 0.5],
                },
                Close {},
            ],
        },
    })
}
/// At frame `f` the shapes have moved `f` times; one crosses the right edge.
fn composition(f: f32) -> Composition {
    let mut composition = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let mut add = |node: Node| {
        let id = node.id;
        composition.nodes.insert(id, node);
        id
    };
    let colour = |rgb: [f32; 3]| {
        of(
            "solid",
            serde_json::json!({"color": [rgb[0], rgb[1], rgb[2], 1.0]}),
        )
    };
    let red = add(Node::new("red", colour([0.9, 0.2, 0.1]), &[]));
    let blue = add(Node::new("blue", colour([0.1, 0.3, 0.9]), &[]));
    let ground = add(Node::new("ground", colour([0.05, 0.05, 0.06]), &[]));
    let mut picture = ground;
    for (k, mode) in ["union", "intersect", "subtract", "exclude"]
        .into_iter()
        .enumerate()
    {
        let k = k as f32;
        let a = add(Node::new(
            "a",
            shape(10.0 + 30.0 * k + f * 3.0, 8.0 + f, 26.0, 30.0),
            &[],
        ));
        let b = add(Node::new(
            "b",
            shape(22.0 + 30.0 * k + f * 2.0, 20.0, 24.0, 28.0),
            &[],
        ));
        let combined = add(Node::new(
            "combined",
            of("combine_mask", serde_json::json!({"mode": mode})),
            &[("mask", a), ("other", b)],
        ));
        let layer = add(Node::new(
            "layer",
            of("mask", serde_json::json!({})),
            &[
                (
                    "image",
                    if (k as u32).is_multiple_of(2) {
                        red
                    } else {
                        blue
                    },
                ),
                ("mask", combined),
            ],
        ));
        picture = add(Node::new(
            "over",
            of("merge", serde_json::json!({})),
            &[("background", picture), ("foreground", layer)],
        ));
    }
    // Two small layers merged together first (a small merge), one crossing the edge.
    let edge = add(Node::new("edge", shape(140.0 + f, 60.0, 40.0, 20.0), &[]));
    let small = add(Node::new("small", shape(100.0, 70.0 - f, 12.0, 12.0), &[]));
    let edge_layer = add(Node::new(
        "edge layer",
        of("mask", serde_json::json!({})),
        &[("image", blue), ("mask", edge)],
    ));
    let small_layer = add(Node::new(
        "small layer",
        of("mask", serde_json::json!({})),
        &[("image", red), ("mask", small)],
    ));
    let pair = add(Node::new(
        "pair",
        of("merge", serde_json::json!({})),
        &[("background", edge_layer), ("foreground", small_layer)],
    ));
    let last = add(Node::new(
        "picture",
        of("merge", serde_json::json!({})),
        &[("background", picture), ("foreground", pair)],
    ));
    composition.output = last;
    composition
}

#[test]
#[ignore = "requires Vulkan"]
fn a_layer_run_over_its_box_is_the_layer_run_over_the_frame() {
    let boxed = Renderer::new().unwrap();
    let mut whole = Renderer::new().unwrap();
    whole.area_dispatch = false;
    let bits = |p: &[[f32; 4]]| -> Vec<u32> { p.iter().flatten().map(|v| v.to_bits()).collect() };
    for frame in 0..6 {
        let composition = composition(frame as f32);
        let plan = Plan::compile(&composition).unwrap();
        let at = RenderRequest::full(Time::new(0, 1).unwrap());
        let inputs = ImageInputs::default();
        let a = boxed.render_frame(&plan, at, &inputs).unwrap().pixels;
        let b = whole.render_frame(&plan, at, &inputs).unwrap().pixels;
        assert!(
            a.iter().any(|p| p[0] > 0.5),
            "nothing drawn in frame {frame}"
        );
        assert_eq!(bits(&a), bits(&b), "frame {frame} differs");
    }
    // The boxes were used: the shapes' passes took less than the frame.
    assert!(boxed.areas_used().unwrap() > 0, "no pass ran over a box");
}
