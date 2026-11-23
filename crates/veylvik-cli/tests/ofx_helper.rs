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
