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
