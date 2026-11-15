//! Headless subject segmentation and model management: the Studio analysis with the worker
//! process, published as a matte sequence beside the project and one Footage node.
use serde_json::{Value, json};
use std::path::Path;
use veylvik_document::{Command, Composition, NodeId};
use veylvik_render::{Plan, Renderer};
use veylvik_segment::{Point, analysis, models, worker::Worker};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// `veylvik models [install ID]`
pub fn models_command(args: &[String]) -> Result<()> {
    match args {
        [] => {
            let list: Vec<Value> = models::MODELS
                .iter()
                .map(|m| -> Result<Value> {
                    Ok(json!({"id": m.id, "title": m.title, "summary": m.summary,
                        "licence": m.licence, "homepage": m.homepage, "bytes": m.bytes(),
                        "uses": m.uses, "pros": m.pros, "cons": m.cons, "speed": m.speed, "memory": m.memory,
                        "installed": models::installed(m)?, "enabled": models::enabled(m)?,
                        "directory": models::dir(m)?}))
                })
                .collect::<Result<_>>()?;
            println!("{}", json!({ "models": list }));
        }
        [install, id] if install == "install" => {
            let model = models::find(id)?;
            eprintln!(
                "Downloading {} ({}, {:.0} MB) into {}",
                model.title,
                model.licence,
                model.bytes() as f64 / 1e6,
                models::dir(model)?.display()
            );
            let mut shown = 0;
            models::install(model, &mut |done, total| {
                let percent = done * 100 / total.max(1);
                if percent >= shown + 10 {
                    shown = percent - percent % 10;
                    eprintln!("{shown}%");
                }
                true
            })?;
            println!(
                "{}",
                json!({"installed": model.id, "directory": models::dir(model)?})
            );
        }
        [action, id] if matches!(action.as_str(), "uninstall" | "enable" | "disable") => {
            let model = models::find(id)?;
            match action.as_str() {
                "uninstall" => models::uninstall(model)?,
                other => models::set_enabled(model, other == "enable")?,
            }
            println!(
                "{}",
                json!({"model": model.id, "installed": models::installed(model)?, "enabled": models::enabled(model)?})
            );
        }
        _ => return Err("usage: veylvik models [install|uninstall|enable|disable ID]".into()),
    }
    Ok(())
}
/// Parses "x,y x,y,- ..." in 0..1 frame coordinates; a trailing ",-" excludes the point.
pub fn points(text: &str) -> Result<Vec<Point>> {
    text.split_whitespace()
        .map(|item| {
            let parts: Vec<&str> = item.split(',').collect();
            let positive = match parts.get(2) {
                None | Some(&"+") => true,
                Some(&"-") => false,
                _ => return Err("points are x,y or x,y,- (to exclude)".into()),
            };
            match parts[..parts.len().min(2)] {
                [x, y] => Ok(Point {
                    x: x.parse()?,
                    y: y.parse()?,
                    positive,
                }),
                _ => Err("points are x,y or x,y,- (to exclude)".into()),
            }
        })
        .collect()
}
pub use veylvik_segment::request::Request;
/// Segments on `doc`, writing the mattes under `project_dir`; returns the matte node, the
/// commands that add it and the refined cutout, and a report.
pub fn run(
    renderer: &Renderer,
    doc: &Composition,
    base: &Path,
    project_dir: &Path,
    request: &Request,
) -> Result<(NodeId, Vec<Command>, Value)> {
    if !doc.nodes.contains_key(&request.source) {
        return Err("segmentation source node does not exist".into());
    }
    let model = models::find(&request.model)?;
    if !models::enabled(model)? {
        return Err(format!(
            "{} is switched off; run `veylvik models enable {}`",
            model.title, model.id
        )
        .into());
    }
    if !models::installed(model)? {
        return Err(format!(
            "{} is not installed; run `veylvik models install {}` ({:.0} MB, {})",
            model.title,
            model.id,
            model.bytes() as f64 / 1e6,
            model.licence
        )
        .into());
    }
    analysis::schedule(&request.analysis, doc.frame_count)?;
    let matting = &models::VITMATTE;
    let use_matting = match request.matting {
        Some(false) => false,
        None => models::ready(matting)?,
        Some(true) if models::ready(matting)? => true,
        Some(true) => {
            return Err(format!(
                "{} is not installed or is switched off; run `veylvik models install {}`",
                matting.title, matting.id
            )
            .into());
        }
    };
    let analysis_request = analysis::Request {
        points: request.analysis.points.clone(),
        frames: request.analysis.frames,
        back_to: request.analysis.back_to,
        detail: request.analysis.detail,
        matting: use_matting.then_some(matting.id),
    };
    let plan = Plan::compile_output(doc, request.source)?;
    let inputs = crate::assets::load_inputs(&plan, base, renderer.budget_bytes)?;
    let exe = std::env::current_exe()?;
    let mut worker = Worker::spawn(&exe, model.id)?;
    let dir = analysis::unique_dir(project_dir, request.source);
    let mut last = std::time::Instant::now();
    let outcome = analysis::analyse(
        renderer,
        &plan,
        &inputs,
        doc,
        &analysis_request,
        &mut worker,
        &dir,
        &mut |done, total| {
            if last.elapsed().as_secs_f64() > 2.0 {
                eprintln!("segmenting: {done}/{total} frames");
                last = std::time::Instant::now();
            }
            true
        },
    )?;
    let asset = pathdiff::diff_paths(&outcome.manifest, project_dir)
        .ok_or("cannot express the matte path relative to the project")?
        .to_string_lossy()
        .into_owned();
    let added = analysis::matte_nodes(doc, request.source, asset, outcome.first_frame)?;
    let mut report = serde_json::to_value(&outcome)?;
    report["device"] = json!(worker.device);
    report["model"] = json!(model.id);
    report["cutout"] = json!(added.cutout);
    Ok((added.matte, added.commands, report))
}
