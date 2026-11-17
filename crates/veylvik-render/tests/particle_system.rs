use std::sync::Arc;
use veylvik_document::*;
use veylvik_render::*;

const W: u32 = 160;
const H: u32 = 120;

fn particles() -> nodes::Particles {
    let Operation::Particles(particles) =
        serde_json::from_value(registry::PARTICLES.default_operation()).unwrap()
    else {
        unreachable!()
    };
    particles
}
fn render(
    renderer: &Renderer,
    particles: nodes::Particles,
    emitter: Option<Vec<[f32; 4]>>,
    seconds: (i64, u32),
) -> Vec<[f32; 4]> {
    render_to(renderer, particles, emitter, None, seconds)
}
fn render_to(
    renderer: &Renderer,
    particles: nodes::Particles,
    emitter: Option<Vec<[f32; 4]>>,
    target: Option<Vec<[f32; 4]>>,
    seconds: (i64, u32),
) -> Vec<[f32; 4]> {
    let mut inputs = ImageInputs::default();
    let mut nodes = Vec::new();
    let mut wires = Vec::new();
    if let Some(pixels) = emitter {
        inputs.insert("emitter", Arc::new(Image::new(W, H, pixels).unwrap()));
        let image = Node::new(
            "emitter",
            Operation::Image(nodes::Image {
                asset: "emitter".into(),
            }),
            &[],
        );
        wires.push(("emitter", image.id));
        nodes.push(image);
    }
    if let Some(pixels) = target {
        inputs.insert("target", Arc::new(Image::new(W, H, pixels).unwrap()));
        let image = Node::new(
            "target",
            Operation::Image(nodes::Image {
                asset: "target".into(),
            }),
            &[],
        );
        wires.push(("target", image.id));
        nodes.push(image);
    }
    let node = Node::new("particles", Operation::Particles(particles), &wires);
    let output = node.id;
    nodes.push(node);
    let doc = Composition {
        ocio: Default::default(),
        working_space: Default::default(),
        display: Default::default(),
        groups: Default::default(),
        schema: 1,
        frame_count: 240,
        width: W,
        height: H,
        frame_rate: Time::new(24, 1).unwrap(),
        output,
        nodes: nodes.into_iter().map(|n| (n.id, n)).collect(),
    };
    let plan = Plan::compile(&doc).unwrap();
    let frame = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(seconds.0, seconds.1).unwrap()),
            &inputs,
        )
        .unwrap();
    renderer.read_frame(&frame).unwrap().pixels
}
/// Total alpha and its centre of mass.
fn mass(pixels: &[[f32; 4]]) -> (f32, f32, f32) {
    let (mut sum, mut x, mut y) = (0.0f64, 0.0f64, 0.0f64);
    for (i, p) in pixels.iter().enumerate() {
        sum += f64::from(p[3]);
        x += f64::from(p[3]) * f64::from(i as u32 % W);
        y += f64::from(p[3]) * f64::from(i as u32 / W);
    }
    (
        sum as f32,
        (x / sum.max(1e-9)) as f32,
        (y / sum.max(1e-9)) as f32,
    )
}
#[test]
#[ignore = "requires Vulkan"]
fn particles_move_as_set_and_any_frame_renders_the_same_alone() {
    let renderer = Renderer::new().unwrap();
    let mut base = particles();
    base.x = 80.0.into();
    base.y = 60.0.into();
    base.rate = 400.0.into();
    base.life = 1.0.into();
    base.speed = 40.0.into();
    base.direction = 0.0.into(); // to the right
    base.spread = 0.0.into();
    base.gravity = 0.0.into();
    base.variation = 0.0.into();
    base.fade = 0.0.into();
    base.size = 1.5.into();
    // Before emission starts there is nothing; after, particles fly right from the point.
    let mut late = base.clone();
    late.start = 5.0.into();
    assert_eq!(mass(&render(&renderer, late, None, (2, 1))).0, 0.0);
    let flying = render(&renderer, base.clone(), None, (2, 1));
    let (sum, x, y) = mass(&flying);
    assert!(sum > 50.0, "{sum}");
    // Ages are spread over the life, so they sit between 80 and 120: centre near 100.
    assert!((x - 100.0).abs() < 3.0 && (y - 59.5).abs() < 1.0, "{x} {y}");
    assert!(
        flying
            .iter()
            .enumerate()
            .all(|(i, p)| p[3] == 0.0 || (i as u32 % W) >= 74),
        "nothing flies left"
    );
    // The same frame again, and after another frame was rendered, is identical.
    render(&renderer, base.clone(), None, (7, 2));
    assert_eq!(render(&renderer, base.clone(), None, (2, 1)), flying);
    // Gravity pulls them down, wind right; turbulence scatters without losing any.
    let mut falling = base.clone();
    falling.gravity = 60.0.into();
    let (_, _, fy) = mass(&render(&renderer, falling, None, (2, 1)));
    assert!(fy > y + 5.0, "{fy}");
    let mut swirling = base.clone();
    swirling.turbulence = 60.0.into();
    swirling.turbulence_scale = 40.0.into();
    let swirled = render(&renderer, swirling, None, (2, 1));
    assert!(swirled != flying);
    assert!(
        swirled
            .iter()
            .enumerate()
            .any(|(i, p)| p[3] > 0.05 && ((i as u32 / W) as f32 - 59.5).abs() > 4.0),
        "turbulence leaves the line"
    );
    // A burst: all born in the first tenth of a second, all gone after their life.
    let mut burst = base.clone();
    burst.duration = 0.1.into();
    assert!(mass(&render(&renderer, burst.clone(), None, (1, 2))).0 > 5.0);
    assert_eq!(mass(&render(&renderer, burst, None, (2, 1))).0, 0.0);
    // Colour runs to the end colour, and values stay finite and premultiplied.
    let mut coloured = base.clone();
    coloured.colour = [4.0, 0.0, 0.0, 1.0];
    coloured.end_colour = [0.0, 0.0, 4.0, 1.0];
    let out = render(&renderer, coloured, None, (2, 1));
    let near = out[(60 * W + 84) as usize];
    let far = out[(60 * W + 116) as usize];
    assert!(near[0] > near[2] && far[2] > far[0], "{near:?} {far:?}");
}
#[test]
#[ignore = "requires Vulkan"]
fn particles_are_born_where_the_emitter_has_alpha() {
    let renderer = Renderer::new().unwrap();
    // The emitter is opaque in a block on the left only.
    let emitter: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            if (20..50).contains(&x) && (40..80).contains(&y) {
                [1.0; 4]
            } else {
                [0.0; 4]
            }
        })
        .collect();
    let mut still = particles();
    still.density = 4000.0.into();
    still.life = 1.0.into();
    still.speed = 0.0.into();
    still.gravity = 0.0.into();
    still.size = 1.0.into();
    let out = render(&renderer, still.clone(), Some(emitter.clone()), (2, 1));
    let (sum, x, y) = mass(&out);
    assert!(sum > 100.0, "{sum}");
    assert!((x - 34.5).abs() < 2.0 && (y - 59.5).abs() < 2.0, "{x} {y}");
    for (i, p) in out.iter().enumerate() {
        let (px, py) = (i as u32 % W, i as u32 / W);
        assert!(
            p[3] == 0.0 || ((17..53).contains(&px) && (37..83).contains(&py)),
            "stray at {px},{py}"
        );
    }
    // An empty emitter gives nothing, and more density gives more particles.
    assert_eq!(
        mass(&render(
            &renderer,
            still.clone(),
            Some(vec![[0.0; 4]; (W * H) as usize]),
            (2, 1)
        ))
        .0,
        0.0
    );
    still.density = 400.0.into();
    still.glow = 1.0.into();
    let fewer = mass(&render(&renderer, still, Some(emitter), (2, 1))).0;
    assert!(fewer > 5.0 && fewer < sum * 0.6, "{fewer} vs {sum}");
}
fn block(x: std::ops::Range<u32>, y: std::ops::Range<u32>, colour: [f32; 4]) -> Vec<[f32; 4]> {
    (0..W * H)
        .map(|i| {
            if x.contains(&(i % W)) && y.contains(&(i / W)) {
                colour
            } else {
                [0.0; 4]
            }
        })
        .collect()
}
#[test]
#[ignore = "requires Vulkan"]
fn particles_take_the_pictures_colour_fly_to_a_target_and_leave_streaks() {
    let renderer = Renderer::new().unwrap();
    let mut base = particles();
    base.density = 3000.0.into();
    base.life = 1.0.into();
    base.speed = 0.0.into();
    base.gravity = 0.0.into();
    base.variation = 0.0.into();
    base.fade = 0.0.into();
    base.size = 1.0.into();
    let red = block(20..50, 40..80, [0.8, 0.1, 0.05, 1.0]);
    let blue = block(110..140, 40..80, [0.05, 0.1, 0.8, 1.0]);
    // Without inherit the dots are the node's white; with it, the emitter's red.
    let plain = render(&renderer, base.clone(), Some(red.clone()), (2, 1));
    let p = plain[(60 * W + 35) as usize];
    assert!(
        p[3] > 0.1 && (p[0] - p[2]).abs() < 0.02 * p[0].max(0.01),
        "{p:?}"
    );
    let mut own = base.clone();
    own.inherit = 1.0.into();
    let tinted = render(&renderer, own.clone(), Some(red.clone()), (2, 1));
    let p = tinted[(60 * W + 35) as usize];
    assert!(p[0] > 4.0 * p[2] && p[0] > 4.0 * p[1], "{p:?}");
    // With a target they travel from the red block to the blue one, turning blue;
    // a burst shows it: early they are on the left, at the end of life on the right.
    let mut travel = own.clone();
    travel.duration = 0.05.into();
    let early = mass(&render_to(
        &renderer,
        travel.clone(),
        Some(red.clone()),
        Some(blue.clone()),
        (1, 10),
    ));
    let late = render_to(
        &renderer,
        travel.clone(),
        Some(red.clone()),
        Some(blue.clone()),
        (19, 20),
    );
    let (sum, x, y) = mass(&late);
    assert!(early.0 > 5.0 && early.1 < 45.0, "{early:?}");
    assert!(
        sum > 20.0 && (x - 124.5).abs() < 6.0 && (y - 59.5).abs() < 4.0,
        "{sum} {x} {y}"
    );
    let (red_light, blue_light) = late
        .iter()
        .fold((0.0, 0.0), |(r, b), p| (r + p[0], b + p[2]));
    assert!(blue_light > 2.0 * red_light, "{red_light} {blue_light}");
    // Homing 0 leaves them where they were born.
    travel.homing = 0.0.into();
    let stay = mass(&render_to(
        &renderer,
        travel,
        Some(red.clone()),
        Some(blue),
        (19, 20),
    ));
    assert!(stay.1 < 45.0, "{stay:?}");
    // An empty target gives no particles.
    assert_eq!(
        mass(&render_to(
            &renderer,
            own,
            Some(red),
            Some(vec![[0.0; 4]; (W * H) as usize]),
            (2, 1)
        ))
        .0,
        0.0
    );
    // A streak stretches a flying particle back along its path, and only that way.
    let mut spark = particles();
    spark.x = 40.0.into();
    spark.y = 60.0.into();
    spark.rate = 1.0.into();
    spark.life = 1.0.into();
    spark.duration = 0.02.into();
    spark.speed = 80.0.into();
    spark.direction = 0.0.into();
    spark.spread = 0.0.into();
    spark.gravity = 0.0.into();
    spark.variation = 0.0.into();
    spark.fade = 0.0.into();
    spark.size = 1.0.into();
    let extent = |pixels: &[[f32; 4]]| {
        let lit: Vec<u32> = pixels
            .iter()
            .enumerate()
            .filter(|(_, p)| p[3] > 0.02)
            .map(|(i, _)| i as u32 % W)
            .collect();
        (lit.iter().copied().min(), lit.iter().copied().max())
    };
    // Rate 1 with a life of 1 is one thread; seek a time where its burst particle is alive.
    let seen = (5..19).find_map(|t| {
        let dot = render(&renderer, spark.clone(), None, (t, 20));
        extent(&dot).0.map(|_| (t, dot))
    });
    let (t, dot) = seen.expect("the burst particle is alive at some frame");
    let (dot_min, dot_max) = extent(&dot);
    spark.streak = 0.2.into();
    let streaked = render(&renderer, spark, None, (t, 20));
    let (min, max) = extent(&streaked);
    assert!(
        dot_max.unwrap() - dot_min.unwrap() <= 4,
        "{dot_min:?} {dot_max:?}"
    );
    assert!(
        max.unwrap() <= dot_max.unwrap() + 1 && min.unwrap() + 8 <= dot_min.unwrap(),
        "{min:?} {max:?} vs {dot_min:?} {dot_max:?}"
    );
}
