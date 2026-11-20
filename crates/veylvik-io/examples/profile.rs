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
    let renderer = veylvik_io::runtime::renderer()?;
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
