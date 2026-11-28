//! Volumes: haze lit by the scene's lights and shaded where objects block them, in the preview and when path traced.
use std::sync::Arc;
use veylvik_document::{nodes::*, *};
use veylvik_render::{ImageInputs, Plan, RenderRequest, Renderer};

const W: u32 = 192;
const H: u32 = 108;
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
/// Even haze filling the frame, a sun shining down through it from above, and maybe a
/// cube in it shading what is below.
fn hazy(renderer: &Renderer, quality: SceneQuality, cube: bool) -> Vec<[f32; 4]> {
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let mut haze = op(&VOLUME_3D);
    if let Operation::Volume3d(v) = &mut haze {
        v.size_x = 400.0.into();
        v.size_y = 300.0.into();
        v.size_z = 200.0.into();
        v.noise = 0.0.into();
        v.softness = 0.0.into();
        v.density = 0.5.into();
        v.forward = 0.0.into();
    }
    let haze = add(&mut doc, haze, &[]);
    let mut sun = op(&LIGHT_3D);
    if let Operation::Light3d(l) = &mut sun {
        l.kind = LightKind::Sun;
        // Pointing down (+y is down): turned about x.
        l.placement.rotation = [90.0, 0.0, 0.0].into();
        l.intensity = 10.0.into();
    }
    let sun = add(&mut doc, sun, &[]);
    let mut parts = vec![("scene", haze), ("scene_2", sun)];
    if cube {
        let mut c = op(&SHAPE_3D);
        if let Operation::Shape3d(s) = &mut c {
            for side in [&mut s.size_x, &mut s.size_y, &mut s.size_z] {
                *side = 40.0.into();
            }
            s.placement.position = [0.0, -60.0, 0.0].into();
        }
        parts.push(("scene_3", add(&mut doc, c, &[])));
    }
    let scene = add(&mut doc, op(&MERGE_SCENES), &parts);
    let mut render = op(&RENDER_SCENE);
    if let Operation::RenderScene(r) = &mut render {
        r.quality = quality;
        r.samples = 256;
    }
    doc.output = add(&mut doc, render, &[("scene", scene)]);
    let pixels = renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap()
        .pixels;
    if let Ok(dir) = std::env::var("VOLUME_SHOTS") {
        let mut ppm = format!("P6 {W} {H} 255\n").into_bytes();
        for p in &pixels {
            for c in 0..3 {
                ppm.push(((p[c] * 0.5).clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8);
            }
        }
        std::fs::write(format!("{dir}/{quality:?}-{cube}.ppm"), ppm).unwrap();
    }
    pixels
}
fn mean(p: &[[f32; 4]], x0: u32, x1: u32, y0: u32, y1: u32) -> [f32; 4] {
    let mut sum = [0.0f32; 4];
    for y in y0..y1 {
        for x in x0..x1 {
            for c in 0..4 {
                sum[c] += p[(y * W + x) as usize][c];
            }
        }
    }
    sum.map(|v| v / ((x1 - x0) * (y1 - y0)) as f32)
}
#[test]
#[ignore = "requires Vulkan"]
fn haze_glows_where_lit_and_is_dark_in_an_objects_shadow() {
    let renderer = renderer();
    let mut glow = Vec::new();
    for quality in [SceneQuality::Preview, SceneQuality::PathTraced] {
        let open = hazy(&renderer, quality, false);
        let shaded = hazy(&renderer, quality, true);
        // Below the cube (it shades the haze beneath it) and off to the side.
        let below = mean(&shaded, W / 2 - 6, W / 2 + 6, H - 20, H - 8);
        let aside = mean(&shaded, 20, 40, H - 20, H - 8);
        let clear = mean(&open, W / 2 - 6, W / 2 + 6, H - 20, H - 8);
        eprintln!(
            "{quality:?}: below the cube {below:?}, aside {aside:?}, without the cube {clear:?}"
        );
        assert!(
            clear[3] > 0.3,
            "{quality:?}: the haze covers the frame: {clear:?}"
        );
        assert!(clear[1] > 0.02, "{quality:?}: lit haze glows: {clear:?}");
        assert!(
            below[1] < 0.93 * aside[1],
            "{quality:?}: a light shaft's shadow: {below:?} {aside:?}"
        );
        glow.push(clear[1]);
    }
    // The preview's haze is as bright as the path traced one (light scattered many times
    // approximated): within a tenth.
    assert!(
        (glow[0] / glow[1] - 1.0).abs() < 0.1,
        "preview {} against path traced {}",
        glow[0],
        glow[1]
    );
}
