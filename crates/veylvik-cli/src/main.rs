use veylvik_io as assets;
mod mcp;
mod outline;
mod track;
mod video_export;
use std::path::Path;
use veylvik_document::{Composition, Node, Operation, Time};
use veylvik_render::{Plan, Region, RenderRequest};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const USAGE: &str = "usage: veylvik open [FILE] [--control-dir DIRECTORY | --no-control] | demo FILE [--motion|--procedural] | import IMAGE.png|IMAGE.exr FILE | outline-text FONT TEXT NEW_FILE [OPTIONS] | collect FILE NEW_DIRECTORY | nodes | check FILE | render FILE OUTPUT.png|OUTPUT.exr [TICKS RATE] [--node UUID] [--crop X Y WIDTH HEIGHT] | render-video FILE OUTPUT.mp4 [OPTIONS] | render-sequence FILE DIRECTORY START END [--format png|exr] [--node UUID] [--crop X Y WIDTH HEIGHT] | track FILE NEW_FILE SOURCE_UUID START END \"X,Y X,Y X,Y X,Y\" | mcp FILE | mcp --studio [SOCKET|DIRECTORY]";
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("open") => {
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
        Some("mcp") if matches!(args.len(), 3 | 4) && args[2] == "--studio" => {
            let socket = match args.get(3) {
                Some(path) => veylvik_control::ipc::resolve_socket(Path::new(path)),
                None => veylvik_control::ipc::default_socket()?,
            };
            veylvik_control::ipc::relay(
                &socket,
                std::io::stdin().lock(),
                std::io::stdout().lock(),
            )?;
        }
        Some("mcp") if args.len() == 3 => mcp::serve(
            Composition::load(Path::new(&args[2]))?,
            assets::document_base(Path::new(&args[2]))?,
            std::io::stdin().lock(),
            std::io::stdout().lock(),
        )?,
        Some("track") if args.len() == 8 => {
            // veylvik track FILE NEW_FILE SOURCE_UUID START END "x,y x,y x,y x,y"
            let (input, output) = (Path::new(&args[2]), Path::new(&args[3]));
            if output.exists() {
                return Err("NEW_FILE already exists; choose a new path".into());
            }
            let corners: Vec<[f64; 2]> = args[7]
                .split_whitespace()
                .map(|pair| {
                    let (x, y) = pair.split_once(',').ok_or("corners are x,y pairs")?;
                    Ok([x.parse()?, y.parse()?])
                })
                .collect::<Result<_>>()?;
            let surface: [[f64; 2]; 4] = corners
                .try_into()
                .map_err(|_| "give exactly four corners")?;
            let args_json = serde_json::json!({"source":args[4],"surface":surface,"start":args[5].parse::<i64>()?,"end":args[6].parse::<i64>()?});
            let request = track::Request::from_json(&args_json)?;
            let mut document = veylvik_document::Document::new(Composition::load(input)?)?;
            let renderer = assets::runtime::renderer()?;
            let (node, commands, mut report) = track::run(
                &renderer,
                document.snapshot(),
                &assets::document_base(input)?,
                &request,
            )?;
            document.apply(document.revision(), &commands)?;
            document.snapshot().save(output)?;
            report["node"] = serde_json::json!(node);
            report["saved"] = serde_json::json!(output);
            println!("{report}");
        }
        Some("render-video") => video_export::run(&args[2..])?,
        Some("outline-text") => outline::run(&args[2..])?,
        Some("nodes") if args.len() == 2 => println!("{}", veylvik_document::registry::catalog()),
        Some("demo")
            if args.len() == 3
                || (args.len() == 4 && matches!(args[3].as_str(), "--motion" | "--procedural")) =>
        {
            let composition = match args.get(3).map(String::as_str) {
                Some("--motion") => veylvik_document::motion_demo(),
                Some("--procedural") => veylvik_document::procedural_demo(),
                _ => veylvik_document::demo(),
            };
            composition.save(Path::new(&args[2]))?;
            println!("{}", serde_json::json!({"saved":args[2]}));
        }
        Some("import") if args.len() == 4 => {
            let source = Path::new(&args[2]).canonicalize()?;
            let image = assets::load_image(&source, 128 * 1024 * 1024)?;
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
                groups: Default::default(),
                schema: 1,
                frame_count: 240,
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
        Some("collect") if args.len() == 4 => {
            let document = Composition::load(Path::new(&args[2]))?;
            let result = assets::collect::collect(
                &document,
                &assets::document_base(Path::new(&args[2]))?,
                Path::new(&args[3]),
            )?;
            println!("{}", serde_json::to_string(&result)?);
        }
        Some("check") if args.len() == 3 => {
            let doc = Composition::load(Path::new(&args[2]))?;
            let plan = Plan::compile(&doc)?;
            let source_bytes =
                assets::inspect_inputs(&plan, &assets::document_base(Path::new(&args[2]))?)?;
            println!(
                "{}",
                serde_json::json!({"valid":true,"nodes":doc.nodes.len(),"reachable_nodes":plan.node_count(),"passes":plan.passes(),"image_slots":plan.image_slots(),"working_bytes":plan.working_bytes(),"source_bytes":source_bytes,"assets":plan.required_assets()})
            );
        }
        Some("render-sequence") if args.len() >= 6 => {
            let doc = Composition::load(Path::new(&args[2]))?;
            let frames = veylvik_document::sequence::FrameRange::new(
                args[4].parse()?,
                args[5].parse()?,
                doc.frame_rate,
            )?;
            if args.get(6).is_some_and(|arg| !arg.starts_with("--")) {
                return Err("sequence options must be --node, --crop or --format".into());
            }
            let mut options = Vec::new();
            let mut format = None;
            let mut index = 6;
            while index < args.len() {
                if args[index] == "--format" {
                    if format.is_some() || index + 1 >= args.len() {
                        return Err(
                            "--format needs one png or exr value and cannot be repeated".into()
                        );
                    }
                    format = Some(serde_json::from_value::<assets::FrameFormat>(
                        serde_json::json!(args[index + 1]),
                    )?);
                    index += 2;
                } else {
                    options.push(args[index].clone());
                    index += 1;
                }
            }
            if options.first().is_some_and(|arg| !arg.starts_with("--")) {
                return Err("sequence options must be --node, --crop or --format".into());
            }
            let (output, request) = render_options(&doc, &options)?;
            let renderer = assets::runtime::renderer()?;
            let result = assets::sequence::render_sequence(
                &renderer,
                &doc,
                &assets::document_base(Path::new(&args[2]))?,
                Path::new(&args[3]),
                assets::sequence::SequenceRequest {
                    format: format.unwrap_or_default(),
                    frames,
                    output,
                    region: request.region,
                },
                |completed, total| {
                    eprintln!(
                        "{}",
                        serde_json::json!({"progress":{"completed":completed,"total":total}})
                    );
                    true
                },
            )?;
            println!("{result}");
        }
        Some("render") if args.len() >= 4 => {
            assets::FrameFormat::from_path(Path::new(&args[3]))?;
            let doc = Composition::load(Path::new(&args[2]))?;
            let (output, request) = render_options(&doc, &args[4..])?;
            let plan = Plan::compile_output(&doc, output)?;
            let renderer = assets::runtime::renderer()?;
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
            save_frame(
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
fn render_options(
    doc: &Composition,
    args: &[String],
) -> Result<(veylvik_document::NodeId, RenderRequest)> {
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
    Ok((output.unwrap_or(doc.output), request))
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{}", serde_json::json!({"error":e.to_string()}));
        std::process::exit(1);
    }
}
use veylvik_io::save_frame;
