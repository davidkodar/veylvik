//! Live edits execute on the UI thread; image encoding uses immutable snapshots.
use crate::Studio;
use serde_json::{Value, json};
use veylvik_document::{Command, NodeId, Operation};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
}
pub fn catalog() -> Value {
    catalog_ref().clone()
}
fn catalog_ref() -> &'static Value {
    static CATALOG: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    CATALOG.get_or_init(build_catalog)
}
fn build_catalog() -> Value {
    let session = json!({"type":"string","format":"uuid"});
    let revision = json!({"type":"integer","minimum":0});
    json!({"tools":[
        tool("inspect","Read project state and revision. Set summary=true for compact composition metadata without node payloads; graph and get_node provide targeted inspection.",json!({"summary":{"type":"boolean"}}),&[]),
        tool("graph","Page node metadata in stable UUID order without parameter payloads. Labels over 256 characters are marked as truncated. Compare revisions between pages.",json!({"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":128}}),&[]),
        tool("get_node","Read one exact authored node, or just a named parameter. Does not evaluate curves or change selection.",json!({"id":{"type":"string","format":"uuid"},"parameter":{"type":"string"}}),&["id"]),
        tool("diagnostics","Read GPU buffer and cache counters without waiting for an active render. Snapshot is null while the renderer is busy. Declared buffers exclude driver and rasterizer scratch allocations.",json!({}),&[]),
        tool("nodes","Read built-in operations, ports, defaults and parameter schemas.",json!({}),&[]),
        tool("get_preset","Read a bundled starter, or capture a group with its current values and animation. Supply exactly one of starter or group. No filesystem access.",json!({"group":{"type":"string","format":"uuid"},"starter":{"type":"string","enum":veylvik_document::presets::builtin::ALL.map(|p|p.id)}}),&[]),
        tool("insert_preset","Insert an independent instance with explicitly bound inputs. One undo step; output remains unchanged. Dry-run IDs are provisional.",json!({"session":session,"revision":revision,"preset":veylvik_document::schema::preset(),"inputs":{"type":"object","additionalProperties":{"type":"string","format":"uuid"}},"sizing":{"type":"string","enum":["fit","fill","original"],"default":"fit"},"position":veylvik_document::schema::position(),"dry_run":{"type":"boolean"}}),&["session","revision","preset","inputs"]),
        tool("apply","Validate or atomically apply commands to the open editor. Requires the session and revision from inspect. set_parameter and set_key preserve node identity and wiring. Image references must already be known to the editor; new paths require import. Unapplied inspector changes or file jobs block mutation.",json!({"session":session,"revision":revision,"dry_run":{"type":"boolean"},"commands":{"type":"array","minItems":1,"maxItems":4096,"items":veylvik_document::schema::command()}}),&["session","revision","commands"]),
        tool("add_effect","Create an editable effect group with exposed controls and ordinary internal nodes as one undoable transaction. Places the effect without overlapping existing nodes, selects/previews its root, and preserves composition output. Returns named role IDs for editing controls. Discover recipes with nodes; dry-run IDs are provisional.",json!({"session":session,"revision":revision,"input":{"type":"string","format":"uuid"},"recipe":{"type":"string","enum":veylvik_document::recipes::Recipe::ALL},"dry_run":{"type":"boolean"}}),&["session","revision","input","recipe"]),
        tool("duplicate_branch","Copy a node and all upstream dependencies as one undoable transaction, preserving shared ancestors and asset references. Places the copy outside existing graph bounds, selects/previews its root, and leaves composition output unchanged. Returns root and old-to-new ID mapping. Dry-run IDs are provisional and are not reused on commit.",json!({"session":session,"revision":revision,"node":{"type":"string","format":"uuid"},"dry_run":{"type":"boolean"}}),&["session","revision","node"]),
        tool("undo","Undo one editor transaction.",json!({"session":session,"revision":revision}),&["session","revision"]),
        tool("redo","Redo one editor transaction.",json!({"session":session,"revision":revision}),&["session","revision"]),
        tool("preview_image","Capture the current completed preview as an inline PNG without changing the editor. Use preview to pause/seek first; pending or stale frames are rejected. Captures applied pixels, excluding inspector drafts, overlays and viewer exposure/alpha settings. Encoding runs in the background; returned session/revision/time identify the captured snapshot.",json!({"session":session,"revision":revision,"max_edge":{"type":"integer","minimum":1,"maximum":1024}}),&["session","revision"]),
        tool("preview","Pause playback, seek to an absolute frame and optionally preview a node. The GPU preview is asynchronous; composition output is unchanged.",json!({"session":session,"revision":revision,"frame":{"type":"integer","minimum":0,"maximum":1000000},"node":{"type":"string","format":"uuid"}}),&["session","revision","frame"])
    ]})
}
fn validate_arguments(name: &str, args: &Value) -> Result<()> {
    let catalog = catalog_ref();
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
    Ok(())
}
pub fn call(studio: &mut Studio, name: &str, args: &Value) -> Result<Value> {
    validate_arguments(name, args)?;
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
        let summary = args
            .get("summary")
            .map(|value| value.as_bool().ok_or("summary must be boolean"))
            .transpose()?
            .unwrap_or(false);
        let mut result = json!({"session":project.session,"revision":project.document.revision(),"history":project.document.history(),"selected":project.selected,"preview":{"node":project.preview.unwrap_or(project.document.snapshot().output),"frame":project.frame,"displayed_time":studio.displayed_time,"rendering":studio.preview_pending,"playing":studio.playing},"viewer":{"exposure":studio.view.exposure,"alpha":studio.view.alpha,"scale":studio.navigation.manual_scale(),"pan":studio.navigation.pan()},"unavailable_assets":project.unavailable_assets(),"inspector_draft":studio.draft_dirty,"busy":studio.files.busy() || studio.authoring_dialog_open() || (studio.graph.dragging() || studio.animation_editor.dragging()),"status":studio.status,"error":studio.error});
        if summary {
            result["summary"] = veylvik_document::inspection::summary(project.document.snapshot());
        } else {
            result["composition"] = serde_json::to_value(project.document.snapshot())?;
        }
        return Ok(result);
    }
    if matches!(name, "graph" | "get_node") {
        let mut result = read_graph(project.document.snapshot(), name, args)?;
        result["session"] = json!(project.session);
        result["revision"] = json!(project.document.revision());
        return Ok(result);
    }
    if name == "get_preset" {
        let preset = veylvik_document::presets::read(
            project.document.snapshot(),
            args.get("group"),
            args.get("starter"),
        )?;
        return Ok(
            json!({"session":project.session,"revision":project.document.revision(),"preset":preset}),
        );
    }
    let (session, revision) = identity(project, args)?;
    if studio.files.busy()
        || studio.authoring_dialog_open()
        || studio.confirm.is_some()
        || (studio.graph.dragging() || studio.animation_editor.dragging())
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
    let mut response = None;
    match name {
        "insert_preset" => {
            let preset: veylvik_document::presets::Preset =
                serde_json::from_value(args["preset"].clone())?;
            let inputs = serde_json::from_value(args["inputs"].clone())?;
            let position = args
                .get("position")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()?
                .flatten();
            let instance = preset.instantiate_sized(
                studio.project.document.snapshot(),
                &inputs,
                position,
                args.get("sizing")
                    .map(|v| serde_json::from_value(v.clone()))
                    .transpose()?
                    .unwrap_or_default(),
            )?;
            let mut result = json!({"session":session,"revision":revision,"group":instance.group,"root":instance.output,"dry_run":dry_run,"valid":true});
            if dry_run {
                return Ok(result);
            }
            studio.project.apply(&instance.commands)?;
            studio.project.selected = instance.output;
            studio.project.preview = Some(instance.output);
            result["revision"] = json!(studio.project.document.revision());
            response = Some(result);
        }
        "add_effect" => {
            let input = args["input"]
                .as_str()
                .ok_or("input must be a UUID")?
                .parse()?;
            let recipe: veylvik_document::recipes::Recipe =
                serde_json::from_value(args["recipe"].clone())?;
            let mut effect = recipe.build(studio.project.document.snapshot(), input)?;
            studio.graph.place_effect(
                studio.project.document.snapshot(),
                revision,
                input,
                &mut effect,
                None,
            )?;
            let mut result = json!({"session":session,"revision":revision,"root":effect.root,"roles":effect.roles,"added_nodes":effect.nodes.len(),"recipe":recipe,"dry_run":dry_run,"valid":true});
            if dry_run {
                studio.project.document.prepare(
                    revision,
                    &effect.into_grouped_commands(studio.project.document.snapshot())?,
                )?;
                return Ok(result);
            }
            studio.project.insert_effect(effect)?;
            result["revision"] = json!(studio.project.document.revision());
            response = Some(result);
        }
        "duplicate_branch" => {
            let root = args["node"]
                .as_str()
                .ok_or("node must be a UUID")?
                .parse()?;
            let mut copy =
                veylvik_document::editing::copy_branch(studio.project.document.snapshot(), root)?;
            studio.graph.place_branch_copy(
                studio.project.document.snapshot(),
                revision,
                &mut copy,
            )?;
            let mut result = json!({"session":session,"revision":revision,"root":copy.root,"mapping":copy.mapping,"copied_nodes":copy.nodes.len(),"dry_run":dry_run,"valid":true});
            if dry_run {
                studio
                    .project
                    .document
                    .prepare(revision, &copy.into_commands())?;
                return Ok(result);
            }
            studio.project.insert_branch(copy)?;
            result["revision"] = json!(studio.project.document.revision());
            response = Some(result);
        }
        "apply" => {
            let commands: Vec<Command> = serde_json::from_value(args["commands"].clone())?;
            let candidate = studio.project.document.prepare(revision, &commands)?;
            let known: std::collections::BTreeSet<_> = studio
                .project
                .document
                .snapshot()
                .nodes
                .values()
                .filter_map(|node| match &node.operation {
                    Operation::Image { asset } => Some(asset.as_str()),
                    _ => None,
                })
                .collect();
            for node in candidate.nodes.values() {
                if let Operation::Image { asset } = &node.operation
                    && !studio.project.inputs.contains_key(asset)
                    && !known.contains(asset.as_str())
                {
                    return Err(format!(
                        "image asset is not known to the editor: {asset}; import it first"
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
    let mut result = response.unwrap_or_else(
        || json!({"session":session,"revision":studio.project.document.revision()}),
    );
    result["frame"] = json!(studio.project.frame);
    result["preview_node"] = json!(
        studio
            .project
            .preview
            .unwrap_or(studio.project.document.snapshot().output)
    );
    Ok(result)
}

fn read_graph(document: &veylvik_document::Composition, name: &str, args: &Value) -> Result<Value> {
    if name == "graph" {
        let offset = args
            .get("offset")
            .map(|value| value.as_u64().ok_or("offset must be an unsigned integer"))
            .transpose()?
            .unwrap_or(0);
        let limit = args
            .get("limit")
            .map(|value| value.as_u64().ok_or("limit must be an unsigned integer"))
            .transpose()?
            .unwrap_or(32);
        Ok(veylvik_document::inspection::page(document, offset, limit)?)
    } else {
        let id = args["id"].as_str().ok_or("id must be a UUID")?.parse()?;
        let parameter = args
            .get("parameter")
            .map(|value| value.as_str().ok_or("parameter must be a string"))
            .transpose()?;
        Ok(veylvik_document::inspection::node(document, id, parameter)?)
    }
}

fn identity(project: &crate::project::Project, args: &Value) -> Result<(NodeId, u64)> {
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
    Ok((session, revision))
}

fn snapshot(studio: &Studio, args: &Value) -> Result<crate::preview_images::Snapshot> {
    validate_arguments("preview_image", args)?;
    let (session, revision) = identity(&studio.project, args)?;
    let max_edge = args
        .get("max_edge")
        .map(|value| value.as_u64().ok_or("max_edge must be an integer"))
        .transpose()?
        .unwrap_or(512);
    if !(1..=veylvik_io::preview::MAX_PREVIEW_EDGE as u64).contains(&max_edge) {
        return Err("max_edge must be within 1..1024".into());
    }
    if studio.playing || studio.preview_pending || studio.displayed_ticket != studio.wanted {
        return Err(
            "preview is not ready; pause/seek with preview and wait for rendering to finish".into(),
        );
    }
    let displayed = studio
        .displayed
        .as_ref()
        .ok_or("no completed preview is available")?;
    let time = studio
        .displayed_time
        .ok_or("no completed preview time is available")?;
    let node = studio
        .project
        .preview
        .unwrap_or(studio.project.document.snapshot().output);
    let metadata = json!({"session":session,"revision":revision,"node":node,"time":time,"frame":studio.project.frame,"source_region":displayed.frame.region(),"inspector_draft":studio.draft_dirty,"viewer_settings_applied":false,"encoding":"sRGB straight-alpha PNG","filter":"linear-premultiplied area"});
    Ok(crate::preview_images::Snapshot {
        frame: displayed.frame.clone(),
        metadata,
        max_edge: max_edge as u32,
    })
}

pub fn dispatch(studio: &mut Studio, request: veylvik_control::ipc::Request) {
    if request.name != "preview_image" {
        let result =
            call(studio, &request.name, &request.arguments).map_err(|error| error.to_string());
        request.respond(result);
        return;
    }
    let prepared = (|| -> Result<_> {
        let snapshot = snapshot(studio, &request.arguments)?;
        if studio.preview_images.is_none() {
            studio.preview_images = Some(crate::preview_images::Worker::new()?);
        }
        Ok(snapshot)
    })();
    match prepared {
        Ok(snapshot) => studio.preview_images.as_ref().unwrap().submit(
            request,
            studio.renderer.clone(),
            snapshot,
        ),
        Err(error) => request.respond(Err(error.to_string())),
    }
}
