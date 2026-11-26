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
                    scene_reflections: false,
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
                    scene_reflections: false,
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
#[test]
#[ignore = "requires Vulkan"]
fn particles_in_a_scene_can_stay_points_and_stay_in_focus_and_live_long() {
    let renderer = renderer();
    // Dots close to the camera, which is focused far behind them with a wide lens.
    let shot = |points: bool, in_focus: bool, f_stop: f32| {
        let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
        let mut camera = op(&CAMERA_3D);
        if let Operation::Camera3d(c) = &mut camera {
            c.f_stop = f_stop.into();
            c.focus_distance = 2000.0.into();
        }
        let camera = add(&mut doc, camera, &[]);
        let mut value = registry::PARTICLES.default_operation();
        for (name, number) in [
            ("x", 80.0),
            ("y", 45.0),
            ("width", 60.0),
            ("height", 30.0),
            ("z", 150.0),
            ("rate", 0.2),
            ("life", 120.0),
            ("start", -120.0),
            ("size", 1.5),
            ("speed", 0.0),
            ("gravity", 0.0),
            ("variation", 0.0),
            ("fade", 0.0),
        ] {
            value[name] = serde_json::json!({"kind": "constant", "value": number});
        }
        value["points"] = points.into();
        value["in_focus"] = in_focus.into();
        doc.output = add(
            &mut doc,
            serde_json::from_value(value).unwrap(),
            &[("scene", camera)],
        );
        doc.validate().unwrap();
        let plan = Plan::compile(&doc).unwrap();
        let pixels = renderer
            .render_gpu_temporal(
                &plan,
                RenderRequest::full(Time::new(1, 1).unwrap()),
                &ImageInputs::default(),
                false,
                || false,
            )
            .unwrap()
            .map(|frame| renderer.read_frame(&frame).unwrap().pixels)
            .unwrap();
        pixels.iter().filter(|p| p[3] > 0.02).count()
    };
    // Twenty-four particles, each born once in two minutes and still there.
    let (grown, points) = (shot(false, false, 0.0), shot(true, false, 0.0));
    assert!(grown > 0 && points > 0, "{grown} {points}");
    assert!(
        points * 3 < grown * 2,
        "near dots grow {grown}, points do not {points}"
    );
    let (blurred, sharp) = (shot(true, false, 1.4), shot(true, true, 1.4));
    assert_eq!(sharp, points, "in focus, the lens changes nothing");
    assert!(blurred > sharp * 2, "blurred {blurred}, sharp {sharp}");
}
#[test]
#[ignore = "requires Vulkan"]
fn in_a_depth_picture_a_cards_solid_parts_are_an_object() {
    let renderer = renderer();
    // The gradient picture: half see-through in its first twenty columns, solid after.
    let (mut doc, inputs, _, image) = gradient();
    let mut card = op(&IMAGE_CARD);
    if let Operation::ImageCard(c) = &mut card {
        c.placement.position = [0.0, 0.0, -100.0].into();
        c.opacity = 0.4.into();
    }
    let faint = add(&mut doc, card, &[("image", image)]);
    // The near card at nine tenths: solid where the picture is, under half where it is not.
    let mut near = op(&IMAGE_CARD);
    if let Operation::ImageCard(c) = &mut near {
        c.opacity = 0.9.into();
    }
    let solid = add(&mut doc, near, &[("image", image)]);
    let scene = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", faint), ("scene_2", solid)],
    );
    let mut depth = op(&RENDER_SCENE);
    if let Operation::RenderScene(r) = &mut depth {
        r.depth = true;
    }
    doc.output = add(&mut doc, depth, &[("scene", scene)]);
    let out = render(&renderer, &doc, &inputs);
    let at = |x: u32| out[(45 * W + x) as usize];
    // Where the near card is solid, the distance is its own: the frame's home distance.
    let home = veylvik_scene::home_distance(W, 50.0, 36.0);
    let middle = at(100);
    assert!(
        middle[3] > 0.99 && (middle[0] - home).abs() < 1.0,
        "{middle:?} {home}"
    );
    // Where both cards are less than half there, nothing stands in the way.
    assert_eq!(at(10)[3], 0.0, "{:?}", at(10));
}
#[test]
#[ignore = "requires Vulkan"]
fn a_shader_is_told_where_a_scene_object_is_in_the_frame() {
    let renderer = renderer();
    let code = "fn shade(at: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(follow.x, follow.y, followed, 1.0);
}";
    let shot = |position: [f32; 3], followed: bool, wired: bool| {
        let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
        let mut cube = cube(10.0, position);
        if let Operation::Shape3d(s) = &mut cube {
            s.placement.position = position.into();
        }
        let cube = add(&mut doc, cube, &[]);
        let camera = add(&mut doc, op(&CAMERA_3D), &[]);
        let scene = add(
            &mut doc,
            op(&MERGE_SCENES),
            &[("scene", cube), ("scene_2", camera)],
        );
        let mut shader = op(&SHADER);
        if let Operation::Shader(s) = &mut shader {
            s.code = code.into();
            s.follow = followed.then_some(cube);
        }
        let inputs: &[(&str, NodeId)] = if wired { &[("scene", scene)] } else { &[] };
        doc.output = add(&mut doc, shader, inputs);
        doc.validate().unwrap();
        render(&renderer, &doc, &ImageInputs::default())[0]
    };
    // The untouched camera sees the frame as it is: 40 right and 20 up of its middle.
    let seen = shot([40.0, -20.0, 0.0], true, true);
    assert!(
        (seen[0] - 120.0).abs() < 0.01 && (seen[1] - 25.0).abs() < 0.01 && seen[2] == 1.0,
        "{seen:?}"
    );
    // Behind the camera, and with no scene to look in: not seen.
    assert_eq!(shot([0.0, 0.0, 5000.0], true, true)[2], -1.0);
    assert_eq!(shot([0.0, 0.0, 0.0], true, false)[2], -1.0);
    // Nothing followed.
    assert_eq!(shot([0.0, 0.0, 0.0], false, true)[2], 0.0);
}
#[test]
#[ignore = "requires Vulkan"]
fn a_followed_object_is_seen_less_as_another_slides_in_front() {
    let renderer = renderer();
    let code = "fn shade(at: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(follow_seen, 0.0, 0.0, 1.0);
}";
    // A wall 100 in front of the followed cube, its left edge at `edge`.
    let shot = |edge: Option<f32>| {
        let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
        let target = add(&mut doc, cube(40.0, [0.0; 3]), &[]);
        let mut parts = vec![("scene".to_string(), target)];
        if let Some(edge) = edge {
            let at = [edge + 100.0, 0.0, 100.0];
            let mut wall = cube(200.0, at);
            if let Operation::Shape3d(s) = &mut wall {
                s.placement.position = at.into();
            }
            parts.push(("scene_2".into(), add(&mut doc, wall, &[])));
        }
        let parts: Vec<(&str, NodeId)> = parts.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        let scene = add(&mut doc, op(&MERGE_SCENES), &parts);
        let mut shader = op(&SHADER);
        if let Operation::Shader(s) = &mut shader {
            s.code = code.into();
            s.follow = Some(target);
        }
        doc.output = add(&mut doc, shader, &[("scene", scene)]);
        doc.validate().unwrap();
        render(&renderer, &doc, &ImageInputs::default())[0][0]
    };
    assert_eq!(shot(None), 1.0);
    assert_eq!(shot(Some(-100.0)), 0.0);
    let half = shot(Some(3.0));
    assert!(half > 0.2 && half < 0.9, "{half}");
    assert_eq!(shot(Some(60.0)), 1.0);
}
#[test]
#[ignore = "requires Vulkan"]
fn an_image_card_in_front_hides_a_followed_object_unless_faded_out() {
    let renderer = renderer();
    let code = "fn shade(at: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(follow_seen, 0.0, 0.0, 1.0);
}";
    let shot = |opacity: f32, in_front: bool| {
        let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
        let target = add(&mut doc, cube(20.0, [0.0; 3]), &[]);
        let image = add(&mut doc, op(&SOLID), &[]);
        let mut card = op(&IMAGE_CARD);
        if let Operation::ImageCard(c) = &mut card {
            // The card is the frame's size at z 0; move it towards or away from the camera.
            c.placement.position = [0.0, 0.0, if in_front { 100.0 } else { -100.0 }].into();
            c.opacity = opacity.into();
        }
        let card = add(&mut doc, card, &[("image", image)]);
        let scene = add(
            &mut doc,
            op(&MERGE_SCENES),
            &[("scene", target), ("scene_2", card)],
        );
        let mut shader = op(&SHADER);
        if let Operation::Shader(s) = &mut shader {
            s.code = code.into();
            s.follow = Some(target);
        }
        doc.output = add(&mut doc, shader, &[("scene", scene)]);
        doc.validate().unwrap();
        render(&renderer, &doc, &ImageInputs::default())[0][0]
    };
    // In front it hides by its rectangle; behind it does not; nearly faded out it does not.
    assert_eq!(shot(1.0, true), 0.0);
    assert_eq!(shot(1.0, false), 1.0);
    assert_eq!(shot(0.2, true), 1.0);
}
#[test]
#[ignore = "requires Vulkan"]
fn the_reflections_view_paints_each_lights_reflection_in_its_own_colour() {
    let renderer = renderer();
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    // A red matte cube: in the picture it is red; as a map of reflections it is not.
    let mut solid = cube(60.0, [0.0; 3]);
    if let Operation::Shape3d(s) = &mut solid {
        s.colour = [0.8, 0.05, 0.05].into();
        s.placement.rotation = [25.0, 35.0, 0.0].into();
    }
    let solid = add(&mut doc, solid, &[]);
    let mut light = op(&LIGHT_3D);
    if let Operation::Light3d(l) = &mut light {
        l.placement.position = [-200.0, -200.0, 400.0].into();
        l.colour = [1.0; 3].into();
        // A point light falls off over pixels of a 1920-wide frame: in this tiny frame
        // its 450 pixels are as far as 5400 would be there, so it is made as strong as
        // it must be to light the cube plainly.
        l.intensity = (1920.0 / W as f32).powi(2).into();
    }
    let light = add(&mut doc, light, &[]);
    let merged = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", solid), ("scene_2", light)],
    );
    doc.output = add(&mut doc, op(&RENDER_SCENE), &[("scene", merged)]);
    let plan = Plan::compile(&doc).unwrap();
    let shot = |reflections: bool| {
        renderer
            .render_frame(
                &plan,
                RenderRequest {
                    scene_reflections: reflections,
                    ..RenderRequest::full(Time::new(0, 1).unwrap())
                },
                &ImageInputs::default(),
            )
            .unwrap()
            .pixels
    };
    let (picture, map) = (shot(false), shot(true));
    let middle = ((H / 2) * W + W / 2) as usize;
    assert!(
        picture[middle][0] > picture[middle][1] * 3.0,
        "{:?}",
        picture[middle]
    );
    // The first light's colour is the first hue, not the cube's red and not the light's white.
    let hue = veylvik_scene::light_hue(0);
    let lit = map
        .iter()
        .filter(|p| p[3] > 0.5)
        .max_by(|a, b| (a[0] + a[1] + a[2]).total_cmp(&(b[0] + b[1] + b[2])))
        .unwrap();
    let strength = lit[1] / hue[1];
    assert!(strength > 0.05, "{lit:?}");
    for c in 0..3 {
        assert!(
            (lit[c] - hue[c] * strength).abs() < 0.03 * strength.max(1.0),
            "{lit:?} {hue:?}"
        );
    }
    // Nothing outside the cube changed, and the ordinary picture is as before.
    assert_eq!(map[0], picture[0]);
    assert_eq!(shot(false), picture);
}

