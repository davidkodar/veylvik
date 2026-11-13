use std::sync::Arc;
use veylvik_document::{Operation, Time};
use veylvik_render::{Image, ImageInputs, Plan, RenderRequest, Renderer};
use veylvik_track::point::PointTracker;
#[test]
#[ignore = "requires Vulkan"]
fn gpu_tracking_finds_translation_rejects_flat_and_ambiguous_patches() {
    let renderer = Renderer::new().unwrap();
    let tracker = PointTracker::new(&renderer);
    let mut doc = veylvik_document::blank(96, 64, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation =
        Operation::Image(veylvik_document::nodes::Image {
            asset: "source".into(),
        });
    let plan = Plan::compile(&doc).unwrap();
    let make = |shift: i32, copies: bool, flat: bool| {
        let mut pixels = vec![[0.0, 0.0, 0.0, 1.0]; 96 * 64];
        if !flat {
            for y in 20..=44 {
                for x in 20..=44 {
                    let v = ((x * 73 + y * 139 + x * y * 29) % 251) as f32 / 250.0;
                    let p = [v, v, v, 1.0];
                    pixels[y * 96 + (x as i32 + shift) as usize] = p;
                    if copies {
                        pixels[y * 96 + x + 32] = p;
                    }
                }
            }
        }
        let mut inputs = ImageInputs::default();
        inputs.insert("source", Arc::new(Image::new(96, 64, pixels).unwrap()));
        renderer
            .render_gpu(
                &plan,
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &inputs,
            )
            .unwrap()
    };
    let reference = make(0, false, false);
    let current = make(7, false, false);
    let found = tracker
        .track(
            &renderer,
            &reference,
            &current,
            [32.0, 32.0],
            [32.0, 32.0],
            [8, 12],
        )
        .unwrap();
    assert_eq!(found.position, [39.0, 32.0]);
    assert!(found.confidence > 0.99);
    assert!(found.separation > 0.03);
    let flat = make(0, false, true);
    assert!(
        tracker
            .track(
                &renderer,
                &reference,
                &flat,
                [32.0, 32.0],
                [32.0, 32.0],
                [8, 12]
            )
            .unwrap()
            .confidence
            < 0.75
    );
    let ambiguous = make(0, true, false);
    assert!(
        tracker
            .track(
                &renderer,
                &reference,
                &ambiguous,
                [32.0, 32.0],
                [48.0, 32.0],
                [8, 24]
            )
            .unwrap()
            .separation
            < 0.03
    );
    assert!(
        tracker
            .track(
                &renderer,
                &reference,
                &current,
                [0.0, 0.0],
                [32.0, 32.0],
                [8, 12]
            )
            .is_err()
    );
    let backward = tracker
        .track(
            &renderer,
            &current,
            &reference,
            [39.0, 32.0],
            [39.0, 32.0],
            [8, 12],
        )
        .unwrap();
    assert_eq!(backward.position, [32.0, 32.0]);
}
