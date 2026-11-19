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
fn render(renderer: &Renderer, doc: &Composition, inputs: &ImageInputs) -> Vec<[f32; 4]> {
    renderer
        .render_frame(
            &Plan::compile(doc).unwrap(),
            RenderRequest::full(Time::new(0, 1).unwrap()),
            inputs,
        )
        .unwrap()
        .pixels
}
fn gradient() -> (Composition, ImageInputs, Vec<[f32; 4]>, NodeId) {
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
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
    (doc, inputs, pixels, image)
}
#[test]
#[ignore = "requires Vulkan"]
fn an_untouched_card_renders_exactly_like_its_image() {
    let renderer = renderer();
    let (mut doc, inputs, pixels, image) = gradient();
    let card = add(&mut doc, op(&IMAGE_CARD), &[("image", image)]);
    let render_node = add(&mut doc, op(&RENDER_SCENE), &[("scene", card)]);
    doc.output = render_node;
    let out = render(&renderer, &doc, &inputs);
    let worst = out
        .iter()
        .zip(&pixels)
        .map(|(a, b)| (0..4).map(|c| (a[c] - b[c]).abs()).fold(0.0, f32::max))
        .fold(0.0, f32::max);
    // Half-float rendering keeps about three significant digits.
    assert!(worst < 2e-3, "worst channel difference {worst}");
    // Pulling the camera back by the home distance halves the card.
    let camera = add(&mut doc, op(&CAMERA_3D), &[]);
    if let Operation::Camera3d(c) = &mut doc.nodes.get_mut(&camera).unwrap().operation {
        c.placement.position = [0.0, 0.0, veylvik_scene::home_distance(W, 50.0, 36.0)].into();
    }
    let merged = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", card), ("scene_2", camera)],
    );
    doc.nodes.get_mut(&render_node).unwrap().inputs = [("scene".into(), merged)].into();
    let far = render(&renderer, &doc, &inputs);
    let at = |p: &[[f32; 4]], x: u32, y: u32| p[(y * W + x) as usize];
    assert_eq!(at(&far, 5, 5)[3], 0.0, "outside the shrunken card");
    let centre = at(&far, W / 2, H / 2);
    assert!(
        (centre[0] - 0.5).abs() < 0.02 && centre[3] > 0.99,
        "{centre:?}"
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn shapes_render_lit_and_solid_with_clean_edges() {
    let renderer = renderer();
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let mut cube = op(&SHAPE_3D);
    if let Operation::Shape3d(s) = &mut cube {
        s.size_x = 60.0.into();
        s.size_y = 60.0.into();
        s.size_z = 60.0.into();
        s.colour = [0.8, 0.05, 0.05].into();
        s.placement.rotation = [25.0, 35.0, 0.0].into();
    }
    let cube = add(&mut doc, cube, &[]);
    let mut light = op(&LIGHT_3D);
    if let Operation::Light3d(l) = &mut light {
        l.placement.position = [-200.0, -200.0, 400.0].into();
        l.colour = [1.0; 3].into();
    }
    let light = add(&mut doc, light, &[]);
    let merged = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", cube), ("scene_2", light)],
    );
    let render_node = add(&mut doc, op(&RENDER_SCENE), &[("scene", merged)]);
    doc.output = render_node;
    let out = render(&renderer, &doc, &Default::default());
    let at = |x: u32, y: u32| out[(y * W + x) as usize];
    let centre = at(W / 2, H / 2);
    assert!(
        centre[3] > 0.999 && centre[0] > centre[1] * 3.0,
        "red and solid: {centre:?}"
    );
    assert_eq!(at(2, 2)[3], 0.0, "empty around the cube");
    assert!(
        out.iter()
            .all(|p| p.iter().all(|v| v.is_finite()) && p[3] <= 1.0001)
    );
    // Antialiased: some edge pixels are partly covered.
    assert!(out.iter().any(|p| p[3] > 0.05 && p[3] < 0.95));
    // Different faces catch different light.
    let shades: std::collections::BTreeSet<i32> = out
        .iter()
        .filter(|p| p[3] > 0.999)
        .map(|p| (p[0] * 20.0) as i32)
        .collect();
    assert!(shades.len() >= 3, "{shades:?}");
}

#[test]
#[ignore = "requires Vulkan"]
fn render_scene_without_a_3d_renderer_says_so() {
    let renderer = Renderer::new().unwrap();
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let shape = add(&mut doc, op(&SHAPE_3D), &[]);
    let render_node = add(&mut doc, op(&RENDER_SCENE), &[("scene", shape)]);
    doc.output = render_node;
    let Err(error) = renderer.render_frame(
        &Plan::compile(&doc).unwrap(),
        RenderRequest::full(Time::new(0, 1).unwrap()),
        &Default::default(),
    ) else {
        panic!("rendered without a 3D renderer")
    };
    let error = error.to_string();
    assert!(error.contains("3D renderer"), "{error}");
}

