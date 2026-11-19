use veylvik_document::{nodes::FadeTiming, *};
use veylvik_render::*;

fn fade_over(doc: &mut Composition, layer: NodeId, timing: FadeTiming, opacity: f32) {
    let fade = Node::new(
        "Fade",
        Operation::Fade(veylvik_document::nodes::Fade {
            opacity: Scalar::Constant { value: opacity },
            timing,
        }),
        &[("image", layer)],
    );
    doc.output = fade.id;
    doc.nodes.insert(fade.id, fade);
}

#[test]
#[ignore = "requires Vulkan"]
fn fade_scales_premultiplied_pixels_by_its_envelope_and_skips_hidden_layers() {
    let renderer = Renderer::new().unwrap();
    let rate = Time { ticks: 24, rate: 1 };
    let at = |frame| Time::at_frame(frame, rate).unwrap();
    let timing = FadeTiming {
        start: at(24),
        end: Some(at(96)),
        fade_in: at(12),
        fade_out: at(24),
    };
    let mut doc = blank(4, 2, rate).unwrap();
    let layer = doc.output;
    doc.nodes.get_mut(&layer).unwrap().operation =
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [0.8, 0.4, 2.0, 0.5],
        });
    fade_over(&mut doc, layer, timing, 0.5);
    let plan = Plan::compile(&doc).unwrap();
    for frame in [0, 30, 50, 84, 96] {
        let pixels = renderer
            .render_frame(
                &plan,
                RenderRequest::full(at(frame)),
                &ImageInputs::default(),
            )
            .unwrap()
            .pixels;
        let level = (0.5 * timing.envelope(at(frame)).unwrap()) as f32;
        for (actual, expected) in pixels[0].iter().zip([0.8 * 0.5, 0.4 * 0.5, 2.0 * 0.5, 0.5]) {
            assert!(
                (actual - expected * level).abs() < 1e-6,
                "frame {frame}: {actual} vs {}",
                expected * level
            );
        }
    }
    // Footage behind a faded-out layer is never decoded: a missing source only fails when visible.
    let mut doc = blank(4, 2, rate).unwrap();
    let clip = doc.output;
    doc.nodes.get_mut(&clip).unwrap().operation =
        Operation::Footage(veylvik_document::nodes::Footage {
            asset: "absent.mov".into(),
            timing: Default::default(),
            placement: Default::default(),
            colour: Default::default(),
            layer: None,
        });
    fade_over(&mut doc, clip, timing, 1.0);
    let plan = Plan::compile(&doc).unwrap();
    let hidden = renderer
        .render_frame(&plan, RenderRequest::full(at(0)), &ImageInputs::default())
        .unwrap();
    assert!(hidden.pixels.iter().all(|p| *p == [0.0; 4]));
    let visible =
        renderer.render_frame(&plan, RenderRequest::full(at(50)), &ImageInputs::default());
    assert!(visible.is_err());
}

#[test]
#[ignore = "requires Vulkan"]
fn a_hidden_layer_does_not_take_an_identical_visible_one_with_it() {
    // Identical nodes share one result. When the one that owns it sits behind a
    // faded-out layer, the visible twin must still get its picture.
    let renderer = Renderer::new().unwrap();
    let rate = Time { ticks: 24, rate: 1 };
    let at = |frame| Time::at_frame(frame, rate).unwrap();
    let colour = [0.2, 0.4, 0.6, 1.0];
    let solid = || {
        Node::new(
            "Solid",
            Operation::Solid(veylvik_document::nodes::Solid { color: colour }),
            &[],
        )
    };
    for hidden_side in ["background", "foreground"] {
        let mut doc = blank(4, 2, rate).unwrap();
        let first = doc.output;
        doc.nodes.get_mut(&first).unwrap().operation = solid().operation;
        let (a, b) = (solid(), solid());
        let fade = Node::new(
            "Fade",
            Operation::Fade(veylvik_document::nodes::Fade {
                opacity: Scalar::Constant { value: 1.0 },
                timing: FadeTiming {
                    start: at(24),
                    end: None,
                    fade_in: at(0),
                    fade_out: at(0),
                },
            }),
            &[("image", a.id)],
        );
        let other = if hidden_side == "background" {
            "foreground"
        } else {
            "background"
        };
        let merge = Node::new(
            "Merge",
            Operation::Merge(veylvik_document::nodes::Merge {}),
            &[(hidden_side, fade.id), (other, b.id)],
        );
        doc.output = merge.id;
        for node in [a, b, fade, merge] {
            doc.nodes.insert(node.id, node);
        }
        let plan = Plan::compile(&doc).unwrap();
        let pixels = renderer
            .render_frame(&plan, RenderRequest::full(at(0)), &ImageInputs::default())
            .unwrap()
            .pixels;
        for (actual, expected) in pixels[0].iter().zip(colour) {
            assert!(
                (actual - expected).abs() < 1e-6,
                "hidden {hidden_side}: {actual} vs {expected}"
            );
        }
    }
}