#[test]
#[ignore = "requires Vulkan"]
fn a_picture_and_its_depth_from_one_render_are_the_two_renders_exactly() {
    // Two see-through cards and a cube, filmed by a picture node and a depth node, and
    // both shown at once: the picture over its depth, and the other way round.
    let film = |picture_over_depth: bool, shared: bool| {
        let mut renderer = renderer();
        renderer.share_scene_renders = shared;
        let (mut doc, inputs, _, image) = gradient();
        let mut far = op(&IMAGE_CARD);
        if let Operation::ImageCard(c) = &mut far {
            c.placement.position = [0.0, 0.0, -100.0].into();
            c.opacity = 0.4.into();
        }
        let far = add(&mut doc, far, &[("image", image)]);
        let mut near = op(&IMAGE_CARD);
        if let Operation::ImageCard(c) = &mut near {
            c.opacity = 0.9.into();
        }
        let near = add(&mut doc, near, &[("image", image)]);
        let block = add(&mut doc, cube(30.0, [60.0, 10.0, 40.0]), &[]);
        let cards = add(
            &mut doc,
            op(&MERGE_SCENES),
            &[("scene", far), ("scene_2", near)],
        );
        let scene = add(
            &mut doc,
            op(&MERGE_SCENES),
            &[("scene", cards), ("scene_2", block)],
        );
        let picture = add(&mut doc, op(&RENDER_SCENE), &[("scene", scene)]);
        let mut depth = op(&RENDER_SCENE);
        if let Operation::RenderScene(r) = &mut depth {
            r.depth = true;
        }
        let depth = add(&mut doc, depth, &[("scene", scene)]);
        let (under, over) = if picture_over_depth {
            (depth, picture)
        } else {
            (picture, depth)
        };
        doc.output = add(
            &mut doc,
            op(registry::TYPES
                .iter()
                .find(|t| t.type_id == "merge")
                .unwrap()),
            &[("background", under), ("foreground", over)],
        );
        render(&renderer, &doc, &inputs)
    };
    assert!(
        film(true, false) != film(false, false),
        "the two orders show the same"
    );
    for picture_over_depth in [true, false] {
        let (apart, together) = (
            film(picture_over_depth, false),
            film(picture_over_depth, true),
        );
        // There is a picture at all, and the two renders that make it are told apart.
        assert!(apart.iter().any(|p| p[3] > 0.0));
        assert!(
            apart
                .iter()
                .zip(&together)
                .all(|(a, b)| a.map(f32::to_bits) == b.map(f32::to_bits)),
            "one render for both differs from two (picture over depth: {picture_over_depth})"
        );
    }
}
#[test]
#[ignore = "requires Vulkan"]
fn a_solid_between_a_light_and_particles_shadows_them() {
    let renderer = renderer();
    // A still particle at the frame's centre, lit wholly by a sun shining from the
    // camera's side into the scene, with no light everywhere; and maybe a cube.
    let shot = |cube_at: Option<[f32; 3]>| {
        let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
        let camera = add(&mut doc, op(&CAMERA_3D), &[]);
        let mut light = op(&LIGHT_3D);
        if let Operation::Light3d(l) = &mut light {
            l.kind = LightKind::Sun;
            l.colour = [1.0; 3].into();
            l.shadows = true;
            // Pointing along -z: from the camera's side straight into the scene.
            l.placement.rotation = [0.0, 0.0, 0.0].into();
        }
        let light = add(&mut doc, light, &[]);
        let mut parts = vec![("scene", camera), ("scene_2", light)];
        if let Some(at) = cube_at {
            let mut cube = op(&SHAPE_3D);
            if let Operation::Shape3d(s) = &mut cube {
                s.size_x = 30.0.into();
                s.size_y = 30.0.into();
                s.size_z = 30.0.into();
                s.placement.position = at.into();
            }
            parts.push(("scene_3", add(&mut doc, cube, &[])));
        }
        let scene = add(&mut doc, op(&MERGE_SCENES), &parts);
        let mut value = registry::PARTICLES.default_operation();
        for (name, number) in [
            ("x", 80.0),
            ("y", 45.0),
            ("rate", 2.0),
            ("life", 60.0),
            ("start", -60.0),
            ("size", 2.0),
            ("speed", 0.0),
            ("gravity", 0.0),
            ("variation", 0.0),
            ("fade", 0.0),
            ("lit", 1.0),
            ("ambient", 0.0),
            ("scatter", 0.0),
        ] {
            value[name] = serde_json::json!({"kind": "constant", "value": number});
        }
        doc.output = add(
            &mut doc,
            serde_json::from_value(value).unwrap(),
            &[("scene", scene)],
        );
        doc.validate().unwrap();
        let pixels = render(&renderer, &doc, &ImageInputs::default());
        (
            pixels.iter().map(|p| p[0]).sum::<f32>(),
            pixels.iter().map(|p| p[3]).sum::<f32>(),
        )
    };
    let (open, open_alpha) = shot(None);
    assert!(open > 0.5, "lit: {open}");
    // A cube between the sun and the particle (objects sit at the frame's centre plus their
    // placement) keeps its light off it; the particle is still there.
    let (shaded, shaded_alpha) = shot(Some([0.0, 0.0, 80.0]));
    assert!(
        shaded < 0.05 * open,
        "in the cube's shadow: {shaded} of {open}"
    );
    assert!((shaded_alpha - open_alpha).abs() < 0.01 * open_alpha);
    // The same cube to one side lets the light through again (the maps are made anew).
    let (aside, _) = shot(Some([-60.0, 0.0, 80.0]));
    assert!((aside - open).abs() < 0.05 * open, "{aside} {open}");
}

