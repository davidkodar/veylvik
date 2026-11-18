use veylvik_document::*;
use veylvik_render::*;

#[test]
#[ignore = "requires Vulkan"]
fn one_pixel_reads_match_the_whole_frame_in_full_and_cropped_frames() {
    let renderer = Renderer::new().unwrap();
    let mut doc = blank(17, 9, Time::new(24, 1).unwrap()).unwrap();
    let id = doc.output;
    doc.nodes.get_mut(&id).unwrap().operation =
        Operation::Gradient(veylvik_document::nodes::Gradient {
            mode: GradientMode::Linear,
            start_x: 0.0.into(),
            start_y: 0.0.into(),
            end_x: 16.0.into(),
            end_y: 8.0.into(),
        });
    let plan = Plan::compile(&doc).unwrap();
    let inputs = ImageInputs::default();
    let time = Time::new(0, 1).unwrap();
    let crop = Region {
        x: 3,
        y: 2,
        width: 5,
        height: 4,
    };
    for request in [
        RenderRequest::full(time),
        RenderRequest {
            time,
            region: Some(crop),
            scene_view: None,
        },
    ] {
        let frame = renderer.render_gpu_cached(&plan, request, &inputs).unwrap();
        let whole = renderer.read_frame(&frame).unwrap();
        let (width, height) = (whole.region.width, whole.region.height);
        let mut distinct = std::collections::BTreeSet::new();
        for (x, y) in [
            (0, 0),
            (width - 1, 0),
            (0, height - 1),
            (width - 1, height - 1),
            (2, 1),
        ] {
            let pixel = renderer.read_pixel(&frame, x, y).unwrap();
            assert_eq!(pixel, whole.pixels[(y * width + x) as usize], "{x},{y}");
            distinct.insert(pixel[0].to_bits());
        }
        assert!(
            distinct.len() >= 4,
            "the gradient must tell the pixels apart"
        );
        assert!(renderer.read_pixel(&frame, width, 0).is_err());
        assert!(renderer.read_pixel(&frame, 0, height).is_err());
    }
}
