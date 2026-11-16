//! Headless camera solving: follow features through footage, solve its camera and add it as scene nodes (camera, shadow-catching ground, a 3D render merged over the footage).
use serde_json::Value;
use std::path::Path;
use veylvik_document::{Command, Composition};
use veylvik_render::{Plan, Renderer};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub use veylvik_track::camera::Request;
/// Solve the request on `doc`; returns the commands that add the nodes, and a report.
pub fn run(
    renderer: &Renderer,
    doc: &Composition,
    base: &Path,
    request: &Request,
) -> Result<(Vec<Command>, Value)> {
    if !doc.nodes.contains_key(&request.source) {
        return Err("the footage node does not exist".into());
    }
    let plan = Plan::compile_output(doc, request.source)?;
    let inputs = crate::assets::load_inputs(&plan, base, renderer.budget_bytes)?;
    let started = std::time::Instant::now();
    let outcome = veylvik_track::camera::analyse(
        renderer,
        &plan,
        &inputs,
        request,
        doc.frame_rate,
        &mut |_, _| true,
    )?;
    let built = veylvik_track::camera::scene(doc, request, &outcome)?;
    let mut report = built.report;
    report["seconds"] = serde_json::json!(started.elapsed().as_secs_f64());
    Ok((built.commands, report))
}