#[test]
#[ignore = "requires Vulkan"]
fn a_3d_title_with_a_look_matches_the_2d_title_face_on() {
    let renderer = renderer();
    let mut doc = blank(W * 2, H * 2, Time::new(24, 1).unwrap()).unwrap();
    let mut text = op(&registry::TEXT);
    if let Operation::Text(t) = &mut text {
        t.content = "Hi".into();
        t.size = 120.0.into();
        t.x = (W as f32).into();
        t.y = (H as f32 + 40.0).into();
    }
    let text = add(&mut doc, text, &[]);
    let mut gold = veylvik_document::presets::material::Source::Builtin("gold_text")
        .material()
        .unwrap();
    // Flat faces: 3D has real geometry instead of the 2D bevel.
    gold.bevel = 0.0.into();
    gold.lighting = 0.0.into();
    let flat = add(
        &mut doc,
        Operation::Material(gold.clone()),
        &[("image", text)],
    );
    let mut title = op(&TEXT_3D);
    if let Operation::Text3d(t) = &mut title {
        t.depth = 20.0.into();
        // Flat faces, like the 2D title without its bevel.
        t.bevel = 0.0.into();
        t.look = Some(Box::new(gold));
    }
    let title = add(&mut doc, title, &[("outline", text)]);
    let solid = add(&mut doc, op(&RENDER_SCENE), &[("scene", title)]);
    let render_at = |doc: &mut Composition, output| {
        doc.output = output;
        render(&renderer, doc, &Default::default())
    };
    let two = render_at(&mut doc, flat);
    let three = render_at(&mut doc, solid);
    // Compare where both are fully covered, away from edges.
    let w = (W * 2) as usize;
    let mut diffs = Vec::new();
    for (i, (a, b)) in two.iter().zip(&three).enumerate() {
        let (x, y) = (i % w, i / w);
        let inside = |p: &[[f32; 4]]| {
            [(0i32, 0i32), (2, 0), (-2, 0), (0, 2), (0, -2)]
                .iter()
                .all(|(dx, dy)| {
                    let (xx, yy) = (x as i32 + dx, y as i32 + dy);
                    xx >= 0
                        && yy >= 0
                        && (xx as usize) < w
                        && p.get(yy as usize * w + xx as usize)
                            .is_some_and(|q| q[3] > 0.999)
                })
        };
        if inside(&two) && inside(&three) {
            diffs.push((0..3).map(|c| (a[c] - b[c]).abs()).sum::<f32>() / 3.0);
        }
    }
    assert!(diffs.len() > 500, "{} covered pixels", diffs.len());
    let mean = diffs.iter().sum::<f32>() / diffs.len() as f32;
    let brightness = two
        .iter()
        .filter(|p| p[3] > 0.999)
        .map(|p| (p[0] + p[1] + p[2]) / 3.0)
        .sum::<f32>()
        / two.iter().filter(|p| p[3] > 0.999).count() as f32;
    assert!(
        mean < brightness * 0.05,
        "mean difference {mean} vs brightness {brightness}"
    );
    // A turned title shows its lit side walls, still in gold (red over blue).
    if let Operation::Text3d(t) = &mut doc.nodes.get_mut(&title).unwrap().operation {
        t.placement.rotation = [0.0, 40.0, 0.0].into();
    }
    let turned = render_at(&mut doc, solid);
    let covered: Vec<_> = turned.iter().filter(|p| p[3] > 0.999).collect();
    assert!(covered.len() > 500);
    let red: f32 = covered.iter().map(|p| p[0]).sum();
    let blue: f32 = covered.iter().map(|p| p[2]).sum();
    assert!(red > blue * 1.5, "gold: {red} vs {blue}");
}

