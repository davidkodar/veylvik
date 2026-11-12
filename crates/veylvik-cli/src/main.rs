use veylvik_io as assets;
mod mcp;
use std::path::Path;
use veylvik_document::{Composition, Node, Operation, Time};
use veylvik_render::{Plan, Region, RenderRequest, Renderer};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const USAGE: &str = "usage: veylvik open [FILE] | demo FILE | import IMAGE.png FILE | nodes | check FILE | render FILE OUTPUT.png [TICKS RATE] [--node UUID] [--crop X Y WIDTH HEIGHT] | mcp FILE";
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("open") if args.len() == 2 || args.len() == 3 => {
            let studio = std::env::current_exe()?.with_file_name("veylvik-studio");
            let status = std::process::Command::new(studio)
                .args(&args[2..])
                .status()
                .map_err(|e| {
                    format!("cannot start studio; build with cargo build --workspace: {e}")
                })?;
            if !status.success() {
                return Err("studio exited with an error".into());
            }
        }
        Some("mcp") if args.len() == 3 => mcp::serve(
            Composition::load(Path::new(&args[2]))?,
            assets::document_base(Path::new(&args[2]))?,
            std::io::stdin().lock(),
            std::io::stdout().lock(),
        )?,
        Some("nodes") if args.len() == 2 => println!("{}", veylvik_document::registry::catalog()),
        Some("demo") if args.len() == 3 => {
            veylvik_document::demo().save(Path::new(&args[2]))?;
            println!("{}", serde_json::json!({"saved":args[2]}));
        }
        Some("import") if args.len() == 4 => {
            let source = Path::new(&args[2]).canonicalize()?;
            let image = assets::load_png(&source, 128 * 1024 * 1024)?;
            let parent = Path::new(&args[3])
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .canonicalize()?;
            let relative =
                pathdiff::diff_paths(&source, parent).ok_or("cannot make image path relative")?;
            let node = Node::new(
                "Image",
                Operation::Image {
                    asset: relative.to_str().ok_or("image path is not UTF-8")?.into(),
                },
                &[],
            );
            let doc = Composition {
                schema: 1,
                width: image.width(),
                height: image.height(),
                frame_rate: Time::new(24, 1)?,
                output: node.id,
                nodes: [(node.id, node)].into_iter().collect(),
            };
            assets::protect_inputs(
                &doc,
                &Path::new(&args[3])
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."))
                    .canonicalize()?,
                Path::new(&args[3]),
            )?;
            doc.save(Path::new(&args[3]))?;
            println!(
                "{}",
                serde_json::json!({"saved":args[3],"width":doc.width,"height":doc.height})
            );
        }
        Some("check") if args.len() == 3 => {
            let doc = Composition::load(Path::new(&args[2]))?;
            let plan = Plan::compile(&doc)?;
            assets::load_inputs(
                &plan,
                &assets::document_base(Path::new(&args[2]))?,
                512 * 1024 * 1024,
            )?;
            println!(
                "{}",
                serde_json::json!({"valid":true,"nodes":doc.nodes.len(),"reachable_nodes":plan.node_count(),"passes":plan.passes(),"image_slots":plan.image_slots(),"working_bytes":plan.working_bytes(),"assets":plan.required_assets()})
            );
        }
        Some("render") if args.len() >= 4 => {
            let doc = Composition::load(Path::new(&args[2]))?;
            let (plan, request) = render_options(&doc, &args[4..])?;
            let renderer = Renderer::new()?;
            let inputs = assets::load_inputs(
                &plan,
                &assets::document_base(Path::new(&args[2]))?,
                renderer.budget_bytes,
            )?;
            assets::protect_inputs(
                &doc,
                &assets::document_base(Path::new(&args[2]))?,
                Path::new(&args[3]),
            )?;
            let frame = renderer.render_frame(&plan, request, &inputs)?;
            save_png(
                Path::new(&args[3]),
                frame.region.width,
                frame.region.height,
                &frame.pixels,
            )?;
            println!(
                "{}",
                serde_json::json!({"output":args[3],"adapter":renderer.adapter.name,"backend":format!("{:?}",renderer.adapter.backend),"region":frame.region})
            );
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}
fn render_options(doc: &Composition, args: &[String]) -> Result<(Plan, RenderRequest)> {
    let mut i = 0;
    let mut request = RenderRequest::full(Time::new(0, 1)?);
    let mut output = None;
    if args.first().is_some_and(|v| !v.starts_with("--")) {
        if args.len() < 2 {
            return Err("time needs TICKS and RATE".into());
        }
        request.time = Time::new(args[0].parse()?, args[1].parse()?)?;
        i = 2;
    }
    while i < args.len() {
        match args[i].as_str() {
            "--node" if output.is_none() && i + 1 < args.len() => {
                output = Some(args[i + 1].parse()?);
                i += 2;
            }
            "--crop" if request.region.is_none() && i + 4 < args.len() => {
                request.region = Some(Region {
                    x: args[i + 1].parse()?,
                    y: args[i + 2].parse()?,
                    width: args[i + 3].parse()?,
                    height: args[i + 4].parse()?,
                });
                i += 5;
            }
            _ => return Err(format!("invalid or repeated render option: {}", args[i]).into()),
        }
    }
    if let Some(region) = request.region {
        region.validate(doc.width, doc.height)?;
    }
    Ok((
        Plan::compile_output(doc, output.unwrap_or(doc.output))?,
        request,
    ))
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{}", serde_json::json!({"error":e.to_string()}));
        std::process::exit(1);
    }
}
fn save_png(path: &Path, width: u32, height: u32, pixels: &[[f32; 4]]) -> Result<()> {
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
