use super::*;
use veylvik_mesh::{Primitive, primitive};

fn rotated(degrees: f32, scale: f32, at: [f32; 3]) -> Mat4 {
    let (s, c) = degrees.to_radians().sin_cos();
    [
        [c * scale, 0.0, s * scale, at[0]],
        [0.0, scale, 0.0, at[1]],
        [-s * scale, 0.0, c * scale, at[2]],
        [0.0, 0.0, 0.0, 1.0],
    ]
}
/// Every triangle of every placing in world space, for testing all of them.
fn world_triangles(placed: &[Placed]) -> Vec<[[f32; 3]; 3]> {
    let mut out = Vec::new();
    for p in placed {
        for t in &p.tree.triangles {
            out.push([0, 1, 2].map(|j| {
                let v = p.tree.vertices[2 * t[j] as usize];
                apply(&p.world, [v[0], v[1], v[2]])
            }));
        }
    }
    out
}
#[test]
fn the_packed_scene_is_hit_where_testing_every_placed_triangle_says() {
    let ball = Arc::new(MeshTree::new(
        &primitive(Primitive::Sphere, [60.0; 3], 24).unwrap(),
    ));
    let block = Arc::new(MeshTree::new(
        &primitive(Primitive::Cube, [80.0, 40.0, 30.0], 1).unwrap(),
    ));
    let s = Surface::default();
    // One mesh placed three times, turned and scaled; another twice; one placing flat.
    let placed = vec![
        Placed {
            tree: ball.clone(),
            world: rotated(0.0, 1.0, [0.0, 0.0, 0.0]),
            surface: s,
            tint: [1.0; 3],
        },
        Placed {
            tree: ball.clone(),
            world: rotated(30.0, 1.5, [120.0, -20.0, -50.0]),
            surface: s,
            tint: [1.0; 3],
        },
        Placed {
            tree: ball.clone(),
            world: rotated(-70.0, 0.5, [-90.0, 40.0, 30.0]),
            surface: s,
            tint: [1.0; 3],
        },
        Placed {
            tree: block.clone(),
            world: rotated(45.0, 1.0, [0.0, 90.0, 0.0]),
            surface: s,
            tint: [1.0; 3],
        },
        Placed {
            tree: block.clone(),
            world: rotated(10.0, 2.0, [-60.0, -80.0, -100.0]),
            surface: s,
            tint: [1.0; 3],
        },
        Placed {
            tree: block.clone(),
            world: rotated(0.0, 0.0, [0.0; 3]),
            surface: s,
            tint: [1.0; 3],
        },
    ];
    let packed = pack(&placed);
    assert_eq!(packed.instances.len(), 5, "the flat placing is left out");
    assert_eq!(packed.surfaces.len(), 1, "one surface stored once");
    assert_eq!(
        packed.triangles.len(),
        ball.triangles.len() + block.triangles.len(),
        "each mesh stored once"
    );
    let all = world_triangles(&placed[..5]);
    let mut seed = 12345u64;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    };
    let mut hits = 0;
    for _ in 0..3000 {
        let o = [rnd() * 400.0, rnd() * 400.0, 600.0 + rnd() * 100.0];
        let target = [rnd() * 150.0, rnd() * 150.0, rnd() * 100.0];
        let d = [0, 1, 2].map(|k| target[k] - o[k]);
        let tree = packed.hit(o, d, f32::INFINITY).map(|h| h.t);
        let every = all
            .iter()
            .filter_map(|c| crate::bvh::triangle(*c, o, d).map(|h| h.0))
            .fold(None, |b: Option<f32>, t| Some(b.map_or(t, |b| b.min(t))));
        match (tree, every) {
            (None, None) => {}
            (Some(a), Some(b)) => {
                hits += 1;
                assert!((a - b).abs() <= 1e-4 * b.max(1.0), "{a} {b}");
            }
            _ => panic!("tree {tree:?}, every triangle {every:?}"),
        }
    }
    assert!(hits > 500, "{hits}");
}
#[test]
fn an_empty_scene_is_hit_by_nothing() {
    let packed = pack(&[]);
    assert!(
        packed
            .hit([0.0; 3], [0.0, 0.0, -1.0], f32::INFINITY)
            .is_none()
    );
}
