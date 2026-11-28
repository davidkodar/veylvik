//! FBX files through the real helper process (`veylvik fbx-read`): a Blender scene's camera frames its card as Blender set it up, and a damaged file comes back as an error while Veylvik carries on.
use std::path::Path;
use std::sync::Arc;
use veylvik_document::{nodes::*, *};
use veylvik_render::{ImageInputs, Plan, RenderRequest};

const W: u32 = 180;
const H: u32 = 90;
fn program() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_veylvik"))
}
fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../veylvik-fbx/tests/fixtures/scene.fbx")
}
fn op(kind: &registry::NodeType) -> Operation {
    serde_json::from_value(kind.default_operation()).unwrap()
}
#[test]
#[ignore = "requires Vulkan"]
fn a_blender_cameras_view_comes_through_the_helper() {
    let model = veylvik_fbx::load(program(), &fixture()).unwrap();
    assert_eq!(model.cameras.len(), 1);
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let mut node = op(&MODEL_3D);
    if let Operation::Model3d(m) = &mut node {
        m.model = "/models/scene.fbx".into();
        m.size = 500.0.into();
        m.extent = model.extent;
        m.cameras = model.cameras.clone();
        m.clips = model
            .clips
            .iter()
            .map(|c| ModelClip {
                name: c.name.clone(),
                seconds: c.seconds(),
            })
            .collect();
        m.clip = 1;
        m.clip_loop = false;
    }
    let card = Node::new("Scene", node, &[]);
    let render = Node::new("Render", op(&RENDER_SCENE), &[("scene", card.id)]);
    doc.output = render.id;
    doc.nodes.insert(card.id, card);
    doc.nodes.insert(render.id, render);
    let mut inputs = ImageInputs::default();
    inputs.insert_model("/models/scene.fbx", Arc::new(model));
    let renderer = veylvik_io::runtime::renderer().unwrap();
    // Half a second in the camera is straight in front of the 2-wide red card, 5 away,
    // through 50 mm on 36 mm: 50 pixels either side of the middle, red.
    let pixels = renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(1, 2).unwrap()),
            &inputs,
        )
        .unwrap()
        .pixels;
    let red = |x: u32, y: u32| {
        let p = pixels[(y * W + x) as usize];
        p[3] > 0.5 && p[0] > p[1] * 3.0 && p[0] > p[2] * 3.0
    };
    let row: Vec<bool> = (0..W).map(|x| red(x, H / 2)).collect();
    let first = row.iter().position(|&c| c).expect("the red card is seen") as u32;
    let last = row.iter().rposition(|&c| c).unwrap() as u32;
    assert!(
        first.abs_diff(40) <= 1 && last.abs_diff(139) <= 1,
        "{first}..{last}"
    );
    // Up is up: the card is 2 high, so 50 pixels above and below the middle too.
    assert!(red(90, 0) && red(90, H - 1));
}
#[test]
fn a_damaged_fbx_file_is_an_error_not_a_crash() {
    let dir = std::env::temp_dir().join(format!("veylvik-fbx-damaged-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("damaged.fbx");
    // A real FBX cut off halfway, and plain rubbish.
    let whole = std::fs::read(fixture()).unwrap();
    for bytes in [
        &whole[..whole.len() / 2],
        b"Kaydara FBX Binary  \0rubbish".as_slice(),
    ] {
        std::fs::write(&path, bytes).unwrap();
        let error = veylvik_fbx::load(program(), &path).unwrap_err();
        assert!(!error.is_empty());
    }
    let _ = std::fs::remove_dir_all(&dir);
}
