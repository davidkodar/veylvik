use veylvik_document::*;
use veylvik_render::*;

/// A grey solid through Exposure whose stops follow a disconnected node by expression.
#[test]
#[ignore = "requires Vulkan"]
fn linked_parameters_drive_renders_and_invalidate_plans() {
    let renderer = Renderer::new().unwrap();
    let solid = Node::new(
        "Grey",
        Operation::Solid(nodes::Solid {
            color: [0.25, 0.25, 0.25, 1.0],
        }),
        &[],
    );
    let driver = Node::new(
        "Driver",
        Operation::Blur(nodes::Blur {
            radius: 2.0.into(),
            filter: Default::default(),
        }),
        &[("image", solid.id)],
    );
    let exposure = Node::new(
        "Exposure",
        Operation::Exposure(nodes::Exposure {
            stops: Scalar::Expression {
                expression: "[Driver].radius / 2 + time".into(),
                resolved: None,
            },
        }),
        &[("image", solid.id)],
    );
    let (driver_id, output) = (driver.id, exposure.id);
    let composition = Composition {
        ocio: Default::default(),
        working_space: Default::default(),
        display: Default::default(),
        groups: Default::default(),
        schema: 1,
        frame_count: 48,
        width: 4,
        height: 4,
        frame_rate: Time::new(24, 1).unwrap(),
        output,
        nodes: [solid, driver, exposure]
            .into_iter()
            .map(|n| (n.id, n))
            .collect(),
    };
    let mut doc = Document::new(composition).unwrap();
    let pixel = |doc: &Document, seconds: i64| {
        let plan = Plan::compile(doc.snapshot()).unwrap();
        let frame = renderer
            .render_gpu_cached(
                &plan,
                RenderRequest::full(Time::new(seconds, 1).unwrap()),
                &ImageInputs::default(),
            )
            .unwrap();
        (plan, renderer.read_frame(&frame).unwrap().pixels[0][0])
    };
    // 1 stop at 0 s, 2 stops at 1 s.
    let (plan, at0) = pixel(&doc, 0);
    assert!((at0 - 0.5).abs() < 1e-5, "{at0}");
    assert!((pixel(&doc, 1).1 - 1.0).abs() < 1e-5);
    // The driver is not rendered, yet changing it changes the plan and the picture.
    doc.apply(
        doc.revision(),
        &[Command::SetParameter {
            id: driver_id,
            parameter: "radius".into(),
            value: serde_json::json!({"kind": "constant", "value": 0.0}),
        }],
    )
    .unwrap();
    assert!(!plan.matches_composition(doc.snapshot(), output).unwrap());
    assert!((pixel(&doc, 0).1 - 0.25).abs() < 1e-5);
}
