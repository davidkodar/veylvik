mod mcp;
use std::path::Path;
use veylvik_document::{Composition, Time};
use veylvik_render::{Plan, Renderer};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("mcp") if args.len() == 3 => { mcp::serve(Composition::load(Path::new(&args[2]))?, std::io::stdin().lock(), std::io::stdout().lock())?; }
        Some("demo") if args.len() == 3 => { veylvik_document::demo().save(Path::new(&args[2]))?; println!("{}", serde_json::json!({"saved":args[2]})); }
        Some("check") if args.len() == 3 => { let doc = Composition::load(Path::new(&args[2]))?; let plan = Plan::compile(&doc)?; println!("{}",serde_json::json!({"valid":true,"nodes":doc.nodes.len(),"reachable_nodes":plan.node_count(),"working_bytes":plan.working_bytes()})); }
        Some("render") if args.len() == 4 || args.len() == 6 => {
            let doc = Composition::load(Path::new(&args[2]))?;
            let time = if args.len() == 6 { Time::new(args[4].parse()?, args[5].parse()?)? } else { Time::new(0,1)? };
            let plan = Plan::compile(&doc)?; let renderer = Renderer::new()?;
            let pixels = renderer.render(&plan,time)?;
            save_png(Path::new(&args[3]),doc.width,doc.height,&pixels)?;
            println!("{}",serde_json::json!({"output":args[3],"adapter":renderer.adapter.name,"backend":format!("{:?}",renderer.adapter.backend),"width":doc.width,"height":doc.height}));
        }
        _ => return Err("usage: veylvik demo FILE | check FILE | render FILE OUTPUT.png [TICKS RATE] | mcp FILE".into()),
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{}", serde_json::json!({"error":e.to_string()}));
        std::process::exit(1);
    }
}

fn save_png(
    path: &Path,
    width: u32,
    height: u32,
    pixels: &[[f32; 4]],
) -> Result<(), Box<dyn std::error::Error>> {
    use image::ImageEncoder;
    let rgba = veylvik_render::to_srgba8(pixels);
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png).write_image(
        &rgba,
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )?;
    veylvik_document::atomic_write(path, &png)?;
    Ok(())
}
