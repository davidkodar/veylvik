use veylvik_document::*;
use veylvik_render::*;

fn coverage(renderer: &Renderer, corner: f32) -> (f32, Vec<[f32; 4]>) {
    let mut doc = blank(200, 100, Time::new(24, 1).unwrap()).unwrap();
    let c = |v: f32| Scalar::Constant { value: v };
    let node = Node::new(
        "Rectangle",
        Operation::Rectangle(nodes::Rectangle {
            x: c(100.0),
            y: c(50.0),
            width: c(120.0),
            height: c(60.0),
            corner: c(corner),
            feather: c(1.0),
            color: [1.0; 4],
        }),
        &[],
    );
    doc.output = node.id;
    doc.nodes.insert(node.id, node);
    doc.validate().unwrap();
    let pixels = renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap()
        .pixels;
    (pixels.iter().map(|p| p[3]).sum(), pixels)
}
#[test]
#[ignore = "requires Vulkan"]
fn a_rectangle_covers_its_area_and_rounded_corners_cut_it() {
    let renderer = Renderer::new().unwrap();
    let (square, pixels) = coverage(&renderer, 0.0);
    assert!((square - 120.0 * 60.0).abs() < 60.0, "{square}");
    let at = |x: usize, y: usize| pixels[y * 200 + x][3];
    // Inside, outside, and premultiplied white.
    assert_eq!(at(100, 50), 1.0);
    assert_eq!(at(20, 50), 0.0);
    assert_eq!(pixels[50 * 200 + 100], [1.0; 4]);
    // A corner radius of half the height makes a pill: two half discs replace the ends.
    let (pill, pixels) = coverage(&renderer, 30.0);
    let expected = 120.0 * 60.0 - (4.0 - std::f32::consts::PI) * 30.0 * 30.0;
    assert!((pill - expected).abs() < 60.0, "{pill} {expected}");
    assert_eq!(pixels[21 * 200 + 41][3], 0.0);
    // A radius beyond half the shorter side is held there.
    let (held, _) = coverage(&renderer, 500.0);
    assert!((held - pill).abs() < 1.0);
}
#[test]
#[ignore = "requires Vulkan"]
fn a_rectangle_is_filled_with_its_colour_and_text_counts_with_a_number() {
    let renderer = Renderer::new().unwrap();
    let mut doc = blank(200, 100, Time::new(24, 1).unwrap()).unwrap();
    let c = |v: f32| Scalar::Constant { value: v };
    let node = Node::new(
        "Rectangle",
        Operation::Rectangle(nodes::Rectangle {
            x: c(100.0),
            y: c(50.0),
            width: c(120.0),
            height: c(60.0),
            corner: c(0.0),
            feather: c(1.0),
            color: [2.0, 0.5, 0.25, 0.5],
        }),
        &[],
    );
    doc.output = node.id;
    doc.nodes.insert(node.id, node);
    doc.validate().unwrap();
    let pixels = renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap()
        .pixels;
    // Premultiplied: colour times opacity, HDR kept.
    assert_eq!(pixels[50 * 200 + 100], [1.0, 0.25, 0.125, 0.5]);
    // The words follow the number: wider at 100 than at 7.
    let mut text: Operation = serde_json::from_value(registry::TEXT.default_operation()).unwrap();
    if let Operation::Text(t) = &mut text {
        t.content = "{n}%".into();
        t.size = c(40.0);
        t.x = c(100.0);
        t.y = c(60.0);
        t.typography.number = Some(Scalar::Linear {
            keys: vec![
                Key {
                    segment: None,
                    time: Time::new(0, 1).unwrap(),
                    value: 7.0,
                },
                Key {
                    segment: None,
                    time: Time::new(1, 1).unwrap(),
                    value: 100.0,
                },
            ],
        });
    }
    let node = Node::new("Count", text, &[]);
    doc.output = node.id;
    doc.nodes.insert(node.id, node);
    doc.validate().unwrap();
    let plan = Plan::compile(&doc).unwrap();
    let ink = |seconds: i64| -> f32 {
        renderer
            .render_frame(
                &plan,
                RenderRequest::full(Time::new(seconds, 1).unwrap()),
                &ImageInputs::default(),
            )
            .unwrap()
            .pixels
            .iter()
            .map(|p| p[3])
            .sum()
    };
    let (early, late) = (ink(0), ink(1));
    assert!(late > early * 1.3, "{early} {late}");
}
#[test]
#[ignore = "requires Vulkan"]
fn repeat_lays_copies_in_a_grid() {
    let renderer = Renderer::new().unwrap();
    let mut doc = blank(200, 100, Time::new(24, 1).unwrap()).unwrap();
    let c = |v: f32| Scalar::Constant { value: v };
    let dot = Node::new(
        "Dot",
        Operation::Rectangle(nodes::Rectangle {
            x: c(20.0),
            y: c(20.0),
            width: c(10.0),
            height: c(10.0),
            corner: c(0.0),
            feather: c(1.0),
            color: [1.0; 4],
        }),
        &[],
    );
    let grid = Node::new(
        "Grid",
        Operation::Repeat(nodes::Repeat {
            columns: c(4.0),
            rows: c(2.0),
            step_x: c(40.0),
            step_y: c(50.0),
            fade: c(0.0),
        }),
        &[("image", dot.id)],
    );
    doc.output = grid.id;
    doc.nodes.insert(dot.id, dot);
    doc.nodes.insert(grid.id, grid);
    doc.validate().unwrap();
    let pixels = renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap()
        .pixels;
    let at = |x: usize, y: usize| pixels[y * 200 + x][3];
    // Eight copies, each where the step puts it, and nothing between.
    for row in 0..2 {
        for column in 0..4 {
            assert_eq!(at(20 + column * 40, 20 + row * 50), 1.0);
        }
    }
    assert_eq!(at(40, 20), 0.0);
    assert_eq!(at(180, 20), 0.0);
    let total: f32 = pixels.iter().map(|p| p[3]).sum();
    assert!((total - 800.0).abs() < 40.0, "{total}");
}
