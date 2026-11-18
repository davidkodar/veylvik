use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

const W: u32 = 64;
const H: u32 = 48;

/// A ramp (red = x, green = y of each pixel centre) through a grid warp.
fn render(warp: nodes::GridWarp) -> Vec<[f32; 4]> {
    render_operation(Operation::GridWarp(warp))
}
fn render_operation(operation: Operation) -> Vec<[f32; 4]> {
    let renderer = Renderer::new().unwrap();
    let pixels: Vec<[f32; 4]> = (0..W * H)
        .map(|i| [(i % W) as f32 + 0.5, (i / W) as f32 + 0.5, 0.0, 1.0])
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert("ramp", Arc::new(Image::new(W, H, pixels).unwrap()));
    let image = Node::new(
        "ramp",
        Operation::Image(nodes::Image {
            asset: "ramp".into(),
        }),
        &[],
    );
    let node = Node::new("warp", operation, &[("image", image.id)]);
    let doc = Composition {
        markers: Default::default(),
        ocio: Default::default(),
        working_space: Default::default(),
        display: Default::default(),
        groups: Default::default(),
        schema: 1,
        frame_count: 1,
        width: W,
        height: H,
        frame_rate: Time::new(24, 1).unwrap(),
        output: node.id,
        nodes: [image, node].into_iter().map(|n| (n.id, n)).collect(),
    };
    let plan = Plan::compile(&doc).unwrap();
    let frame = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap();
    renderer.read_frame(&frame).unwrap().pixels
}
fn grid(columns: u32, rows: u32, amount: f32) -> nodes::GridWarp {
    nodes::GridWarp {
        columns,
        rows,
        points: vec![[0.0; 2]; (columns * rows) as usize],
        amount: amount.into(),
    }
}
#[test]
#[ignore = "requires Vulkan"]
fn the_image_under_a_point_follows_it_and_the_rest_stays_smooth() {
    // An untouched mesh leaves the image alone.
    let still = render(grid(5, 4, 1.0));
    for (i, p) in still.iter().enumerate() {
        assert_eq!(p[0], (i as u32 % W) as f32 + 0.5);
    }
    // Every point moved the same way moves the whole image that way.
    let mut all = grid(4, 4, 1.0);
    all.points.fill([6.0, -3.0]);
    let moved = render(all);
    let p = moved[(20 * W + 30) as usize];
    assert!(
        (p[0] - 24.5).abs() < 1e-3 && (p[1] - 23.5).abs() < 1e-3,
        "{p:?}"
    );
    // One point of a 5 x 5 mesh, at the frame's centre, moved 5 px right and 4 down:
    // the image that was at the centre is now where the point went.
    let mut one = grid(5, 5, 1.0);
    one.points[12] = [5.0, 4.0];
    let out = render(one.clone());
    let at = |x: u32, y: u32| out[(y * W + x) as usize];
    // Centre of the frame is (32, 24); its content shows at (37, 28).
    let p = at(36, 27); // pixel centre (36.5, 27.5), half a pixel short of the target
    assert!(
        (p[0] - 31.5).abs() < 0.2 && (p[1] - 23.5).abs() < 0.2,
        "{p:?}"
    );
    // Far from the point (beyond its neighbours) nothing moves, and the corners hold.
    for (x, y) in [(2, 2), (61, 45), (2, 45), (61, 2)] {
        let p = at(x, y);
        assert!(
            (p[0] - (x as f32 + 0.5)).abs() < 0.05 && (p[1] - (y as f32 + 0.5)).abs() < 0.05,
            "{p:?}"
        );
    }
    // The warp is smooth: neighbouring pixels read neighbouring sources.
    for y in 1..H - 1 {
        for x in 1..W - 2 {
            let step = at(x + 1, y)[0] - at(x, y)[0];
            assert!((0.3..1.9).contains(&step), "step {step} at {x},{y}");
        }
    }
    // Amount scales it: half the warp moves the content half as far.
    one.amount = 0.5.into();
    let half = render(one);
    let p = half[(25 * W + 34) as usize]; // centre (34.5, 25.5) = centre + (2.5, 2)... minus half a pixel
    assert!(
        (p[0] - 32.0).abs() < 0.25 && (p[1] - 23.5).abs() < 0.3,
        "{p:?}"
    );
}
fn rectangle(x: f32, y: f32, w: f32, h: f32) -> vector::PathMask {
    use vector::PathCommand::*;
    vector::PathMask {
        keys: vec![],
        fill_rule: Default::default(),
        commands: vec![
            Move { to: [x, y] },
            Line { to: [x + w, y] },
            Line { to: [x + w, y + h] },
            Line { to: [x, y + h] },
            Close {},
        ],
    }
}
#[test]
#[ignore = "requires Vulkan"]
fn a_start_shape_is_carried_onto_the_end_shape() {
    let warp = |to: vector::PathMask, amount: f32| {
        render_operation(Operation::ShapeWarp(nodes::ShapeWarp {
            from: rectangle(20.0, 14.0, 16.0, 12.0),
            to,
            amount: amount.into(),
            reach: 12.0.into(),
        }))
    };
    // The same shape twice leaves the picture alone.
    let still = warp(rectangle(20.0, 14.0, 16.0, 12.0), 1.0);
    for (i, p) in still.iter().enumerate() {
        assert_eq!(p[0], (i as u32 % W) as f32 + 0.5);
    }
    // Moved 8 px right and 6 down: what was inside the start shape is inside the end shape.
    let out = warp(rectangle(28.0, 20.0, 16.0, 12.0), 1.0);
    let at = |x: u32, y: u32| out[(y * W + x) as usize];
    let p = at(36, 26); // centre (36.5, 26.5) shows the content from (28.5, 20.5)
    assert!(
        (p[0] - 28.5).abs() < 0.35 && (p[1] - 20.5).abs() < 0.35,
        "{p:?}"
    );
    // Beyond the reach the picture is untouched.
    for (x, y) in [(2, 2), (61, 45), (60, 3), (3, 44)] {
        let p = at(x, y);
        assert!(
            (p[0] - (x as f32 + 0.5)).abs() < 0.05 && (p[1] - (y as f32 + 0.5)).abs() < 0.05,
            "{p:?}"
        );
    }
    // Half the amount moves it half as far.
    let half = warp(rectangle(28.0, 20.0, 16.0, 12.0), 0.5);
    let p = half[(23 * W + 32) as usize]; // centre (32.5, 23.5) shows (28.5, 20.5)
    assert!(
        (p[0] - 28.5).abs() < 0.35 && (p[1] - 20.5).abs() < 0.35,
        "{p:?}"
    );
    // Shapes with different numbers of outlines are refused.
    let mut two = rectangle(20.0, 14.0, 16.0, 12.0);
    two.commands
        .extend(rectangle(44.0, 30.0, 6.0, 6.0).commands);
    assert!(
        Operation::ShapeWarp(nodes::ShapeWarp {
            from: rectangle(20.0, 14.0, 16.0, 12.0),
            to: two,
            amount: 1.0.into(),
            reach: 12.0.into(),
        })
        .validate()
        .is_err()
    );
}
