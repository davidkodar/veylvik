//! `veylvik ofx list | describe | render`: OpenFX plugins from the command line, run in the helper process like everywhere else.
use std::path::{Path, PathBuf};
use veylvik_ofx::{Value, Worker};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const USAGE: &str = "usage: veylvik ofx list [FOLDER...] | describe BUNDLE INDEX | render BUNDLE INDEX OUTPUT.png|.exr [--input IMAGE] [--size WIDTHxHEIGHT] [--time T] [--set NAME=VALUE[,VALUE...]]...";

fn worker() -> Result<Worker> {
    Ok(Worker::new(&std::env::current_exe()?))
}
fn plugin(worker: &mut Worker, bundle: &str, index: &str) -> Result<veylvik_ofx::Plugin> {
    let index: usize = index
        .parse()
        .map_err(|_| "INDEX is a number from `veylvik ofx list`")?;
    let (plugins, _) = worker.list(Path::new(bundle))?;
    Ok(plugins
        .into_iter()
        .find(|p| p.index == index)
        .ok_or("no image effect with that index in the bundle")?)
}
pub fn command(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("list") => {
            let folders: Vec<PathBuf> = if args.len() > 1 {
                args[1..].iter().map(PathBuf::from).collect()
            } else {
                veylvik_ofx::folders()
            };
            let mut worker = worker()?;
            let mut out = Vec::new();
            for bundle in veylvik_ofx::bundles(&folders) {
                match worker.list(&bundle) {
                    Ok((plugins, refused)) => out.push(serde_json::json!({"bundle": bundle, "plugins": plugins, "refused": refused})),
                    Err(e) => out.push(serde_json::json!({"bundle": bundle, "error": e})),
                }
            }
            println!(
                "{}",
                serde_json::json!({"folders": folders, "bundles": out})
            );
        }
        Some("describe") if args.len() == 3 => {
            let mut worker = worker()?;
            let plugin = plugin(&mut worker, &args[1], &args[2])?;
            let context = plugin.context().ok_or(
                "the plugin offers no context Veylvik runs (filter, general or generator)",
            )?;
            println!(
                "{}",
                serde_json::to_string(&worker.describe(&plugin, context)?)?
            );
        }
        Some("render") if args.len() >= 4 => {
            let mut worker = worker()?;
            let plugin = plugin(&mut worker, &args[1], &args[2])?;
            let context = plugin
                .context()
                .ok_or("the plugin offers no context Veylvik runs")?;
            let description = worker.describe(&plugin, context)?;
            let (mut input, mut size, mut time, mut values) = (None, None, 0.0, Vec::new());
            let mut rest = args[4..].iter();
            while let Some(flag) = rest.next() {
                let value = rest.next().ok_or(USAGE)?;
                match flag.as_str() {
                    "--input" => input = Some(veylvik_io::load_image(Path::new(value), 1 << 30)?),
                    "--size" => {
                        let (w, h) = value.split_once('x').ok_or("--size is WIDTHxHEIGHT")?;
                        size = Some([w.parse()?, h.parse()?]);
                    }
                    "--time" => time = value.parse()?,
                    "--set" => {
                        let (name, v) = value.split_once('=').ok_or("--set is NAME=VALUE")?;
                        let param = description
                            .params
                            .iter()
                            .find(|p| p.name == name)
                            .ok_or_else(|| format!("the plugin has no setting {name}"))?;
                        let v = if param.default.is_empty() && !param.text.is_empty()
                            || param.kind == "OfxParamTypeString"
                        {
                            Value::Text(v.to_string())
                        } else {
                            Value::Numbers(
                                v.split(',')
                                    .map(str::parse)
                                    .collect::<std::result::Result<_, _>>()?,
                            )
                        };
                        values.push((name.to_string(), v));
                    }
                    _ => return Err(USAGE.into()),
                }
            }
            let [width, height] = match (&input, size) {
                (Some(image), _) => [image.width(), image.height()],
                (None, Some(size)) => size,
                (None, None) => return Err("give --input IMAGE or --size WIDTHxHEIGHT".into()),
            };
            let flat: Vec<f32> = input
                .as_ref()
                .map(|i| i.pixels().iter().flatten().copied().collect())
                .unwrap_or_default();
            let source = description.inputs().next().map(|c| c.name.clone());
            let inputs: Vec<(&str, &[f32])> = match (&source, input.is_some()) {
                (Some(name), true) => vec![(name.as_str(), flat.as_slice())],
                _ => Vec::new(),
            };
            let started = std::time::Instant::now();
            let (out, messages) =
                worker.render(&plugin, context, [width, height], time, &values, &inputs)?;
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            let pixels: Vec<[f32; 4]> = out.as_chunks::<4>().0.to_vec();
            veylvik_io::export::save_frame(Path::new(&args[3]), width, height, &pixels)?;
            println!(
                "{}",
                serde_json::json!({"saved": args[3], "plugin": plugin.identifier, "milliseconds": ms, "messages": messages})
            );
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}
