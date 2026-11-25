//! How long a pose and its body mesh take: cargo run --release -p veylvik-figure --example timing
fn main() {
    for detail in [80, 110, 160] {
        let start = std::time::Instant::now();
        let mut triangles = 0;
        for k in 0..20 {
            let pose = veylvik_figure::walk(k as f32 * 0.05, 1.0);
            triangles = veylvik_figure::mesh(&pose, 1000.0, detail).indices.len() / 3;
        }
        println!(
            "detail {detail}: {:.1} ms a body, {triangles} triangles",
            start.elapsed().as_secs_f64() * 1000.0 / 20.0
        );
    }
}
