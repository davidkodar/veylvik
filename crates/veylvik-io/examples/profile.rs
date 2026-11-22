//! Where a frame's time goes: renders the composition at a few frames, then each node as if it were the output (so a node's figure includes everything upstream of it), and lists the dearest. cargo run --release -p veylvik-io --example profile -- PROJECT.veylvik FRAME...
use std::time::Instant;
use veylvik_document::{Composition, Time};
use veylvik_render::{Plan, RenderRequest};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = std::path::Path::new(
        args.first()
            .ok_or("profile needs PROJECT.veylvik FRAME...")?,
    );
    let doc = Composition::load(path)?;
    let base = veylvik_io::document_base(path)?;
    let begun = Instant::now();
    let mut renderer = veylvik_io::runtime::renderer()?;
    // To compare: PROFILE_NO_AREA=1 runs every pass over the whole frame.
    if std::env::var_os("PROFILE_NO_AREA").is_some() {
        renderer.area_dispatch = false;
    }
    // PROFILE_NO_KEPT=1: no node's result is kept between frames.
    if std::env::var_os("PROFILE_NO_KEPT").is_some() {
        renderer.keep_results = false;
    }
    // What a first frame costs, part by part, when asked for (a host wants to know what
    // to do beforehand so its first frame does not wait).
    if std::env::var_os("PROFILE_START").is_some() {
        let ms = |since: Instant| since.elapsed().as_secs_f64() * 1000.0;
        println!("starting the renderer: {:.0} ms", ms(begun));
        let at = Time::at_frame(args.get(1).map_or(Ok(0), |f| f.parse())?, doc.frame_rate)?;
        let step = Instant::now();
        let plan = Plan::compile(&doc)?;
        println!("compiling the plan: {:.1} ms", ms(step));
        let step = Instant::now();
        let inputs = veylvik_io::load_inputs(&plan, &base, renderer.budget_bytes)?;
        println!("loading the inputs: {:.0} ms", ms(step));
        if std::env::var_os("PROFILE_WARM").is_some() {
            let step = Instant::now();
            let count = renderer.compile_for(&plan)?;
            println!(
                "compiling the {count} kernels this plan uses, beforehand: {:.0} ms",
                ms(step)
            );
        }
        for n in 0..3 {
            let step = Instant::now();
            let times = renderer.time_passes(&plan, RenderRequest::full(at), &inputs)?;
            println!("render {}: {:.0} ms", n + 1, ms(step));
            if n == 0 {
                let mut slow: Vec<_> = times.iter().filter(|t| t.milliseconds > 100.0).collect();
                slow.sort_by(|a, b| b.milliseconds.total_cmp(&a.milliseconds));
                for t in slow {
                    println!(
                        "    {:.0} ms  {}  ({} {})",
                        t.milliseconds, t.node, t.kind, t.pass
                    );
                }
            }
        }
        return Ok(());
    }
    // Everything submitted so far has finished on the GPU.
    let done = || {
        renderer.compute(|scratch| {
            let encoder = scratch.device().create_command_encoder(&Default::default());
            scratch.run(encoder)
        })
    };
    // The time of one render that nothing has cached: the fastest of three neighbouring frames.
    let cost = |plan: &Plan,
                inputs: &veylvik_render::ImageInputs,
                frame: i64|
     -> Result<f64, Box<dyn std::error::Error>> {
        let mut best = f64::MAX;
        for k in 1..=3 {
            let at = Time::at_frame(frame + k, doc.frame_rate)?;
            let begun = Instant::now();
            let picture = renderer.render_gpu(plan, RenderRequest::full(at), inputs)?;
            done()?;
            best = best.min(begun.elapsed().as_secs_f64() * 1000.0);
            drop(picture);
        }
        Ok(best)
    };
    for frame in args[1..].iter().map(|f| f.parse::<i64>()) {
        let frame = frame?;
        let plan = Plan::compile(&doc)?;
        let inputs = veylvik_io::load_inputs(&plan, &base, renderer.budget_bytes)?;
        renderer.render_gpu(
            &plan,
            RenderRequest::full(Time::at_frame(frame, doc.frame_rate)?),
            &inputs,
        )?;
        done()?;
        let whole = cost(&plan, &inputs, frame)?;
        println!(
            "frame {frame}: the whole picture {whole:.1} ms ({:.1} a second)",
            1000.0 / whole
        );
        // Each pass on its own, finished on the card before the next: where the time goes.
        if std::env::var_os("PROFILE_PASSES").is_some() {
            let at = Time::at_frame(frame, doc.frame_rate)?;
            renderer.time_passes(&plan, RenderRequest::full(at), &inputs)?;
            let mut times = renderer.time_passes(&plan, RenderRequest::full(at), &inputs)?;
            let outside: Vec<_> = times
                .iter()
                .filter(|t| t.kind == "outside the passes")
                .map(|t| format!("{} {:.1} ms", t.node, t.milliseconds))
                .collect();
            println!("  outside the passes: {}", outside.join(", "));
            let shapes = renderer.vector_cache_stats()?;
            println!(
                "  drawn shapes and text kept: {} ({} MiB), found again {} times, drawn {} times, let go {} times",
                shapes.entries,
                shapes.bytes / (1024 * 1024),
                shapes.hits,
                shapes.misses,
                shapes.evictions
            );
            times.retain(|t| t.kind != "outside the passes");
            let sum: f64 = times.iter().map(|t| t.milliseconds).sum();
            println!(
                "  {} passes, {sum:.1} ms when each waits for the one before",
                times.len()
            );
            times.sort_by(|a, b| b.milliseconds.total_cmp(&a.milliseconds));
            for t in times.iter().take(24) {
                println!(
                    "  {:6.2} ms  {:4.1}%  {}  ({}: {})",
                    t.milliseconds,
                    100.0 * t.milliseconds / sum,
                    t.node,
                    t.kind,
                    t.pass
                );
            }
            let mut by_kind = std::collections::BTreeMap::<&str, (f64, usize)>::new();
            for t in &times {
                let entry = by_kind.entry(t.kind).or_default();
                entry.0 += t.milliseconds;
                entry.1 += 1;
            }
            let mut kinds: Vec<_> = by_kind.into_iter().collect();
            kinds.sort_by(|a, b| b.1.0.total_cmp(&a.1.0));
            println!(
                "  by kind of node: {}",
                kinds
                    .iter()
                    .map(|(kind, (ms, n))| format!("{kind} {ms:.1} ms in {n}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            continue;
        }
        if std::env::var_os("PROFILE_WHOLE").is_some() {
            // Only the whole picture, a few more times, to see how steady the figure is.
            for _ in 0..4 {
                println!("  again: {:.1} ms", cost(&plan, &inputs, frame)?);
            }
            continue;
        }
        let mut each = Vec::new();
        for (id, node) in &doc.nodes {
            let Ok(plan) = Plan::compile_output(&doc, *id) else {
                continue;
            };
            let Ok(inputs) = veylvik_io::load_inputs(&plan, &base, renderer.budget_bytes) else {
                continue;
            };
            if let Ok(ms) = cost(&plan, &inputs, frame) {
                each.push((ms, node.label.clone(), node.operation.descriptor().type_id));
            }
        }
        each.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (ms, label, kind) in each.iter().take(usize::MAX) {
            println!("  {ms:7.1} ms  {label}  ({kind})");
        }
        // The cheapest things, to show what a pass costs at least.
        let floor = each.iter().map(|e| e.0).fold(f64::MAX, f64::min);
        println!(
            "  the cheapest node alone: {floor:.1} ms; {} nodes measured",
            each.len()
        );
    }
    Ok(())
}
