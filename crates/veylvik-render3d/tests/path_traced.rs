//! Render scene's path traced picture against its preview: the same framing, and what only
//! path tracing can show, such as one object seen in another's reflection.
use std::sync::Arc;
use veylvik_document::{nodes::*, *};
use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer};

const W: u32 = 160;
const H: u32 = 90;
fn op(kind: &registry::NodeType) -> Operation {
    serde_json::from_value(kind.default_operation()).unwrap()
}
fn add(doc: &mut Composition, op: Operation, inputs: &[(&str, NodeId)]) -> NodeId {
    let node = Node::new("n", op, inputs);
    let id = node.id;
    doc.nodes.insert(id, node);
    id
}
fn renderer() -> Renderer {
    let renderer = Renderer::new().unwrap();
    renderer.set_scene_renderer(Arc::new(veylvik_render3d::Engine::new(renderer.device())));
    renderer
}
fn sphere(x: f32, colour: [f32; 3], metallic: f32, roughness: f32, emission: f32) -> Operation {
    let mut o = op(&SHAPE_3D);
    if let Operation::Shape3d(s) = &mut o {
        s.kind = ShapeKind::Sphere;
        for size in [&mut s.size_x, &mut s.size_y, &mut s.size_z] {
            *size = 50.0.into();
        }
        s.detail = 48.0.into();
        s.placement.position = [x, 0.0, 0.0].into();
        s.colour = colour.into();
        s.metallic = metallic.into();
        s.roughness = roughness.into();
        s.emission = emission.into();
    }
    o
}
/// A chrome sphere on the left and a glowing red one on the right, in the studio.
fn two_spheres(quality: SceneQuality) -> Vec<[f32; 4]> {
    let renderer = renderer();
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let chrome = add(&mut doc, sphere(-35.0, [1.0; 3], 1.0, 0.05, 0.0), &[]);
    let red = add(
        &mut doc,
        sphere(35.0, [1.0, 0.02, 0.02], 0.0, 0.8, 3.0),
        &[],
    );
    let scene = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", chrome), ("scene_2", red)],
    );
    let mut render = op(&RENDER_SCENE);
    if let Operation::RenderScene(r) = &mut render {
        r.quality = quality;
        r.samples = 64;
    }
    doc.output = add(&mut doc, render, &[("scene", scene)]);
    renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap()
        .pixels
}
#[test]
#[ignore = "requires Vulkan"]
fn a_path_traced_scene_is_framed_as_its_preview() {
    let (traced, preview) = (
        two_spheres(SceneQuality::PathTraced),
        two_spheres(SceneQuality::Preview),
    );
    let covered = |p: &[[f32; 4]]| p.iter().filter(|p| p[3] > 0.5).count();
    let differ = traced
        .iter()
        .zip(&preview)
        .filter(|(a, b)| (a[3] > 0.5) != (b[3] > 0.5))
        .count();
    assert!(
        covered(&preview) > 1000,
        "the spheres show: {}",
        covered(&preview)
    );
    // Only pixels on the outlines may fall either way.
    assert!(
        differ * 20 < covered(&preview),
        "{differ} of {} pixels framed differently",
        covered(&preview)
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn a_chrome_sphere_shows_its_red_neighbour_only_when_path_traced() {
    // The chrome sphere's side facing the red one (centre x 45 px, radius 25 px).
    let redness = |p: &[[f32; 4]]| {
        let (mut r, mut g) = (0.0f32, 0.0f32);
        for y in 40..50 {
            for x in 62..69 {
                let v = p[(y * W + x) as usize];
                r += v[0];
                g += v[1];
            }
        }
        r / g.max(1e-4)
    };
    let (traced, preview) = (
        redness(&two_spheres(SceneQuality::PathTraced)),
        redness(&two_spheres(SceneQuality::Preview)),
    );
    assert!(
        traced > 2.0,
        "the red sphere is seen in the chrome: red/green {traced}"
    );
    assert!(
        preview < 1.5,
        "the preview reflects only the studio: red/green {preview}"
    );
}
