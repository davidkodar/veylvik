//! A USD scene's camera comes with the model and films it as the file framed it: the model's size and the frame's size do not change what it sees, and a keyed camera moves with the model's animation.
use std::sync::Arc;
use veylvik_document::{nodes::*, *};
use veylvik_render::{ImageInputs, Plan, RenderRequest};

const W: u32 = 180;
const H: u32 = 90;
fn op(kind: &registry::NodeType) -> Operation {
    serde_json::from_value(kind.default_operation()).unwrap()
}
/// A 2 x 2 card at the origin facing +z, and a camera 5 away on +z looking at it (50 mm
/// over a 36 mm aperture), gliding from x -0.5 to x 0.5 over a second when `moving`.
fn scene(moving: bool) -> String {
    let place = if moving {
        "double3 xformOp:translate.timeSamples = { 0: (-0.5, 0, 5), 24: (0.5, 0, 5) }"
    } else {
        "double3 xformOp:translate = (0, 0, 5)"
    };
    format!(
        r#"#usda 1.0
(
    startTimeCode = 0
    endTimeCode = 24
    timeCodesPerSecond = 24
    upAxis = "Y"
)
def Mesh "Card"
{{
    int[] faceVertexCounts = [4]
    int[] faceVertexIndices = [0, 1, 2, 3]
    point3f[] points = [(-1, -1, 0), (1, -1, 0), (1, 1, 0), (-1, 1, 0)]
}}
def Camera "Shot"
{{
    float focalLength = 50
    float horizontalAperture = 36
    {place}
    uniform token[] xformOpOrder = ["xformOp:translate"]
}}
"#
    )
}
/// The card's covered columns at the middle row, filmed through the file's camera at
/// `seconds`, with the model `size` pixels.
fn columns(moving: bool, size: f32, seconds: i64) -> (u32, u32) {
    let path = std::env::temp_dir().join(format!(
        "veylvik-usd-camera-{}-{moving}.usda",
        std::process::id()
    ));
    std::fs::write(&path, scene(moving)).unwrap();
    let model = veylvik_io::model::load(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(model.cameras.len(), 1);
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let mut node = op(&MODEL_3D);
    if let Operation::Model3d(m) = &mut node {
        m.model = "/models/card.usda".into();
        m.size = size.into();
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
        m.clip = u32::from(moving);
        m.clip_loop = false;
    }
    let card = Node::new("Card", node, &[]);
    let render = Node::new("Render", op(&RENDER_SCENE), &[("scene", card.id)]);
    doc.output = render.id;
    doc.nodes.insert(card.id, card);
    doc.nodes.insert(render.id, render);
    let mut inputs = ImageInputs::default();
    inputs.insert_model("/models/card.usda", Arc::new(model));
    let renderer = veylvik_io::runtime::renderer().unwrap();
    let pixels = renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(seconds, 1).unwrap()),
            &inputs,
        )
        .unwrap()
        .pixels;
    let row: Vec<bool> = (0..W)
        .map(|x| pixels[((H / 2) * W + x) as usize][3] > 0.5)
        .collect();
    let first = row.iter().position(|&c| c).expect("the card is seen") as u32;
    let last = row.iter().rposition(|&c| c).unwrap() as u32;
    (first, last)
}
#[test]
#[ignore = "requires Vulkan"]
fn a_usd_camera_frames_the_model_as_the_file_did() {
    // The card's half width over its distance, through 50 mm on 36 mm: 0.2 x 180 x 50/36
    // = 50 pixels either side of the middle (90).
    for size in [100.0, 900.0] {
        let (first, last) = columns(false, size, 0);
        assert!(
            first.abs_diff(40) <= 1 && last.abs_diff(139) <= 1,
            "{size}: {first}..{last}"
        );
    }
    // Keyed: at the start the camera is half a unit to the left, so the card is 25 pixels
    // right of the middle (0.5 / 5 x 250); a second in, 25 pixels left.
    let (a, b) = columns(true, 300.0, 0);
    assert!(
        a.abs_diff(65) <= 1 && b.abs_diff(164) <= 1,
        "start {a}..{b}"
    );
    let (a, b) = columns(true, 300.0, 1);
    assert!(a.abs_diff(15) <= 1 && b.abs_diff(114) <= 1, "end {a}..{b}");
}
#[test]
#[ignore = "requires Vulkan"]
fn a_turned_usd_camera_sees_up_as_up() {
    // A card in the y-z plane, only above the ground (y 0 to 1), seen by a camera on +x
    // turned a quarter about y to look back along -x: the card is in the upper half.
    let text = r#"#usda 1.0
(
    upAxis = "Y"
)
def Mesh "Card"
{
    int[] faceVertexCounts = [4]
    int[] faceVertexIndices = [0, 1, 2, 3]
    point3f[] points = [(0, 0, 1), (0, 0, -1), (0, 1, -1), (0, 1, 1)]
}
def Camera "Side"
{
    float focalLength = 50
    float horizontalAperture = 36
    double3 xformOp:translate = (5, 0, 0)
    float xformOp:rotateY = 90
    uniform token[] xformOpOrder = ["xformOp:translate", "xformOp:rotateY"]
}
"#;
    let path = std::env::temp_dir().join(format!("veylvik-usd-side-{}.usda", std::process::id()));
    std::fs::write(&path, text).unwrap();
    let model = veylvik_io::model::load(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let mut node = op(&MODEL_3D);
    if let Operation::Model3d(m) = &mut node {
        m.model = "/models/side.usda".into();
        m.size = 400.0.into();
        m.extent = model.extent;
        m.cameras = model.cameras.clone();
    }
    let card = Node::new("Card", node, &[]);
    let render = Node::new("Render", op(&RENDER_SCENE), &[("scene", card.id)]);
    doc.output = render.id;
    doc.nodes.insert(card.id, card);
    doc.nodes.insert(render.id, render);
    let mut inputs = ImageInputs::default();
    inputs.insert_model("/models/side.usda", Arc::new(model));
    let renderer = veylvik_io::runtime::renderer().unwrap();
    let pixels = renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &inputs,
        )
        .unwrap()
        .pixels;
    let seen = |x: u32, y: u32| pixels[(y * W + x) as usize][3] > 0.5;
    // Above the middle: covered (1 up over 5 away is 50 pixels); below: empty.
    assert!(seen(90, 30) && !seen(90, 60), "up is up");
    // Across: 2 wide at 5 away, 50 pixels either side of the middle.
    assert!(seen(45, 40) && seen(135, 40) && !seen(35, 40) && !seen(145, 40));
}
