use veylvik_document::{color_management::OcioTransform, *};
#[test]
fn save_as_rebases_missing_configuration_and_protects_its_path() {
    let root = std::env::temp_dir().join(format!("veylvik-ocio-paths-{}", std::process::id()));
    std::fs::create_dir_all(root.join("source/config")).unwrap();
    std::fs::create_dir_all(root.join("destination")).unwrap();
    let mut doc = blank(8, 8, Time::new(24, 1).unwrap()).unwrap();
    doc.ocio.view = Some(OcioTransform {
        config: "config/missing.ocio".into(),
        ..Default::default()
    });
    let base = root.join("source");
    let target = root.join("destination/project.veylvik");
    veylvik_io::save(&doc, &base, &target).unwrap();
    let saved = Composition::load(&target).unwrap();
    assert_eq!(
        saved.ocio.view.unwrap().config,
        "../source/config/missing.ocio"
    );
    assert!(veylvik_io::protect_inputs(&doc, &base, &base.join("config/missing.ocio")).is_err());
    assert_eq!(
        doc.ocio.view.unwrap().config,
        "config/missing.ocio",
        "Save As must not mutate the live document"
    );
    std::fs::remove_dir_all(root).unwrap();
}
