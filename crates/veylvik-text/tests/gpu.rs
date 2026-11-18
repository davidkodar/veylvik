use std::sync::Arc;
use veylvik_document::{Composition, Node, Operation, Time};
use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer};
use veylvik_text::{Font, RunOptions};
#[test]
#[ignore = "requires Vulkan"]
fn shaped_outlines_feed_document_nodes_and_gpu_rasterization() {
    let font = Font::from_bytes(Arc::from(epaint_default_fonts::UBUNTU_LIGHT), 0).unwrap();
    let run = font
        .shape_run(
            "Veylvik ffi Å",
            &RunOptions {
                origin: [12.0, 80.0],
                ..Default::default()
            },
        )
        .unwrap();
    assert!(run.glyphs.iter().all(|g| !g.missing));
    let path = run.outline.unwrap();
    let renderer = Renderer::new().unwrap();
    let reference = renderer.render_path_mask(&path, 640, 128).unwrap();
    let expected = renderer.read_frame(&reference).unwrap().pixels;
    assert!(expected.iter().filter(|p| p[3] > 0.0).count() > 500);
    assert!(expected.iter().any(|p| p[3] > 0.0 && p[3] < 1.0));
    assert_eq!(expected[0], [0.0; 4]);
    let node = Node::new(
        "Outlined title",
        Operation::Path(veylvik_document::nodes::Path { geometry: path }),
        &[],
    );
    let document = Composition {
        markers: Default::default(),
        backdrops: Default::default(),
        ocio: Default::default(),
        working_space: Default::default(),
        display: Default::default(),
        groups: Default::default(),
        schema: 1,
        frame_count: 240,
        width: 640,
        height: 128,
        frame_rate: Time::new(24, 1).unwrap(),
        output: node.id,
        nodes: [(node.id, node)].into(),
    };
    let pixels = renderer
        .render_frame(
            &Plan::compile(&document).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap()
        .pixels;
    assert_eq!(pixels, expected);
    assert_eq!(renderer.resource_stats().unwrap().source_uploads, 0);
}
