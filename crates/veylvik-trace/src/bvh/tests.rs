use super::*;

/// A small repeatable random source, so a failure can be reproduced.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
    fn in_range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}
/// The nearest triangle by testing every one.
fn brute(positions: &[[f32; 3]], indices: &[u32], o: [f32; 3], d: [f32; 3]) -> Option<Hit> {
    let mut best: Option<Hit> = None;
    for t in 0..indices.len() / 3 {
        if let Some((dist, u, v)) = triangle(corners(positions, indices, t), o, d)
            && best.is_none_or(|b| dist < b.t)
        {
            best = Some(Hit {
                t: dist,
                triangle: t as u32,
                u,
                v,
            });
        }
    }
    best
}
/// `count` small triangles scattered in a box, and rays from outside through it.
fn soup(seed: u64, count: usize, spread: f32) -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut r = Rng(seed);
    let mut positions = Vec::new();
    for _ in 0..count {
        let c = [0; 3].map(|_| r.in_range(-spread, spread));
        for _ in 0..3 {
            positions.push([0, 1, 2].map(|k| c[k] + r.in_range(-1.0, 1.0)));
        }
    }
    let indices = (0..positions.len() as u32).collect();
    (positions, indices)
}
fn agrees(positions: &[[f32; 3]], indices: &[u32], seed: u64, rays: usize) {
    let bvh = Bvh::build(positions, indices);
    let mut r = Rng(seed ^ 0x9e37_79b9);
    let mut hits = 0;
    for _ in 0..rays {
        let o = [0; 3].map(|_| r.in_range(-40.0, 40.0));
        // Most rays aim near a triangle's middle; the rest anywhere in the box.
        let target = if r.next() < 0.7 {
            let t = ((r.next() * (indices.len() / 3) as f32) as usize).min(indices.len() / 3 - 1);
            let c = corners(positions, indices, t);
            [0, 1, 2].map(|k| (c[0][k] + c[1][k] + c[2][k]) / 3.0 + r.in_range(-0.2, 0.2))
        } else {
            [0; 3].map(|_| r.in_range(-8.0, 8.0))
        };
        let d = [0, 1, 2].map(|k| target[k] - o[k]);
        let (a, b) = (
            bvh.hit(positions, indices, o, d, f32::INFINITY),
            brute(positions, indices, o, d),
        );
        match (a, b) {
            (None, None) => {}
            (Some(a), Some(b)) => {
                hits += 1;
                // The same distance; the same triangle unless two meet the ray at that distance.
                assert!((a.t - b.t).abs() <= 1e-6 * b.t.max(1.0), "{a:?} {b:?}");
            }
            _ => panic!("the tree and every triangle disagree: {a:?} {b:?}"),
        }
    }
    assert!(hits > rays / 3, "the rays mostly miss: {hits}");
}
#[test]
fn the_tree_finds_the_same_nearest_triangle_as_testing_every_one() {
    for (seed, count) in [(1, 1), (2, 5), (3, 200), (4, 5000)] {
        let (p, i) = soup(seed, count, 10.0);
        agrees(&p, &i, seed, 2000);
    }
}
#[test]
fn triangles_all_in_one_place_still_make_a_bounded_tree() {
    // Every centroid the same: no split separates them, so the tree halves by count.
    let mut positions = Vec::new();
    for k in 0..300 {
        let s = 1.0 + k as f32 * 0.001;
        positions.extend([[-s, -1.0, 0.0], [s, -1.0, 0.0], [0.0, 2.0, 0.0]]);
    }
    let indices: Vec<u32> = (0..positions.len() as u32).collect();
    let bvh = Bvh::build(&positions, &indices);
    assert!(bvh.nodes.iter().all(|n| n.count as usize <= 2 * LEAF));
    assert!(bvh.nodes.len() < 2 * 300);
    agrees(&positions, &indices, 7, 500);
}
#[test]
fn every_triangle_is_in_exactly_one_leaf_and_inside_its_box() {
    let (p, i) = soup(9, 3000, 30.0);
    let bvh = Bvh::build(&p, &i);
    let mut seen = vec![0u32; 3000];
    for n in bvh.nodes.iter().filter(|n| n.count > 0) {
        for &t in &bvh.order[n.first as usize..(n.first + n.count) as usize] {
            seen[t as usize] += 1;
            for c in corners(&p, &i, t as usize) {
                assert!((0..3).all(|k| c[k] >= n.min[k] && c[k] <= n.max[k]));
            }
        }
    }
    assert!(seen.iter().all(|&s| s == 1));
}
#[test]
fn an_empty_mesh_is_hit_by_nothing() {
    let bvh = Bvh::build(&[], &[]);
    assert_eq!(bvh.nodes.len(), 1);
    assert!(
        bvh.hit(&[], &[], [0.0; 3], [0.0, 0.0, 1.0], f32::INFINITY)
            .is_none()
    );
}
#[test]
#[ignore = "timing; run in release"]
fn how_long_a_large_tree_takes_to_build() {
    let (p, i) = soup(42, 500_000, 400.0);
    let start = std::time::Instant::now();
    let bvh = Bvh::build(&p, &i);
    let built = start.elapsed();
    let start = std::time::Instant::now();
    agrees(&p, &i, 3, 20_000);
    eprintln!(
        "500,000 triangles: built in {built:?} ({} nodes); 20,000 rays checked in {:?}",
        bvh.nodes.len(),
        start.elapsed()
    );
}
