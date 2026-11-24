//! File tools of the live MCP server: saving, rendering and exporting run on the editor's own file worker and render queue, so nothing blocks the window and the user sees the work.
use crate::{Studio, files::Completed, render_queue};
use serde_json::{Value, json};
use std::path::PathBuf;
use veylvik_document::{NodeId, Time, sequence::FrameRange};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn absolute(args: &Value) -> Result<PathBuf> {
    let path = PathBuf::from(args["path"].as_str().ok_or("path must be a string")?);
    if !path.is_absolute() {
        return Err("path must be absolute".into());
    }
    Ok(path)
}
impl Studio {
    fn live_files_ready(&self, args: &Value) -> Result<()> {
        crate::control::identity(&self.project, args)?;
        if self.files.busy() || self.authoring_dialog_open() || self.confirm.is_some() {
            return Err("editor is busy; finish the current interaction first".into());
        }
        if self.draft_dirty {
            return Err("Apply or revert the inspector draft first".into());
        }
        Ok(())
    }
    /// The node a render shows: the named one, or the composition's output.
    fn live_render_node(&self, args: &Value) -> Result<NodeId> {
        let doc = self.project.document.snapshot();
        match args.get("node") {
            None => Ok(doc.output),
            Some(node) => {
                let id: NodeId = node.as_str().ok_or("node must be a UUID")?.parse()?;
                if !doc.nodes.contains_key(&id) {
                    return Err("node does not exist".into());
                }
                Ok(id)
            }
        }
    }
    /// Runs a live file tool, or returns `None` for any other tool.
    pub(crate) fn live_files(&mut self, name: &str, args: &Value) -> Result<Option<Value>> {
        if !matches!(
            name,
            "save" | "render" | "render_sequence" | "render_video" | "outline_text"
        ) {
            return Ok(None);
        }
        self.live_files_ready(args)?;
        let doc = self.project.document.snapshot();
        let session = self.project.session;
        let started = |studio: &mut Self, work: std::result::Result<(), String>| -> Result<Value> {
            let ok = work.is_ok();
            studio.file_started(work);
            if !ok {
                return Err(studio.status.clone().into());
            }
            Ok(json!({"started": true, "session": session}))
        };
        if name == "save" {
            let path = match args.get("path") {
                Some(_) => absolute(args)?,
                None => self
                    .project
                    .path
                    .clone()
                    .ok_or("the project has never been saved: give a path")?,
            };
            let snapshot = self.project.save_snapshot();
            let work = self.files.start(move || {
                snapshot
                    .save(&path)
                    .map(|saved| Completed::then(move |s, ctx| s.saved(saved, ctx)))
                    .map_err(|e| e.to_string())
            });
            return started(self, work).map(Some);
        }
        if name == "outline_text" {
            let mut payload = args.clone();
            for key in ["session", "revision"] {
                payload.as_object_mut().unwrap().remove(key);
            }
            let request: veylvik_io::text::OutlineRequest = serde_json::from_value(payload)?;
            let base = self.project.base.clone();
            let work = self.files.start(move || {
                let outlined =
                    veylvik_io::text::outline(&base, &request).map_err(|e| e.to_string())?;
                let result = json!({"operation":{"type":"path","geometry":outlined.geometry},"glyph_count":outlined.glyph_count,"advance":outlined.advance,"line_count":outlined.line_count});
                Ok(Completed::then(move |s, _| s.live_done("outline_text", result)))
            });
            let reply = started(self, work)?;
            self.begin_live_job("outline_text");
            return Ok(Some(reply));
        }
        let path = absolute(args)?;
        let output = match name {
            "render" => {
                let frame = match args.get("frame") {
                    Some(frame) => frame.as_i64().ok_or("frame must be an integer")?,
                    None => self.project.frame,
                };
                if !(0..i64::from(doc.frame_count)).contains(&frame) {
                    return Err(format!("frame must lie within 0..{}", doc.frame_count).into());
                }
                render_queue::Output::Frame {
                    time: Time::at_frame(frame, doc.frame_rate)?,
                    output: self.live_render_node(args)?,
                }
            }
            "render_sequence" => {
                let frame = |key: &str| {
                    args[key]
                        .as_i64()
                        .ok_or(format!("{key} must be an integer"))
                };
                render_queue::Output::Sequence {
                    frames: FrameRange::new(frame("start")?, frame("end")?, doc.frame_rate)?,
                    format: match args.get("format").and_then(Value::as_str) {
                        None | Some("png") => veylvik_io::FrameFormat::Png,
                        Some("exr") => veylvik_io::FrameFormat::Exr,
                        Some(_) => return Err("format must be png or exr".into()),
                    },
                    output: self.live_render_node(args)?,
                }
            }
            _ => {
                let mut settings =
                    serde_json::to_value(veylvik_io::video_export::Settings::for_document(doc))?;
                if let Some(given) = args.get("settings") {
                    for (key, value) in given.as_object().ok_or("settings must be an object")? {
                        settings[key.as_str()] = value.clone();
                    }
                }
                let mut settings: veylvik_io::video_export::Settings =
                    serde_json::from_value(settings)?;
                settings.audio.base = Some(self.project.base.clone());
                render_queue::Output::Video(Box::new(settings))
            }
        };
        let document = doc.clone();
        let base = self.project.base.clone();
        let label = self.file_stem();
        let reach = self.project.reach.clone();
        let work = self.files.start(move || {
            render_queue::Job::prepare(label, document, base, path, output, reach)
                .map(|job| Completed::then(move |s, _| s.queued(job)))
        });
        let reply = started(self, work)?;
        self.start_queued = true;
        Ok(Some(reply))
    }
}
