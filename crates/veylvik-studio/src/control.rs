//! Live edits execute on the UI thread; image encoding uses immutable snapshots.
use crate::Studio;
use serde_json::{Value, json};
use veylvik_document::{Command, NodeId};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub fn catalog() -> Value {
    catalog_ref().clone()
}
use veylvik_control::live::catalog as catalog_ref;
fn validate_arguments(name: &str, args: &Value) -> Result<()> {
    let catalog = catalog_ref();
    let descriptor = catalog["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == name)
        .ok_or_else(|| {
            format!("this Studio has no tool named {name}: it may be an older build than the MCP server; restart Studio after updating Veylvik")
        })?;
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
pub fn call(
    studio: &mut Studio,
    name: &str,
    args: &Value,
    ctx: &eframe::egui::Context,
) -> Result<Value> {
    validate_arguments(name, args)?;
    if let Some(result) = project_files(studio, name, args, ctx)? {
        return Ok(result);
    }
    if let Some(result) = studio.live_files(name, args)? {
        return Ok(result);
    }
    if name == "nodes" {
        return Ok(veylvik_document::registry::catalog());
    }
    if name == "diagnostics" {
        return Ok(
            json!({"initialized":true,"adapter":studio.renderer.adapter.name,"snapshot":studio.renderer.try_diagnostics()?,"preview_rendering":studio.preview_pending,"playback_cache":studio.scheduler.as_ref().map(|s|s.playback_cache().status())}),
        );
    }
    match name {
        "track_surface" => return studio.live_track_surface(args, ctx),
        "solve_camera" => return studio.live_solve_camera(args, ctx),
        "segment_subject" => return studio.live_segment_subject(args, ctx),
        _ => {}
    }
    let project = &studio.project;
    if name == "inspect" {
        let summary = args
            .get("summary")
            .map(|value| value.as_bool().ok_or("summary must be boolean"))
            .transpose()?
            .unwrap_or(false);
        let mut result = json!({"session":project.session,"revision":project.document.revision(),"history":project.document.history(),"selected":project.selected,"preview":{"node":project.preview.unwrap_or(project.document.snapshot().output),"frame":project.frame,"displayed_time":studio.displayed_time,"rendering":studio.preview_pending,"playing":studio.playing},"viewer":{"exposure":studio.view.exposure,"alpha":studio.view.alpha,"scale":studio.navigation.manual_scale(),"pan":studio.navigation.pan()},"unavailable_assets":project.unavailable_assets(),"inspector_draft":studio.draft_dirty,"busy":studio.files.busy() || studio.authoring_dialog_open() || (studio.graph.dragging() || studio.animation_editor.dragging()),"status":studio.status,"error":studio.error});
        result["job"] = studio.live_job_report();
        result["comparison"] = studio.comparison.status();
        result["scopes"] = studio.scopes.status();
        result["render_queue"] = studio.render_queue.status();
        result["node_editor_visible"] = json!(!studio.node_editor_hidden);
        result["playback"] = json!({"range":studio.playback_controls.range(project.document.snapshot().frame_count),"range_enabled":studio.playback_controls.enabled,"preview_divisor":studio.playback_controls.divisor(),"caching":studio.playback_controls.warming.is_some(),"cache":studio.scheduler.as_ref().map(|s|s.playback_cache().status())});
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
    if name == "materials" {
        return veylvik_io::materials::library(&veylvik_io::materials::directory()?);
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
    let authored = if name == "stand_on_ground" {
        let (commands, result) =
            veylvik_io::placing::stand(studio.project.document.snapshot(), args)?;
        Some(veylvik_presets::authoring::Authored { commands, result })
    } else {
        veylvik_presets::authoring::build(studio.project.document.snapshot(), name, args)?
    };
    if let Some(authored) = authored {
        let mut result = authored.result;
        result["session"] = json!(session);
        result["dry_run"] = json!(dry_run);
        if dry_run {
            studio
                .project
                .document
                .prepare(revision, &authored.commands)?;
            result["revision"] = json!(revision);
            return Ok(result);
        }
        studio.project.apply(&authored.commands)?;
        result["revision"] = json!(studio.project.document.revision());
        response = Some(result);
    }
    match name {
        "add_node" | "gold_title" | "particle_look" | "stand_on_ground" => {}
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
        "apply_material" => {
            let target = args["target"]
                .as_str()
                .ok_or("target must be a UUID")?
                .parse()?;
            let commands = veylvik_io::materials::apply(
                studio.project.document.snapshot(),
                target,
                args["material"]
                    .as_str()
                    .ok_or("material must be a string")?,
                &veylvik_io::materials::directory()?,
            )?;
            if dry_run {
                studio.project.document.prepare(revision, &commands)?;
                return Ok(
                    json!({"session":session,"revision":revision,"dry_run":true,"valid":true}),
                );
            }
            studio.project.apply(&commands)?;
            studio.project.selected = target;
            studio.project.preview = Some(target);
            response = Some(
                json!({"session":session,"revision":studio.project.document.revision(),"dry_run":false,"changed_nodes":commands.len()}),
            );
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
                .filter_map(|node| node.operation.asset())
                .collect();
            for node in candidate.nodes.values() {
                if let Some(asset) = node.operation.asset()
                    && !studio.project.inputs.contains_key(asset)
                    && !known.contains(asset)
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

pub(crate) fn identity(project: &crate::project::Project, args: &Value) -> Result<(NodeId, u64)> {
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

fn snapshot(studio: &Studio, name: &str, args: &Value) -> Result<crate::preview_images::Snapshot> {
    validate_arguments(name, args)?;
    if studio.draft_dirty
        && matches!(
            studio.draft.operation,
            veylvik_document::Operation::Text(veylvik_document::nodes::Text { .. })
        )
    {
        return Err("Apply or revert the text preview before capturing applied pixels".into());
    }
    let (session, revision) = identity(&studio.project, args)?;
    let max_edge = args
        .get("max_edge")
        .map(|value| value.as_u64().ok_or("max_edge must be an integer"))
        .transpose()?
        .unwrap_or(512);
    if !(1..=veylvik_io::preview::MAX_PREVIEW_EDGE as u64).contains(&max_edge) {
        return Err("max_edge must be within 1..2048".into());
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
        measure: if name == "measure" {
            let doc = studio.project.document.snapshot();
            Some((
                if doc.ocio.view.is_some() {
                    veylvik_document::media::DisplayTransform::Standard
                } else {
                    doc.display
                },
                args.get("region")
                    .map(|region| serde_json::from_value(region.clone()))
                    .transpose()?,
            ))
        } else {
            None
        },
    })
}

pub fn dispatch(
    studio: &mut Studio,
    request: veylvik_control::ipc::Request,
    ctx: &eframe::egui::Context,
) {
    if !matches!(request.name.as_str(), "preview_image" | "measure") {
        let result =
            call(studio, &request.name, &request.arguments, ctx).map_err(|error| error.to_string());
        request.respond(result);
        return;
    }
    let prepared = (|| -> Result<_> {
        let snapshot = snapshot(studio, &request.name, &request.arguments)?;
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

/// New, open and import: project-level file actions through the editor's own file jobs.
/// `None` for every other tool.
fn project_files(
    studio: &mut Studio,
    name: &str,
    args: &Value,
    ctx: &eframe::egui::Context,
) -> Result<Option<Value>> {
    if !matches!(
        name,
        "new_project" | "open_project" | "new_from_media" | "import_media"
    ) {
        return Ok(None);
    }
    if studio.files.busy()
        || studio.authoring_dialog_open()
        || studio.confirm.is_some()
        || studio.graph.dragging()
        || studio.animation_editor.dragging()
    {
        return Err("editor is busy; finish the current interaction first".into());
    }
    let path = || -> Result<std::path::PathBuf> {
        let path = std::path::PathBuf::from(args["path"].as_str().ok_or("path must be a string")?);
        if !path.is_absolute() {
            return Err("path must be absolute".into());
        }
        if !path.exists() {
            return Err(format!("{} does not exist", path.display()).into());
        }
        Ok(path)
    };
    if name == "import_media" {
        // The job re-checks both on completion, so an edit in between makes it obsolete.
        identity(&studio.project, args)?;
        if studio.draft_dirty {
            return Err("Apply or revert the inspector draft before importing".into());
        }
        let path = path()?;
        let sequence = args
            .get("sequence")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let matching = args
            .get("match_composition")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let position = match args.get("position") {
            Some(p) => serde_json::from_value(p.clone())?,
            None => studio.graph.insertion_position(ctx),
        };
        studio.import_media_from(position, sequence, Some(path), Some(matching));
        return Ok(Some(
            json!({"started": studio.files.busy(), "session": studio.project.session}),
        ));
    }
    let discard = args
        .get("discard_changes")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if (studio.project.dirty() || studio.draft_dirty) && !discard {
        return Err("The open project has unsaved changes; save it in the editor, or pass discard_changes: true".into());
    }
    let previous = studio.project.session;
    studio.draft_dirty = false;
    match name {
        "new_project" => {
            let size = |key: &str| -> Result<u32> {
                Ok(u32::try_from(
                    args[key]
                        .as_u64()
                        .ok_or(format!("{key} must be an integer"))?,
                )?)
            };
            let frame_rate = match args.get("frame_rate") {
                Some(rate) => serde_json::from_value(rate.clone())?,
                None => veylvik_document::Time { ticks: 24, rate: 1 },
            };
            let frame_count = match args.get("frame_count") {
                Some(count) => {
                    u32::try_from(count.as_u64().ok_or("frame_count must be an integer")?)?
                }
                None => veylvik_document::DEFAULT_FRAME_COUNT,
            };
            let project = crate::project::Project::blank(
                size("width")?,
                size("height")?,
                frame_rate,
                frame_count,
            )?;
            studio.open_project(project, ctx);
            Ok(Some(
                json!({"session": studio.project.session, "revision": studio.project.document.revision()}),
            ))
        }
        "open_project" => {
            studio.open_path(Some(path()?));
            Ok(Some(
                json!({"started": studio.files.busy(), "previous_session": previous}),
            ))
        }
        _ => {
            studio.new_from_media_path(Some(path()?));
            Ok(Some(
                json!({"started": studio.files.busy(), "previous_session": previous}),
            ))
        }
    }
}
