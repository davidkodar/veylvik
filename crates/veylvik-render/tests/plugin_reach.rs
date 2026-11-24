//! An OpenFX plugin is native code: a composition held to its folder never gets one opened from elsewhere, and the refusal says why.
use veylvik_document::access::Reach;
use veylvik_document::nodes::{OfxContext, OfxEffect, Solid};
use veylvik_document::*;
use veylvik_render::*;

#[test]
#[ignore = "requires Vulkan"]
fn a_plugin_from_outside_the_folder_is_never_opened_for_a_composition_held_to_it() {
    let renderer = Renderer::new().unwrap();
    let rate = Time { ticks: 24, rate: 1 };
    let mut doc = blank(4, 2, rate).unwrap();
    let solid = Node::new(
        "Solid",
        Operation::Solid(Solid {
            color: [0.2, 0.4, 0.6, 1.0],
        }),
        &[],
    );
    let effect = Node::new(
        "Effect",
        Operation::OfxEffect(OfxEffect {
            bundle: "/somewhere/else/Fx.ofx.bundle".into(),
            plugin: "com.example.fx".into(),
            context: OfxContext::Filter,
            clips: vec!["Source".into()],
            settings: Default::default(),
            other_frames: false,
        }),
        &[("source", solid.id)],
    );
    doc.output = effect.id;
    for node in [solid, effect] {
        doc.nodes.insert(node.id, node);
    }
    let plan = Plan::compile(&doc).unwrap();
    let mut inputs = ImageInputs::default();
    inputs.set_base_directory(std::env::temp_dir().join("veylvik-plugin-reach"));
    inputs.set_reach(Reach::Within(Vec::new()));
    let refused = renderer
        .render_frame(
            &plan,
            RenderRequest::full(Time { ticks: 0, rate: 24 }),
            &inputs,
        )
        .err()
        .unwrap()
        .to_string();
    assert!(refused.contains("plugin folders"), "{refused}");
    // Trusted, the plugin gets as far as the effect host (none is loaded in this test).
    inputs.set_reach(Reach::Anywhere);
    let further = renderer
        .render_frame(
            &plan,
            RenderRequest::full(Time { ticks: 0, rate: 24 }),
            &inputs,
        )
        .err()
        .unwrap()
        .to_string();
    assert!(!further.contains("plugin folders"), "{further}");
}
