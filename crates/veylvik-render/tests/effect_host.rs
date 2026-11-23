//! OpenFX effect nodes through the renderer's effect host seam, with a stand-in host: it gets the real input pictures, the settings at the frame and the frame number, and its picture becomes the node's.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use veylvik_document::nodes::{OfxContext, OfxEffect, OfxSetting, OfxValue};
use veylvik_document::*;
use veylvik_render::effect_hook::{EffectHost, EffectJob};
use veylvik_render::*;

/// What the host was given: the frame, the settings and the clips.
type Call = (f64, Vec<(String, OfxValue)>, Vec<String>);
/// Multiplies the source by its `gain` setting and records what it was given.
#[derive(Default)]
struct Gain {
    calls: Mutex<Vec<Call>>,
}
impl EffectHost for Gain {
    fn render(
        &self,
        job: EffectJob<'_>,
        deliver: &mut dyn FnMut(&[f32]),
    ) -> veylvik_render::Result<()> {
        let gain = job
            .values
            .iter()
            .find_map(|(n, v)| match (n.as_str(), v) {
                ("gain", OfxValue::Numbers(g)) => Some(g[0] as f32),
                _ => None,
            })
            .unwrap_or(1.0);
        let clips = job.inputs.iter().map(|(c, _)| c.to_string()).collect();
        self.calls
            .lock()
            .unwrap()
            .push((job.frame, job.values.clone(), clips));
        let pixels = job.width as usize * job.height as usize * 4;
        let picture: Vec<f32> = match job.inputs.first() {
            Some((_, source)) => source.iter().map(|v| v * gain).collect(),
            None => vec![0.25; pixels],
        };
        deliver(&picture);
        Ok(())
    }
}
fn effect(gain: Scalar) -> OfxEffect {
    OfxEffect {
        bundle: "/nowhere/Gain.ofx.bundle".into(),
        plugin: "org.veylvik.test.gain".into(),
        context: OfxContext::Filter,
        clips: vec!["Source".into()],
        settings: BTreeMap::from([("gain".to_string(), OfxSetting::Numbers(vec![gain]))]),
    }
}
#[test]
#[ignore = "requires Vulkan"]
fn an_openfx_node_is_what_its_host_makes_of_the_real_input() {
    let renderer = Renderer::new().unwrap();
    let rate = Time { ticks: 24, rate: 1 };
    let mut doc = blank(4, 3, rate).unwrap();
    let solid = doc.output;
    doc.nodes.get_mut(&solid).unwrap().operation = Operation::Solid(nodes::Solid {
        color: [0.2, 0.4, 0.6, 1.0],
    });
    let keyed = Scalar::Linear {
        keys: vec![
            Key {
                segment: None,
                time: Time::at_frame(0, rate).unwrap(),
                value: 1.0,
            },
            Key {
                segment: None,
                time: Time::at_frame(10, rate).unwrap(),
                value: 3.0,
            },
        ],
    };
    let node = Node::new(
        "Gain",
        Operation::OfxEffect(effect(keyed)),
        &[("source", solid)],
    );
    doc.output = node.id;
    doc.nodes.insert(node.id, node.clone());
    let plan = Plan::compile(&doc).unwrap();
    // Without a host the node says what is missing.
    let refused = renderer.render_frame(
        &plan,
        RenderRequest::full(Time::at_frame(0, rate).unwrap()),
        &ImageInputs::default(),
    );
    match refused {
        Err(e) => assert!(e.to_string().contains("effect host"), "{e}"),
        Ok(_) => panic!("rendered without an effect host"),
    }
    let host = Arc::new(Gain::default());
    renderer.set_effect_host(host.clone());
    let at = |frame| RenderRequest::full(Time::at_frame(frame, rate).unwrap());
    let five = renderer
        .render_frame(&plan, at(5), &ImageInputs::default())
        .unwrap()
        .pixels;
    for (got, want) in five[0].iter().zip([0.4, 0.8, 1.2, 2.0]) {
        assert!((got - want).abs() < 1e-6, "{:?}", five[0]);
    }
    {
        let calls = host.calls.lock().unwrap();
        let (frame, values, clips) = calls.last().unwrap();
        assert_eq!(*frame, 5.0);
        assert_eq!(
            values,
            &vec![("gain".to_string(), OfxValue::Numbers(vec![2.0]))]
        );
        assert_eq!(clips, &vec!["Source".to_string()]);
    }
    // The cache knows the settings: the same frame again is kept, another moment is not.
    let first = renderer
        .render_gpu_cached(&plan, at(5), &ImageInputs::default())
        .unwrap();
    let again = renderer
        .render_gpu_cached(&plan, at(5), &ImageInputs::default())
        .unwrap();
    assert!(Arc::ptr_eq(&first, &again));
    let later = renderer
        .render_gpu_cached(&plan, at(10), &ImageInputs::default())
        .unwrap();
    assert!(!Arc::ptr_eq(&first, &later));
    let ten = renderer.read_frame(&later).unwrap().pixels;
    assert!((ten[0][0] - 0.6).abs() < 1e-6, "{:?}", ten[0]);
    // A changed constant setting is a new picture too, even at the same moment.
    let mut changed = doc.clone();
    changed.nodes.get_mut(&node.id).unwrap().operation =
        Operation::OfxEffect(effect(Scalar::Constant { value: 0.5 }));
    let plan = Plan::compile(&changed).unwrap();
    let half = renderer
        .render_gpu_cached(&plan, at(5), &ImageInputs::default())
        .unwrap();
    assert!((renderer.read_frame(&half).unwrap().pixels[0][0] - 0.1).abs() < 1e-6);
    // An effect downstream of another runs on the first one's picture.
    let mut chained = changed.clone();
    let second = Node::new(
        "Again",
        Operation::OfxEffect(effect(Scalar::Constant { value: 4.0 })),
        &[("source", node.id)],
    );
    chained.output = second.id;
    chained.nodes.insert(second.id, second);
    let plan = Plan::compile(&chained).unwrap();
    let twice = renderer
        .render_frame(&plan, at(5), &ImageInputs::default())
        .unwrap()
        .pixels;
    assert!((twice[0][0] - 0.4).abs() < 1e-6, "{:?}", twice[0]);
    // With no plugin chosen the source passes on.
    let mut empty = doc.clone();
    let mut none = effect(Scalar::Constant { value: 9.0 });
    none.plugin.clear();
    empty.nodes.get_mut(&node.id).unwrap().operation = Operation::OfxEffect(none);
    let plan = Plan::compile(&empty).unwrap();
    let passed = renderer
        .render_frame(&plan, at(5), &ImageInputs::default())
        .unwrap()
        .pixels;
    assert!((passed[0][0] - 0.2).abs() < 1e-6, "{:?}", passed[0]);
    // Nothing chosen and nothing connected: a clear picture, not a failure.
    let mut bare = blank(4, 3, rate).unwrap();
    let mut nothing = effect(Scalar::Constant { value: 1.0 });
    nothing.plugin.clear();
    let lone = Node::new("Nothing yet", Operation::OfxEffect(nothing), &[]);
    bare.output = lone.id;
    bare.nodes.insert(lone.id, lone);
    let plan = Plan::compile(&bare).unwrap();
    let clear = renderer
        .render_frame(&plan, at(5), &ImageInputs::default())
        .unwrap()
        .pixels;
    assert!(clear.iter().all(|p| *p == [0.0; 4]));
}
