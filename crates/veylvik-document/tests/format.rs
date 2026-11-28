//! The project format's promise, checked against files Veylvik 0.1.0 saved: every one of them must open in every later Veylvik, as it was, through the format's own upgrade steps. These files are never changed or regenerated; when the format changes, a step is added to `format.rs` and new files are added beside them for the new version.
use std::path::{Path, PathBuf};
use veylvik_document::{Composition, Operation, format, registry};

fn folder(version: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/format")
        .join(version)
}
/// Every project saved by `version` opens, is valid and saves again in the current format.
fn projects_open(version: &str) {
    let mut seen = 0;
    for entry in std::fs::read_dir(folder(version)).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "veylvik") {
            continue;
        }
        let composition = format::read(&std::fs::read(&path).unwrap())
            .unwrap_or_else(|e| panic!("{}: {}", path.display(), e.0));
        let again = format::read(&format::write(&composition).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(&again).unwrap(),
            serde_json::to_value(&composition).unwrap(),
            "{}",
            path.display()
        );
        seen += 1;
    }
    assert!(seen > 0, "no projects saved by {version}");
}
/// Every node type `version` had, as it saved it, reads into a valid node now.
fn nodes_open(version: &str) {
    let text = std::fs::read_to_string(folder(version).join("nodes.json")).unwrap();
    let nodes: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&text).unwrap();
    assert!(!nodes.is_empty());
    for (kind, saved) in nodes {
        let mut operation = saved["operation"].clone();
        let from = saved["version"].as_u64().unwrap() as u32;
        format::upgrade_operation(&kind, from, &mut operation)
            .unwrap_or_else(|e| panic!("{kind}: {}", e.0));
        let operation: Operation =
            serde_json::from_value(operation).unwrap_or_else(|e| panic!("{kind}: {e}"));
        operation
            .validate()
            .unwrap_or_else(|e| panic!("{kind}: {}", e.0));
    }
}
#[test]
fn projects_saved_by_0_1_0_open() {
    projects_open("0.1.0");
}
#[test]
fn every_node_type_0_1_0_saved_opens() {
    nodes_open("0.1.0");
}
/// Writes this version's files (once, when the version is released; never again for a
/// version that is out): `FORMAT_FIXTURES=1 cargo test -p veylvik-document --test format
/// -- --ignored write_this_versions_files`.
#[test]
#[ignore = "writes the files a release keeps; run once per release"]
fn write_this_versions_files() {
    if std::env::var("FORMAT_FIXTURES").is_err() {
        return;
    }
    let dir = folder(format::WRITER);
    std::fs::create_dir_all(&dir).unwrap();
    let mut nodes = serde_json::Map::new();
    for kind in registry::TYPES {
        nodes.insert(
            kind.type_id.to_owned(),
            serde_json::json!({"version": kind.version, "operation": kind.default_operation()}),
        );
    }
    std::fs::write(
        dir.join("nodes.json"),
        serde_json::to_vec_pretty(&nodes).unwrap(),
    )
    .unwrap();
    let save = |name: &str, composition: &Composition| {
        std::fs::write(
            dir.join(format!("{name}.veylvik")),
            format::write(composition).unwrap(),
        )
        .unwrap();
    };
    for preset in veylvik_document::presets::builtin::ALL {
        let scene = veylvik_presets::showcase::build(preset.id).unwrap();
        save(preset.id, &scene.composition);
    }
    let blank =
        veylvik_document::blank(1920, 1080, veylvik_document::Time::new(24, 1).unwrap()).unwrap();
    let built = veylvik_presets::gold_title(&blank, "VEYLVIK", "THE NODE COMPOSITOR").unwrap();
    let mut doc = veylvik_document::Document::new(blank).unwrap();
    doc.apply(0, &built.commands).unwrap();
    save("gold_title", doc.snapshot());
    save("demo", &veylvik_document::demo());
}
