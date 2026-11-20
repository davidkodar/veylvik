//! How much of the frame each node's picture really covers: renders every node as if it were the output and measures the box round its pixels that are not clear. It shows what working only where there is something could save, before any of it is built.
use veylvik_document::{Composition, Time};
use veylvik_render::{Plan, RenderRequest};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = std::path::Path::new(
        args.first()
            .ok_or("coverage needs PROJECT.veylvik FRAME...")?,
    );
    let doc = Composition::load(path)?;
    let base = veylvik_io::document_base(path)?;
    let renderer = veylvik_io::runtime::renderer()?;
    let (width, height) = (doc.width as usize, doc.height as usize);
    // Only what the output is made of.
    let whole = Plan::compile(&doc)?;
    for frame in args[1..].iter().map(|f| f.parse::<i64>()) {
        let frame = frame?;
        let at = Time::at_frame(frame, doc.frame_rate)?;
        let visible = whole.visible_at(at)?;
        let mut rows = Vec::new();
        for (id, node) in &doc.nodes {
            if !visible.contains(id) {
                continue;
            }
            let Ok(plan) = Plan::compile_output(&doc, *id) else {
                continue;
            };
            let Ok(inputs) = veylvik_io::load_inputs(&plan, &base, renderer.budget_bytes) else {
                continue;
            };
            let Ok(picture) = renderer.render_frame(&plan, RenderRequest::full(at), &inputs) else {
                continue;
            };
            let (mut left, mut top, mut right, mut bottom) = (width, height, 0, 0);
            for (i, pixel) in picture.pixels.iter().enumerate() {
                if *pixel != [0.0; 4] {
                    let (x, y) = (i % width, i / width);
                    left = left.min(x);
                    right = right.max(x + 1);
                    top = top.min(y);
                    bottom = bottom.max(y + 1);
                }
            }
            let share = if right > left {
                ((right - left) * (bottom - top)) as f64 / (width * height) as f64
            } else {
                0.0
            };
            rows.push((
                share,
                node.label.clone(),
                node.operation.descriptor().type_id,
            ));
        }
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
        let total: f64 = rows.iter().map(|r| r.0).sum();
        println!(
            "frame {frame}: {} nodes are part of the picture; together their pictures cover {:.0}% of what {} whole frames would ({} cover nothing, {} under a quarter, {} the whole frame)",
            rows.len(),
            100.0 * total / rows.len().max(1) as f64,
            rows.len(),
            rows.iter().filter(|r| r.0 == 0.0).count(),
            rows.iter().filter(|r| r.0 > 0.0 && r.0 < 0.25).count(),
            rows.iter().filter(|r| r.0 >= 0.999).count(),
        );
        if std::env::var_os("COVERAGE_LIST").is_some() {
            for (share, label, kind) in &rows {
                println!("  {:5.1}%  {label}  ({kind})", share * 100.0);
            }
        }
    }
    Ok(())
}
