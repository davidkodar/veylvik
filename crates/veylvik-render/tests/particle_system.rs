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
    let mut images = Vec::new();
    if let Some(pixels) = emitter {
        images.push(("emitter", pixels));
    }
    if let Some(pixels) = target {
        images.push(("target", pixels));
    }
    render_with(renderer, particles, images, seconds)
}
fn render_with(
    renderer: &Renderer,
    particles: nodes::Particles,
    images: Vec<(&'static str, Vec<[f32; 4]>)>,
    seconds: (i64, u32),
) -> Vec<[f32; 4]> {
    let mut inputs = ImageInputs::default();
    let mut nodes = Vec::new();
    let mut wires = Vec::new();
    for (port, pixels) in images {
        inputs.insert(port, Arc::new(Image::new(W, H, pixels).unwrap()));
        let image = Node::new(
            port,
            Operation::Image(nodes::Image { asset: port.into() }),
            &[],
        );
        wires.push((port, image.id));
        nodes.push(image);
    }
    let node = Node::new("particles", Operation::Particles(particles), &wires);
    let output = node.id;
    nodes.push(node);
    let doc = Composition {
        published: Default::default(),
        markers: Default::default(),
        backdrops: Default::default(),
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
/// A still cloud at a point: no launch speed, no gravity, no fades.
fn still() -> nodes::Particles {
    let mut base = particles();
    base.x = 80.0.into();
    base.y = 60.0.into();
    base.rate = 400.0.into();
    base.life = 1.0.into();
    base.speed = 0.0.into();
    base.gravity = 0.0.into();
    base.variation = 0.0.into();
    base.fade = 0.0.into();
    base.size = 1.5.into();
    base
}
fn span(pixels: &[[f32; 4]]) -> (u32, u32, u32, u32) {
    let lit: Vec<(u32, u32)> = pixels
        .iter()
        .enumerate()
        .filter(|(_, p)| p[3] > 0.02)
        .map(|(i, _)| (i as u32 % W, i as u32 / W))
        .collect();
    (
        lit.iter().map(|p| p.0).min().unwrap_or(0),
        lit.iter().map(|p| p.0).max().unwrap_or(0),
        lit.iter().map(|p| p.1).min().unwrap_or(0),
        lit.iter().map(|p| p.1).max().unwrap_or(0),
    )
}
#[test]
#[ignore = "requires Vulkan"]
fn keyed_settings_apply_from_each_particles_birth() {
    let renderer = Renderer::new().unwrap();
    // The emitter point moves from x 30 to x 130 over two seconds. Particles do not
    // move, so at two seconds those born in the last second lie along 80..130.
    let mut moving = still();
    let key = |ticks: i64, value: f32| Key {
        segment: None,
        time: Time::new(ticks, 1).unwrap(),
        value,
    };
    moving.x = Scalar::Linear {
        keys: vec![key(0, 30.0), key(2, 130.0)],
    };
    let out = render(&renderer, moving.clone(), None, (2, 1));
    let (left, right, top, bottom) = span(&out);
    assert!(
        (77..=83).contains(&left) && (127..=133).contains(&right),
        "{left} {right}"
    );
    assert!(top >= 56 && bottom <= 64, "{top} {bottom}");
    // The same frame again is identical, and an earlier frame shows the earlier stretch.
    assert_eq!(render(&renderer, moving.clone(), None, (2, 1)), out);
    let (left, right, ..) = span(&render(&renderer, moving.clone(), None, (3, 2)));
    assert!(
        (52..=58).contains(&left) && (102..=108).contains(&right),
        "{left} {right}"
    );
    // Rate keyed from nothing to full: more particles are young than old.
    let mut rising = still();
    rising.size = 0.6.into();
    rising.variation = 0.0.into();
    rising.speed = 40.0.into();
    rising.direction = 0.0.into();
    rising.spread = 0.0.into();
    rising.rate = Scalar::Linear {
        keys: vec![key(1, 0.0), key(2, 60.0)],
    };
    let out = render(&renderer, rising, None, (2, 1));
    let half = |from: u32, to: u32| {
        out.iter()
            .enumerate()
            .filter(|(i, _)| (from..to).contains(&(*i as u32 % W)))
            .map(|(_, p)| p[3])
            .sum::<f32>()
    };
    // Flying right at 40 px/s from x 80: the young are at 80..100, the old at 100..120.
    assert!(
        half(78, 100) > 2.0 * half(100, 124),
        "{} {}",
        half(78, 100),
        half(100, 124)
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn a_small_target_loses_no_particles() {
    let renderer = Renderer::new().unwrap();
    let mut burst = still();
    burst.width = 160.0.into();
    burst.height = 120.0.into();
    burst.rate = 2000.0.into();
    burst.duration = 0.05.into();
    burst.size = 0.6.into();
    burst.glow = 1.0.into();
    // Homing only late in the life, so halfway through they are still spread out.
    burst.homing = 0.2.into();
    let free = mass(&render(&renderer, burst.clone(), None, (1, 2))).0;
    // A target of 6 x 6 pixels in a frame of 19200: every particle still finds a place.
    let small = block(100..106, 30..36, [1.0; 4]);
    let homed = render_with(
        &renderer,
        burst.clone(),
        vec![("target", small.clone())],
        (1, 2),
    );
    assert!(mass(&homed).0 > 0.97 * free, "{} of {free}", mass(&homed).0);
    // At the end of their lives they are all on it (the dots' soft edges reach a little past).
    burst.rate = 200_000.0.into();
    burst.duration = 0.004.into();
    let landed = render_with(&renderer, burst, vec![("target", small)], (995, 1000));
    let (left, right, top, bottom) = span(&landed);
    assert!(
        left >= 96 && right <= 109 && top >= 26 && bottom <= 39,
        "{left} {right} {top} {bottom}"
    );
    assert!(mass(&landed).0 > 10.0);
}
#[test]
#[ignore = "requires Vulkan"]
fn fields_pull_points_and_obstacles_move_particles() {
    let renderer = Renderer::new().unwrap();
    let centre = |pixels: &[[f32; 4]]| mass(pixels);
    let (_, x0, y0) = centre(&render(&renderer, still(), None, (2, 1)));
    assert!(
        (x0 - 79.5).abs() < 1.0 && (y0 - 59.5).abs() < 1.0,
        "{x0} {y0}"
    );
    // A field brightening to the right pushes particles left, towards the dark;
    // flow carries them along the slope instead.
    let ramp: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            let v = (i % W) as f32 / W as f32;
            [v, v, v, 1.0]
        })
        .collect();
    let mut pushed = still();
    pushed.field_push = 400.0.into();
    let (_, x, y) = centre(&render_with(
        &renderer,
        pushed.clone(),
        vec![("field", ramp.clone())],
        (2, 1),
    ));
    assert!(x < x0 - 3.0 && (y - y0).abs() < 1.0, "{x} {y}");
    pushed.field_push = 0.0.into();
    pushed.field_flow = 400.0.into();
    let (_, x, y) = centre(&render_with(
        &renderer,
        pushed,
        vec![("field", ramp)],
        (2, 1),
    ));
    assert!((x - x0).abs() < 1.0 && (y - y0).abs() > 3.0, "{x} {y}");
    // A pull point draws them towards it; a vortex swings them round it.
    let mut pulled = still();
    pulled.pull_x = 130.0.into();
    pulled.pull_y = 60.0.into();
    pulled.pull = 200.0.into();
    let (_, x, y) = centre(&render(&renderer, pulled.clone(), None, (2, 1)));
    assert!(x > x0 + 5.0 && (y - y0).abs() < 1.0, "{x} {y}");
    pulled.pull = 0.0.into();
    pulled.vortex = 200.0.into();
    let (_, _, y) = centre(&render(&renderer, pulled, None, (2, 1)));
    assert!((y - y0).abs() > 5.0, "{y}");
    // Falling particles stop at an obstacle and bounce; without it they fall through.
    let mut falling = still();
    falling.gravity = 300.0.into();
    falling.life = 1.5.into();
    falling.speed = 40.0.into();
    falling.spread = 360.0.into();
    falling.rate = 150.0.into();
    falling.size = 0.7.into();
    let floor = block(0..W, 90..H, [1.0; 4]);
    let below = |pixels: &[[f32; 4]]| {
        pixels
            .iter()
            .enumerate()
            .filter(|(i, _)| *i as u32 / W >= 96)
            .map(|(_, p)| p[3])
            .sum::<f32>()
    };
    let through = render(&renderer, falling.clone(), None, (3, 1));
    let stopped = render_with(&renderer, falling, vec![("obstacle", floor)], (3, 1));
    assert!(below(&through) > 5.0, "{}", below(&through));
    assert!(
        below(&stopped) < 0.02 * below(&through),
        "{} vs {}",
        below(&stopped),
        below(&through)
    );
    assert!(
        mass(&stopped).0 > 0.6 * mass(&through).0,
        "particles are kept, not lost"
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn a_sprite_is_drawn_as_each_particle() {
    let renderer = Renderer::new().unwrap();
    // The sprite is red in its left half and empty in its right half.
    let sprite = block(0..W / 2, 0..H, [1.0, 0.0, 0.0, 1.0]);
    let mut one = still();
    one.rate = 1.0.into();
    one.duration = 0.02.into();
    one.size = 12.0.into();
    let seen = (5..19).find_map(|t| {
        let out = render_with(
            &renderer,
            one.clone(),
            vec![("sprite", sprite.clone())],
            (t, 20),
        );
        (mass(&out).0 > 1.0).then_some((t, out))
    });
    let (t, out) = seen.expect("the burst particle is alive at some frame");
    // 12 pixels wide either side of x 80, 9 either side of y 60 (the frame's shape);
    // only the left half is drawn, and it is red.
    let (left, right, top, bottom) = span(&out);
    assert!(
        (67..=70).contains(&left) && (78..=81).contains(&right),
        "{left} {right}"
    );
    assert!(
        (50..=53).contains(&top) && (67..=70).contains(&bottom),
        "{top} {bottom}"
    );
    let p = out[(60 * W + 74) as usize];
    assert!(
        p[0] > 0.8 && p[1] < 0.05 && p[2] < 0.05 && p[3] > 0.8,
        "{p:?}"
    );
    // Grow halves it by the end of the life; spin turns it, so the right side fills.
    let mut turning = one.clone();
    turning.spin = 720.0.into();
    let spun = render_with(&renderer, turning, vec![("sprite", sprite)], (t, 20));
    assert!(spun != out);
}
#[test]
#[ignore = "requires Vulkan"]
fn hues_vary_sprites_trail_and_thin_obstacles_hold() {
    let renderer = Renderer::new().unwrap();
    // Red particles with full colour variation come out in many hues.
    let mut confetti = still();
    confetti.width = 120.0.into();
    confetti.height = 80.0.into();
    confetti.rate = 60.0.into();
    confetti.size = 1.0.into();
    confetti.colour = [1.0, 0.0, 0.0, 1.0];
    confetti.end_colour = [1.0, 0.0, 0.0, 1.0];
    let plain = render(&renderer, confetti.clone(), None, (2, 1));
    assert!(plain.iter().all(|p| p[1] < 0.01 && p[2] < 0.01));
    confetti.colour_variation = 1.0.into();
    let varied = render(&renderer, confetti, None, (2, 1));
    let leaning = |c: usize| {
        varied
            .iter()
            .filter(|p| p[3] > 0.1 && p[c] > p[(c + 1) % 3] && p[c] > p[(c + 2) % 3])
            .count()
    };
    assert!(
        leaning(0) > 3 && leaning(1) > 3 && leaning(2) > 3,
        "{} {} {}",
        leaning(0),
        leaning(1),
        leaning(2)
    );
    // Fast particles do not pass a floor two pixels thick.
    let mut falling = still();
    falling.gravity = 0.0.into();
    falling.speed = 900.0.into();
    falling.direction = 270.0.into();
    falling.spread = 20.0.into();
    falling.life = 0.3.into();
    falling.rate = 300.0.into();
    falling.size = 0.7.into();
    let thin = block(0..W, 90..92, [1.0; 4]);
    let below = |pixels: &[[f32; 4]]| {
        pixels
            .iter()
            .enumerate()
            .filter(|(i, _)| *i as u32 / W >= 94)
            .map(|(_, p)| p[3])
            .sum::<f32>()
    };
    let through = below(&render(&renderer, falling.clone(), None, (2, 1)));
    let held = below(&render_with(
        &renderer,
        falling,
        vec![("obstacle", thin)],
        (2, 1),
    ));
    assert!(
        through > 3.0 && held < 0.02 * through,
        "{held} vs {through}"
    );
    // A sprite with a streak leaves copies behind it, and only behind it.
    let sprite = block(0..W, 0..H, [1.0; 4]);
    let mut one = still();
    one.x = 40.0.into();
    one.rate = 1.0.into();
    one.duration = 0.02.into();
    one.size = 4.0.into();
    one.speed = 80.0.into();
    one.direction = 0.0.into();
    one.spread = 0.0.into();
    let seen = (5..19).find_map(|t| {
        let out = render_with(
            &renderer,
            one.clone(),
            vec![("sprite", sprite.clone())],
            (t, 20),
        );
        (mass(&out).0 > 1.0).then_some((t, out))
    });
    let (t, out) = seen.expect("the burst particle is alive at some frame");
    let (left, right, ..) = span(&out);
    one.streak = 0.2.into();
    let (trail_left, trail_right, ..) = span(&render_with(
        &renderer,
        one,
        vec![("sprite", sprite)],
        (t, 20),
    ));
    assert!(
        trail_right <= right + 1 && trail_left + 8 <= left,
        "{trail_left} {trail_right} vs {left} {right}"
    );
}
/// Particles filmed by a 3D camera moved `shift` pixels to the right.
fn render_filmed(renderer: &Renderer, particles: nodes::Particles, shift: f32) -> Vec<[f32; 4]> {
    let mut camera = registry::CAMERA_3D.default_operation();
    camera["placement"]["position"] = serde_json::json!([shift, 0.0, 0.0]);
    let camera = Node::new("camera", serde_json::from_value(camera).unwrap(), &[]);
    let node = Node::new(
        "particles",
        Operation::Particles(particles),
        &[("scene", camera.id)],
    );
    let doc = Composition {
        published: Default::default(),
        markers: Default::default(),
        backdrops: Default::default(),
        ocio: Default::default(),
        working_space: Default::default(),
        display: Default::default(),
        groups: Default::default(),
        schema: 1,
        frame_count: 240,
        width: W,
        height: H,
        frame_rate: Time::new(24, 1).unwrap(),
        output: node.id,
        nodes: [camera, node].into_iter().map(|n| (n.id, n)).collect(),
    };
    let plan = Plan::compile(&doc).unwrap();
    let frame = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(2, 1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap();
    renderer.read_frame(&frame).unwrap().pixels
}
#[test]
#[ignore = "requires Vulkan"]
fn depth_spreads_particles_and_a_scene_camera_films_them() {
    let renderer = Renderer::new().unwrap();
    // A still point of particles to the right of the frame's centre. Flat, it is a dot.
    let mut point = still();
    point.x = 120.0.into();
    point.size = 1.0.into();
    let (left, right, top, bottom) = span(&render(&renderer, point.clone(), None, (2, 1)));
    assert!(
        right - left <= 4 && bottom - top <= 4,
        "{left} {right} {top} {bottom}"
    );
    // With depth, births lie 50 px in front of and behind the frame: seen from the
    // frame's centre they spread along the line through it (x 113..132 for this lens),
    // and stay on the centre's height.
    point.depth = 100.0.into();
    let deep = render(&renderer, point.clone(), None, (2, 1));
    let (left, right, top, bottom) = span(&deep);
    assert!(
        (110..=114).contains(&left) && (130..=135).contains(&right),
        "{left} {right}"
    );
    assert!(top >= 56 && bottom <= 64, "{top} {bottom}");
    // The same frame again is identical.
    assert_eq!(render(&renderer, point.clone(), None, (2, 1)), deep);
    // A camera in the scene films them: moved 20 px right, flat particles shift 20 px
    // left; a camera where the home view is leaves the picture as it was.
    point.depth = 0.0.into();
    let home = render(&renderer, point.clone(), None, (2, 1));
    assert_eq!(render_filmed(&renderer, point.clone(), 0.0), home);
    let (_, x, y) = mass(&render_filmed(&renderer, point.clone(), 20.0));
    assert!((x - 99.5).abs() < 1.0 && (y - 59.5).abs() < 1.0, "{x} {y}");
    // In depth, near particles shift more than far ones: the cloud widens (parallax).
    point.depth = 100.0.into();
    let (left, right, ..) = span(&render_filmed(&renderer, point.clone(), 20.0));
    assert!(
        left < 98 && right > 104 && right - left > 8,
        "{left} {right}"
    );
    // Camera 0 is the home view even with a scene connected.
    point.camera = 0.0.into();
    assert_eq!(render_filmed(&renderer, point, 20.0), deep);
}
/// Still particles born on a small disc that moves from x 30 to x 130 over two seconds,
/// seen at two seconds.
fn render_moving(renderer: &Renderer, history: u32) -> Vec<[f32; 4]> {
    let mut disc = registry::ELLIPSE.default_operation();
    disc["x"] = serde_json::json!({"kind": "linear", "keys": [
        {"time": {"ticks": 0, "rate": 1}, "value": 30.0},
        {"time": {"ticks": 2, "rate": 1}, "value": 130.0}]});
    disc["y"] = serde_json::json!({"kind": "constant", "value": 60.0});
    disc["radius_x"] = serde_json::json!({"kind": "constant", "value": 6.0});
    disc["radius_y"] = serde_json::json!({"kind": "constant", "value": 6.0});
    let disc = Node::new("disc", serde_json::from_value(disc).unwrap(), &[]);
    let mut particles = still();
    particles.density = 6000.0.into();
    particles.size = 0.7.into();
    particles.history = history;
    let node = Node::new(
        "particles",
        Operation::Particles(particles),
        &[("emitter", disc.id)],
    );
    let doc = Composition {
        published: Default::default(),
        markers: Default::default(),
        backdrops: Default::default(),
        ocio: Default::default(),
        working_space: Default::default(),
        display: Default::default(),
        groups: Default::default(),
        schema: 1,
        frame_count: 240,
        width: W,
        height: H,
        frame_rate: Time::new(24, 1).unwrap(),
        output: node.id,
        nodes: [disc, node].into_iter().map(|n| (n.id, n)).collect(),
    };
    let plan = Plan::compile(&doc).unwrap();
    let frame = renderer
        .render_gpu_temporal(
            &plan,
            RenderRequest::full(Time::new(2, 1).unwrap()),
            &ImageInputs::default(),
            false,
            || false,
        )
        .unwrap()
        .unwrap();
    renderer.read_frame(&frame).unwrap().pixels
}
#[test]
#[ignore = "requires Vulkan"]
fn a_moving_emitter_leaves_its_particles_behind_when_history_is_kept() {
    let renderer = Renderer::new().unwrap();
    // Without history every particle sits on the disc where it is now (x 124..136).
    let (left, right, top, bottom) = span(&render_moving(&renderer, 0));
    assert!(
        left >= 121 && right <= 139 && top >= 51 && bottom <= 69,
        "{left} {right} {top} {bottom}"
    );
    // With history, particles born in the last second lie where the disc was then:
    // back to x 80, a second (50 pixels) ago.
    for history in [4, 16] {
        let out = render_moving(&renderer, history);
        let (left, right, top, bottom) = span(&out);
        assert!(
            (70..=80).contains(&left) && (130..=139).contains(&right),
            "{history}: {left} {right}"
        );
        assert!(top >= 50 && bottom <= 70, "{top} {bottom}");
        // Rendered again it is identical.
        assert_eq!(render_moving(&renderer, history), out);
    }
    // Sixteen moments fill the trail more evenly than four: fewer empty columns.
    let empty = |pixels: &[[f32; 4]]| {
        (86..124)
            .filter(|x| (0..H).all(|y| pixels[(y * W + x) as usize][3] < 0.02))
            .count()
    };
    let (four, sixteen) = (
        empty(&render_moving(&renderer, 4)),
        empty(&render_moving(&renderer, 16)),
    );
    assert!(sixteen < four || four == 0, "{four} {sixteen}");
}
#[test]
#[ignore = "requires Vulkan"]
fn a_depth_picture_hides_the_particles_behind_it() {
    let renderer = Renderer::new().unwrap();
    // Births 50 px in front of and behind the frame, right of its centre: from the
    // home view (222 px away for this lens) they spread over x 113..132, the near
    // ones outermost.
    let mut point = still();
    point.x = 120.0.into();
    point.size = 1.0.into();
    point.depth = 100.0.into();
    let open = render(&renderer, point.clone(), None, (2, 1));
    let (left, right, ..) = span(&open);
    assert!((110..=114).contains(&left) && (130..=135).contains(&right));
    // A wall where the frame is hides everything behind it: only the near half stays.
    let wall = vec![[222.2, 222.2, 222.2, 1.0]; (W * H) as usize];
    let hidden = render_with(&renderer, point.clone(), vec![("depth", wall)], (2, 1));
    let (left, right, ..) = span(&hidden);
    assert!(
        left >= 118 && (130..=135).contains(&right),
        "{left} {right}"
    );
    let lit = |pixels: &[[f32; 4]]| pixels.iter().filter(|p| p[3] > 0.01).count();
    assert!(
        lit(&hidden) < lit(&open),
        "{} of {}",
        lit(&hidden),
        lit(&open)
    );
    // Where the depth picture is empty nothing is hidden, and a far wall hides nothing.
    let empty = vec![[0.0; 4]; (W * H) as usize];
    assert_eq!(
        render_with(&renderer, point.clone(), vec![("depth", empty)], (2, 1)),
        open
    );
    let far = vec![[900.0, 900.0, 900.0, 1.0]; (W * H) as usize];
    assert_eq!(
        render_with(&renderer, point.clone(), vec![("depth", far)], (2, 1)),
        open
    );
    // Beside a target the depth is read from the prepare pass's copy, without one from
    // the picture itself: with a target too, the wall hides some and a far wall none.
    let target = || ("target", vec![[1.0; 4]; (W * H) as usize]);
    let aimed = render_with(&renderer, point.clone(), vec![target()], (2, 1));
    let walled = render_with(
        &renderer,
        point.clone(),
        vec![
            ("depth", vec![[222.2, 222.2, 222.2, 1.0]; (W * H) as usize]),
            target(),
        ],
        (2, 1),
    );
    assert!(
        lit(&walled) < lit(&aimed),
        "{} of {}",
        lit(&walled),
        lit(&aimed)
    );
    assert_eq!(
        render_with(
            &renderer,
            point,
            vec![
                ("depth", vec![[900.0, 900.0, 900.0, 1.0]; (W * H) as usize]),
                target()
            ],
            (2, 1)
        ),
        aimed
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn a_box_sits_in_depth_particles_fly_at_the_viewer_and_a_reach_hides_the_far_ones() {
    let renderer = Renderer::new().unwrap();
    // A still point 40 px right of the centre. Seen from the home view, 222 px away.
    let mut point = still();
    point.x = 120.0.into();
    point.size = 1.0.into();
    let (_, x, _) = mass(&render(&renderer, point.clone(), None, (2, 1)));
    assert!((x - 119.5).abs() < 1.0, "{x}");
    // 74 px nearer it is a third closer, so half as far again from the centre.
    point.z = 74.0.into();
    let (_, x, _) = mass(&render(&renderer, point.clone(), None, (2, 1)));
    assert!((x - 139.5).abs() < 1.5, "{x}");
    // 200 px behind the frame it is 422 px away: drawn within a reach of 600, not of 300.
    point.z = (-200.0).into();
    point.reach = 600.0.into();
    assert!(mass(&render(&renderer, point.clone(), None, (2, 1))).0 > 0.5);
    point.reach = 300.0.into();
    assert_eq!(mass(&render(&renderer, point.clone(), None, (2, 1))).0, 0.0);
    // Launched at the viewer, the particles of the last second lie between the frame
    // and 100 px in front of it: they spread from x 120 outwards, towards x 153.
    point.z = 0.0.into();
    point.reach = 0.0.into();
    point.towards = 100.0.into();
    let (left, right, ..) = span(&render(&renderer, point, None, (2, 1)));
    assert!(
        (117..=121).contains(&left) && (146..=155).contains(&right),
        "{left} {right}"
    );
}
/// Particles born in the first twentieth of a second at the frame's centre, still,
/// each living half a second.
fn shells() -> nodes::Particles {
    let mut shells = still();
    shells.duration = 0.05.into();
    shells.life = 0.5.into();
    shells
}
#[test]
#[ignore = "requires Vulkan"]
fn a_particle_bursts_into_more_where_it_ends() {
    let renderer = Renderer::new().unwrap();
    let lit = |pixels: &[[f32; 4]]| pixels.iter().filter(|p| p[3] > 0.02).count();
    // Without a burst the shells are gone once their life is over.
    assert_eq!(lit(&render(&renderer, shells(), None, (9, 10))), 0);
    let mut fireworks = shells();
    fireworks.burst = 16.0.into();
    fireworks.burst_speed = 60.0.into();
    fireworks.burst_life = 1.0.into();
    // Before the end: only the shells, at the centre.
    let early = render(&renderer, fireworks.clone(), None, (4, 10));
    let (left, right, top, bottom) = span(&early);
    assert!(
        right - left <= 8 && bottom - top <= 8,
        "{left}..{right} {top}..{bottom}"
    );
    // After it: a burst thrown all round from where they ended.
    let late = render(&renderer, fireworks.clone(), None, (9, 10));
    let (left, right, top, bottom) = span(&late);
    assert!(lit(&late) > 0, "no burst");
    assert!(
        right - left >= 30 && bottom - top >= 30,
        "{left}..{right} {top}..{bottom}"
    );
    assert!(
        left < 80 && right > 80 && top < 60 && bottom > 60,
        "round the centre"
    );
    // Every frame on its own: the same frame again is the same picture.
    assert_eq!(render(&renderer, fireworks.clone(), None, (9, 10)), late);
    // Burst particles live their own life: gone a second after the shells ended.
    assert_eq!(lit(&render(&renderer, fireworks, None, (17, 10))), 0);
}
#[test]
#[ignore = "requires Vulkan"]
fn an_obstacle_sets_off_a_burst_thrown_back_off_it() {
    let renderer = Renderer::new().unwrap();
    // Shells falling from the centre at 100 px a second onto a floor at row 80.
    let mut falling = shells();
    falling.life = 2.0.into();
    falling.speed = 100.0.into();
    falling.direction = (-90.0).into();
    falling.spread = 0.0.into();
    falling.burst = 12.0.into();
    falling.burst_speed = 60.0.into();
    falling.burst_spread = 90.0.into();
    falling.burst_life = 1.0.into();
    falling.burst_on_impact = true;
    let floor: Vec<[f32; 4]> = (0..W * H)
        .map(|i| if i / W >= 80 { [1.0; 4] } else { [0.0; 4] })
        .collect();
    let at = |seconds| {
        render_with(
            &renderer,
            falling.clone(),
            vec![("obstacle", floor.clone())],
            seconds,
        )
    };
    // Before reaching the floor: the shells alone, above it.
    let (_, _, _, bottom) = span(&at((1, 10)));
    assert!((60..=74).contains(&bottom), "{bottom}");
    // After: the shells have ended on the floor and their burst flies back up off it.
    let after = at((5, 10));
    let (left, right, top, bottom) = span(&after);
    assert!(after.iter().any(|p| p[3] > 0.02), "no burst off the floor");
    assert!(bottom <= 81, "nothing goes through the floor: {bottom}");
    assert!(
        top < 72 && right - left >= 10,
        "thrown back up and apart: {left}..{right} {top}..{bottom}"
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn a_trail_follows_the_path_the_particle_took() {
    let renderer = Renderer::new().unwrap();
    // Thrown right from the left at 120 px a second and falling: a curve.
    let mut thrown = still();
    thrown.x = 20.0.into();
    thrown.y = 20.0.into();
    thrown.duration = 0.05.into();
    thrown.life = 3.0.into();
    thrown.speed = 120.0.into();
    thrown.direction = 0.0.into();
    thrown.spread = 0.0.into();
    thrown.gravity = 60.0.into();
    let centre = |pixels: &[[f32; 4]]| {
        let lit: Vec<(f32, f32)> = pixels
            .iter()
            .enumerate()
            .filter(|(_, p)| p[3] > 0.05)
            .map(|(i, _)| ((i as u32 % W) as f32, (i as u32 / W) as f32))
            .collect();
        let n = lit.len().max(1) as f32;
        (
            lit.iter().map(|p| p.0).sum::<f32>() / n,
            lit.iter().map(|p| p.1).sum::<f32>() / n,
        )
    };
    // Where the particles were half a second before, drawn alone.
    let (bx, by) = centre(&render(&renderer, thrown.clone(), None, (5, 10)));
    let (ax, ay) = centre(&render(&renderer, thrown.clone(), None, (10, 10)));
    thrown.trail = 0.7.into();
    let trailed = render(&renderer, thrown.clone(), None, (10, 10));
    // The trail passes where they were (within a pixel or two), not along a chord.
    let lit_near = |x: f32, y: f32| {
        (-2..=2).any(|dy| {
            (-2..=2).any(|dx| {
                let (px, py) = (
                    (x.round() as i32 + dx) as u32,
                    (y.round() as i32 + dy) as u32,
                );
                trailed[(py * W + px) as usize][3] > 0.02
            })
        })
    };
    assert!(
        lit_near(bx, by),
        "the trail misses where they were: ({bx}, {by})"
    );
    // Unbroken from there to the head: every column between is lit.
    let row_lit = |x: u32| (0..H).any(|y| trailed[(y * W + x) as usize][3] > 0.02);
    assert!(
        (bx.round() as u32..=ax.round() as u32).all(row_lit),
        "the trail is broken"
    );
    // The curve sags: half way along in time it is lower than the straight line would be.
    assert!(by < ay, "falling: {by} {ay}");
    // Without a trail nothing is drawn back there.
    let plain = render(
        &renderer,
        {
            let mut p = thrown.clone();
            p.trail = 0.0.into();
            p
        },
        None,
        (10, 10),
    );
    let (px, py) = (bx.round() as u32, by.round() as u32);
    assert!(plain[(py * W + px) as usize][3] < 0.02);
}
#[test]
#[ignore = "requires Vulkan"]
fn lines_join_particles_nearer_than_the_link_distance() {
    let renderer = Renderer::new().unwrap();
    // A dozen still particles scattered along a row, all alive at the frame.
    let mut row = still();
    row.width = 120.0.into();
    row.height = 0.0.into();
    row.rate = 3.0.into();
    row.life = 4.0.into();
    row.start = (-4.0).into();
    row.size = 1.0.into();
    let filled = |pixels: &[[f32; 4]]| {
        let lit: Vec<u32> = (0..W)
            .filter(|x| pixels[(60 * W + x) as usize][3] > 0.02)
            .collect();
        let (first, last) = (*lit.first().unwrap(), *lit.last().unwrap());
        lit.len() as f32 / (last - first + 1) as f32
    };
    let plain = render(&renderer, row.clone(), None, (0, 1));
    assert!(
        filled(&plain) < 0.6,
        "dots alone leave gaps: {}",
        filled(&plain)
    );
    // Lines longer than the gaps join them all along the row.
    row.link_distance = 80.0.into();
    let linked = render(&renderer, row.clone(), None, (0, 1));
    assert!(filled(&linked) > 0.95, "joined: {}", filled(&linked));
    // Lines shorter than any gap join nothing: the picture is the dots alone.
    row.link_distance = 0.5.into();
    let short = render(&renderer, row, None, (0, 1));
    let diff: f32 = short
        .iter()
        .zip(&plain)
        .map(|(a, b)| (a[3] - b[3]).abs())
        .sum();
    assert!(diff < 1e-3, "{diff}");
}
#[test]
#[ignore = "requires Vulkan"]
fn curves_shape_size_opacity_colour_and_speed_over_the_life() {
    let renderer = Renderer::new().unwrap();
    // Particles born together at the centre and living four seconds.
    let mut base = still();
    base.duration = 0.05.into();
    base.life = 4.0.into();
    let lit = |pixels: &[[f32; 4]]| pixels.iter().filter(|p| p[3] > 0.02).count();
    let sum = |pixels: &[[f32; 4]], c: usize| pixels.iter().map(|p| p[c]).sum::<f32>();
    // Opacity: hidden for the first half of the life, then shown.
    let mut hidden = base.clone();
    hidden.opacity_over_life = Some([0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
    assert_eq!(
        lit(&render(&renderer, hidden.clone(), None, (1, 2))),
        0,
        "hidden early"
    );
    assert!(
        lit(&render(&renderer, hidden, None, (3, 1))) > 0,
        "shown late"
    );
    // Size: four times as large late in the life as early.
    let mut growing = base.clone();
    growing.size_over_life = Some([1.0, 1.0, 1.0, 1.0, 4.0, 4.0, 4.0, 4.0]);
    let (small, large) = (
        lit(&render(&renderer, growing.clone(), None, (1, 2))),
        lit(&render(&renderer, growing, None, (3, 1))),
    );
    assert!(large > small * 4, "{small} {large}");
    // Colour: red at birth, blue at the end.
    let mut ramp = base.clone();
    ramp.colour_over_life = Some([
        [1.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
    ]);
    let (early, late) = (
        render(&renderer, ramp.clone(), None, (1, 2)),
        render(&renderer, ramp, None, (3, 1)),
    );
    assert!(sum(&early, 0) > 10.0 * sum(&early, 2), "red early");
    assert!(sum(&late, 2) > 10.0 * sum(&late, 0), "blue late");
    // Speed: still for the first half of the life, then away at the launch speed.
    let mut moving = base.clone();
    moving.speed = 40.0.into();
    moving.direction = 0.0.into();
    moving.spread = 0.0.into();
    moving.speed_over_life = Some([0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
    let (_, right_early, ..) = span(&render(&renderer, moving.clone(), None, (3, 2)));
    let (_, right_late, ..) = span(&render(&renderer, moving, None, (3, 1)));
    assert!(right_early <= 83, "held still: {right_early}");
    assert!(right_late >= 100, "then away: {right_late}");
    // No curves: as the plain settings make it.
    assert_eq!(
        render(&renderer, base.clone(), None, (1, 1)),
        render(&renderer, base, None, (1, 1))
    );
}
/// Particles filmed from the home view of a scene that holds a light.
fn render_lit(
    renderer: &Renderer,
    particles: nodes::Particles,
    light: serde_json::Value,
) -> Vec<[f32; 4]> {
    let camera = Node::new(
        "camera",
        serde_json::from_value(registry::CAMERA_3D.default_operation()).unwrap(),
        &[],
    );
    let mut lamp = registry::LIGHT_3D.default_operation();
    for (key, value) in light.as_object().unwrap() {
        lamp[key] = value.clone();
    }
    let lamp = Node::new("lamp", serde_json::from_value(lamp).unwrap(), &[]);
    let merged = Node::new(
        "scene",
        serde_json::from_value(registry::MERGE_SCENES.default_operation()).unwrap(),
        &[("scene", camera.id), ("scene_2", lamp.id)],
    );
    let node = Node::new(
        "particles",
        Operation::Particles(particles),
        &[("scene", merged.id)],
    );
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    doc.output = node.id;
    for n in [camera, lamp, merged, node] {
        doc.nodes.insert(n.id, n);
    }
    let plan = Plan::compile(&doc).unwrap();
    let frame = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(2, 1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap();
    renderer.read_frame(&frame).unwrap().pixels
}
#[test]
#[ignore = "requires Vulkan"]
fn lit_particles_take_their_light_from_the_scene() {
    let renderer = Renderer::new().unwrap();
    let mut point = still();
    point.size = 1.5.into();
    let sum = |pixels: &[[f32; 4]], c: usize| pixels.iter().map(|p| p[c]).sum::<f32>();
    let sun = |colour: [f32; 3], rotation: [f32; 3], intensity: f32| {
        serde_json::json!({
            "kind": "sun",
            "colour": colour,
            "intensity": {"kind": "constant", "value": intensity},
            "placement": {"position": [0.0, 0.0, 400.0], "rotation": rotation, "scale": [1.0, 1.0, 1.0]},
        })
    };
    let glowing = render_lit(&renderer, point.clone(), sun([1.0; 3], [0.0; 3], 1.0));
    // Lit wholly, no light everywhere, no scattering: a white sun of 1 lights them as
    // they glow by themselves.
    point.lit = 1.0.into();
    point.ambient = 0.0.into();
    point.scatter = 0.0.into();
    let white = render_lit(&renderer, point.clone(), sun([1.0; 3], [0.0; 3], 1.0));
    let (a, b) = (sum(&glowing, 0), sum(&white, 0));
    assert!((a - b).abs() < 0.02 * a, "{a} {b}");
    // A red sun makes them red.
    let red = render_lit(
        &renderer,
        point.clone(),
        sun([1.0, 0.0, 0.0], [0.0; 3], 1.0),
    );
    assert!(
        sum(&red, 0) > 0.9 * a && sum(&red, 1) < 0.01 * a,
        "{} {}",
        sum(&red, 0),
        sum(&red, 1)
    );
    // No light reaching them: dark, but still there.
    let dark = render_lit(&renderer, point.clone(), sun([1.0; 3], [0.0; 3], 0.0));
    assert!(sum(&dark, 0) < 0.01 * a && sum(&dark, 3) > 0.5 * sum(&glowing, 3));
    // Scattering forwards: looking into the light (a sun shining at the camera) they are
    // brighter than with the light behind the camera.
    point.scatter = 0.6.into();
    let behind = render_lit(&renderer, point.clone(), sun([1.0; 3], [0.0; 3], 1.0));
    let facing = render_lit(&renderer, point, sun([1.0; 3], [0.0, 180.0, 0.0], 1.0));
    assert!(
        sum(&facing, 0) > 4.0 * sum(&behind, 0),
        "{} {}",
        sum(&facing, 0),
        sum(&behind, 0)
    );
}
/// Particles born on a sphere of radius 20 at the frame's centre (moving from `from` to
/// `to` pixels across over two seconds), filmed from the home view at `seconds`.
fn render_on_sphere(
    renderer: &Renderer,
    mut particles: nodes::Particles,
    from: f32,
    to: f32,
    seconds: (i64, u32),
) -> Vec<[f32; 4]> {
    let camera = Node::new(
        "camera",
        serde_json::from_value(registry::CAMERA_3D.default_operation()).unwrap(),
        &[],
    );
    let mut ball = registry::SHAPE_3D.default_operation();
    ball["kind"] = serde_json::json!("sphere");
    for size in ["size_x", "size_y", "size_z"] {
        ball[size] = serde_json::json!({"kind": "constant", "value": 40.0});
    }
    ball["placement"]["position"] = serde_json::json!({"interpolation": "linear", "keys": [
        {"time": {"ticks": 0, "rate": 1}, "value": [from, 0.0, 0.0]},
        {"time": {"ticks": 2, "rate": 1}, "value": [to, 0.0, 0.0]}]});
    let ball = Node::new("ball", serde_json::from_value(ball).unwrap(), &[]);
    let scene = Node::new(
        "scene",
        serde_json::from_value(registry::MERGE_SCENES.default_operation()).unwrap(),
        &[("scene", camera.id), ("scene_2", ball.id)],
    );
    particles.emit_from = Some(ball.id);
    let node = Node::new(
        "particles",
        Operation::Particles(particles),
        &[("scene", scene.id)],
    );
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    doc.output = node.id;
    for n in [camera, ball, scene, node] {
        doc.nodes.insert(n.id, n);
    }
    let plan = Plan::compile(&doc).unwrap();
    let frame = renderer
        .render_gpu_cached(
            &plan,
            RenderRequest::full(Time::new(seconds.0, seconds.1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap();
    renderer.read_frame(&frame).unwrap().pixels
}
#[test]
#[ignore = "requires Vulkan"]
fn particles_are_born_on_a_3d_object_and_launched_out_of_it() {
    let renderer = Renderer::new().unwrap();
    // A burst at the start, living two seconds, small dots.
    let mut burst = still();
    burst.rate = 4000.0.into();
    burst.duration = 0.05.into();
    burst.life = 2.0.into();
    burst.size = 0.7.into();
    let distances = |pixels: &[[f32; 4]], cx: f32| -> Vec<f32> {
        pixels
            .iter()
            .enumerate()
            .filter(|(_, p)| p[3] > 0.05)
            .map(|(i, _)| {
                let (x, y) = ((i as u32 % W) as f32 + 0.5, (i as u32 / W) as f32 + 0.5);
                ((x - cx).powi(2) + (y - 60.0).powi(2)).sqrt()
            })
            .collect()
    };
    // Still: on the sphere, so within its outline on the frame, and not all at its middle.
    let on = distances(
        &render_on_sphere(&renderer, burst.clone(), 0.0, 0.0, (1, 2)),
        80.0,
    );
    assert!(!on.is_empty(), "nothing born on the sphere");
    assert!(
        on.iter().all(|d| *d <= 22.5),
        "beyond the sphere: {:?}",
        on.iter().cloned().fold(0.0, f32::max)
    );
    assert!(
        on.iter().any(|d| *d > 15.0),
        "the sphere's edge is covered too"
    );
    // Launched out of it at 30 px a second: a second later all are well away from it.
    let mut thrown = burst.clone();
    thrown.speed = 30.0.into();
    thrown.spread = 0.0.into();
    let away = distances(&render_on_sphere(&renderer, thrown, 0.0, 0.0, (1, 1)), 80.0);
    let mean = away.iter().sum::<f32>() / away.len().max(1) as f32;
    assert!(mean > 26.0, "out of the surface: {mean}");
    // A moving sphere leaves them where it was at their birth: at the left, not where it
    // has gone.
    // (It moves 2 px during the burst's twentieth of a second.)
    let left = distances(
        &render_on_sphere(&renderer, burst, -40.0, 40.0, (3, 2)),
        41.0,
    );
    assert!(
        !left.is_empty() && left.iter().all(|d| *d <= 24.0),
        "left behind at x 40, where it is now x 100"
    );
}
/// Particles driven by a chain of forces (each a JSON object over the force defaults),
/// rendered at `seconds`.
fn forced(
    renderer: &Renderer,
    particles: nodes::Particles,
    forces: &[serde_json::Value],
    seconds: (i64, u32),
) -> Vec<[f32; 4]> {
    let mut doc = blank(W, H, Time::new(24, 1).unwrap()).unwrap();
    let mut next: Option<NodeId> = None;
    for settings in forces.iter().rev() {
        let mut value = registry::FORCE.default_operation();
        for (k, v) in settings.as_object().unwrap() {
            value[k] = if v.is_number() {
                serde_json::json!({"kind": "constant", "value": v})
            } else {
                v.clone()
            };
        }
        let inputs: Vec<(&str, NodeId)> = next.iter().map(|id| ("forces", *id)).collect();
        let node = Node::new("force", serde_json::from_value(value).unwrap(), &inputs);
        next = Some(node.id);
        doc.nodes.insert(node.id, node);
    }
    let inputs: Vec<(&str, NodeId)> = next.iter().map(|id| ("forces", *id)).collect();
    let node = Node::new("particles", Operation::Particles(particles), &inputs);
    doc.output = node.id;
    doc.nodes.insert(node.id, node);
    let plan = Plan::compile(&doc).unwrap();
    renderer
        .render_frame(
            &plan,
            RenderRequest::full(Time::new(seconds.0, seconds.1).unwrap()),
            &ImageInputs::default(),
        )
        .unwrap()
        .pixels
}
fn centre_x(pixels: &[[f32; 4]]) -> f32 {
    let (mut sum, mut weight) = (0.0, 0.0);
    for (i, p) in pixels.iter().enumerate() {
        sum += (i as u32 % W) as f32 * p[3];
        weight += p[3];
    }
    sum / weight.max(1e-6)
}
#[test]
#[ignore = "requires Vulkan"]
fn chained_forces_pull_particles_into_two_clumps() {
    let renderer = Renderer::new().unwrap();
    // A still cloud over the whole frame, there from the first frame.
    let mut cloud = still();
    cloud.width = 160.0.into();
    cloud.height = 120.0.into();
    // Sparse, so dots seldom overlap and coverage still grows where they gather.
    cloud.rate = 400.0.into();
    cloud.life = 3.0.into();
    cloud.start = (-3.0).into();
    cloud.size = 0.7.into();
    let near = |pixels: &[[f32; 4]], cx: f32| {
        pixels
            .iter()
            .enumerate()
            .filter(|(i, p)| {
                let (x, y) = ((*i as u32 % W) as f32, (*i as u32 / W) as f32);
                p[3] > 0.05 && (x - cx).powi(2) + (y - 60.0).powi(2) < 64.0
            })
            .map(|(_, p)| p[3])
            .sum::<f32>()
    };
    let corner = |pixels: &[[f32; 4]]| {
        pixels
            .iter()
            .enumerate()
            .filter(|(i, p)| (*i as u32 % W) < 12 && (*i as u32 / W) < 12 && p[3] > 0.05)
            .count()
    };
    let free = forced(&renderer, cloud.clone(), &[], (0, 1));
    let pulled = forced(
        &renderer,
        cloud,
        &[
            serde_json::json!({"kind": "attract", "strength": 3000.0, "x": 40.0, "y": 60.0, "radius": 45.0, "softness": 0.0}),
            serde_json::json!({"kind": "attract", "strength": 3000.0, "x": 120.0, "y": 60.0, "radius": 45.0, "softness": 0.0}),
            serde_json::json!({"kind": "drag", "strength": 4.0}),
        ],
        (0, 1),
    );
    for x in [40.0, 120.0] {
        assert!(
            near(&pulled, x) > 4.0 * near(&free, x),
            "clumped at x {x}: {} against {}",
            near(&pulled, x),
            near(&free, x)
        );
    }
    // Out of both reaches nothing moves them.
    assert_eq!(corner(&pulled), corner(&free), "the corner is out of reach");
}
#[test]
#[ignore = "requires Vulkan"]
fn wind_blows_within_its_reach_and_drag_slows_it() {
    let renderer = Renderer::new().unwrap();
    // A puff at x 40 at the start, looked at a second later.
    let mut puff = still();
    puff.x = 40.0.into();
    puff.rate = 2000.0.into();
    puff.duration = 0.05.into();
    puff.life = 2.0.into();
    puff.start = 0.0.into();
    puff.size = 1.0.into();
    let wind = serde_json::json!({"kind": "wind", "strength": 100.0, "direction": 0.0});
    let blown = centre_x(&forced(
        &renderer,
        puff.clone(),
        std::slice::from_ref(&wind),
        (1, 1),
    ));
    // Half of 100 px/s² for about a second: some 50 px to the right.
    assert!((80.0..96.0).contains(&blown), "blown to {blown}");
    let slowed = centre_x(&forced(
        &renderer,
        puff.clone(),
        &[
            wind.clone(),
            serde_json::json!({"kind": "drag", "strength": 3.0}),
        ],
        (1, 1),
    ));
    assert!(slowed < blown - 15.0 && slowed > 45.0, "slowed to {slowed}");
    // A wind whose reach ends before the puff does nothing to it.
    let mut far = wind;
    far["x"] = 140.0.into();
    far["y"] = 60.0.into();
    far["radius"] = 30.0.into();
    let untouched = centre_x(&forced(&renderer, puff, &[far], (1, 1)));
    assert!((untouched - 40.0).abs() < 1.0, "out of reach: {untouched}");
}
/// How far a picture's particles are spread: the mean distance of its light from the
/// light's middle.
fn spread_of(pixels: &[[f32; 4]]) -> f32 {
    let (mut x, mut y, mut w) = (0.0, 0.0, 0.0);
    for (i, p) in pixels.iter().enumerate() {
        x += (i as u32 % W) as f32 * p[3];
        y += (i as u32 / W) as f32 * p[3];
        w += p[3];
    }
    let (cx, cy) = (x / w, y / w);
    pixels
        .iter()
        .enumerate()
        .map(|(i, p)| {
            (((i as u32 % W) as f32 - cx).powi(2) + ((i as u32 / W) as f32 - cy).powi(2)).sqrt()
                * p[3]
        })
        .sum::<f32>()
        / w
}
fn render_alone(
    renderer: &Renderer,
    particles: &nodes::Particles,
    seconds: (i64, u32),
) -> Vec<[f32; 4]> {
    forced(renderer, particles.clone(), &[], seconds)
}
/// A puff of particles flying off every way from the middle, all born at once.
fn scattering() -> nodes::Particles {
    let mut puff = still();
    puff.width = 20.0.into();
    puff.height = 20.0.into();
    puff.rate = 6000.0.into();
    puff.duration = 0.05.into();
    puff.life = 10.0.into();
    puff.speed = 25.0.into();
    puff.spread = 360.0.into();
    puff.size = 0.7.into();
    puff
}
#[test]
#[ignore = "requires Vulkan"]
fn a_flock_keeps_together_and_keeps_its_distance() {
    let renderer = Renderer::new().unwrap();
    let free = spread_of(&render_alone(&renderer, &scattering(), (2, 1)));
    let mut flock = scattering();
    flock.flock_radius = 30.0.into();
    flock.cohesion = 120.0.into();
    flock.alignment = 1.0.into();
    flock.top_speed = 30.0.into();
    let together = spread_of(&render_alone(&renderer, &flock, (2, 1)));
    assert!(
        together < 0.8 * free,
        "kept together: {together} against {free}"
    );
    let mut apart = flock.clone();
    apart.separation = 60.0.into();
    let spaced = spread_of(&render_alone(&renderer, &apart, (2, 1)));
    assert!(
        spaced > together * 1.1,
        "kept their distance: {spaced} against {together}"
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn a_flocks_frame_is_the_same_however_it_is_reached() {
    let mut flock = scattering();
    flock.flock_radius = 30.0.into();
    flock.cohesion = 120.0.into();
    flock.separation = 40.0.into();
    flock.alignment = 1.0.into();
    flock.top_speed = 30.0.into();
    flock.turbulence = 20.0.into();
    // Straight to 2.5 s in a renderer of its own.
    let straight = render_alone(&Renderer::new().unwrap(), &flock, (5, 2));
    // Played on to it, frame after frame, in another.
    let playing = Renderer::new().unwrap();
    for frame in 1..=60 {
        let _ = render_alone(&playing, &flock, (frame, 24));
    }
    let played = render_alone(&playing, &flock, (5, 2));
    assert!(straight.iter().any(|p| p[3] > 0.0), "something is drawn");
    assert!(straight == played, "played on to it, the frame is the same");
    // Back to an earlier frame and on again, from what was kept.
    let _ = render_alone(&playing, &flock, (1, 2));
    assert!(
        render_alone(&playing, &flock, (5, 2)) == straight,
        "and after a seek back"
    );
}
/// A sprite sheet of two cells: red on the left, green on the right.
fn red_green() -> Vec<[f32; 4]> {
    let mut sheet = block(0..W / 2, 0..H, [1.0, 0.0, 0.0, 1.0]);
    for (i, p) in sheet.iter_mut().enumerate() {
        if i as u32 % W >= W / 2 {
            *p = [0.0, 1.0, 0.0, 1.0];
        }
    }
    sheet
}
/// How much red and green light a picture has.
fn red_and_green(pixels: &[[f32; 4]]) -> (f32, f32) {
    pixels
        .iter()
        .fold((0.0, 0.0), |(r, g), p| (r + p[0], g + p[1]))
}
#[test]
#[ignore = "requires Vulkan"]
fn a_sprite_sheet_gives_each_particle_a_cell() {
    let renderer = Renderer::new().unwrap();
    let mut many = still();
    many.width = 140.0.into();
    many.height = 100.0.into();
    many.rate = 60.0.into();
    many.duration = 0.05.into();
    many.life = 2.0.into();
    many.size = 3.0.into();
    many.sprite_columns = 2;
    let shot = |p: &nodes::Particles, t: (i64, u32)| {
        red_and_green(&render_with(
            &renderer,
            p.clone(),
            vec![("sprite", red_green())],
            t,
        ))
    };
    // Picked at random: some particles red, some green, and each a whole cell (no
    // particle is half red, half green: the whole image would be).
    let (red, green) = shot(&many, (1, 2));
    assert!(
        red > 0.25 * (red + green) && green > 0.25 * (red + green),
        "{red} {green}"
    );
    // Over the life: all red in the first half of it, all green in the second.
    let mut life = many.clone();
    life.sprite_play = nodes::SpritePlay::OverLife;
    let (red, green) = shot(&life, (1, 2));
    assert!(red > 0.0 && green == 0.0, "first half {red} {green}");
    let (red, green) = shot(&life, (3, 2));
    assert!(green > 0.0 && red == 0.0, "second half {red} {green}");
    // At a rate: two cells a second, so each particle has changed cell half a second on.
    let mut rate = many.clone();
    rate.sprite_play = nodes::SpritePlay::AtRate;
    rate.sprite_rate = 2.0.into();
    let a = render_with(
        &renderer,
        rate.clone(),
        vec![("sprite", red_green())],
        (5, 4),
    );
    let b = render_with(&renderer, rate, vec![("sprite", red_green())], (7, 4));
    let flipped = a
        .iter()
        .zip(&b)
        .filter(|(x, y)| x[3] > 0.5 && y[3] > 0.5 && (x[0] > 0.5) != (y[0] > 0.5))
        .count();
    let held = a
        .iter()
        .zip(&b)
        .filter(|(x, y)| x[3] > 0.5 && y[3] > 0.5)
        .count();
    assert!(
        held > 50 && flipped as f32 > 0.9 * held as f32,
        "{flipped} of {held}"
    );
    // One cell is the whole image, as before: half red, half green in each particle.
    let mut whole = many;
    whole.sprite_columns = 1;
    let (red, green) = shot(&whole, (1, 2));
    assert!((red / green - 1.0).abs() < 0.2, "{red} {green}");
}
#[test]
#[ignore = "requires Vulkan"]
fn a_light_limit_rolls_dense_light_off_and_leaves_sparse_light_alone() {
    // 400 glowing particles on one spot add up to far more light than white; with a limit
    // of 2 the brightest pixel reaches no more than 2, in its own colour, and every pixel that
    // gathered under half the limit is exactly as it was.
    let renderer = Renderer::new().unwrap();
    let mut dense = still();
    dense.colour = [1.0, 0.5, 0.25, 1.0];
    dense.end_colour = dense.colour;
    let free = render(&renderer, dense.clone(), None, (1, 2));
    dense.light_limit = 2.0.into();
    let limited = render(&renderer, dense, None, (1, 2));
    let peak = |p: &[f32; 4]| p[0].max(p[1]).max(p[2]);
    let (brightest, _) = free
        .iter()
        .enumerate()
        .max_by(|a, b| peak(a.1).total_cmp(&peak(b.1)))
        .unwrap();
    assert!(peak(&free[brightest]) > 20.0, "{:?}", free[brightest]);
    let top = limited[brightest];
    assert!(peak(&top) <= 2.0 && peak(&top) > 1.9, "{top:?}");
    assert!(
        (top[1] / top[0] - 0.5).abs() < 1e-3 && (top[2] / top[0] - 0.25).abs() < 1e-3,
        "{top:?}"
    );
    let mut untouched = 0;
    for (a, b) in free.iter().zip(&limited) {
        assert!(peak(b) <= peak(a) + 1e-6 && peak(b) <= 2.0, "{a:?} {b:?}");
        if peak(a) < 1.0 {
            assert_eq!(a, b);
            untouched += usize::from(peak(a) > 0.0);
        }
    }
    assert!(
        untouched > 0,
        "the dot's soft rim should be under half the limit"
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn a_flock_born_in_a_short_burst_has_every_bird_of_the_burst() {
    // A thousand birds born in half a second that live thirty: as a flock they used to
    // get one thread's birth for every life, so only a sixtieth of them were ever born.
    let renderer = Renderer::new().unwrap();
    let mut burst = particles();
    burst.x = 80.0.into();
    burst.y = 60.0.into();
    burst.width = 140.0.into();
    burst.height = 100.0.into();
    burst.rate = 2000.0.into();
    burst.duration = 0.5.into();
    burst.life = 30.0.into();
    burst.speed = 0.0.into();
    burst.gravity = 0.0.into();
    burst.variation = 0.0.into();
    burst.fade = 0.0.into();
    burst.size = 0.6.into();
    let lit = |p: &[[f32; 4]]| p.iter().filter(|p| p[3] > 0.05).count();
    let free = lit(&render(&renderer, burst.clone(), None, (1, 1)));
    let mut flock = burst;
    flock.flock_radius = 6.0.into();
    let flocked = lit(&render(&renderer, flock, None, (1, 1)));
    assert!(free > 500, "{free}");
    assert!(
        flocked as f32 > free as f32 * 0.85,
        "flock {flocked} of {free}"
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn birds_born_on_one_point_push_one_another_apart() {
    // Two thousand birds born on the very same point: each keeps clear of the birds
    // themselves near it (not of the middle of their cell, which is where they all are),
    // so they spread into a disc instead of staying one dot.
    let renderer = Renderer::new().unwrap();
    let mut flock = particles();
    flock.x = 80.0.into();
    flock.y = 60.0.into();
    flock.rate = 4000.0.into();
    flock.duration = 0.5.into();
    flock.life = 30.0.into();
    flock.speed = 0.0.into();
    flock.gravity = 0.0.into();
    flock.variation = 0.0.into();
    flock.fade = 0.0.into();
    flock.size = 0.6.into();
    flock.flock_radius = 24.0.into();
    flock.separation = 400.0.into();
    flock.top_speed = 40.0.into();
    let lit = render(&renderer, flock, None, (2, 1))
        .iter()
        .filter(|p| p[3] > 0.02)
        .count();
    assert!(lit > 150, "the birds spread over {lit} pixels");
}
