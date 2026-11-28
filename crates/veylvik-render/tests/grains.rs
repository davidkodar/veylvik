//! Grains: particles simulated as sand, landing on one another and the floor and piling up.
use veylvik_document::*;
use veylvik_render::*;

const W: u32 = 320;
const H: u32 = 240;
fn particles() -> nodes::Particles {
    let Operation::Particles(particles) =
        serde_json::from_value(registry::PARTICLES.default_operation()).unwrap()
    else {
        unreachable!()
    };
    particles
}
/// Sand poured from a spout at the top middle onto the floor (the frame's bottom edge).
fn sand(friction: f32) -> nodes::Particles {
    let mut p = particles();
    p.grains = true;
    p.friction = friction.into();
    p.x = 160.0.into();
    p.y = 40.0.into();
    p.width = 8.0.into();
    p.height = 4.0.into();
    p.rate = 400.0.into();
    p.life = 60.0.into();
    p.duration = 4.0.into();
    p.speed = 0.0.into();
    p.gravity = 600.0.into();
    p.turbulence = 0.0.into();
    p.variation = 0.0.into();
    p.size = 1.5.into();
    p
}
/// Where every grain alive is at `seconds`, simulated.
fn grains_at(renderer: &Renderer, sand: nodes::Particles, seconds: i64) -> Vec<[f32; 3]> {
    let mut doc = blank(W, H, Time::new(30, 1).unwrap()).unwrap();
    let node = Node::new("sand", Operation::Particles(sand), &[]);
    let id = node.id;
    doc.output = id;
    doc.nodes.insert(id, node);
    let plan = Plan::compile(&doc).unwrap();
    renderer
        .flock_states(
            &plan,
            id,
            Time::new(seconds, 1).unwrap(),
            &ImageInputs::default(),
        )
        .unwrap()
        .into_iter()
        .filter(|s| s[3] >= 0.0)
        .map(|s| [s[0], s[1], s[2]])
        .collect()
}
/// The pile's height above the floor and its width at its base, and how many pairs of
/// grains overlap by more than a quarter of a grain's width.
fn pile(g: &[[f32; 3]]) -> (f32, f32, usize) {
    let floor = H as f32 - 1.5;
    let top = g.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
    let on: Vec<f32> = g
        .iter()
        .filter(|p| p[1] > floor - 3.0)
        .map(|p| p[0])
        .collect();
    // The base: the middle 98% of the grains near the floor (a few roll off far).
    let mut xs = on.clone();
    xs.sort_by(f32::total_cmp);
    let k = xs.len() / 100;
    let width = xs[xs.len() - 1 - k] - xs[k];
    let mut overlaps = 0;
    for i in 0..g.len() {
        for j in i + 1..g.len() {
            let d = ((g[i][0] - g[j][0]).powi(2) + (g[i][1] - g[j][1]).powi(2)).sqrt();
            if d < 2.25 {
                overlaps += 1;
            }
        }
    }
    (floor - top, width, overlaps)
}
#[test]
#[ignore = "requires Vulkan"]
fn poured_grains_pile_up_steeper_the_more_friction_they_have() {
    let renderer = Renderer::new().unwrap();
    let mut slopes = Vec::new();
    for friction in [0.1, 0.5, 0.9] {
        let g = grains_at(&renderer, sand(friction), 8);
        assert_eq!(g.len(), 1600, "every grain poured is there");
        let (height, width, overlaps) = pile(&g);
        // Grains rest on one another rather than passing into each other.
        assert!(
            overlaps < g.len() / 100,
            "{friction}: {overlaps} overlapping pairs"
        );
        assert!(height > 20.0, "{friction}: a pile stands ({height} high)");
        slopes.push(height / (width * 0.5));
    }
    assert!(
        slopes[0] < slopes[1] && slopes[1] < slopes[2],
        "more friction, steeper sides: {slopes:?}"
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn a_pile_left_alone_stays_as_it_is() {
    let renderer = Renderer::new().unwrap();
    let (early, late) = (
        grains_at(&renderer, sand(0.5), 9),
        grains_at(&renderer, sand(0.5), 14),
    );
    let (h1, w1, _) = pile(&early);
    let (h2, w2, _) = pile(&late);
    assert!(
        (h1 - h2).abs() < 2.0 && (w1 - w2).abs() < 3.0,
        "{h1} {w1} -> {h2} {w2}"
    );
}
#[test]
#[ignore = "requires Vulkan"]
fn a_grains_frame_is_the_same_however_it_is_reached() {
    // Straight to 6 s, and through 2 s and 4 s first (from the copies kept on the way).
    let straight = grains_at(&Renderer::new().unwrap(), sand(0.5), 6);
    let renderer = Renderer::new().unwrap();
    for t in [2, 4] {
        grains_at(&renderer, sand(0.5), t);
    }
    let stepped = grains_at(&renderer, sand(0.5), 6);
    assert_eq!(straight, stepped);
}
