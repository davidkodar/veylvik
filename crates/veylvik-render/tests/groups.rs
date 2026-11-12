use serde_json::json;
use veylvik_document::{
    groups::{Control, Group},
    *,
};
use veylvik_render::*;
#[test]
#[ignore = "requires Vulkan"]
fn group_boundaries_add_no_passes_and_animated_controls_match_expanded_rendering() {
    let mut doc = blank(13, 7, Time::new(24, 1).unwrap()).unwrap();
    doc.nodes.get_mut(&doc.output).unwrap().operation = Operation::Solid {
        color: [0.25, 0.5, 0.75, 1.0],
    };
    let exposure = Node::new(
        "Exposure",
        Operation::Exposure { stops: 1.0.into() },
        &[("image", doc.output)],
    );
    let id = exposure.id;
    doc.nodes.insert(id, exposure);
    doc.output = id;
    let plain = Plan::compile(&doc).unwrap();
    let mut group = Group::capture(&doc, [id].into(), id, "Intensity").unwrap();
    group.controls.insert(
        "intensity".into(),
        Control::new(&doc, id, "stops", "Intensity").unwrap(),
    );
    doc.groups.insert(group.id, group.clone());
    assert!(plain.matches_composition(&doc, id).unwrap());
    let renderer = Renderer::new().unwrap();
    let inputs = ImageInputs::default();
    let mut owner = Document::new(doc).unwrap();
    owner.apply(0, &[Command::SetControl { id: group.id, control: "intensity".into(), value: json!({"kind":"smooth","keys":[{"time":{"ticks":0,"rate":24},"value":0},{"time":{"ticks":48,"rate":24},"value":2}]}) }]).unwrap();
    assert!(!plain.matches_composition(owner.snapshot(), id).unwrap());
    let grouped = Plan::compile(owner.snapshot()).unwrap();
    let mut expanded = owner.snapshot().clone();
    expanded.groups.clear();
    let ungrouped = Plan::compile(&expanded).unwrap();
    for (tick, stops) in [(0, 0.0f32), (24, 1.0), (48, 2.0)] {
        let request = RenderRequest::full(Time::new(tick, 24).unwrap());
        let a = renderer
            .render_gpu_cached(&grouped, request, &inputs)
            .unwrap();
        let b = renderer
            .render_gpu_cached(&ungrouped, request, &inputs)
            .unwrap();
        let a = renderer.read_frame(&a).unwrap();
        let b = renderer.read_frame(&b).unwrap();
        assert_eq!(a.pixels, b.pixels);
        for p in a.pixels {
            for (actual, expected) in p.into_iter().zip([
                0.25 * stops.exp2(),
                0.5 * stops.exp2(),
                0.75 * stops.exp2(),
                1.0,
            ]) {
                assert!((actual - expected).abs() < 1e-6);
            }
        }
    }
}