#[test]
#[ignore = "requires Vulkan"]
fn a_free_view_films_the_scene_without_changing_it() {
    let renderer = renderer();
    let (mut doc, inputs, pixels, image) = gradient();
    let card = add(&mut doc, op(&IMAGE_CARD), &[("image", image)]);
    let render_node = add(&mut doc, op(&RENDER_SCENE), &[("scene", card)]);
    doc.output = render_node;
    let plan = Plan::compile(&doc).unwrap();
    let frame = |view: Option<veylvik_scene::Camera>| {
        let frame = renderer
            .render_gpu_cached(
                &plan,
                RenderRequest {
                    scene_view: view,
                    ..RenderRequest::full(Time::new(0, 1).unwrap())
                },
                &inputs,
            )
            .unwrap();
        renderer.read_frame(&frame).unwrap().pixels
    };
    // The home free view is the scene's own view.
    let home = veylvik_scene::FreeView::home(W, H);
    let same = frame(Some(home.camera()));
    assert!(
        same.iter()
            .zip(&pixels)
            .all(|(a, b)| (a[0] - b[0]).abs() < 2e-3)
    );
    // Orbiting shows the card at an angle; the cached plain frame stays as it was.
    let mut turned = home;
    turned.orbit(40.0, 0.0);
    let angled = frame(Some(turned.camera()));
    let differing = angled
        .iter()
        .zip(&pixels)
        .filter(|(a, b)| (a[3] - b[3]).abs() > 0.5)
        .count();
    assert!(differing > 500, "{differing}");
    let plain = frame(None);
    assert!(
        plain
            .iter()
            .zip(&pixels)
            .all(|(a, b)| (a[0] - b[0]).abs() < 2e-3)
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn the_preview_scheduler_shows_the_free_view_not_a_cached_frame() {
    let renderer = Arc::new(renderer());
    let scheduler =
        veylvik_preview::GpuPreviewScheduler::with_renderer(renderer.clone(), || {}).unwrap();
    let (mut doc, inputs, _, image) = gradient();
    let card = add(&mut doc, op(&IMAGE_CARD), &[("image", image)]);
    let render_node = add(&mut doc, op(&RENDER_SCENE), &[("scene", card)]);
    doc.output = render_node;
    let plan = Arc::new(Plan::compile(&doc).unwrap());
    let show = |view: Option<veylvik_scene::Camera>| {
        let ticket = scheduler
            .submit(
                1,
                plan.clone(),
                RenderRequest {
                    scene_view: view,
                    ..RenderRequest::full(Time::new(0, 1).unwrap())
                },
                inputs.clone(),
            )
            .unwrap();
        loop {
            if let Some(result) = scheduler.try_take().unwrap()
                && result.ticket == ticket
            {
                return renderer.read_frame(&result.frame.unwrap()).unwrap().pixels;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    let plain = show(None);
    let mut turned = veylvik_scene::FreeView::home(W, H);
    turned.orbit(45.0, 0.0);
    let orbited = show(Some(turned.camera()));
    let changed = plain
        .iter()
        .zip(&orbited)
        .filter(|(a, b)| (a[3] - b[3]).abs() > 0.5)
        .count();
    assert!(
        changed > 500,
        "the free view must not come from the playback cache: {changed}"
    );
    // And the plain frame is still the plain frame afterwards.
    assert_eq!(show(None), plain);
}

#[test]
#[ignore = "requires Vulkan"]
fn an_environment_lights_reflects_and_fills_the_backdrop() {
    let renderer = renderer();
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    // A panorama: the left half warm, the right half cool.
    let panorama: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            if i % W < W / 2 {
                [0.9, 0.3, 0.1, 1.0]
            } else {
                [0.1, 0.3, 0.9, 1.0]
            }
        })
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert("sky", Arc::new(Image::new(W, H, panorama).unwrap()));
    let sky = add(
        &mut doc,
        Operation::Image(nodes::Image {
            asset: "sky".into(),
        }),
        &[],
    );
    let mut environment = op(&ENVIRONMENT_3D);
    if let Operation::Environment3d(e) = &mut environment {
        e.background = true;
    }
    let environment = add(&mut doc, environment, &[("image", sky)]);
    let mut ball = op(&SHAPE_3D);
    if let Operation::Shape3d(s) = &mut ball {
        s.kind = ShapeKind::Sphere;
        s.size_x = 50.0.into();
        s.size_y = 50.0.into();
        s.size_z = 50.0.into();
        s.colour = [0.95; 3].into();
        s.metallic = 1.0.into();
        s.roughness = 0.1.into();
    }
    let ball = add(&mut doc, ball, &[]);
    let merged = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", ball), ("scene_2", environment)],
    );
    let render_node = add(&mut doc, op(&RENDER_SCENE), &[("scene", merged)]);
    doc.output = render_node;
    let at = |p: &[[f32; 4]], x: u32, y: u32| p[(y * W + x) as usize];
    let out = render(&renderer, &doc, &inputs);
    // The backdrop is opaque and shows the panorama.
    let corner = at(&out, 2, 2);
    assert!(corner[3] > 0.999, "{corner:?}");
    assert!(
        out.iter()
            .all(|p| p[3] > 0.999 && p.iter().all(|v| v.is_finite()))
    );
    // A mirror ball reflects both colours of the panorama.
    let ball_pixels: Vec<_> = (30..60)
        .flat_map(|y| (65..95).map(move |x| (x, y)))
        .map(|(x, y)| at(&out, x, y))
        .collect();
    assert!(
        ball_pixels.iter().any(|p| p[0] > p[2] * 1.5)
            && ball_pixels.iter().any(|p| p[2] > p[0] * 1.5)
    );
    // Turning the environment half a turn swaps what the backdrop shows.
    let set = |doc: &mut Composition, f: &dyn Fn(&mut nodes::Environment3d)| {
        if let Operation::Environment3d(e) = &mut doc.nodes.get_mut(&environment).unwrap().operation
        {
            f(e);
        }
    };
    set(&mut doc, &|e| e.rotation = 180.0.into());
    let turned = render(&renderer, &doc, &inputs);
    let (a, b) = (at(&out, 2, H / 2), at(&turned, 2, H / 2));
    assert!((a[0] > a[2]) != (b[0] > b[2]), "{a:?} vs {b:?}");
    // Intensity zero: no environment light on the ball, no backdrop light either.
    set(&mut doc, &|e| e.intensity = 0.0.into());
    let dark = render(&renderer, &doc, &inputs);
    assert!(
        dark.iter().all(|p| p[0] + p[1] + p[2] < 1e-3),
        "nothing lights the scene"
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn lights_cast_shadows_and_a_catcher_shows_only_them() {
    let renderer = renderer();
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let shape = |kind, size: [f32; 3], position: [f32; 3]| {
        let mut s = op(&SHAPE_3D);
        if let Operation::Shape3d(s) = &mut s {
            s.kind = kind;
            s.size_x = size[0].into();
            s.size_y = size[1].into();
            s.size_z = size[2].into();
            s.colour = [0.8; 3].into();
            s.roughness = 0.9.into();
            s.placement.position = position.into();
        }
        s
    };
    let cube = add(&mut doc, shape(ShapeKind::Cube, [20.0; 3], [0.0; 3]), &[]);
    let wall = add(
        &mut doc,
        shape(ShapeKind::Plane, [400.0, 300.0, 0.0], [0.0, 0.0, -30.0]),
        &[],
    );
    // A sun from the upper left and the front: the cube's shadow falls down and right.
    let mut sun = op(&LIGHT_3D);
    if let Operation::Light3d(l) = &mut sun {
        l.kind = LightKind::Sun;
        l.intensity = 3.0.into();
        l.colour = [1.0; 3].into();
        l.placement.rotation = [35.26, -45.0, 0.0].into();
    }
    let sun = add(&mut doc, sun, &[]);
    let merged = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", cube), ("scene_2", wall), ("scene_3", sun)],
    );
    let render_node = add(&mut doc, op(&RENDER_SCENE), &[("scene", merged)]);
    doc.output = render_node;
    let at = |p: &[[f32; 4]], x: u32, y: u32| p[(y * W + x) as usize];
    let luma = |p: [f32; 4]| p[0] * 0.2126 + p[1] * 0.7152 + p[2] * 0.0722;
    let shadowed_point = (108, 73);
    let lit_point = (30, 15);
    let out = render(&renderer, &doc, &Default::default());
    let (dark, bright) = (
        luma(at(&out, shadowed_point.0, shadowed_point.1)),
        luma(at(&out, lit_point.0, lit_point.1)),
    );
    assert!(dark < bright * 0.7, "shadow {dark} vs lit {bright}");
    // Without shadows the wall is evenly lit by the sun.
    let set_light = |doc: &mut Composition, shadows: bool| {
        if let Operation::Light3d(l) = &mut doc.nodes.get_mut(&sun).unwrap().operation {
            l.shadows = shadows;
        }
    };
    set_light(&mut doc, false);
    let open = render(&renderer, &doc, &Default::default());
    let (a, b) = (
        luma(at(&open, shadowed_point.0, shadowed_point.1)),
        luma(at(&open, lit_point.0, lit_point.1)),
    );
    assert!((a - b).abs() < b * 0.1, "no shadow: {a} vs {b}");
    // As a shadow catcher the wall shows only the shadow: black, alpha where blocked.
    set_light(&mut doc, true);
    if let Operation::Shape3d(s) = &mut doc.nodes.get_mut(&wall).unwrap().operation {
        s.shadow_catcher = true;
    }
    let caught = render(&renderer, &doc, &Default::default());
    let shade = at(&caught, shadowed_point.0, shadowed_point.1);
    assert!(shade[3] > 0.3 && shade[0] < 1e-4, "{shade:?}");
    assert!(at(&caught, lit_point.0, lit_point.1)[3] < 0.05);
}

#[test]
#[ignore = "requires Vulkan"]
fn occlusion_darkens_the_environment_light_where_surfaces_meet() {
    let renderer = renderer();
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let shape = |kind, size: [f32; 3], position: [f32; 3]| {
        let mut s = op(&SHAPE_3D);
        if let Operation::Shape3d(s) = &mut s {
            s.kind = kind;
            s.size_x = size[0].into();
            s.size_y = size[1].into();
            s.size_z = size[2].into();
            s.colour = [0.8; 3].into();
            s.roughness = 0.9.into();
            s.placement.position = position.into();
        }
        s
    };
    // A cube just in front of a wall, lit only by the studio environment.
    let cube = add(&mut doc, shape(ShapeKind::Cube, [20.0; 3], [0.0; 3]), &[]);
    let wall = add(
        &mut doc,
        shape(ShapeKind::Plane, [400.0, 300.0, 0.0], [0.0, 0.0, -12.0]),
        &[],
    );
    let merged = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", cube), ("scene_2", wall)],
    );
    let render_node = add(&mut doc, op(&RENDER_SCENE), &[("scene", merged)]);
    doc.output = render_node;
    let set = |doc: &mut Composition, amount: f32| {
        if let Operation::RenderScene(r) = &mut doc.nodes.get_mut(&render_node).unwrap().operation {
            r.occlusion = amount.into();
        }
    };
    let luma = |p: &[[f32; 4]], x: u32, y: u32| {
        let p = p[(y * W + x) as usize];
        p[0] * 0.2126 + p[1] * 0.7152 + p[2] * 0.0722
    };
    let (near, far) = ((93, 45), (20, 15));
    set(&mut doc, 0.0);
    let open = render(&renderer, &doc, &Default::default());
    set(&mut doc, 1.0);
    let shaded = render(&renderer, &doc, &Default::default());
    let (a, b) = (luma(&shaded, near.0, near.1), luma(&open, near.0, near.1));
    assert!(a < b * 0.85, "beside the cube: {a} vs {b}");
    let (a, b) = (luma(&shaded, far.0, far.1), luma(&open, far.0, far.1));
    assert!(a > b * 0.95, "open wall: {a} vs {b}");
}

fn cube(size: f32, position: [f32; 3]) -> Operation {
    let mut s = op(&SHAPE_3D);
    if let Operation::Shape3d(s) = &mut s {
        s.kind = ShapeKind::Cube;
        s.size_x = size.into();
        s.size_y = size.into();
        s.size_z = size.into();
        s.colour = [0.8; 3].into();
        s.placement.position = position.into();
    }
    s
}
/// Sum of horizontal steps along row `y` between `x0` and `x1`: how sharp the edges there are.
fn edges(p: &[[f32; 4]], y: u32, x0: u32, x1: u32) -> f32 {
    (x0..x1)
        .map(|x| {
            (p[(y * W + x + 1) as usize][3] - p[(y * W + x) as usize][3])
                .abs()
                .powi(2)
        })
        .sum()
}

#[test]
#[ignore = "requires Vulkan"]
fn depth_of_field_blurs_what_is_out_of_focus_only() {
    let renderer = renderer();
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    // A cube at the frame centre (autofocus picks it) and one far behind, to the left.
    let near = add(&mut doc, cube(16.0, [0.0; 3]), &[]);
    let far = add(&mut doc, cube(160.0, [-250.0, 0.0, -900.0]), &[]);
    let camera = add(&mut doc, op(&CAMERA_3D), &[]);
    let merged = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", near), ("scene_2", far), ("scene_3", camera)],
    );
    let mut render_op = op(&RENDER_SCENE);
    if let Operation::RenderScene(r) = &mut render_op {
        r.camera = 1.0.into();
    }
    let render_node = add(&mut doc, render_op, &[("scene", merged)]);
    doc.output = render_node;
    let sharp = render(&renderer, &doc, &Default::default());
    if let Operation::Camera3d(c) = &mut doc.nodes.get_mut(&camera).unwrap().operation {
        c.f_stop = 1.4.into();
    }
    let soft = render(&renderer, &doc, &Default::default());
    let (near_sharp, near_soft) = (edges(&sharp, 45, 60, 100), edges(&soft, 45, 60, 100));
    let (far_sharp, far_soft) = (edges(&sharp, 45, 0, 50), edges(&soft, 45, 0, 50));
    assert!(far_sharp > 0.5, "far cube edges visible: {far_sharp}");
    assert!(
        far_soft < far_sharp * 0.5,
        "far cube blurred: {far_soft} vs {far_sharp}"
    );
    assert!(
        near_soft > near_sharp * 0.7,
        "focused cube stays sharp: {near_soft} vs {near_sharp}"
    );
    // Focus on the far cube instead: it sharpens and the near one blurs.
    if let Operation::Camera3d(c) = &mut doc.nodes.get_mut(&camera).unwrap().operation {
        c.focus_on = Some(far);
    }
    let pulled = render(&renderer, &doc, &Default::default());
    let (near_pulled, far_pulled) = (edges(&pulled, 45, 60, 100), edges(&pulled, 45, 0, 50));
    assert!(
        far_pulled > far_sharp * 0.7,
        "far cube in focus: {far_pulled} vs {far_sharp}"
    );
    assert!(
        near_pulled < near_sharp * 0.5,
        "near cube blurred: {near_pulled} vs {near_sharp}"
    );
}

#[test]
#[ignore = "requires Vulkan"]
fn motion_blur_smears_a_moving_cube_across_the_shutter() {
    let renderer = renderer();
    let rate = Time::new(24, 1).unwrap();
    let mut doc = blank(W, H, rate).unwrap();
    // 20 pixels a frame to the right, at x = 0 on frame 1: the half-frame shutter
    // sweeps it 10 pixels.
    let mut moving = cube(20.0, [0.0; 3]);
    if let Operation::Shape3d(s) = &mut moving {
        let track = &mut s.placement.position;
        track
            .set_key_at(Time::at_frame(0, rate).unwrap(), [-20.0, 0.0, 0.0])
            .unwrap();
        track
            .set_key_at(Time::at_frame(2, rate).unwrap(), [20.0, 0.0, 0.0])
            .unwrap();
        track.set_interpolation(Interpolation::Linear);
    }
    let moving = add(&mut doc, moving, &[]);
    let render_node = add(&mut doc, op(&RENDER_SCENE), &[("scene", moving)]);
    doc.output = render_node;
    let frame = |doc: &Composition| {
        renderer
            .render_frame(
                &Plan::compile(doc).unwrap(),
                RenderRequest::full(Time::at_frame(1, rate).unwrap()),
                &Default::default(),
            )
            .unwrap()
            .pixels
    };
    let still = frame(&doc);
    if let Operation::RenderScene(r) = &mut doc.nodes.get_mut(&render_node).unwrap().operation {
        r.motion_blur = 180.0.into();
    }
    let blurred = frame(&doc);
    let alpha = |p: &[[f32; 4]], x: u32| p[(45 * W + x) as usize][3];
    // Just past the cube's right edge: empty when still, partly covered while moving.
    assert!(alpha(&still, 93) < 0.01);
    let edge = alpha(&blurred, 93);
    assert!(edge > 0.1 && edge < 0.9, "{edge}");
    // The middle stays covered the whole shutter.
    assert!(alpha(&blurred, 80) > 0.99);
}

#[test]
#[ignore = "requires Vulkan"]
fn a_model_renders_its_parts_with_their_textures_and_materials() {
    use veylvik_scene::{Model, Part, Texture};
    let renderer = renderer();
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    // Two flat parts facing the viewer: the left one textured red then blue, the right one plain green.
    let quad = |x0: f32, x1: f32| veylvik_mesh::Mesh {
        vertices: [
            [x0, -0.25, 0.0, 0.0, 0.0],
            [x1, -0.25, 0.0, 1.0, 0.0],
            [x1, 0.25, 0.0, 1.0, 1.0],
            [x0, 0.25, 0.0, 0.0, 1.0],
        ]
        .map(|[x, y, z, u, v]| veylvik_mesh::Vertex {
            position: [x, y, z],
            normal: [0.0, 0.0, 1.0],
            uv: [u, v],
        })
        .to_vec(),
        indices: vec![0, 2, 1, 0, 3, 2],
        cap_indices: 0,
    };
    let texture = Arc::new(Texture {
        width: 2,
        height: 1,
        pixels: vec![[1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]],
    });
    let part = |mesh, colour: [f32; 4], texture| Part {
        mesh,
        colour,
        metallic: 0.0,
        roughness: 1.0,
        texture,
    };
    let model = Model {
        parts: vec![
            part(quad(-0.5, 0.0), [1.0; 4], Some(texture)),
            part(quad(0.0, 0.5), [0.0, 1.0, 0.0, 1.0], None),
        ],
        extent: [1.0, 0.5, 0.0],
    };
    let mut inputs = ImageInputs::default();
    inputs.insert_model("/models/test.glb", Arc::new(model));
    let mut node = op(&MODEL_3D);
    if let Operation::Model3d(m) = &mut node {
        m.model = "/models/test.glb".into();
        m.size = 120.0.into();
        m.extent = [1.0, 0.5, 0.0];
    }
    let model = add(&mut doc, node, &[]);
    let render_node = add(&mut doc, op(&RENDER_SCENE), &[("scene", model)]);
    doc.output = render_node;
    let out = render(&renderer, &doc, &inputs);
    let at = |x: u32, y: u32| out[(y * W + x) as usize];
    // The model spans x 20..140 at y 45: textured red then blue on the left half, green on the right.
    let (red, blue, green) = (at(35, 45), at(65, 45), at(110, 45));
    assert!(red[0] > red[1] * 3.0 && red[0] > red[2] * 3.0, "{red:?}");
    assert!(
        blue[2] > blue[0] * 3.0 && blue[2] > blue[1] * 3.0,
        "{blue:?}"
    );
    assert!(
        green[1] > green[0] * 3.0 && green[1] > green[2] * 3.0,
        "{green:?}"
    );
    assert!(at(5, 45)[3] < 0.01, "outside the model");
    // Without the model loaded it shows nothing.
    let empty = render(&renderer, &doc, &Default::default());
    assert!(empty.iter().all(|p| p[3] == 0.0));
}
/// A gold plate facing the camera under one thin horizontal strip light; returns the
/// red channel. `finish` is (brushed, brush_angle, imperfections and ridges).
fn plate_under_a_strip(renderer: &Renderer, finish: [f32; 3], lit: bool) -> Vec<f32> {
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let mut plate = op(&SHAPE_3D);
    if let Operation::Shape3d(s) = &mut plate {
        s.kind = ShapeKind::Plane;
        s.size_x = 400.0.into();
        s.size_y = 300.0.into();
        s.colour = [1.0, 0.6, 0.2].into();
        s.metallic = 1.0.into();
        s.roughness = 0.12.into();
        s.brushed = finish[0].into();
        s.brush_angle = finish[1].into();
        s.imperfections = finish[2].into();
        s.ridges = finish[2].into();
    }
    let plate = add(&mut doc, plate, &[]);
    let mut strip = op(&LIGHT_3D);
    if let Operation::Light3d(l) = &mut strip {
        l.kind = LightKind::Area;
        l.intensity = (if lit { 4.0 } else { 0.0 }).into();
        l.colour = [1.0; 3].into();
        l.width = 600.0.into();
        l.height = 4.0.into();
        l.placement.position = [0.0, 0.0, 120.0].into();
    }
    let strip = add(&mut doc, strip, &[]);
    let merged = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", plate), ("scene_2", strip)],
    );
    doc.output = add(&mut doc, op(&RENDER_SCENE), &[("scene", merged)]);
    render(renderer, &doc, &ImageInputs::default())
        .iter()
        .map(|p| p[0])
        .collect()
}
#[test]
#[ignore = "requires Vulkan"]
fn an_area_light_reflects_as_a_band_that_brushing_and_wear_reshape() {
    let renderer = renderer();
    let at = |p: &[f32], x: u32, y: u32| p[(y * W + x) as usize];
    let dark = plate_under_a_strip(&renderer, [0.0; 3], false);
    let smooth = plate_under_a_strip(&renderer, [0.0; 3], true);
    // The strip's light alone, down the middle column: a band across the centre.
    let band = |p: &[f32], y: u32| at(p, W / 2, y) - at(&dark, W / 2, y);
    assert!(band(&smooth, H / 2) > 0.5, "{}", band(&smooth, H / 2));
    assert!(band(&smooth, H / 2) > 8.0 * band(&smooth, H / 2 - 20).max(0.0));
    assert!(band(&smooth, 4).abs() < 0.02 && band(&smooth, H - 5).abs() < 0.02);
    // How far the band reaches: rows where it keeps a tenth of its peak.
    let reach = |p: &[f32]| {
        let peak = band(p, H / 2);
        (0..H).filter(|&y| band(p, y) > 0.1 * peak).count()
    };
    // Brushing scatters light across the grain only: with the grain along the strip
    // the band widens, with the grain across it the band keeps its width.
    let along = plate_under_a_strip(&renderer, [1.0, 0.0, 0.0], true);
    let across = plate_under_a_strip(&renderer, [1.0, 90.0, 0.0], true);
    let (s, a, c) = (reach(&smooth), reach(&along), reach(&across));
    assert!(
        a.max(c) as f32 > 1.5 * s as f32,
        "smooth {s}, along {a}, across {c}"
    );
    assert!(
        a.min(c) as f32 <= 1.2 * s as f32,
        "smooth {s}, along {a}, across {c}"
    );
    // Wear makes the plate uneven: the band is no longer the same all along.
    let spread = |p: &[f32]| {
        let row: Vec<f32> = (20..W - 20).map(|x| at(p, x, H / 2 - 3)).collect();
        let mean = row.iter().sum::<f32>() / row.len() as f32;
        (row.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / row.len() as f32).sqrt() / mean
    };
    let worn = plate_under_a_strip(&renderer, [0.0, 0.0, 1.0], true);
    assert!(
        spread(&worn) > 4.0 * spread(&smooth) && spread(&worn) > 0.05,
        "worn {} smooth {}",
        spread(&worn),
        spread(&smooth)
    );
    assert!(worn.iter().all(|v| v.is_finite()));
}
#[test]
#[ignore = "requires Vulkan"]
fn an_area_light_casts_a_soft_shadow_and_lights_a_look() {
    let renderer = renderer();
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let shape = |kind, size: [f32; 3], position: [f32; 3]| {
        let mut s = op(&SHAPE_3D);
        if let Operation::Shape3d(s) = &mut s {
            s.kind = kind;
            s.size_x = size[0].into();
            s.size_y = size[1].into();
            s.size_z = size[2].into();
            s.colour = [0.8; 3].into();
            s.roughness = 0.9.into();
            s.placement.position = position.into();
        }
        s
    };
    let cube = add(&mut doc, shape(ShapeKind::Cube, [24.0; 3], [0.0; 3]), &[]);
    let wall = add(
        &mut doc,
        shape(ShapeKind::Plane, [400.0, 300.0, 0.0], [0.0, 0.0, -30.0]),
        &[],
    );
    // A panel in front of the scene, up and to the left, facing it.
    let mut panel = op(&LIGHT_3D);
    if let Operation::Light3d(l) = &mut panel {
        l.kind = LightKind::Area;
        l.width = 60.0.into();
        l.height = 60.0.into();
        l.intensity = 12.0.into();
        l.colour = [1.0; 3].into();
        l.placement.position = [-90.0, -70.0, 120.0].into();
        l.placement.rotation = [28.0, -34.0, 0.0].into();
    }
    let panel = add(&mut doc, panel, &[]);
    let merged = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", cube), ("scene_2", wall), ("scene_3", panel)],
    );
    let render_node = add(&mut doc, op(&RENDER_SCENE), &[("scene", merged)]);
    doc.output = render_node;
    let luma = |p: [f32; 4]| p[0] * 0.2126 + p[1] * 0.7152 + p[2] * 0.0722;
    let set = |doc: &mut Composition, shadows: bool| {
        if let Operation::Light3d(l) = &mut doc.nodes.get_mut(&panel).unwrap().operation {
            l.shadows = shadows;
        }
    };
    set(&mut doc, false);
    let open = render(&renderer, &doc, &Default::default());
    set(&mut doc, true);
    let shadowed = render(&renderer, &doc, &Default::default());
    // With shadows the wall is darker somewhere down and right of the cube, by a good
    // margin, and nowhere brighter; far from the cube it is unchanged.
    let mut deepest = (0.0f32, 0u32, 0u32);
    for y in 0..H {
        for x in 0..W {
            let i = (y * W + x) as usize;
            let (a, b) = (luma(open[i]), luma(shadowed[i]));
            assert!(b <= a + 1e-3, "brighter at {x},{y}: {b} vs {a}");
            if a > 0.02 && (a - b) / a > deepest.0 {
                deepest = ((a - b) / a, x, y);
            }
        }
    }
    assert!(deepest.0 > 0.3, "deepest shadow {deepest:?}");
    assert!(
        deepest.1 > W / 2 && deepest.2 > H / 2,
        "shadow falls away from the light: {deepest:?}"
    );
    let far = (8 * W + 8) as usize;
    assert!((luma(open[far]) - luma(shadowed[far])).abs() < 0.02 * luma(open[far]).max(0.01));
    // The shadow's edge is soft: between its core and the lit wall lie in-between values.
    let (_, sx, sy) = deepest;
    let row: Vec<f32> = (sx..W)
        .map(|x| {
            let i = (sy * W + x) as usize;
            (luma(open[i]) - luma(shadowed[i])) / luma(open[i]).max(1e-4)
        })
        .collect();
    assert!(
        row.iter()
            .filter(|v| **v > 0.05 && **v < deepest.0 * 0.8)
            .count()
            >= 3,
        "{row:?}"
    );
    // A surface with a library look is lit by the panel too.
    let look_of = |doc: &mut Composition, on: bool| {
        if let Operation::Light3d(l) = &mut doc.nodes.get_mut(&panel).unwrap().operation {
            l.intensity = if on { 12.0 } else { 0.0 }.into();
        }
    };
    if let Operation::Shape3d(s) = &mut doc.nodes.get_mut(&wall).unwrap().operation {
        let Operation::Material(material) = op(&MATERIAL) else {
            unreachable!()
        };
        s.look = Some(Box::new(material));
    }
    look_of(&mut doc, false);
    let dark = render(&renderer, &doc, &Default::default());
    look_of(&mut doc, true);
    let lit = render(&renderer, &doc, &Default::default());
    let total = |p: &[[f32; 4]]| p.iter().map(|p| luma(*p)).sum::<f32>();
    assert!(
        total(&lit) > total(&dark) * 1.05,
        "{} vs {}",
        total(&lit),
        total(&dark)
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn a_texture_wraps_a_shape_the_right_way_round_and_its_alpha_cuts_holes() {
    let renderer = renderer();
    // Red on the left, a see-through band in the middle, green on the right.
    let pixels: Vec<[f32; 4]> = (0..W * H)
        .map(|i| match i % W {
            0..70 => [0.5, 0.0, 0.0, 1.0],
            70..90 => [0.0; 4],
            _ => [0.0, 0.5, 0.0, 1.0],
        })
        .collect();
    let mut inputs = ImageInputs::default();
    inputs.insert("picture", Arc::new(Image::new(W, H, pixels).unwrap()));
    let shot = |kind: ShapeKind, colour: f32| {
        let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
        let image = add(
            &mut doc,
            Operation::Image(nodes::Image {
                asset: "picture".into(),
            }),
            &[],
        );
        let mut shape = op(&SHAPE_3D);
        if let Operation::Shape3d(s) = &mut shape {
            s.kind = kind;
            s.size_x = 80.0.into();
            s.size_y = 80.0.into();
            s.size_z = 80.0.into();
            s.colour = [colour; 3].into();
            s.roughness = 1.0.into();
        }
        let shape = add(&mut doc, shape, &[("texture", image)]);
        let mut light = op(&LIGHT_3D);
        if let Operation::Light3d(l) = &mut light {
            l.placement.position = [0.0, 0.0, 600.0].into();
            l.colour = [1.0; 3].into();
        }
        let light = add(&mut doc, light, &[]);
        let merged = add(
            &mut doc,
            op(&MERGE_SCENES),
            &[("scene", shape), ("scene_2", light)],
        );
        let out = add(&mut doc, op(&RENDER_SCENE), &[("scene", merged)]);
        doc.output = out;
        render(&renderer, &doc, &inputs)
    };
    let at = |p: &[[f32; 4]], x: u32| p[(45 * W + x) as usize];
    // A plane shows the picture as it is: red, a hole, green.
    let plane = shot(ShapeKind::Plane, 1.0);
    let (left, hole, right) = (at(&plane, 50), at(&plane, 80), at(&plane, 110));
    assert!(
        left[0] > 4.0 * left[1].max(1e-4) && left[3] > 0.99,
        "{left:?}"
    );
    assert!(
        right[1] > 4.0 * right[0].max(1e-4) && right[3] > 0.99,
        "{right:?}"
    );
    assert!(hole[3] < 0.01, "{hole:?}");
    // A colour above 1 brightens the picture.
    let bright = at(&shot(ShapeKind::Plane, 3.0), 50);
    assert!(bright[0] > 2.0 * left[0], "{bright:?} {left:?}");
    // Round a sphere it reads left to right from outside, its middle facing the viewer.
    let ball = shot(ShapeKind::Sphere, 1.0);
    let (left, right) = (at(&ball, 60), at(&ball, 100));
    assert!(left[0] > 4.0 * left[1].max(1e-4), "{left:?}");
    assert!(right[1] > 4.0 * right[0].max(1e-4), "{right:?}");
}
#[test]
#[ignore = "requires Vulkan"]
fn a_card_is_tinted_brightened_and_faded() {
    let renderer = renderer();
    let (mut doc, inputs, pixels, image) = gradient();
    let mut card = op(&IMAGE_CARD);
    if let Operation::ImageCard(c) = &mut card {
        c.colour = [2.0, 0.5, 1.0].into();
        c.opacity = 0.5.into();
    }
    let card = add(&mut doc, card, &[("image", image)]);
    let out_node = add(&mut doc, op(&RENDER_SCENE), &[("scene", card)]);
    doc.output = out_node;
    let out = render(&renderer, &doc, &inputs);
    let worst = out
        .iter()
        .zip(&pixels)
        .map(|(a, b)| {
            let want = [b[0] * 2.0 * 0.5, b[1] * 0.5 * 0.5, b[2] * 0.5, b[3] * 0.5];
            (0..4).map(|c| (a[c] - want[c]).abs()).fold(0.0, f32::max)
        })
        .fold(0.0, f32::max);
    assert!(worst < 2e-3, "worst channel difference {worst}");
}
#[test]
#[ignore = "requires Vulkan"]
fn an_own_size_picture_is_rendered_small_and_gives_a_card_its_shape() {
    let renderer = renderer();
    // A shader that paints its own frame: red rising across, green down, and the
    // picture's width in blue.
    let code = "fn shade(at: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(at.x / size.x, at.y / size.y, size.x / 100.0, 1.0);
}";
    let mut shader = op(&SHADER);
    if let Operation::Shader(s) = &mut shader {
        s.code = code.into();
    }
    let shot = |own: Option<(u32, u32)>, wrap: bool| {
        let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
        let mut picture = add(&mut doc, shader.clone(), &[]);
        if let Some((width, height)) = own {
            picture = add(
                &mut doc,
                Operation::OwnSize(OwnSize {
                    width,
                    height,
                    fit: Fit::Fit,
                }),
                &[("image", picture)],
            );
        }
        let thing = if wrap {
            let mut plane = op(&SHAPE_3D);
            if let Operation::Shape3d(s) = &mut plane {
                s.kind = ShapeKind::Plane;
                s.size_x = 40.0.into();
                s.size_y = 40.0.into();
                s.colour = [1.0; 3].into();
            }
            add(&mut doc, plane, &[("texture", picture)])
        } else {
            add(&mut doc, op(&IMAGE_CARD), &[("image", picture)])
        };
        let out = add(&mut doc, op(&RENDER_SCENE), &[("scene", thing)]);
        doc.output = out;
        let plan = Plan::compile(&doc).unwrap();
        renderer
            .render_gpu_temporal(
                &plan,
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &ImageInputs::default(),
                false,
                || false,
            )
            .unwrap()
            .map(|frame| renderer.read_frame(&frame).unwrap().pixels)
            .unwrap()
    };
    let at = |p: &[[f32; 4]], x: u32, y: u32| p[(y * W + x) as usize];
    // A 40 by 20 picture on an untouched card: a 40 by 20 card in the middle of the
    // 160 by 90 frame, showing the whole picture, made at its own size.
    let small = shot(Some((40, 20)), false);
    assert_eq!(at(&small, 50, 45)[3], 0.0, "left of the card");
    assert_eq!(at(&small, 80, 30)[3], 0.0, "above the card");
    let (left, right) = (at(&small, 62, 45), at(&small, 98, 45));
    assert!(left[3] > 0.99 && right[3] > 0.99, "{left:?} {right:?}");
    assert!(left[0] < 0.1 && right[0] > 0.9, "{left:?} {right:?}");
    assert!((left[2] - 0.4).abs() < 1e-3, "made 40 wide: {left:?}");
    let (top, bottom) = (at(&small, 80, 37), at(&small, 80, 53));
    assert!(top[1] < 0.2 && bottom[1] > 0.8, "{top:?} {bottom:?}");
    // Without the node the same shader fills the frame at the frame's size.
    let whole = shot(None, false);
    assert!((at(&whole, 5, 5)[2] - 1.6).abs() < 1e-3);
    // As a texture it wraps the shape whatever its size.
    let wrapped = shot(Some((16, 16)), true);
    let (left, right) = (at(&wrapped, 64, 45), at(&wrapped, 96, 45));
    assert!(left[0] < right[0] - 0.3, "{left:?} {right:?}");
}
#[test]
#[ignore = "requires Vulkan"]
fn an_own_size_picture_meets_the_frame_as_its_fit_says_wherever_an_image_goes() {
    let renderer = renderer();
    let code = "fn shade(at: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(at.x / size.x, at.y / size.y, size.x / 100.0, 1.0);
}";
    let mut shader = op(&SHADER);
    if let Operation::Shader(s) = &mut shader {
        s.code = code.into();
    }
    // The picture is 40 by 20; the frame 160 by 90.
    let shot = |fit: Fit, through: Option<&registry::NodeType>| {
        let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
        let picture = add(&mut doc, shader.clone(), &[]);
        let own = add(
            &mut doc,
            Operation::OwnSize(OwnSize {
                width: 40,
                height: 20,
                fit,
            }),
            &[("image", picture)],
        );
        doc.output = match through {
            Some(kind) if kind.type_id == "particles" => {
                // A small busy system in the middle of this small frame.
                let mut value = kind.default_operation();
                for (name, number) in [
                    ("x", 80.0),
                    ("y", 45.0),
                    ("width", 100.0),
                    ("height", 60.0),
                    ("rate", 400.0),
                    ("size", 8.0),
                    ("speed", 0.0),
                    ("gravity", 0.0),
                ] {
                    value[name] = serde_json::json!({"kind": "constant", "value": number});
                }
                add(
                    &mut doc,
                    serde_json::from_value(value).unwrap(),
                    &[("image", own)],
                )
            }
            Some(kind) => add(&mut doc, op(kind), &[("image", own)]),
            None => own,
        };
        if let Some(kind) = through
            && kind.type_id == "particles"
        {
            // The picture as a sprite instead.
            let node = doc.nodes.get_mut(&doc.output).unwrap();
            node.inputs = [("sprite".to_string(), own)].into();
        }
        doc.validate().unwrap();
        let plan = Plan::compile(&doc).unwrap();
        renderer
            .render_gpu_temporal(
                &plan,
                RenderRequest::full(Time::new(1, 1).unwrap()),
                &ImageInputs::default(),
                false,
                || false,
            )
            .unwrap()
            .map(|frame| renderer.read_frame(&frame).unwrap().pixels)
            .unwrap()
    };
    let at = |p: &[[f32; 4]], x: u32, y: u32| p[(y * W + x) as usize];
    // Viewed on its own, fitted: four times larger, 160 by 80, bars above and below.
    let fitted = shot(Fit::Fit, None);
    assert_eq!(at(&fitted, 80, 2)[3], 0.0);
    let (left, right) = (at(&fitted, 10, 45), at(&fitted, 150, 45));
    assert!(left[3] > 0.99 && (left[2] - 0.4).abs() < 1e-3, "{left:?}");
    assert!((left[0] - 10.5 / 160.0).abs() < 0.02 && (right[0] - 150.5 / 160.0).abs() < 0.02);
    // Actual size: 40 by 20 in the middle.
    let actual = shot(Fit::Actual, None);
    assert_eq!(at(&actual, 50, 45)[3], 0.0);
    assert_eq!(at(&actual, 80, 30)[3], 0.0);
    assert!(at(&actual, 80, 45)[3] > 0.99);
    // Fill covers the frame and loses the sides; stretch shows all of it.
    let fill = shot(Fit::Fill, None);
    assert!(at(&fill, 1, 1)[3] > 0.99 && at(&fill, 1, 45)[0] > 0.04);
    let stretch = shot(Fit::Stretch, None);
    let corner = at(&stretch, 158, 88);
    assert!(
        corner[3] > 0.99 && corner[0] > 0.95 && corner[1] > 0.95,
        "{corner:?}"
    );
    // An ordinary node takes it, fitted the same way.
    let blurred = shot(Fit::Fit, Some(&registry::BLUR));
    assert!(at(&blurred, 80, 45)[3] > 0.99 && at(&blurred, 80, 0)[3] < 0.2);
    // And particles take it as their sprite.
    let dots = shot(Fit::Fit, Some(&registry::PARTICLES));
    assert!(dots.iter().all(|p| p.iter().all(|v| v.is_finite())));
    let lit = |p: &[[f32; 4]]| p.iter().filter(|p| p[3] > 0.0).count();
    assert!(lit(&dots) > 0, "no particle drawn");
}
#[test]
#[ignore = "requires Vulkan"]
fn an_unblurred_backdrop_shows_its_panorama_at_full_sharpness() {
    let renderer = renderer();
    // A panorama of six-pixel squares, finer than the 1024 by 512 copy that lights
    // the scene can hold.
    let code = "fn shade(at: vec2<f32>) -> vec4<f32> {
    let cell = floor(at / 6.0);
    let on = (cell.x + cell.y) - 2.0 * floor((cell.x + cell.y) / 2.0);
    return vec4<f32>(vec3<f32>(on), 1.0);
}";
    let shot = |blur: f32| {
        let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
        let mut shader = op(&SHADER);
        if let Operation::Shader(s) = &mut shader {
            s.code = code.into();
        }
        let picture = add(&mut doc, shader, &[]);
        let sky = add(
            &mut doc,
            Operation::OwnSize(OwnSize {
                width: 8192,
                height: 4096,
                fit: Fit::Fit,
            }),
            &[("image", picture)],
        );
        let mut around = op(&ENVIRONMENT_3D);
        if let Operation::Environment3d(e) = &mut around {
            e.background = true;
            e.blur = blur.into();
        }
        let around = add(&mut doc, around, &[("image", sky)]);
        // A long lens: each square of the panorama covers several pixels.
        let mut camera = op(&CAMERA_3D);
        if let Operation::Camera3d(c) = &mut camera {
            c.focal_length = 800.0.into();
        }
        let camera = add(&mut doc, camera, &[]);
        let scene = add(
            &mut doc,
            op(&MERGE_SCENES),
            &[("scene", around), ("scene_2", camera)],
        );
        doc.output = add(&mut doc, op(&RENDER_SCENE), &[("scene", scene)]);
        let plan = Plan::compile(&doc).unwrap();
        renderer
            .render_gpu_temporal(
                &plan,
                RenderRequest::full(Time::new(0, 1).unwrap()),
                &ImageInputs::default(),
                false,
                || false,
            )
            .unwrap()
            .map(|frame| renderer.read_frame(&frame).unwrap().pixels)
            .unwrap()
    };
    // The share of pixels that are clearly one square or the other.
    let crisp = |p: &[[f32; 4]]| {
        p.iter().filter(|p| p[0] < 0.2 || p[0] > 0.8).count() as f32 / p.len() as f32
    };
    let (sharp, soft) = (shot(0.0), shot(0.05));
    assert!(crisp(&sharp) > 0.5, "sharp {}", crisp(&sharp));
    assert!(crisp(&soft) < 0.3, "blurred {}", crisp(&soft));
}