/// Falling particles (a box across the top, a full cloud at the first frame) in a scene
/// of `solids`, maybe bouncing off them; the scene's own picture too.
fn rain_on(
    renderer: &Renderer,
    doc: &mut Composition,
    solids: &[NodeId],
    collide: bool,
) -> (Vec<[f32; 4]>, Vec<[f32; 4]>) {
    let camera = add(doc, op(&CAMERA_3D), &[]);
    let names = ["scene_2", "scene_3", "scene_4"];
    let mut parts = vec![("scene", camera)];
    parts.extend(solids.iter().zip(names).map(|(id, name)| (name, *id)));
    let scene = add(doc, op(&MERGE_SCENES), &parts);
    let mut value = registry::PARTICLES.default_operation();
    for (name, number) in [
        ("x", 80.0),
        ("y", 4.0),
        ("width", 110.0),
        ("height", 2.0),
        ("rate", 4000.0),
        ("life", 1.5),
        ("start", -1.5),
        ("size", 1.0),
        ("speed", 0.0),
        ("gravity", 300.0),
        ("turbulence", 0.0),
        ("variation", 0.0),
        ("fade", 0.0),
        ("bounce", 0.2),
    ] {
        value[name] = serde_json::json!({"kind": "constant", "value": number});
    }
    value["collide"] = serde_json::json!(collide);
    let particles = add(
        doc,
        serde_json::from_value(value).unwrap(),
        &[("scene", scene)],
    );
    let picture = add(doc, op(&RENDER_SCENE), &[("scene", scene)]);
    doc.output = particles;
    let rain = render(renderer, doc, &Default::default());
    doc.output = picture;
    (rain, render(renderer, doc, &Default::default()))
}
fn lit(pixels: &[[f32; 4]], keep: impl Fn(u32, u32) -> bool) -> usize {
    pixels
        .iter()
        .enumerate()
        .filter(|(i, p)| p[3] > 0.3 && keep(*i as u32 % W, *i as u32 / W))
        .count()
}
#[test]
#[ignore = "requires Vulkan"]
fn particles_bounce_off_the_scenes_solids() {
    let renderer = renderer();
    // A slab across the frame whose top is at y 55 (the frame's middle is 45).
    let slab = |doc: &mut Composition| {
        let mut slab = op(&SHAPE_3D);
        if let Operation::Shape3d(s) = &mut slab {
            s.size_x = 140.0.into();
            s.size_y = 10.0.into();
            s.size_z = 200.0.into();
            s.placement.position = [0.0, 15.0, 0.0].into();
        }
        add(doc, slab, &[])
    };
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let solid = slab(&mut doc);
    let (passing, _) = rain_on(&renderer, &mut doc, &[solid], false);
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let solid = slab(&mut doc);
    let (stopped, _) = rain_on(&renderer, &mut doc, &[solid], true);
    let under = |x: u32, y: u32| (30..130).contains(&x) && y > 67;
    // Just above the top: 4 rows of 100 pixels, which they pile up and cover.
    let on_top = |x: u32, y: u32| (30..130).contains(&x) && (51..55).contains(&y);
    assert!(
        lit(&passing, under) > 200,
        "without collide they fall through: {}",
        lit(&passing, under)
    );
    assert_eq!(lit(&stopped, under), 0, "under the slab");
    assert!(
        lit(&stopped, on_top) > 380 && lit(&passing, on_top) < 250,
        "gathered on its top: {} (falling through: {})",
        lit(&stopped, on_top),
        lit(&passing, on_top)
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn particles_bounce_off_the_letters_of_a_3d_text() {
    let renderer = renderer();
    let title = |doc: &mut Composition| {
        let mut text = op(&registry::TEXT);
        if let Operation::Text(t) = &mut text {
            t.content = "HI".into();
            t.size = 70.0.into();
            t.x = 80.0.into();
            t.y = 70.0.into();
        }
        let text = add(doc, text, &[]);
        let mut title = op(&TEXT_3D);
        if let Operation::Text3d(t) = &mut title {
            t.depth = 40.0.into();
            t.bevel = 0.0.into();
        }
        add(doc, title, &[("outline", text)])
    };
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let solid = title(&mut doc);
    let (passing, letters) = rain_on(&renderer, &mut doc, &[solid], false);
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let solid = title(&mut doc);
    let (stopped, _) = rain_on(&renderer, &mut doc, &[solid], true);
    // Well inside the letters as filmed: covered three pixels round.
    let covered = |x: i32, y: i32| {
        x >= 0
            && y >= 0
            && x < W as i32
            && y < H as i32
            && letters[(y as u32 * W + x as u32) as usize][3] > 0.5
    };
    let inside = |x: u32, y: u32| {
        (-3..=3).all(|dy| (-3..=3).all(|dx| covered(x as i32 + dx, y as i32 + dy)))
    };
    assert!(
        lit(&letters, |_, _| true) > 500,
        "the letters are in the frame"
    );
    assert!(
        lit(&passing, inside) > 50,
        "without collide they fall through the letters"
    );
    assert_eq!(lit(&stopped, inside), 0, "inside the letters");
}

/// Copies of a small cube (placed at the frame's middle) at particles born in a box on
/// the left, filmed by a Render scene at `seconds`.
fn copies_at(renderer: &Renderer, speed: f32, seconds: (i64, u32)) -> Vec<[f32; 4]> {
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let camera = add(&mut doc, op(&CAMERA_3D), &[]);
    let mut cube = op(&SHAPE_3D);
    if let Operation::Shape3d(s) = &mut cube {
        for size in [&mut s.size_x, &mut s.size_y, &mut s.size_z] {
            *size = 6.0.into();
        }
        s.colour = [1.0; 3].into();
        s.roughness = 1.0.into();
    }
    let cube = add(&mut doc, cube, &[]);
    let mut value = registry::PARTICLES.default_operation();
    for (name, number) in [
        ("x", 30.0),
        ("y", 45.0),
        ("width", 30.0),
        ("height", 60.0),
        ("rate", 20.0),
        ("life", 10.0),
        ("start", -10.0),
        ("speed", speed),
        ("direction", 0.0),
        ("spread", 0.0),
        ("gravity", 0.0),
        ("turbulence", 0.0),
        ("variation", 0.0),
    ] {
        value[name] = serde_json::json!({"kind": "constant", "value": number});
    }
    value["colour"] = serde_json::json!([1.0, 0.1, 0.1, 1.0]);
    value["end_colour"] = serde_json::json!([1.0, 0.1, 0.1, 1.0]);
    let particles = add(&mut doc, serde_json::from_value(value).unwrap(), &[]);
    let copies = add(
        &mut doc,
        op(&PARTICLE_INSTANCES),
        &[("scene", cube), ("particles", particles)],
    );
    let scene = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", camera), ("scene_2", copies)],
    );
    doc.output = add(&mut doc, op(&RENDER_SCENE), &[("scene", scene)]);
    renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(seconds.0, seconds.1).unwrap()),
            &Default::default(),
        )
        .unwrap()
        .pixels
}
#[test]
#[ignore = "requires Vulkan"]
fn a_3d_object_is_copied_at_every_particle() {
    let renderer = renderer();
    let still = copies_at(&renderer, 0.0, (0, 1));
    let solid = |p: &[f32; 4]| p[3] > 0.5;
    // The box spans x 15 to 45, and a copy at its edge reaches half a cube beyond it.
    let in_box = |i: usize| (10..51).contains(&(i as u32 % W));
    let copies = still
        .iter()
        .enumerate()
        .filter(|(i, p)| solid(p) && in_box(*i))
        .count();
    let elsewhere = still
        .iter()
        .enumerate()
        .filter(|(i, p)| solid(p) && !in_box(*i))
        .count();
    // 200 copies of a 6-pixel cube in the box on the left; none at the cube's own place.
    assert!(copies > 600, "copies in the box: {copies}");
    assert_eq!(
        elsewhere, 0,
        "nothing outside the box (the cube itself is not drawn)"
    );
    // They take the particles' red as a tint.
    let (r, g) = still
        .iter()
        .filter(|p| solid(p))
        .fold((0.0, 0.0), |(r, g), p| (r + p[0], g + p[1]));
    assert!(r > 4.0 * g, "tinted red: {r} {g}");
    // Moving particles carry their copies: at 40 px a second they spread to the right.
    let moving = copies_at(&renderer, 40.0, (0, 1));
    let right = moving
        .iter()
        .enumerate()
        .filter(|(i, p)| solid(p) && (i as &usize % W as usize) > 60)
        .count();
    assert!(right > 200, "copies carried along: {right}");
}

