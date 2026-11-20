use veylvik_document::*;

#[test]
fn footage_named_by_a_hosts_key_is_read_from_the_file_beside_it() {
    let dir = std::env::temp_dir().join(format!("veylvik-fallback-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("clip.png");
    image::RgbaImage::from_pixel(4, 2, image::Rgba([200, 100, 50, 255]))
        .save(&file)
        .unwrap();
    let footage = |fallback: &str| {
        let mut doc = blank(4, 2, Time { ticks: 24, rate: 1 }).unwrap();
        let node = Node::new(
            "Clip",
            Operation::Footage(nodes::Footage {
                asset: "host-media:17".into(),
                timing: Default::default(),
                placement: Default::default(),
                colour: Default::default(),
                layer: None,
                fallback: fallback.into(),
            }),
            &[],
        );
        doc.output = node.id;
        doc.nodes.insert(node.id, node);
        doc
    };
    // With the file named beside the key, the key's picture is the file's.
    let doc = footage(file.to_str().unwrap());
    let plan = veylvik_render::Plan::compile(&doc).unwrap();
    assert_eq!(plan.fallback("host-media:17"), file.to_str());
    let inputs = veylvik_io::load_inputs(&plan, &dir, 64 * 1024 * 1024).unwrap();
    assert!(inputs.contains_key("host-media:17"));
    // Without it the key cannot be read, and the error names the key.
    let bare = footage("");
    let plan = veylvik_render::Plan::compile(&bare).unwrap();
    assert_eq!(plan.fallback("host-media:17"), None);
    let error = veylvik_io::load_inputs(&plan, &dir, 64 * 1024 * 1024)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("host-media:17"), "{error}");
    // A composition without a fallback is written as it always was; one with it keeps it.
    let json = |doc: &Composition| serde_json::to_string(doc).unwrap();
    assert!(!json(&bare).contains("fallback"));
    let back: Composition = serde_json::from_str(&json(&doc)).unwrap();
    assert!(json(&back).contains("clip.png"));
    std::fs::remove_dir_all(&dir).unwrap();
}
