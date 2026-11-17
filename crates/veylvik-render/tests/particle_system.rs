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
