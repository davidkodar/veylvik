//! Prints what a composition's picture is made of at some frames: its identity in this run and the one that holds between runs (or "-"). Run it twice: the second column must not change.
use veylvik_document::{Composition, Time};
use veylvik_render::{Plan, RenderRequest};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = std::path::Path::new(
        args.first()
            .ok_or("identity needs PROJECT.veylvik FRAME...")?,
    );
    let doc = Composition::load(path)?;
    let base = veylvik_io::document_base(path)?;
    let renderer = veylvik_io::runtime::renderer()?;
    let plan = Plan::compile(&doc)?;
    let inputs = veylvik_io::load_inputs(&plan, &base, renderer.budget_bytes)?;
    for frame in args[1..].iter().map(|f| f.parse::<i64>()) {
        let frame = frame?;
        let at = Time::at_frame(frame, doc.frame_rate)?;
        let begun = std::time::Instant::now();
        let (now, lasting) = renderer.identities(&plan, RenderRequest::full(at), &inputs)?;
        let took = begun.elapsed().as_secs_f64() * 1000.0;
        println!(
            "frame {frame}: {now:016x} {}  ({took:.1} ms to tell)",
            lasting.map_or("-".into(), |name| format!("{name:016x}"))
        );
    }
    Ok(())
}
