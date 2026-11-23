//! The OpenFX host against a plugin written from the standard (tests/ofx/gain.c), built here with the system C compiler into a real bundle.
use std::path::PathBuf;
use veylvik_interop::ofx::{Frame, Library, Value};

/// Builds the test plugin into `NAME.ofx.bundle` and returns the bundle.
fn bundle(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let arch = if cfg!(target_arch = "aarch64") {
        "Linux-arm64"
    } else {
        "Linux-x86-64"
    };
    let bundle = dir.join("Gain.ofx.bundle");
    let binary = bundle.join("Contents").join(arch);
    std::fs::create_dir_all(&binary).unwrap();
    let status = std::process::Command::new("cc")
        .args(["-std=c11", "-shared", "-fPIC", "-O1", "-o"])
        .arg(binary.join("Gain.ofx"))
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ofx/gain.c"))
        .status()
        .expect("a C compiler (cc) builds the test plugin");
    assert!(status.success());
    bundle
}
const FILTER: &str = "OfxImageEffectContextFilter";

#[test]
fn a_plugin_describes_itself_and_its_settings() {
    let lib = Library::open(&bundle("describe")).unwrap();
    assert_eq!(lib.len(), 1);
    let info = lib.info(0).unwrap();
    assert_eq!(info.identifier, "org.veylvik.test.gain");
    assert_eq!(
        (info.label.as_str(), info.grouping.as_str()),
        ("Test gain", "Veylvik tests")
    );
    assert_eq!(info.contexts, vec![FILTER.to_string()]);
    let d = lib.describe(0, FILTER).unwrap();
    let clips: Vec<_> = d.clips.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(clips, ["Source", "Output"]);
    let names: Vec<_> = d.params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "gain", "lift", "mode", "colour", "note", "steps", "centre", "written", "curve"
        ]
    );
    let gain = &d.params[0];
    assert_eq!(
        (gain.label.as_str(), gain.default.as_slice()),
        ("Gain", [1.0].as_slice())
    );
    assert_eq!(
        (gain.display_min.as_slice(), gain.display_max.as_slice()),
        ([0.0].as_slice(), [4.0].as_slice())
    );
    assert_eq!(d.params[2].options, ["Multiply", "Flat colour"]);
    assert_eq!(d.params[3].default, [0.25, 0.5, 0.75, 1.0]);
    assert_eq!(d.params[4].text, "hello");
    // A context the plugin does not offer is refused by the plugin, not guessed.
    assert!(lib.describe(0, "OfxImageEffectContextGenerator").is_err());
}
#[test]
fn a_plugin_renders_with_the_settings_given_and_the_right_way_up() {
    let lib = Library::open(&bundle("render")).unwrap();
    let (w, h) = (8u32, 4u32);
    // Each row of the source is different, so a picture handed over upside down shows.
    let source: Vec<f32> = (0..w * h * 4)
        .map(|i| (i % 4) as f32 * 0.1 + (i / (w * 4)) as f32 * 0.01 + 0.05)
        .collect();
    let mut instance = lib.instance(0, FILTER, w, h).unwrap();
    let values = [
        ("gain".to_string(), Value::Numbers(vec![2.0])),
        ("lift".to_string(), Value::Numbers(vec![0.4])),
    ];
    let frame = Frame {
        pixels: &source,
        width: w,
        height: h,
        time: None,
    };
    let out = instance.render(0.0, &values, &[("Source", frame)]).unwrap();
    // OpenFX counts rows from the bottom: our top row is its row h - 1, lifted most.
    let at = |x: u32, y: u32, k: u32| out[((y * w + x) * 4 + k) as usize];
    let lifted = |row_from_bottom: u32| 0.4 * row_from_bottom as f32 / h as f32;
    for k in 0..3 {
        let s = |y: u32| source[((y * w + 3) * 4 + k) as usize] * 2.0;
        assert!(
            (at(3, 0, k) - (s(0) + lifted(h - 1))).abs() < 1e-6,
            "top row"
        );
        assert!(
            (at(3, h - 1, k) - (s(h - 1) + lifted(0))).abs() < 1e-6,
            "bottom row"
        );
    }
    assert!(
        (at(0, 0, 3) - source[3] * 2.0).abs() < 1e-6,
        "alpha is not lifted"
    );
    // Settings not given keep the value they had; the text setting kept its default.
    let flat = instance
        .render(1.0, &[("mode".to_string(), Value::Numbers(vec![1.0]))], &[])
        .unwrap();
    assert_eq!(&flat[..4], &[0.25, 0.5, 0.75, 1.0]);
    let said = veylvik_interop::ofx::messages();
    assert!(
        !said.iter().any(|m| m.contains("note was not hello")),
        "the plugin read its text setting: {said:?}"
    );
}
#[test]
fn a_plugin_sets_values_of_every_kind_itself() {
    let lib = Library::open(&bundle("setting")).unwrap();
    let mut instance = lib.instance(0, FILTER, 2, 2).unwrap();
    assert_eq!(instance.value("lift"), Some(Value::Numbers(vec![0.125])));
    assert_eq!(instance.value("steps"), Some(Value::Numbers(vec![7.0])));
    assert_eq!(
        instance.value("centre"),
        Some(Value::Numbers(vec![1.5, -2.5]))
    );
    assert_eq!(
        instance.value("written"),
        Some(Value::Text("by the plugin".into()))
    );
    // And the value it set is the one it renders with.
    let one = vec![1.0f32; 16];
    let frame = Frame {
        pixels: &one,
        width: 2,
        height: 2,
        time: None,
    };
    let out = instance.render(0.0, &[], &[("Source", frame)]).unwrap();
    assert!((out[0] - (1.0 + 0.125 * 0.5)).abs() < 1e-6, "{}", out[0]);
}
#[test]
fn a_curve_setting_keeps_its_default_points_and_renders_through_the_hosts_curve() {
    let lib = Library::open(&bundle("curves")).unwrap();
    let d = lib.describe(0, FILTER).unwrap();
    let curve = d.params.iter().find(|p| p.name == "curve").unwrap();
    assert_eq!(curve.curves, vec![vec![[0.0, 0.0], [1.0, 0.5]]]);
    assert_eq!(
        (curve.range, curve.curve_labels.clone()),
        ([0.0, 1.0], vec!["master".to_string()])
    );
    let source: Vec<f32> = (0..16).map(|i| i as f32 / 15.0).collect();
    let frame = || Frame {
        pixels: &source,
        width: 2,
        height: 2,
        time: None,
    };
    let mut instance = lib.instance(0, FILTER, 2, 2).unwrap();
    let mode = ("mode".to_string(), Value::Numbers(vec![3.0]));
    // The default curve halves the source (a straight line through its two points).
    let out = instance
        .render(0.0, std::slice::from_ref(&mode), &[("Source", frame())])
        .unwrap();
    // OpenFX rows are bottom-up but this source is per-row symmetric only by value; compare sorted.
    let mut got: Vec<f32> = out.clone();
    let mut want: Vec<f32> = source.iter().map(|v| v * 0.5).collect();
    got.sort_by(f32::total_cmp);
    want.sort_by(f32::total_cmp);
    assert!(
        got.iter().zip(&want).all(|(a, b)| (a - b).abs() < 1e-6),
        "{got:?}"
    );
    // Curves given for a render replace the default.
    let inverted = (
        "curve".to_string(),
        Value::Curves(vec![vec![[0.0, 1.0], [1.0, 0.0]]]),
    );
    let out = instance
        .render(0.0, &[mode, inverted], &[("Source", frame())])
        .unwrap();
    let mut got: Vec<f32> = out;
    let mut want: Vec<f32> = source.iter().map(|v| 1.0 - v).collect();
    got.sort_by(f32::total_cmp);
    want.sort_by(f32::total_cmp);
    assert!(
        got.iter().zip(&want).all(|(a, b)| (a - b).abs() < 1e-6),
        "{got:?}"
    );
}
#[test]
fn a_failing_render_says_what_the_plugin_said() {
    let lib = Library::open(&bundle("failing")).unwrap();
    let mut instance = lib.instance(0, FILTER, 4, 4).unwrap();
    let error = instance.render(0.0, &[], &[]).unwrap_err();
    assert!(
        error.contains("no source, as the support library says it"),
        "{error}"
    );
    assert!(
        instance
            .render(0.0, &[("nope".to_string(), Value::Numbers(vec![1.0]))], &[])
            .is_err()
    );
}
#[test]
fn what_is_not_a_plugin_is_refused() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("not-a-plugin");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("empty.ofx");
    std::fs::write(&file, b"not a library").unwrap();
    assert!(Library::open(&file).is_err());
    assert!(Library::open(&dir.join("Missing.ofx.bundle")).is_err());
}
#[test]
fn a_plugin_names_the_other_frames_it_needs_and_renders_from_them() {
    let lib = Library::open(&bundle("other-frames")).unwrap();
    let mut instance = lib.instance(0, FILTER, 2, 1).unwrap();
    instance.set_duration(48);
    let mode = |m: f64| vec![("mode".to_string(), Value::Numbers(vec![m]))];
    // Without the previous-frame mode only its own frame is needed.
    assert_eq!(
        instance
            .frames_needed(10.0, &mode(0.0), &["Source"])
            .unwrap(),
        vec![("Source".to_string(), vec![10.0])]
    );
    assert_eq!(
        instance
            .frames_needed(10.0, &mode(4.0), &["Source"])
            .unwrap(),
        vec![("Source".to_string(), vec![9.0])]
    );
    // Rendering frame 10 it reads the picture of frame 9, whatever frame 10 holds.
    let now = vec![0.25f32; 8];
    let before = vec![0.75f32; 8];
    let frames = [
        (
            "Source",
            Frame {
                pixels: &now,
                width: 2,
                height: 1,
                time: None,
            },
        ),
        (
            "Source",
            Frame {
                pixels: &before,
                width: 2,
                height: 1,
                time: Some(9.0),
            },
        ),
    ];
    let out = instance.render(10.0, &mode(4.0), &frames).unwrap();
    assert_eq!(out, before);
    // A frame it needs but was not given fails, with the plugin's message.
    let only_now = [(
        "Source",
        Frame {
            pixels: &now,
            width: 2,
            height: 1,
            time: None,
        },
    )];
    assert!(instance.render(10.0, &mode(4.0), &only_now).is_err());
}
