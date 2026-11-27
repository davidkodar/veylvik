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
    let tools = catalog_ref()["tools"].as_array().unwrap();
    if !tools.iter().any(|tool| tool["name"] == name) {
        return Err(format!("this Studio has no tool named {name}: it may be an older build than the MCP server; restart Studio after updating Veylvik").into());
    }
    // The headless server takes a time on these; here they show Studio's viewer.
    let hint = |key: &str| {
        if matches!(key, "ticks" | "rate" | "frame")
            && matches!(
                name,
                "preview_image" | "measure" | "compare_look" | "render"
            )
        {
            ". The live server works on Studio's own viewer: call preview with frame (and node) first, then this tool without a time (render takes frame)".to_string()
        } else {
            String::new()
        }
    };
    veylvik_presets::tools::check(tools, name, args, &hint)?;
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
        let mut result = json!({"session":project.session,"revision":project.document.revision(),"history":project.document.history(),"selected":project.selected,"preview":{"pictures_shown":studio.pictures_shown,"node":project.preview.unwrap_or(project.document.snapshot().output),"frame":project.frame,"displayed_time":studio.displayed_time,"rendering":studio.preview_pending,"playing":studio.transport.playing},"viewer":{"exposure":studio.view.exposure,"alpha":studio.view.alpha,"scale":studio.navigation.manual_scale(),"pan":studio.navigation.pan()},"unavailable_assets":project.unavailable_assets(),"inspector_draft":studio.draft_dirty,"busy":!studio.ready(),"busy_because":studio.busy().map(|b| b.message()),"status":studio.status,"error":studio.error});
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
    if let Some(mut result) = veylvik_presets::tools::read(project.document.snapshot(), name, args)?
    {
        result["session"] = json!(project.session);
        result["revision"] = json!(project.document.revision());
        return Ok(result);
    }
    if name == "materials" {
        return Ok(veylvik_presets::tools::Host::materials(&Live {
            graph: None,
        })?);
    }
    let (session, revision) = identity(project, args)?;
    if let Some(busy) = studio.busy() {
        return Err(busy.message().into());
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
    if name == "apply" {
        // Live only: the editor must already hold every picture the commands name.
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
    }
    let edit = veylvik_presets::tools::edit(
        studio.project.document.snapshot(),
        revision,
        name,
        args,
        &mut Live {
            graph: Some(&mut studio.graph),
        },
    )?;
    if let Some(edit) = edit {
        let focus = edit.focus;
        let mut result =
            veylvik_presets::tools::commit(&mut Opened(&mut studio.project), edit, dry_run)?;
        result["session"] = json!(session);
        if dry_run {
            return Ok(result);
        }
        if let Some(focus) = focus {
            studio.project.selected = focus;
            studio.project.preview = Some(focus);
        }
        response = Some(result);
    }
    match name {
        _ if response.is_some() => {}
        "match_shot" => {
            let node = |name: &str| -> Result<Option<NodeId>> {
                args.get(name)
                    .map(|id| Ok(id.as_str().ok_or("node ids must be strings")?.parse()?))
                    .transpose()
            };
            let render = node("render")?.ok_or("render must be a Render scene node")?;
            if !matches!(
                studio
                    .project
                    .document
                    .snapshot()
                    .nodes
                    .get(&render)
                    .map(|n| &n.operation),
                Some(veylvik_document::Operation::RenderScene(_))
            ) {
                return Err("render must be a Render scene node".into());
            }
            studio.live_match_shot(render, node("light")?, ctx)?;
            return Ok(json!({"session":session,"started":true}));
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
    studio.transport.pause();
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
    if studio.transport.playing
        || studio.preview_pending
        || studio.displayed_ticket != studio.wanted
    {
        return Err(
            "preview is not ready; pause/seek with preview and wait for rendering to finish".into(),
        );
    }
    let displayed = studio
        .displayed
        .as_ref()
        // A played picture is not the renderer's frame: its values are the screen's.
        .filter(|shown| !shown.baked)
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
        task: {
            let doc = studio.project.document.snapshot();
            let display = if doc.ocio.view.is_some() {
                veylvik_document::media::DisplayTransform::Standard
            } else {
                doc.display
            };
            match name {
                "measure" => crate::preview_images::Task::Measure(
                    display,
                    args.get("region")
                        .map(|region| serde_json::from_value(region.clone()))
                        .transpose()?,
                ),
                "compare_look" => crate::preview_images::Task::Look(display, args.clone()),
                _ => crate::preview_images::Task::Picture,
            }
        },
    })
}

