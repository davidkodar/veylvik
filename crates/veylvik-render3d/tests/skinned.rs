//! Imported models with skins bend with their joints while a clip plays, in the preview and when path traced; textured model parts path trace too.
use std::sync::Arc;
use veylvik_document::{nodes::*, *};
use veylvik_mesh::animation::{Between, Channel, Clip, IDENTITY, Joint, Keyed};
use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer};
use veylvik_scene::{Model, Part};

const W: u32 = 160;
const H: u32 = 90;
fn op(kind: &registry::NodeType) -> Operation {
    serde_json::from_value(kind.default_operation()).unwrap()
}
fn renderer() -> Renderer {
    let renderer = Renderer::new().unwrap();
    renderer.set_scene_renderer(Arc::new(veylvik_render3d::Engine::new(renderer.device())));
    renderer
}
fn rest(translation: [f32; 3], parent: Option<usize>) -> Joint {
    Joint {
        parent,
        translation,
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0; 3],
    }
}
/// A flat strip from x 0 to 2 (glTF's frame, y up), its first half on a root joint and
/// its second on a child joint at x 1 that turns a right angle about z over a second,
/// so the far half swings up; textured red.
fn strip() -> Model {
    let xs = [0.0f32, 1.0, 2.0];
    let mut positions = Vec::new();
    for x in xs {
        positions.push([x, -0.1, 0.0]);
        positions.push([x, 0.1, 0.0]);
    }
    let vertices = positions
        .iter()
        .map(|p| veylvik_mesh::Vertex {
            position: [p[0] - 1.0, -p[1], p[2]],
            normal: [0.0, 0.0, 1.0],
            uv: [0.5, 0.5],
        })
        .collect();
    // On the child for the far pair, half on each for the middle pair.
    let influences = vec![
        [0, 0, 0, 0],
        [0, 0, 0, 0],
        [0, 1, 0, 0],
        [0, 1, 0, 0],
        [1, 0, 0, 0],
        [1, 0, 0, 0],
    ];
    let weights = vec![
        [1.0, 0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.5, 0.5, 0.0, 0.0],
        [0.5, 0.5, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.0],
    ];
    let mut child_bind = IDENTITY;
    child_bind[0][3] = -1.0;
    let half = std::f32::consts::FRAC_PI_4;
    Model {
        parts: vec![Part {
            mesh: veylvik_mesh::Mesh {
                vertices,
                indices: vec![0, 1, 2, 1, 3, 2, 2, 3, 4, 3, 5, 4],
                cap_indices: 0,
            },
            colour: [1.0; 4],
            metallic: 0.0,
            roughness: 1.0,
            texture: Some(Arc::new(veylvik_scene::Texture {
                width: 1,
                height: 1,
                pixels: vec![[1.0, 0.0, 0.0, 1.0]],
            })),
            joint: None,
            rest_inverse: IDENTITY,
            skin: Some(Arc::new(veylvik_mesh::Skin {
                joints: vec![0, 1],
                inverse_bind: vec![IDENTITY, child_bind],
                positions,
                normals: vec![[0.0, 0.0, 1.0]; 6],
                influences,
                weights,
            })),
            cache: None,
        }],
        extent: [1.0, 0.1, 0.0],
        joints: vec![rest([0.0; 3], None), rest([1.0, 0.0, 0.0], Some(0))],
        clips: vec![Clip {
            length: 0.0,
            name: "lift".into(),
            channels: vec![Channel {
                node: 1,
                keyed: Keyed::Rotation,
                between: Between::Linear,
                times: vec![0.0, 1.0],
                values: vec![[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, half.sin(), half.cos()]],
            }],
        }],
        centre: [1.0, 0.0, 0.0],
        largest: 2.0,
        cameras: Vec::new(),
    }
}
/// Its coverage (alpha) at `seconds`, played or resting, in `quality`.
fn filmed(renderer: &Renderer, play: bool, seconds: i64, quality: SceneQuality) -> Vec<[f32; 4]> {
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let mut node = op(&MODEL_3D);
    if let Operation::Model3d(m) = &mut node {
        m.model = "/models/strip.glb".into();
        m.size = 80.0.into();
        m.extent = [1.0, 0.1, 0.0];
        m.clips = vec![ModelClip {
            name: "lift".into(),
            seconds: 1.0,
        }];
        m.clip = u32::from(play);
    }
    let model = Node::new("Strip", node, &[]);
    let mut render = op(&RENDER_SCENE);
    if let Operation::RenderScene(r) = &mut render {
        r.camera = 0.0.into();
        r.quality = quality;
        r.samples = 16;
    }
    let render = Node::new("Render", render, &[("scene", model.id)]);
    doc.output = render.id;
    doc.nodes.insert(model.id, model);
    doc.nodes.insert(render.id, render);
    let mut inputs = ImageInputs::default();
    inputs.insert_model("/models/strip.glb", Arc::new(strip()));
    renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(seconds, 1).unwrap()),
            &inputs,
        )
        .unwrap()
        .pixels
}
#[test]
#[ignore = "requires Vulkan"]
fn a_skinned_model_bends_at_its_joint_as_its_clip_plays_in_both_qualities() {
    let renderer = renderer();
    let at = |p: &[[f32; 4]], x: u32, y: u32| p[(y * W + x) as usize];
    for quality in [SceneQuality::Preview, SceneQuality::PathTraced] {
        // At rest the strip lies across the middle, x 40 to 120.
        let flat = filmed(&renderer, false, 1, quality);
        assert!(
            at(&flat, 110, 45)[3] > 0.9,
            "{quality:?}: the far half lies flat"
        );
        assert!(at(&flat, 80, 20)[3] < 0.01, "{quality:?}");
        // A second into its clip the far half stands straight up from the middle.
        let bent = filmed(&renderer, true, 1, quality);
        assert!(
            at(&bent, 110, 45)[3] < 0.01,
            "{quality:?}: the far half has left"
        );
        assert!(
            at(&bent, 80, 20)[3] > 0.9,
            "{quality:?}: and stands above the joint"
        );
        assert!(
            at(&bent, 60, 45)[3] > 0.9,
            "{quality:?}: the near half stays"
        );
        // Its own texture colours it, path traced as well.
        let red = at(&bent, 60, 45);
        assert!(
            red[0] > red[1] * 3.0 && red[0] > red[2] * 3.0,
            "{quality:?}: {red:?}"
        );
    }
}
