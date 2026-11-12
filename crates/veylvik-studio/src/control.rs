//! Live tools execute on the UI thread against its single document owner.
use crate::Studio;
use serde_json::{Value, json};
use veylvik_document::{Command, NodeId, Operation};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
}
pub fn catalog() -> Value {
    let session = json!({"type":"string","format":"uuid"});
    let revision = json!({"type":"integer","minimum":0});
    json!({"tools":[
        tool("inspect","Read the open project, session identity, revision, selection and preview state.",json!({}),&[]),
        tool("diagnostics","Read GPU buffer and cache counters without waiting for an active render. Snapshot is null while the renderer is busy. Declared buffers exclude driver and rasterizer scratch allocations.",json!({}),&[]),
        tool("nodes","Read built-in operations, ports, defaults and parameter schemas.",json!({}),&[]),
        tool("apply","Validate or atomically apply commands to the open editor. Requires the session and revision from inspect. Image assets must already be loaded. Unapplied inspector changes or file jobs block mutation.",json!({"session":session,"revision":revision,"dry_run":{"type":"boolean"},"commands":{"type":"array","minItems":1,"maxItems":4096,"items":veylvik_document::schema::command()}}),&["session","revision","commands"]),
        tool("undo","Undo one editor transaction.",json!({"session":session,"revision":revision}),&["session","revision"]),
        tool("redo","Redo one editor transaction.",json!({"session":session,"revision":revision}),&["session","revision"]),
        tool("preview","Pause playback, seek to an absolute frame and optionally preview a node. The GPU preview is asynchronous; composition output is unchanged.",json!({"session":session,"revision":revision,"frame":{"type":"integer","minimum":0,"maximum":1000000},"node":{"type":"string","format":"uuid"}}),&["session","revision","frame"])
    ]})
}
pub fn call(studio: &mut Studio, name: &str, args: &Value) -> Result<Value> {
    let catalog = catalog();
    let descriptor = catalog["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == name)
        .ok_or("unknown live tool")?;
    let object = args.as_object().ok_or("arguments must be an object")?;
    for key in object.keys() {
        if descriptor["inputSchema"]["properties"].get(key).is_none() {
            return Err(format!("unknown argument {key}").into());
        }
    }
    for key in descriptor["inputSchema"]["required"].as_array().unwrap() {
        if !object.contains_key(key.as_str().unwrap()) {
            return Err(format!("missing argument {key}").into());
        }
    }
    if name == "nodes" {
        return Ok(veylvik_document::registry::catalog());
    }
    if name == "diagnostics" {
        return Ok(
            json!({"initialized":true,"adapter":studio.renderer.adapter.name,"snapshot":studio.renderer.try_diagnostics()?,"preview_rendering":studio.preview_pending}),
        );
    }
    let project = &studio.project;
    if name == "inspect" {
        return Ok(
            json!({"session":project.session,"revision":project.document.revision(),"composition":project.document.snapshot(),"history":project.document.history(),"selected":project.selected,"preview":{"node":project.preview.unwrap_or(project.document.snapshot().output),"frame":project.frame,"displayed_time":studio.displayed_time,"rendering":studio.preview_pending},"viewer":{"exposure":studio.view.exposure,"alpha":studio.view.alpha,"scale":studio.navigation.manual_scale(),"pan":studio.navigation.pan()},"inspector_draft":studio.draft_dirty,"busy":studio.files.busy() || studio.authoring_dialog_open() || studio.graph.dragging(),"status":studio.status,"error":studio.error}),
        );
    }
    let session: NodeId = args["session"]
        .as_str()
        .ok_or("session must be a UUID")?
        .parse()?;
    if session != project.session {
        return Err("editor session changed; inspect the open project again".into());
    }
    let revision = args["revision"]
        .as_u64()
        .ok_or("revision must be a nonnegative integer")?;
    if revision != project.document.revision() {
        return Err(format!("revision conflict: current {}", project.document.revision()).into());
    }
    if studio.files.busy()
        || studio.authoring_dialog_open()
        || studio.confirm.is_some()
        || studio.graph.dragging()
    {
        return Err("editor is busy; finish the current interaction first".into());
    }
    let dry_run = args
        .get("dry_run")
        .map(|value| value.as_bool().ok_or("dry_run must be boolean"))
        .transpose()?
        .unwrap_or(false);
    if studio.draft_dirty && !dry_run {
        return Err("Apply or revert the inspector draft before live edits".into());
    }
    match name {
        "apply" => {
            let commands: Vec<Command> = serde_json::from_value(args["commands"].clone())?;
            let candidate = studio.project.document.prepare(revision, &commands)?;
            for node in candidate.nodes.values() {
                if let Operation::Image { asset } = &node.operation
                    && !studio.project.inputs.contains_key(asset)
                {
                    return Err(format!(
                        "image asset is not loaded in the editor: {asset}; import it first"
                    )
                    .into());
                }
            }
            if dry_run {
                return Ok(
                    json!({"session":session,"revision":revision,"valid":true,"candidate":candidate}),
                );
            }
            studio.project.apply(&commands)?;
        }
        "undo" => studio.project.undo(false)?,
        "redo" => studio.project.undo(true)?,
        "preview" => {
            let frame = args["frame"]
                .as_i64()
                .filter(|frame| (0..=1_000_000).contains(frame))
                .ok_or("frame must be an integer in 0..=1000000")?;
            let node: Option<NodeId> = args
                .get("node")
                .map(|node| node.as_str().ok_or("node must be a UUID"))
                .transpose()?
                .map(str::parse)
                .transpose()?;
            if node.is_some_and(|id| !studio.project.document.snapshot().nodes.contains_key(&id)) {
                return Err("preview node does not exist".into());
            }
            veylvik_document::Time::at_frame(frame, studio.project.document.snapshot().frame_rate)?;
            studio.project.frame = frame;
            studio.project.preview = node;
        }
        _ => return Err("unknown live tool".into()),
    }
    studio.playing = false;
    studio.refresh_draft();
    studio.request_preview();
    Ok(
        json!({"session":session,"revision":studio.project.document.revision(),"frame":studio.project.frame,"preview_node":studio.project.preview.unwrap_or(studio.project.document.snapshot().output)}),
    )
}
