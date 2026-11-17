use veylvik_io as assets;
mod mcp;
mod outline;
mod segment;
mod solve;
mod track;
mod video_export;
use std::path::Path;
use veylvik_document::{Composition, Node, Operation, Time};
use veylvik_render::{Plan, Region, RenderRequest};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const USAGE: &str = "usage: veylvik open [FILE] [--control-dir DIRECTORY | --no-control] | demo FILE [--motion|--procedural|--gold-title] | import IMAGE.png|IMAGE.exr FILE | outline-text FONT TEXT NEW_FILE [OPTIONS] | collect FILE NEW_DIRECTORY | verify-collection DIRECTORY | nodes | check FILE | render FILE OUTPUT.png|OUTPUT.exr [TICKS RATE] [--node UUID] [--crop X Y WIDTH HEIGHT] | render-video FILE OUTPUT.mp4 [OPTIONS] | render-sequence FILE DIRECTORY START END [--format png|exr] [--node UUID] [--crop X Y WIDTH HEIGHT] | track FILE NEW_FILE SOURCE_UUID START END \"X,Y X,Y X,Y X,Y\" [\"AREA X,Y X,Y X,Y X,Y\"] [--screen-area] [--motion auto|perspective|affine|similarity|translation] [--no-refine] [--back-to FRAME] | solve-camera FILE NEW_FILE SOURCE_UUID [START END] [--focal MM] [--sensor MM] | segment FILE NEW_FILE SOURCE_UUID FRAME END \"X,Y X,Y,- ...\" [--back-to FRAME] [--model ID] [--no-detail] [--matting|--no-matting] | models [install|uninstall|enable|disable ID] | mcp FILE | mcp --studio [SOCKET|DIRECTORY]";
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
        Some("track") => {
            // Flags may appear anywhere after the command; positional arguments keep their order.
            let mut screen_area = false;
            let mut no_refine = false;
            let mut back_to = None;
            let mut motion = None;
            let mut positional = Vec::new();
            let mut flags = args.iter();
            while let Some(arg) = flags.next() {
                match arg.as_str() {
                    "--screen-area" => screen_area = true,
                    "--no-refine" => no_refine = true,
                    "--back-to" => {
                        back_to = Some(
                            flags
                                .next()
                                .ok_or("--back-to needs a frame")?
                                .parse::<i64>()?,
                        )
                    }
                    "--motion" => {
                        motion = Some(flags.next().ok_or("--motion needs a model")?.clone())
                    }
                    _ => positional.push(arg.clone()),
                }
            }
            let args = positional;
            if !matches!(args.len(), 8 | 9) {
                return Err(USAGE.into());
            }
            // veylvik track FILE NEW_FILE SOURCE_UUID START END "x,y x,y x,y x,y" ["area x,y ..."]
            let (input, output) = (Path::new(&args[2]), Path::new(&args[3]));
            if output.exists() {
                return Err("NEW_FILE already exists; choose a new path".into());
            }
            let quad = |text: &str| -> Result<[[f64; 2]; 4]> {
                let corners: Vec<[f64; 2]> = text
                    .split_whitespace()
                    .map(|pair| {
                        let (x, y) = pair.split_once(',').ok_or("corners are x,y pairs")?;
                        Ok([x.parse()?, y.parse()?])
                    })
                    .collect::<Result<_>>()?;
                Ok(corners
                    .try_into()
                    .map_err(|_| "give exactly four corners")?)
            };
            let mut args_json = serde_json::json!({"source":args[4],"surface":quad(&args[7])?,"start":args[5].parse::<i64>()?,"end":args[6].parse::<i64>()?});
            if let Some(region) = args.get(8) {
                args_json["region"] = serde_json::json!(quad(region)?);
            }
            if screen_area {
                args_json["area_follows"] = serde_json::json!("screen");
            }
            if let Some(motion) = motion {
                args_json["motion"] = serde_json::json!(motion);
            }
            if no_refine {
                args_json["refine"] = serde_json::json!(false);
            }
            if let Some(frame) = back_to {
                args_json["back_to"] = serde_json::json!(frame);
            }
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
        Some("solve-camera") => {
            let mut focal = None;
            let mut sensor = None;
            let mut positional = Vec::new();
            let mut flags = args.iter();
            while let Some(arg) = flags.next() {
                match arg.as_str() {
                    "--focal" => {
                        focal = Some(
                            flags
                                .next()
                                .ok_or("--focal needs millimetres")?
                                .parse::<f64>()?,
                        )
                    }
                    "--sensor" => {
                        sensor = Some(
                            flags
                                .next()
                                .ok_or("--sensor needs millimetres")?
                                .parse::<f64>()?,
                        )
                    }
                    _ => positional.push(arg.clone()),
                }
            }
            // veylvik solve-camera FILE NEW_FILE SOURCE_UUID [START END]
            if !matches!(positional.len(), 5 | 7) {
                return Err(USAGE.into());
            }
            let (input, output) = (Path::new(&positional[2]), Path::new(&positional[3]));
            if output.exists() {
                return Err("NEW_FILE already exists; choose a new path".into());
            }
            let mut document = veylvik_document::Document::new(Composition::load(input)?)?;
            let mut request = serde_json::json!({"source": positional[4]});
            if positional.len() == 7 {
                request["start"] = serde_json::json!(positional[5].parse::<i64>()?);
                request["end"] = serde_json::json!(positional[6].parse::<i64>()?);
            }
            if let Some(mm) = focal {
                request["focal_length"] = serde_json::json!(mm);
            }
            if let Some(mm) = sensor {
                request["sensor_width"] = serde_json::json!(mm);
            }
            let request = solve::Request::from_json(&request, document.snapshot().frame_count)?;
            let renderer = assets::runtime::renderer()?;
            let (commands, mut report) = solve::run(
                &renderer,
                document.snapshot(),
                &assets::document_base(input)?,
                &request,
            )?;
            document.apply(document.revision(), &commands)?;
            document.snapshot().save(output)?;
            report["saved"] = serde_json::json!(output);
            println!("{report}");
        }
        Some(veylvik_segment::worker::SUBCOMMAND) if args.len() == 2 => {
            veylvik_segment::worker::serve(std::io::stdin().lock(), std::io::stdout().lock())?
        }
        Some("models") => segment::models_command(&args[2..])?,
        Some("segment") => {
            let mut back_to = None;
            let mut model = None;
            let mut detail = true;
            let mut matting = None;
            let mut positional = Vec::new();
            let mut flags = args.iter();
            while let Some(arg) = flags.next() {
                match arg.as_str() {
                    "--back-to" => {
                        back_to = Some(
                            flags
                                .next()
                                .ok_or("--back-to needs a frame")?
                                .parse::<i64>()?,
                        )
                    }
                    "--model" => model = Some(flags.next().ok_or("--model needs an id")?.clone()),
                    "--no-detail" => detail = false,
                    "--no-matting" => matting = Some(false),
                    "--matting" => matting = Some(true),
                    _ => positional.push(arg.clone()),
                }
            }
            let args = positional;
            if args.len() != 8 {
                return Err(USAGE.into());
            }
            // veylvik segment FILE NEW_FILE SOURCE_UUID FRAME END "x,y x,y,- ..."
            let (input, output) = (Path::new(&args[2]), Path::new(&args[3]));
            if output.exists() {
                return Err("NEW_FILE already exists; choose a new path".into());
            }
            let mut args_json = serde_json::json!({"source": args[4], "frame": args[5].parse::<i64>()?,
                "end": args[6].parse::<i64>()?, "points": segment::points(&args[7])?});
            if let Some(frame) = back_to {
                args_json["back_to"] = serde_json::json!(frame);
            }
            if let Some(model) = model {
                args_json["model"] = serde_json::json!(model);
            }
            args_json["detail"] = serde_json::json!(detail);
            if let Some(matting) = matting {
                args_json["matting"] = serde_json::json!(matting);
            }
            let mut document = veylvik_document::Document::new(Composition::load(input)?)?;
            let request = segment::Request::from_json(&args_json, document.snapshot().frame_count)?;
            let renderer = assets::runtime::renderer()?;
            let project_dir = std::path::absolute(output)?
                .parent()
                .ok_or("NEW_FILE needs a directory")?
                .to_path_buf();
            let (node, commands, mut report) = segment::run(
                &renderer,
                document.snapshot(),
                &assets::document_base(input)?,
                &project_dir,
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
                || (args.len() == 4
                    && matches!(
                        args[3].as_str(),
                        "--motion" | "--procedural" | "--gold-title"
                    )) =>
        {
            let composition = match args.get(3).map(String::as_str) {
                Some("--motion") => veylvik_document::motion_demo(),
                Some("--procedural") => veylvik_document::procedural_demo(),
                Some("--gold-title") => {
                    let blank =
                        veylvik_document::blank(1920, 1080, veylvik_document::Time::new(24, 1)?)?;
                    let scene = veylvik_presets::gold_title(
                        &blank,
                        "VEYLVIK",
                        "CREATE SOMETHING EXTRAORDINARY",
                    )?;
                    let mut document = veylvik_document::Document::new(blank)?;
                    document.apply(0, &scene.commands)?;
                    document.snapshot().clone()
                }
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
                Operation::Image(veylvik_document::nodes::Image {
                    asset: relative.to_str().ok_or("image path is not UTF-8")?.into(),
                }),
                &[],
            );
            let doc = Composition {
                ocio: Default::default(),
                working_space: Default::default(),
                display: Default::default(),
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
        Some("verify-collection") if args.len() == 3 => {
            let count = assets::collect::verify(Path::new(&args[2]))?;
            println!("{}", serde_json::json!({"verified":true,"files":count}));
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
            let frame = renderer.render_frame_export(&plan, request, &inputs)?;
            let pixels = assets::export::sdr_pixels(
                Path::new(&args[3]),
                &frame.pixels,
                doc.output_display(),
            );
            save_frame(
                Path::new(&args[3]),
                frame.region.width,
                frame.region.height,
                &pixels,
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