/// A picture with something to recognise anywhere in it: a smooth gradient, and a red
/// square left of the middle.
fn marked() -> Vec<[f32; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            if (50..70).contains(&x) && (35..55).contains(&y) {
                [1.0, 0.0, 0.0, 1.0]
            } else {
                // Never mostly blue, so it cannot pass for the plane's own colour.
                [
                    0.1 + x as f32 / W as f32 * 0.5,
                    0.1 + y as f32 / H as f32 * 0.5,
                    0.05,
                    1.0,
                ]
            }
        })
        .collect()
}
/// `marked()` cast through `projector` (a camera, maybe keyed) onto a plane filling the
/// frame at depth 0, filmed by the scene's first camera `filming`, at `seconds`.
fn projected(
    renderer: &Renderer,
    projector: Operation,
    filming: Operation,
    cast: CameraProjection,
    plane: ([f32; 3], f32),
    seconds: (i64, u32),
) -> Vec<[f32; 4]> {
    projected_with(renderer, projector, filming, cast, plane, None, seconds)
}
/// As `projected`, with a cube of its own colour (not cast onto) at `blocker`.
fn projected_with(
    renderer: &Renderer,
    projector: Operation,
    filming: Operation,
    cast: CameraProjection,
    plane: ([f32; 3], f32),
    blocker: Option<[f32; 3]>,
    seconds: (i64, u32),
) -> Vec<[f32; 4]> {
    let (plane_at, plane_scale) = plane;
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let mut inputs = ImageInputs::default();
    inputs.insert("marked.png", Arc::new(Image::new(W, H, marked()).unwrap()));
    let picture = add(
        &mut doc,
        Operation::Image(nodes::Image {
            asset: "marked.png".into(),
        }),
        &[],
    );
    let filming = add(&mut doc, filming, &[]);
    let projector = add(&mut doc, projector, &[]);
    let mut plane = op(&SHAPE_3D);
    if let Operation::Shape3d(s) = &mut plane {
        s.kind = ShapeKind::Plane;
        s.size_x = (W as f32 * plane_scale).into();
        s.size_y = (H as f32 * plane_scale).into();
        s.colour = [0.0, 0.0, 1.0].into();
        s.placement.position = plane_at.into();
    }
    let plane = add(&mut doc, plane, &[]);
    let cast = add(
        &mut doc,
        Operation::CameraProjection(cast),
        &[("scene", plane), ("camera", projector), ("image", picture)],
    );
    let mut parts = vec![("scene", filming), ("scene_2", cast)];
    if let Some(at) = blocker {
        let mut cube = op(&SHAPE_3D);
        if let Operation::Shape3d(s) = &mut cube {
            for size in [&mut s.size_x, &mut s.size_y, &mut s.size_z] {
                *size = 40.0.into();
            }
            s.placement.position = at.into();
        }
        parts.push(("scene_3", add(&mut doc, cube, &[])));
    }
    let scene = add(&mut doc, op(&MERGE_SCENES), &parts);
    doc.output = add(&mut doc, op(&RENDER_SCENE), &[("scene", scene)]);
    renderer
        .render_frame(
            &Plan::compile(&doc).unwrap(),
            RenderRequest::full(Time::new(seconds.0, seconds.1).unwrap()),
            &inputs,
        )
        .unwrap()
        .pixels
}
fn plain_cast() -> CameraProjection {
    let Operation::CameraProjection(cast) = op(&CAMERA_PROJECTION) else {
        unreachable!()
    };
    cast
}
#[test]
#[ignore = "requires Vulkan"]
fn a_picture_cast_through_a_camera_is_seen_again_by_it() {
    let renderer = renderer();
    // Cast and filmed by the same camera, onto a plane filling its view: the picture back.
    let back = projected(
        &renderer,
        op(&CAMERA_3D),
        op(&CAMERA_3D),
        plain_cast(),
        ([0.0; 3], 1.0),
        (0, 1),
    );
    let picture = marked();
    let mut worst = 0.0f32;
    for y in 2..H - 2 {
        for x in 2..W - 2 {
            let i = (y * W + x) as usize;
            for c in 0..4 {
                worst = worst.max((back[i][c] - picture[i][c]).abs());
            }
        }
    }
    assert!(
        worst < 0.02,
        "the picture comes back as it was: worst {worst}"
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn a_held_projection_stays_on_the_surface_as_the_camera_moves() {
    let renderer = renderer();
    // The projecting camera slides right over a second; the scene is filmed from the home view.
    let mut moving = op(&CAMERA_3D);
    if let Operation::Camera3d(c) = &mut moving {
        c.placement.position =
            serde_json::from_value(serde_json::json!({"interpolation": "linear", "keys": [
            {"time": {"ticks": 0, "rate": 1}, "value": [0.0, 0.0, 0.0]},
            {"time": {"ticks": 1, "rate": 1}, "value": [40.0, 0.0, 0.0]}]}))
            .unwrap();
    }
    let red_x = |pixels: &[[f32; 4]]| {
        let reds: Vec<u32> = pixels
            .iter()
            .enumerate()
            .filter(|(_, p)| p[0] > 0.9 && p[1] < 0.1)
            .map(|(i, _)| i as u32 % W)
            .collect();
        reds.iter().sum::<u32>() as f32 / reds.len().max(1) as f32
    };
    let mut held = plain_cast();
    held.hold = true;
    let start = red_x(&projected(
        &renderer,
        moving.clone(),
        op(&CAMERA_3D),
        held.clone(),
        ([0.0; 3], 1.0),
        (0, 1),
    ));
    let later_held = red_x(&projected(
        &renderer,
        moving.clone(),
        op(&CAMERA_3D),
        held,
        ([0.0; 3], 1.0),
        (1, 1),
    ));
    let later_moving = red_x(&projected(
        &renderer,
        moving,
        op(&CAMERA_3D),
        plain_cast(),
        ([0.0; 3], 1.0),
        (1, 1),
    ));
    // Held, the square stays where it was cast; following the camera, it moves with it.
    assert!(
        (later_held - start).abs() < 0.5,
        "held: {start} then {later_held}"
    );
    assert!(
        (later_moving - start).abs() > 20.0,
        "moving: {start} then {later_moving}"
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn what_the_camera_did_not_see_keeps_its_own_look_or_is_left_out() {
    let renderer = renderer();
    // Filmed from twice as far back as the projector, onto a plane three frames wide: the
    // middle half of the picture is cast, the rest of the plane the camera did not see.
    let mut back = op(&CAMERA_3D);
    if let Operation::Camera3d(c) = &mut back {
        let home = veylvik_scene::home_camera(W, H);
        let distance = home.world[2][3];
        c.placement.position = [0.0, 0.0, distance].into();
    }
    let blue = |pixels: &[[f32; 4]]| {
        pixels
            .iter()
            .filter(|p| p[3] > 0.5 && p[2] > 0.02 && p[2] > 3.0 * p[0].max(p[1]))
            .count()
    };
    let clear = |pixels: &[[f32; 4]]| pixels.iter().filter(|p| p[3] < 0.01).count();
    let kept = projected(
        &renderer,
        op(&CAMERA_3D),
        back.clone(),
        plain_cast(),
        ([0.0; 3], 3.0),
        (0, 1),
    );
    let mut hide = plain_cast();
    hide.hide_unseen = true;
    let left = projected(
        &renderer,
        op(&CAMERA_3D),
        back,
        hide,
        ([0.0; 3], 3.0),
        (0, 1),
    );
    let all = (W * H) as usize;
    // Kept: the plane fills the frame, blue round the cast middle (three quarters of it).
    assert_eq!(clear(&kept), 0, "the plane fills the frame");
    assert!(
        blue(&kept) > all / 2,
        "its own colour round the picture: {}",
        blue(&kept)
    );
    // Left out: those parts are clear, the middle still shows the picture.
    assert_eq!(blue(&left), 0, "nothing of its own colour");
    assert!(
        clear(&left) > all / 2,
        "left out round the picture: {}",
        clear(&left)
    );
    assert!(clear(&left) < all * 7 / 8, "the middle is still cast");
}
#[test]
#[ignore = "requires Vulkan"]
fn what_was_hidden_from_the_camera_behind_something_is_not_cast_onto() {
    let renderer = renderer();
    // A cube stands in front of the wall the picture is cast on; filmed from a camera
    // moved to the right, part of the wall it hid from the projecting camera shows.
    let mut aside = op(&CAMERA_3D);
    if let Operation::Camera3d(c) = &mut aside {
        c.placement.position = [50.0, 0.0, 0.0].into();
    }
    let mut hide = plain_cast();
    hide.hide_unseen = true;
    let wall = ([0.0; 3], 3.0);
    let open = projected_with(
        &renderer,
        op(&CAMERA_3D),
        aside.clone(),
        hide.clone(),
        wall,
        None,
        (0, 1),
    );
    let blocked = projected_with(
        &renderer,
        op(&CAMERA_3D),
        aside,
        hide,
        wall,
        Some([0.0, 0.0, 120.0]),
        (0, 1),
    );
    // Pixels that show the cast picture on the open wall but nothing with the cube there:
    // wall the cube hid from the projector (the cube itself is drawn, never clear).
    let holes = open
        .iter()
        .zip(&blocked)
        .filter(|(a, b)| a[3] > 0.5 && b[3] < 0.01)
        .count();
    assert!(
        holes > 100,
        "the wall hidden behind the cube is left out: {holes}"
    );
}

/// Gaussian splats from `splats` (place, colour, opacity, size, turn) in a scene filmed
/// from the home view, their box `size` pixels, maybe with a cube at `cube`.
fn splatted(
    renderer: &Renderer,
    splats: &[veylvik_splat::Described],
    size: f32,
    cube: Option<[f32; 3]>,
) -> Vec<[f32; 4]> {
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let mut inputs = ImageInputs::default();
    let parsed = veylvik_splat::parse(&veylvik_splat::write_ply(splats)).unwrap();
    inputs.insert_splats("scan.ply", Arc::new(parsed));
    let camera = add(&mut doc, op(&CAMERA_3D), &[]);
    let mut node = op(&GAUSSIAN_SPLATS);
    if let Operation::GaussianSplats(g) = &mut node {
        g.file = "scan.ply".into();
        g.size = size.into();
    }
    let node = add(&mut doc, node, &[]);
    let mut parts = vec![("scene", camera), ("scene_2", node)];
    if let Some(at) = cube {
        let mut c = op(&SHAPE_3D);
        if let Operation::Shape3d(s) = &mut c {
            for side in [&mut s.size_x, &mut s.size_y, &mut s.size_z] {
                *side = 30.0.into();
            }
            s.colour = [0.0, 1.0, 0.0].into();
            s.placement.position = at.into();
        }
        parts.push(("scene_3", add(&mut doc, c, &[])));
    }
    let scene = add(&mut doc, op(&MERGE_SCENES), &parts);
    doc.output = add(&mut doc, op(&RENDER_SCENE), &[("scene", scene)]);
    render(renderer, &doc, &inputs)
}
const STILL: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[test]
#[ignore = "requires Vulkan"]
fn a_splat_is_drawn_soft_and_where_it_is() {
    let renderer = renderer();
    // One red splat at the middle of its box: at the frame's middle, fading out.
    let one = splatted(
        &renderer,
        &[([0.0; 3], [1.0, 0.0, 0.0], 0.9, [1.0; 3], STILL)],
        40.0,
        None,
    );
    let at = |x: u32, y: u32| one[(y * W + x) as usize];
    let middle = at(W / 2, H / 2);
    assert!(
        middle[0] > 0.6 && middle[1] < 0.05 && middle[3] > 0.6,
        "red and nearly solid at its middle: {middle:?}"
    );
    let edge = at(W / 2 + 12, H / 2);
    assert!(
        edge[3] > 0.0 && edge[3] < middle[3],
        "softer away from it: {edge:?}"
    );
    assert!(at(5, 5)[3] < 0.001, "nothing far from it");
}
#[test]
#[ignore = "requires Vulkan"]
fn splats_are_drawn_back_to_front_and_solids_hide_them() {
    let renderer = renderer();
    // Blue behind and red in front (z towards the viewer), written blue first: the
    // front one covers the back one whatever their order in the file.
    let pair = [
        ([0.0, 0.0, -1.0], [0.0, 0.0, 1.0], 0.95, [1.0; 3], STILL),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 0.95, [1.0; 3], STILL),
    ];
    let both = splatted(&renderer, &pair, 40.0, None);
    let middle = both[((H / 2) * W + W / 2) as usize];
    assert!(
        middle[0] > 4.0 * middle[2],
        "the front splat covers the back one: {middle:?}"
    );
    let swapped = splatted(&renderer, &[pair[1], pair[0]], 40.0, None);
    let again = swapped[((H / 2) * W + W / 2) as usize];
    assert!(
        (again[0] - middle[0]).abs() < 0.02,
        "the file's order does not matter: {again:?}"
    );
    // A cube in front of them hides them: green, not red.
    let hidden = splatted(&renderer, &pair, 40.0, Some([0.0, 0.0, 60.0]));
    let front = hidden[((H / 2) * W + W / 2) as usize];
    assert!(
        front[1] > 4.0 * front[0],
        "the cube in front hides the splats: {front:?}"
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn what_the_engine_keeps_between_frames_is_counted_and_can_be_let_go() {
    use veylvik_render::SceneRenderer;
    let renderer = Renderer::new().unwrap();
    let engine = Arc::new(veylvik_render3d::Engine::new(renderer.device()));
    renderer.set_scene_renderer(engine.clone());
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let mut cube = op(&SHAPE_3D);
    if let Operation::Shape3d(s) = &mut cube {
        s.size_x = 60.0.into();
        s.size_y = 60.0.into();
        s.size_z = 60.0.into();
        s.placement.rotation = [25.0, 35.0, 0.0].into();
    }
    let cube = add(&mut doc, cube, &[]);
    doc.output = add(&mut doc, op(&RENDER_SCENE), &[("scene", cube)]);
    assert_eq!(engine.held_bytes(), 0);
    let first = render(&renderer, &doc, &ImageInputs::default());
    // Its render targets (multisampled colour and depth, resolved colour) at least.
    let targets = u64::from(W) * u64::from(H) * (4 * 12 + 8);
    assert!(engine.held_bytes() >= targets, "{}", engine.held_bytes());
    engine.release();
    assert_eq!(engine.held_bytes(), 0);
    // Made again when needed, the picture is the same.
    assert_eq!(render(&renderer, &doc, &ImageInputs::default()), first);
}
#[test]
#[ignore = "requires Vulkan"]
fn particles_are_born_on_an_imported_models_surface() {
    use veylvik_scene::{Model, Part};
    let renderer = renderer();
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    // A model of one flat part on the right half of its box: at size 120 it covers
    // x 0..60 and y -15..15 about the frame's middle.
    let quad = veylvik_mesh::Mesh {
        vertices: [
            [0.0, -0.125, 0.0],
            [0.5, -0.125, 0.0],
            [0.5, 0.125, 0.0],
            [0.0, 0.125, 0.0],
        ]
        .map(|position| veylvik_mesh::Vertex {
            position,
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 0.0],
        })
        .to_vec(),
        indices: vec![0, 2, 1, 0, 3, 2],
        cap_indices: 0,
    };
    let model = Model {
        parts: vec![Part {
            mesh: quad,
            colour: [1.0; 4],
            metallic: 0.0,
            roughness: 1.0,
            texture: None,
        }],
        extent: [1.0, 0.5, 0.0],
    };
    let mut inputs = ImageInputs::default();
    inputs.insert_model("/models/plate.glb", Arc::new(model));
    let mut node = op(&MODEL_3D);
    if let Operation::Model3d(m) = &mut node {
        m.model = "/models/plate.glb".into();
        m.size = 120.0.into();
        m.extent = [1.0, 0.5, 0.0];
    }
    let model = add(&mut doc, node, &[]);
    let camera = add(&mut doc, op(&CAMERA_3D), &[]);
    let scene = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", camera), ("scene_2", model)],
    );
    let mut dust = op(&PARTICLES);
    if let Operation::Particles(p) = &mut dust {
        p.emit_from = Some(model);
        p.rate = 4000.0.into();
        p.life = 1.0.into();
        p.start = (-1.0).into();
        p.speed = 0.0.into();
        p.gravity = 0.0.into();
        p.turbulence = 0.0.into();
        p.variation = 0.0.into();
        p.size = 0.8.into();
    }
    doc.output = add(&mut doc, dust, &[("scene", scene)]);
    let out = render(&renderer, &doc, &inputs);
    let lit: Vec<(u32, u32)> = (0..W * H)
        .filter(|&i| out[i as usize][3] > 0.05)
        .map(|i| (i % W, i / W))
        .collect();
    assert!(lit.len() > 400, "particles on the model: {}", lit.len());
    let (cx, cy) = (W as i32 / 2, H as i32 / 2);
    for (x, y) in lit {
        let (dx, dy) = (x as i32 - cx, y as i32 - cy);
        assert!(
            (-2..=62).contains(&dx) && (-17..=17).contains(&dy),
            "a particle off the model at {dx}, {dy}"
        );
    }
}
#[test]
#[ignore = "requires Vulkan"]
fn grains_on_a_surface_are_lit_as_the_surface_is() {
    let renderer = renderer();
    // Grains all over a sphere in the frame's middle, a sun from the left.
    let shot = |shading: f32| {
        let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
        let mut ball = op(&SHAPE_3D);
        if let Operation::Shape3d(s) = &mut ball {
            s.kind = ShapeKind::Sphere;
            s.size_x = 60.0.into();
            s.size_y = 60.0.into();
            s.size_z = 60.0.into();
        }
        let ball = add(&mut doc, ball, &[]);
        let camera = add(&mut doc, op(&CAMERA_3D), &[]);
        let mut sun = op(&LIGHT_3D);
        if let Operation::Light3d(l) = &mut sun {
            l.kind = LightKind::Sun;
            l.shadows = false;
            l.colour = [1.0; 3].into();
            l.placement.rotation = [0.0, -90.0, 0.0].into();
        }
        let sun = add(&mut doc, sun, &[]);
        let scene = add(
            &mut doc,
            op(&MERGE_SCENES),
            &[("scene", camera), ("scene_2", ball), ("scene_3", sun)],
        );
        let mut grains = op(&PARTICLES);
        if let Operation::Particles(p) = &mut grains {
            p.emit_from = Some(ball);
            p.rate = 20000.0.into();
            p.life = 1.0.into();
            p.start = (-1.0).into();
            p.speed = 0.0.into();
            p.gravity = 0.0.into();
            p.turbulence = 0.0.into();
            p.variation = 0.0.into();
            p.size = 0.6.into();
            p.lit = 1.0.into();
            p.ambient = 0.05.into();
            p.scatter = 0.0.into();
            p.shading = shading.into();
        }
        doc.output = add(&mut doc, grains, &[("scene", scene)]);
        let out = render(&renderer, &doc, &ImageInputs::default());
        // Light on the left half of the ball and on the right half.
        let half = |left: bool| {
            (0..W * H)
                .filter(|&i| {
                    let x = (i % W) as i32 - W as i32 / 2;
                    if left { x < -5 } else { x > 5 }
                })
                .map(|i| out[i as usize][0])
                .sum::<f32>()
        };
        (half(true), half(false))
    };
    let (plain_left, plain_right) = shot(0.0);
    let (left, right) = shot(1.0);
    // Without shading a sun lights every grain alike, whichever way it faces.
    assert!(
        (plain_left / plain_right - 1.0).abs() < 0.25,
        "{plain_left} / {plain_right}"
    );
    // With it the side facing the sun is lit and the other side is dark.
    assert!(left > right * 4.0, "lit side {left}, dark side {right}");
}
#[test]
#[ignore = "requires Vulkan"]
fn out_of_focus_particles_can_be_drawn_as_a_lenss_discs() {
    let renderer = renderer();
    // One spot of particles far behind the frame, a fast lens focused on the frame.
    let shot = |bokeh: f32| {
        let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
        let mut camera = op(&CAMERA_3D);
        if let Operation::Camera3d(c) = &mut camera {
            c.f_stop = 1.0.into();
            c.focus_distance = 200.0.into();
        }
        let camera = add(&mut doc, camera, &[]);
        let scene = add(&mut doc, op(&MERGE_SCENES), &[("scene", camera)]);
        let mut spot = op(&PARTICLES);
        if let Operation::Particles(p) = &mut spot {
            p.x = (W as f32 / 2.0).into();
            p.y = (H as f32 / 2.0).into();
            p.width = 0.0.into();
            p.height = 0.0.into();
            p.z = 90.0.into();
            p.rate = 4.0.into();
            p.life = 1.0.into();
            p.start = (-1.0).into();
            p.speed = 0.0.into();
            p.gravity = 0.0.into();
            p.turbulence = 0.0.into();
            p.variation = 0.0.into();
            p.fade = 0.0.into();
            p.glow = 1.0.into();
            p.size = 1.0.into();
            p.colour = [0.25; 4];
            p.end_colour = [0.25; 4];
            p.bokeh = bokeh.into();
            p.camera = 1.0.into();
        }
        doc.output = add(&mut doc, spot, &[("scene", scene)]);
        render(&renderer, &doc, &ImageInputs::default())
    };
    let (dot, disc) = (shot(0.0), shot(1.0));
    let row = |px: &[[f32; 4]]| -> Vec<f32> {
        (0..W).map(|x| px[((H / 2) * W + x) as usize][3]).collect()
    };
    let (dot_row, disc_row) = (row(&dot), row(&disc));
    let middle = (W / 2) as usize;
    // How far the disc reaches from the middle.
    let reach = (0..middle)
        .take_while(|&d| disc_row[middle + d] > disc_row[middle] * 0.05)
        .count();
    assert!(reach > 4, "out of focus it spreads: {reach} px");
    let half = middle + reach / 2;
    // A dot falls off from its middle; a disc is flat across (its rim a little brighter).
    assert!(
        dot_row[half] < dot_row[middle] * 0.8,
        "dot: {} at half against {} in the middle",
        dot_row[half],
        dot_row[middle]
    );
    assert!(
        disc_row[half] > disc_row[middle] * 0.9,
        "disc: {} at half against {} in the middle",
        disc_row[half],
        disc_row[middle]
    );
    // Both carry about the same light.
    let total = |px: &[[f32; 4]]| px.iter().map(|p| p[3]).sum::<f32>();
    assert!(
        (total(&disc) / total(&dot) - 1.0).abs() < 0.3,
        "light {} against {}",
        total(&disc),
        total(&dot)
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn particles_gather_onto_a_3d_object_and_stay_on_it() {
    let renderer = renderer();
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    // A sphere 40 across, 40 to the right of the middle.
    let mut ball = op(&SHAPE_3D);
    if let Operation::Shape3d(s) = &mut ball {
        s.kind = ShapeKind::Sphere;
        s.size_x = 40.0.into();
        s.size_y = 40.0.into();
        s.size_z = 40.0.into();
        s.placement.position = [40.0, 0.0, 0.0].into();
    }
    let ball = add(&mut doc, ball, &[]);
    let camera = add(&mut doc, op(&CAMERA_3D), &[]);
    let scene = add(
        &mut doc,
        op(&MERGE_SCENES),
        &[("scene", camera), ("scene_2", ball)],
    );
    let mut dust = op(&PARTICLES);
    if let Operation::Particles(p) = &mut dust {
        // A burst in a box on the left, flying to the ball over its life and staying.
        p.x = 20.0.into();
        p.y = (H as f32 / 2.0).into();
        p.width = 20.0.into();
        p.height = 20.0.into();
        p.rate = 6000.0.into();
        p.start = 0.0.into();
        p.duration = 0.1.into();
        p.life = 1.0.into();
        p.hold = 5.0.into();
        p.homing = 1.0.into();
        p.speed = 0.0.into();
        p.gravity = 0.0.into();
        p.turbulence = 0.0.into();
        p.variation = 0.0.into();
        p.size = 0.6.into();
        p.camera = 1.0.into();
        p.gather_on = Some(ball);
    }
    let dust = add(&mut doc, dust, &[("scene", scene)]);
    doc.output = dust;
    let plan = Plan::compile(&doc).unwrap();
    let out = renderer
        .render_frame(
            &plan,
            RenderRequest::full(Time::new(3, 1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap()
        .pixels;
    let lit: Vec<(f32, f32)> = (0..W * H)
        .filter(|&i| out[i as usize][3] > 0.05)
        .map(|i| {
            (
                (i % W) as f32 + 0.5 - (W / 2 + 40) as f32,
                (i / W) as f32 + 0.5 - (H / 2) as f32,
            )
        })
        .collect();
    assert!(lit.len() > 300, "on the ball: {} pixels", lit.len());
    // The ball shows as a disc of radius 20 (a little more, as it is nearer than the frame).
    let far = lit
        .iter()
        .map(|(x, y)| (x * x + y * y).sqrt())
        .fold(0.0f32, f32::max);
    assert!(far < 26.0, "a particle {far} px from the ball's middle");
    // Not with a target picture as well.
    let target = add(&mut doc, op(&SOLID), &[]);
    doc.nodes
        .get_mut(&dust)
        .unwrap()
        .inputs
        .insert("target".into(), target);
    let both = Plan::compile(&doc).map(|plan| {
        renderer.render_frame(
            &plan,
            RenderRequest::full(Time::new(3, 1).unwrap()),
            &ImageInputs::default(),
        )
    });
    assert!(
        matches!(both, Ok(Err(_)) | Err(_)),
        "a target picture and Gather on together are refused"
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn a_fluttering_banner_catches_the_light_in_bands_along_its_folds() {
    let renderer = renderer();
    let shot = |waves: bool| {
        let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
        let mut banner = op(&SHAPE_3D);
        if let Operation::Shape3d(s) = &mut banner {
            s.kind = ShapeKind::Plane;
            s.size_x = 120.0.into();
            s.size_y = 60.0.into();
            s.detail = 48.0.into();
            s.colour = [0.8, 0.8, 0.8].into();
            s.roughness = 0.8.into();
        }
        let mut banner = add(&mut doc, banner, &[]);
        if waves {
            let Operation::Flutter3d(mut f) =
                serde_json::from_value(veylvik_document::nodes::FLUTTER_3D.default_operation())
                    .unwrap()
            else {
                unreachable!()
            };
            f.amplitude = 10.0.into();
            f.wavelength = 40.0.into();
            banner = add(&mut doc, Operation::Flutter3d(f), &[("scene", banner)]);
        }
        let camera = add(&mut doc, op(&CAMERA_3D), &[]);
        let mut sun = op(&LIGHT_3D);
        if let Operation::Light3d(l) = &mut sun {
            l.kind = LightKind::Sun;
            l.shadows = false;
            l.colour = [1.0; 3].into();
            l.placement.rotation = [0.0, -50.0, 0.0].into();
        }
        let sun = add(&mut doc, sun, &[]);
        let scene = add(
            &mut doc,
            op(&MERGE_SCENES),
            &[("scene", camera), ("scene_2", banner), ("scene_3", sun)],
        );
        doc.output = add(&mut doc, op(&RENDER_SCENE), &[("scene", scene)]);
        let out = render(&renderer, &doc, &ImageInputs::default());
        // Brightness along the banner's middle row, over its right (free) half.
        (W / 2 + 10..W / 2 + 55)
            .map(|x| out[((H / 2) * W + x) as usize][0])
            .collect::<Vec<f32>>()
    };
    let spread = |row: &[f32]| {
        let mean = row.iter().sum::<f32>() / row.len() as f32;
        (row.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / row.len() as f32).sqrt()
    };
    let (flat, waving) = (shot(false), shot(true));
    assert!(
        spread(&flat) < 0.01,
        "a flat banner is lit evenly: {}",
        spread(&flat)
    );
    assert!(
        spread(&waving) > 0.05,
        "its folds catch the light in bands: {}",
        spread(&waving)
    );
}
