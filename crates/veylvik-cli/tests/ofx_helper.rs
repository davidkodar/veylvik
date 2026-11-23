//! OpenFX plugins through the real helper process (`veylvik ofx-worker`): listing, describing and rendering, and a plugin that crashes costing only the helper.
use std::path::{Path, PathBuf};
use veylvik_ofx::{FILTER, Value, Worker};

/// The test plugin from the host's tests, built into a bundle under `folder`.
fn bundle(folder: &str) -> (PathBuf, PathBuf) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(folder);
    let arch = if cfg!(target_arch = "aarch64") {
        "Linux-arm64"
    } else {
        "Linux-x86-64"
    };
    let bundle = dir.join("Plugins").join("Gain.ofx.bundle");
    let binary = bundle.join("Contents").join(arch);
    std::fs::create_dir_all(&binary).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../veylvik-interop/tests/ofx/gain.c");
    let status = std::process::Command::new("cc")
        .args(["-std=c11", "-shared", "-fPIC", "-O1", "-o"])
        .arg(binary.join("Gain.ofx"))
        .arg(source)
        .status()
        .expect("a C compiler (cc) builds the test plugin");
    assert!(status.success());
    (dir.join("Plugins"), bundle)
}
fn worker() -> Worker {
    Worker::new(Path::new(env!("CARGO_BIN_EXE_veylvik")))
}
#[test]
fn plugins_are_found_described_and_rendered_in_the_helper() {
    let (folder, bundle) = bundle("helper");
    assert_eq!(veylvik_ofx::bundles(&[folder]), vec![bundle.clone()]);
    let mut worker = worker();
    let (plugins, refused) = worker.list(&bundle).unwrap();
    assert!(refused.is_empty(), "{refused:?}");
    let plugin = &plugins[0];
    assert_eq!(
        (plugin.identifier.as_str(), plugin.context()),
        ("org.veylvik.test.gain", Some(FILTER))
    );
    let d = worker.describe(plugin, FILTER).unwrap();
    assert_eq!(
        d.inputs().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["Source"]
    );
    assert_eq!(d.params[0].name, "gain");
    let (w, h) = (16u32, 8u32);
    let source: Vec<f32> = (0..w * h * 4).map(|i| (i % 97) as f32 / 97.0).collect();
    let values = [
        ("gain".to_string(), Value::Numbers(vec![3.0])),
        ("lift".to_string(), Value::Numbers(vec![0.0])),
    ];
    let (out, _) = worker
        .render(plugin, FILTER, [w, h], 0.0, &values, &[("Source", &source)])
        .unwrap();
    assert!(
        out.iter()
            .zip(&source)
            .all(|(o, s)| (o - s * 3.0).abs() < 1e-6)
    );
    // A second render reuses the instance, with the new values.
    let values = [("gain".to_string(), Value::Numbers(vec![0.5]))];
    let (out, _) = worker
        .render(plugin, FILTER, [w, h], 1.0, &values, &[("Source", &source)])
        .unwrap();
    assert!(
        out.iter()
            .zip(&source)
            .all(|(o, s)| (o - s * 0.5).abs() < 1e-6)
    );
}
#[test]
fn a_plugin_that_crashes_costs_only_the_helper() {
    let (_, bundle) = bundle("crashing");
    let mut worker = worker();
    let plugin = worker.list(&bundle).unwrap().0.remove(0);
    let source = vec![0.5f32; 4 * 4 * 4];
    let crash = [("mode".to_string(), Value::Numbers(vec![2.0]))];
    let error = worker
        .render(&plugin, FILTER, [4, 4], 0.0, &crash, &[("Source", &source)])
        .unwrap_err();
    assert!(error.contains("exited"), "{error}");
    // The next call starts a new helper; this process is still here to ask.
    let fine = [
        ("mode".to_string(), Value::Numbers(vec![0.0])),
        ("gain".to_string(), Value::Numbers(vec![2.0])),
        ("lift".to_string(), Value::Numbers(vec![0.0])),
    ];
    let (out, _) = worker
        .render(&plugin, FILTER, [4, 4], 0.0, &fine, &[("Source", &source)])
        .unwrap();
    assert!((out[0] - 1.0).abs() < 1e-6, "{}", out[0]);
}
#[test]
#[ignore = "requires Vulkan"]
fn an_openfx_node_renders_through_the_helper_in_a_composition() {
    use veylvik_document::nodes::{OfxContext, OfxEffect, OfxSetting};
    use veylvik_document::{Node, Operation, Scalar, Time, blank};
    use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer};
    let (_, bundle) = bundle("composition");
    let renderer = Renderer::new().unwrap();
    renderer.set_effect_host(std::sync::Arc::new(veylvik_io::ofx::Runner::with_program(
        Some(PathBuf::from(env!("CARGO_BIN_EXE_veylvik"))),
    )));
    let rate = Time { ticks: 24, rate: 1 };
    let mut doc = blank(6, 4, rate).unwrap();
    let solid = doc.output;
    doc.nodes.get_mut(&solid).unwrap().operation =
        Operation::Solid(veylvik_document::nodes::Solid {
            color: [0.1, 0.2, 0.3, 1.0],
        });
    let constant = |v: f32| OfxSetting::Numbers(vec![Scalar::Constant { value: v }]);
    let effect = OfxEffect {
        bundle: bundle.to_string_lossy().into_owned(),
        plugin: "org.veylvik.test.gain".into(),
        context: OfxContext::Filter,
        clips: vec!["Source".into()],
        settings: [
            ("gain".to_string(), constant(3.0)),
            ("lift".to_string(), constant(0.0)),
        ]
        .into(),
    };
    let node = Node::new(
        "Gain",
        Operation::OfxEffect(effect.clone()),
        &[("source", solid)],
    );
    doc.output = node.id;
    doc.nodes.insert(node.id, node.clone());
    let plan = Plan::compile(&doc).unwrap();
    let at = RenderRequest::full(Time::at_frame(3, rate).unwrap());
    let pixels = renderer
        .render_frame(&plan, at, &ImageInputs::default())
        .unwrap()
        .pixels;
    for pixel in &pixels {
        for (got, want) in pixel.iter().zip([0.3, 0.6, 0.9, 3.0]) {
            assert!((got - want).abs() < 1e-5, "{pixel:?}");
        }
    }
    // A plugin that is not in the bundle is named in the error.
    let mut missing = doc.clone();
    let mut wrong = effect;
    wrong.plugin = "org.veylvik.test.absent".into();
    missing.nodes.get_mut(&node.id).unwrap().operation = Operation::OfxEffect(wrong);
    let plan = Plan::compile(&missing).unwrap();
    match renderer.render_frame(&plan, at, &ImageInputs::default()) {
        Err(e) => assert!(e.to_string().contains("org.veylvik.test.absent"), "{e}"),
        Ok(_) => panic!("rendered a plugin that is not there"),
    }
}
#[test]
fn frames_through_shared_memory_and_through_the_pipes_are_the_same() {
    let (_, bundle) = bundle("both-ways");
    let (w, h) = (37u32, 23u32);
    let source: Vec<f32> = (0..w * h * 4)
        .map(|i| ((i * 7919) % 1000) as f32 / 1000.0)
        .collect();
    let values = [
        ("gain".to_string(), Value::Numbers(vec![1.5])),
        ("lift".to_string(), Value::Numbers(vec![0.25])),
    ];
    let mut results = Vec::new();
    for mut worker in [
        worker(),
        Worker::without_shared_memory(Path::new(env!("CARGO_BIN_EXE_veylvik"))),
    ] {
        let plugin = worker.list(&bundle).unwrap().0.remove(0);
        // Twice, the second time with another size: the shared memory grows and remaps.
        let small = vec![0.5f32; 4 * 4 * 4];
        worker
            .render(&plugin, FILTER, [4, 4], 0.0, &values, &[("Source", &small)])
            .unwrap();
        results.push(
            worker
                .render(
                    &plugin,
                    FILTER,
                    [w, h],
                    0.0,
                    &values,
                    &[("Source", &source)],
                )
                .unwrap()
                .0,
        );
    }
    assert_eq!(results[0], results[1]);
    // And they are what the plugin computes: source x gain + lift x row from the bottom.
    let at = |x: u32, y: u32, k: u32| results[0][((y * w + x) * 4 + k) as usize];
    let expected = source[((5 * w + 2) * 4) as usize] * 1.5 + 0.25 * (h - 1 - 5) as f32 / h as f32;
    assert!((at(2, 5, 0) - expected).abs() < 1e-6);
}
