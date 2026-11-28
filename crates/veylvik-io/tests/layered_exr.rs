//! Layered EXR export: a Render scene's depth and render passes written beside the picture under the channel names compositing programs read, with Cryptomatte's metadata and manifest naming each object by its label.
use std::path::Path;
use veylvik_document::sequence::FrameRange;
use veylvik_document::{nodes::*, *};
use veylvik_io::sequence::{ExportLayer, SequenceRequest, render_sequence};

struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("veylvik-layered-{}", NodeId::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn op(kind: &registry::NodeType) -> Operation {
    serde_json::from_value(kind.default_operation()).unwrap()
}
fn add(doc: &mut Composition, label: &str, op: Operation, inputs: &[(&str, NodeId)]) -> NodeId {
    let node = Node::new(label, op, inputs);
    let id = node.id;
    doc.nodes.insert(id, node);
    id
}
fn ball(x: f32) -> Operation {
    let mut o = op(&SHAPE_3D);
    if let Operation::Shape3d(s) = &mut o {
        s.kind = ShapeKind::Sphere;
        for size in [&mut s.size_x, &mut s.size_y, &mut s.size_z] {
            *size = 40.0.into();
        }
        s.placement.position = [x, 0.0, 0.0].into();
    }
    o
}
fn render(depth: bool, pass: ScenePass) -> Operation {
    let mut o = op(&RENDER_SCENE);
    if let Operation::RenderScene(r) = &mut o {
        r.camera = 0.0.into();
        r.depth = depth;
        r.pass = pass;
    }
    o
}
#[test]
#[ignore = "requires Vulkan"]
fn render_passes_go_into_one_exr_as_compositing_programs_name_them() {
    let mut doc = blank(96, 64, Time::new(24, 1).unwrap()).unwrap();
    let left = add(&mut doc, "Red ball", ball(-24.0), &[]);
    let right = add(&mut doc, "Blue ball", ball(24.0), &[]);
    let scene = add(
        &mut doc,
        "Scene",
        op(&MERGE_SCENES),
        &[("scene", left), ("scene_2", right)],
    );
    doc.output = add(
        &mut doc,
        "Picture",
        render(false, ScenePass::Picture),
        &[("scene", scene)],
    );
    let mut layers = Vec::new();
    for (name, depth, pass) in [
        ("depth", true, ScenePass::Picture),
        ("N", false, ScenePass::Normals),
        ("P", false, ScenePass::Position),
        ("motion", false, ScenePass::Motion),
        ("CryptoObject", false, ScenePass::Objects),
        ("CryptoObject", false, ScenePass::ObjectsMore),
    ] {
        let node = add(&mut doc, name, render(depth, pass), &[("scene", scene)]);
        layers.push(ExportLayer {
            name: name.into(),
            node,
        });
    }
    let root = Scratch::new();
    let renderer = veylvik_io::runtime::renderer().unwrap();
    render_sequence(
        &renderer,
        &doc,
        root.path(),
        &root.path().join("frames"),
        SequenceRequest {
            frames: FrameRange::new(0, 0, doc.frame_rate).unwrap(),
            format: veylvik_io::FrameFormat::Exr,
            output: doc.output,
            region: None,
            layers: &layers,
        },
        |_, _| true,
    )
    .unwrap();
    let path = root.path().join("frames/frame-000000.exr");
    // To look at it with other programs' readers.
    if let Ok(keep) = std::env::var("LAYERED_EXR_KEEP") {
        std::fs::copy(&path, keep).unwrap();
    }
    let image = exr::prelude::read_all_flat_layers_from_file(&path).unwrap();
    let layer = &image.layer_data[0];
    let mut names: Vec<String> = layer
        .channel_data
        .list
        .iter()
        .map(|c| c.name.to_string())
        .collect();
    names.sort();
    let mut wanted: Vec<String> = [
        "A",
        "B",
        "G",
        "R",
        "depth.Z",
        "N.X",
        "N.Y",
        "N.Z",
        "P.X",
        "P.Y",
        "P.Z",
        "forward.u",
        "forward.v",
        "backward.u",
        "backward.v",
        "CryptoObject00.R",
        "CryptoObject00.G",
        "CryptoObject00.B",
        "CryptoObject00.A",
        "CryptoObject01.R",
        "CryptoObject01.G",
        "CryptoObject01.B",
        "CryptoObject01.A",
    ]
    .map(String::from)
    .to_vec();
    wanted.sort();
    assert_eq!(names, wanted);
    let channel = |name: &str| -> Vec<f32> {
        let c = layer
            .channel_data
            .list
            .iter()
            .find(|c| c.name.to_string() == name)
            .unwrap();
        (0..96 * 64)
            .map(|i| c.sample_data.value_by_flat_index(i).to_f32())
            .collect()
    };
    // The red ball's middle is pixel (24, 32): its id there, covering all of it.
    let at = 32 * 96 + 24;
    let red = veylvik_render::cryptomatte::id("Red ball");
    assert_eq!(channel("CryptoObject00.R")[at].to_bits(), red);
    assert_eq!(channel("CryptoObject00.G")[at], 1.0);
    assert!(channel("depth.Z")[at] > 0.0);
    assert!(channel("N.Z")[at] > 0.9);
    // Cryptomatte's metadata: the layer's name, hash, conversion and manifest.
    let key = veylvik_render::cryptomatte::layer_key("CryptoObject");
    let text = |field: &str| -> String {
        // A one-part file has one header: the library reads it back as the layer's.
        let name = exr::prelude::Text::from(format!("cryptomatte/{key}/{field}").as_str());
        match image
            .attributes
            .other
            .get(&name)
            .or(layer.attributes.other.get(&name))
        {
            Some(exr::meta::attribute::AttributeValue::Text(t)) => t.to_string(),
            other => panic!("{field}: {other:?}"),
        }
    };
    assert_eq!(text("name"), "CryptoObject");
    assert_eq!(text("hash"), "MurmurHash3_32");
    assert_eq!(text("conversion"), "uint32_to_float32");
    let manifest: serde_json::Value = serde_json::from_str(&text("manifest")).unwrap();
    assert_eq!(manifest["Red ball"], format!("{red:08x}"));
    assert_eq!(
        manifest["Blue ball"],
        format!("{:08x}", veylvik_render::cryptomatte::id("Blue ball"))
    );
    // Veylvik reads its own layers back.
    let read = veylvik_io::import::exr_layers(Path::new(&path)).unwrap();
    assert!(read.iter().any(|l| l.name == "CryptoObject00"));
}
#[test]
#[ignore = "requires Vulkan"]
fn layers_are_refused_for_png() {
    let doc = blank(16, 16, Time::new(24, 1).unwrap()).unwrap();
    let root = Scratch::new();
    let renderer = veylvik_io::runtime::renderer().unwrap();
    let layers = [ExportLayer {
        name: "depth".into(),
        node: doc.output,
    }];
    let error = render_sequence(
        &renderer,
        &doc,
        root.path(),
        &root.path().join("frames"),
        SequenceRequest {
            frames: FrameRange::new(0, 0, doc.frame_rate).unwrap(),
            format: veylvik_io::FrameFormat::Png,
            output: doc.output,
            region: None,
            layers: &layers,
        },
        |_, _| true,
    )
    .unwrap_err();
    assert!(error.to_string().contains("only EXR"), "{error}");
}
