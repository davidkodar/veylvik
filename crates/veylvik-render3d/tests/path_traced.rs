//! Render scene's path traced picture against its preview: the same framing, and what only
//! path tracing can show, such as one object seen in another's reflection.
use std::sync::Arc;
use veylvik_document::{nodes::*, *};
use veylvik_render::{Image, ImageInputs, Plan, RenderRequest, Renderer};

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
fn filmed(
    doc: &mut Composition,
    scene: NodeId,
    quality: SceneQuality,
    samples: u32,
    inputs: &ImageInputs,
) -> Vec<[f32; 4]> {
    let renderer = renderer();
    let mut render = op(&RENDER_SCENE);
    if let Operation::RenderScene(r) = &mut render {
        r.quality = quality;
        r.samples = samples;
    }
    doc.output = add(doc, render, &[("scene", scene)]);
    renderer
        .render_frame(
            &Plan::compile(doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            inputs,
        )
        .unwrap()
        .pixels
}
#[test]
#[ignore = "requires Vulkan"]
fn a_path_traced_card_shows_its_picture_and_its_transparency() {
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    // A gradient, half transparent in its left 20 columns.
    let pixels: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            let (x, y) = ((i % W) as f32 / W as f32, (i / W) as f32 / H as f32);
            let a = if (i % W) < 20 { 0.5 } else { 1.0 };
            [x * a, y * a, 0.3 * a, a]
        })
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "picture",
        Arc::new(Image::new(W, H, pixels.clone()).unwrap()),
    );
    let image = add(
        &mut doc,
        Operation::Image(nodes::Image {
            asset: "picture".into(),
        }),
        &[],
    );
    let card = add(&mut doc, op(&IMAGE_CARD), &[("image", image)]);
    let out = filmed(&mut doc, card, SceneQuality::PathTraced, 256, &inputs);
    // Inside, every channel as the picture's: the path tracer spreads a pixel's samples
    // over the pixel, so at the picture's edges (the frame's, and where it turns half
    // transparent) it smooths over a pixel, as anti-aliasing does.
    let mut worst = 0.0f32;
    for y in 2..H - 2 {
        for x in 23..W - 2 {
            let i = (y * W + x) as usize;
            worst = (0..4)
                .map(|k| (out[i][k] - pixels[i][k]).abs())
                .fold(worst, f32::max);
        }
    }
    assert!(
        worst < 0.02,
        "an opaque card is its picture: off by {worst}"
    );
    // Half transparent: met by half the rays, so half covered on average.
    let half: Vec<usize> = (0..(W * H) as usize)
        .filter(|i| (2..17).contains(&(*i as u32 % W)) && (2..H - 2).contains(&(*i as u32 / W)))
        .collect();
    let alpha = half.iter().map(|&i| out[i][3]).sum::<f32>() / half.len() as f32;
    assert!(
        (alpha - 0.5).abs() < 0.03,
        "half transparent where the picture is: {alpha}"
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn a_path_traced_shadow_catcher_shows_the_shadow_as_the_preview_does() {
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let ball = add(&mut doc, sphere(0.0, [0.8; 3], 0.0, 0.8, 0.0), &[]);
    let mut wall = op(&SHAPE_3D);
    if let Operation::Shape3d(s) = &mut wall {
        s.kind = ShapeKind::Plane;
        s.size_x = 600.0.into();
        s.size_y = 600.0.into();
        s.placement.position = [0.0, 0.0, -60.0].into();
        s.shadow_catcher = true;
        s.shadow_strength = 1.0.into();
    }
    let wall = add(&mut doc, wall, &[]);
    let mut lamp = op(&LIGHT_3D);
    if let Operation::Light3d(l) = &mut lamp {
        l.kind = LightKind::Point;
        l.placement.position = [150.0, 0.0, 200.0].into();
        // Bright enough to outweigh the studio's light in this small frame.
        l.intensity = 50.0.into();
        l.shadows = true;
    }
    let lamp = add(&mut doc, lamp, &[]);
    let scene = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", ball), ("scene_2", wall), ("scene_3", lamp)],
    );
    let out = filmed(
        &mut doc.clone(),
        scene,
        SceneQuality::PathTraced,
        64,
        &ImageInputs::default(),
    );
    let preview = filmed(
        &mut doc,
        scene,
        SceneQuality::Preview,
        1,
        &ImageInputs::default(),
    );
    // The lamp is to the right, so the ball's shadow falls on the wall to its left.
    let alpha = |p: &[[f32; 4]], x0: u32, x1: u32| {
        let (mut sum, mut n) = (0.0, 0);
        for y in 40..50 {
            for x in x0..x1 {
                sum += p[(y * W + x) as usize][3];
                n += 1;
            }
        }
        sum / n as f32
    };
    let (shadow, open) = (alpha(&out, 35, 50), alpha(&out, 130, 150));
    eprintln!(
        "catcher: shadow {shadow}, open {open}, preview's shadow {}",
        alpha(&preview, 35, 50)
    );
    assert!(shadow > 0.5, "the shadow shows on the catcher: {shadow}");
    assert!(open < 0.1, "the lit catcher is clear: {open}");
    let previewed = alpha(&preview, 35, 50);
    assert!(
        (shadow - previewed).abs() < 0.15,
        "as dark as the preview's: {shadow} against {previewed}"
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn a_turned_environment_lights_the_same_side_in_both_qualities() {
    // A matte sphere under the studio turned a quarter round: its brighter side must be
    // the same side whether previewed or path traced.
    let sides = |quality| {
        let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
        let ball = add(&mut doc, sphere(0.0, [0.8; 3], 0.0, 1.0, 0.0), &[]);
        let mut env = op(&ENVIRONMENT_3D);
        if let Operation::Environment3d(e) = &mut env {
            e.rotation = 90.0.into();
        }
        let env = add(&mut doc, env, &[]);
        let scene = add(
            &mut doc,
            op(&MERGE_SCENES),
            &[("scene", ball), ("scene_2", env)],
        );
        let out = filmed(&mut doc, scene, quality, 256, &ImageInputs::default());
        let side = |x0: u32, x1: u32| {
            (35..55)
                .flat_map(|y| (x0..x1).map(move |x| (y, x)))
                .map(|(y, x)| out[(y * W + x) as usize][1])
                .sum::<f32>()
        };
        (side(62, 75), side(85, 98))
    };
    let (pl, pr) = sides(SceneQuality::Preview);
    let (tl, tr) = sides(SceneQuality::PathTraced);
    eprintln!("left/right: preview {pl:.2}/{pr:.2}, path traced {tl:.2}/{tr:.2}");
    assert!(
        (pl - pr).abs() > 0.1 * (pl + pr),
        "the turned studio lights one side more: {pl} {pr}"
    );
    assert!(
        (pl > pr) == (tl > tr),
        "the same side is brighter: preview {pl}/{pr}, path traced {tl}/{tr}"
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn a_light_with_no_reflections_leaves_shiny_surfaces_and_still_lights_matte_ones() {
    // A chrome sphere and a matte one under a softbox in front of them, the studio dimmed.
    let shot = |quality, reflections: f32| {
        let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
        let chrome = add(&mut doc, sphere(-35.0, [1.0; 3], 1.0, 0.1, 0.0), &[]);
        let matte = add(&mut doc, sphere(35.0, [0.8; 3], 0.0, 1.0, 0.0), &[]);
        let mut lamp = op(&LIGHT_3D);
        if let Operation::Light3d(l) = &mut lamp {
            l.kind = LightKind::Area;
            l.width = 120.0.into();
            l.height = 60.0.into();
            l.intensity = 3.0.into();
            l.shadows = false;
            l.placement.position = [0.0, -40.0, 160.0].into();
            l.placement.rotation = [-15.0, 0.0, 0.0].into();
            l.reflections = reflections.into();
        }
        let lamp = add(&mut doc, lamp, &[]);
        let mut env = op(&ENVIRONMENT_3D);
        if let Operation::Environment3d(e) = &mut env {
            e.intensity = 0.05.into();
        }
        let env = add(&mut doc, env, &[]);
        let scene = add(
            &mut doc,
            op(&MERGE_SCENES),
            &[
                ("scene", chrome),
                ("scene_2", matte),
                ("scene_3", lamp),
                ("scene_4", env),
            ],
        );
        let out = filmed(&mut doc, scene, quality, 128, &ImageInputs::default());
        let region = |x0: u32, x1: u32| -> Vec<f32> {
            (30..60)
                .flat_map(|y| (x0..x1).map(move |x| (y, x)))
                .map(|(y, x)| out[(y * W + x) as usize][1])
                .collect()
        };
        let chrome_peak = region(25, 65).into_iter().fold(0.0f32, f32::max);
        let m = region(100, 130);
        let matte_mean = m.iter().sum::<f32>() / m.len() as f32;
        (chrome_peak, matte_mean)
    };
    for quality in [SceneQuality::Preview, SceneQuality::PathTraced] {
        let (shown, matte_shown) = shot(quality, 1.0);
        let (hidden, matte_hidden) = shot(quality, 0.0);
        eprintln!(
            "{quality:?}: chrome peak {shown:.3} -> {hidden:.3}, matte {matte_shown:.4} -> {matte_hidden:.4}"
        );
        assert!(
            hidden < 0.5 * shown,
            "{quality:?}: the softbox leaves the chrome: {shown} -> {hidden}"
        );
        assert!(
            (matte_hidden / matte_shown - 1.0).abs() < 0.05,
            "{quality:?}: the matte sphere keeps its light: {matte_shown} -> {matte_hidden}"
        );
    }
}