pub fn dispatch(
    studio: &mut Studio,
    request: veylvik_control::ipc::Request,
    ctx: &eframe::egui::Context,
) {
    if !matches!(
        request.name.as_str(),
        "preview_image" | "measure" | "compare_look"
    ) {
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
    if let Some(busy) = studio.busy() {
        return Err(busy.message().into());
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

/// The live editor's own parts of the shared tools: the user's material library and
/// ground placement, and new effects and copies laid out in Studio's graph.
struct Live<'a> {
    graph: Option<&'a mut crate::graph::Graph>,
}
impl veylvik_presets::tools::Host for Live<'_> {
    fn materials(&self) -> veylvik_document::Result<Value> {
        veylvik_io::materials::directory()
            .and_then(|dir| veylvik_io::materials::library(&dir))
            .map_err(|e| veylvik_document::Error(e.to_string()))
    }
    fn material(
        &self,
        document: &veylvik_document::Composition,
        target: NodeId,
        material: &str,
    ) -> veylvik_document::Result<Vec<Command>> {
        veylvik_io::materials::directory()
            .and_then(|dir| veylvik_io::materials::apply(document, target, material, &dir))
            .map_err(|e| veylvik_document::Error(e.to_string()))
    }
    fn stand(
        &self,
        document: &veylvik_document::Composition,
        args: &Value,
    ) -> veylvik_document::Result<(Vec<Command>, Value)> {
        veylvik_io::placing::stand(document, args)
            .map_err(|e| veylvik_document::Error(e.to_string()))
    }
    fn shine(
        &self,
        document: &veylvik_document::Composition,
        args: &Value,
    ) -> veylvik_document::Result<(Vec<Command>, Value)> {
        veylvik_io::placing::shine(document, args)
            .map_err(|e| veylvik_document::Error(e.to_string()))
    }
    fn place_effect(
        &mut self,
        document: &veylvik_document::Composition,
        revision: u64,
        input: NodeId,
        effect: &mut veylvik_document::recipes::Effect,
    ) -> veylvik_document::Result<()> {
        match self.graph.as_deref_mut() {
            Some(graph) => graph
                .place_effect(document, revision, input, effect, None)
                .map_err(|e| veylvik_document::Error(e.to_string())),
            None => Ok(()),
        }
    }
    fn place_copy(
        &mut self,
        document: &veylvik_document::Composition,
        revision: u64,
        copy: &mut veylvik_document::editing::BranchCopy,
    ) -> veylvik_document::Result<()> {
        match self.graph.as_deref_mut() {
            Some(graph) => graph
                .place_branch_copy(document, revision, copy)
                .map_err(|e| veylvik_document::Error(e.to_string())),
            None => Ok(()),
        }
    }
}
/// The open project as the shared tools' target: applying goes through the project, so
/// its media are reconciled as for every edit.
struct Opened<'a>(&'a mut crate::project::Project);
impl veylvik_presets::tools::Target for Opened<'_> {
    fn revision(&self) -> u64 {
        self.0.document.revision()
    }
    fn prepare(
        &self,
        commands: &[Command],
    ) -> veylvik_document::Result<veylvik_document::Composition> {
        self.0
            .document
            .prepare(self.0.document.revision(), commands)
    }
    fn apply(&mut self, commands: &[Command]) -> veylvik_document::Result<()> {
        self.0
            .apply(commands)
            .map_err(|e| veylvik_document::Error(e.to_string()))
    }
}
