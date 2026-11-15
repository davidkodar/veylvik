//! Long analyses requested over live MCP run on the editor's own windows and jobs, so the
//! user sees their progress; one record per Studio reports them through `inspect`.
use crate::Studio;
use eframe::egui;
use serde_json::{Value, json};
use veylvik_document::NodeId;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct LiveJob {
    tool: &'static str,
    session: NodeId,
    outcome: Option<std::result::Result<Value, String>>,
}
impl Studio {
    fn live_job_ready(&self, args: &Value) -> Result<()> {
        crate::control::identity(&self.project, args)?;
        if self.files.busy() || self.authoring_dialog_open() || self.confirm.is_some() {
            return Err("editor is busy; finish the current interaction first".into());
        }
        if self.planar.active.is_some() || self.subject.active.is_some() {
            return Err("a tracking or segmentation job is already running".into());
        }
        if self.draft_dirty {
            return Err("Apply or revert the inspector draft before live edits".into());
        }
        Ok(())
    }
    fn live_job_started(&mut self, tool: &'static str, running: bool) -> Result<Value> {
        if !running {
            return Err(if self.error {
                self.status.clone()
            } else {
                format!("{tool} could not start")
            }
            .into());
        }
        self.live_job = Some(LiveJob {
            tool,
            session: self.project.session,
            outcome: None,
        });
        Ok(json!({"started": true, "session": self.project.session}))
    }
    pub(crate) fn live_track_surface(
        &mut self,
        args: &Value,
        ctx: &egui::Context,
    ) -> Result<Value> {
        self.live_job_ready(args)?;
        let request = veylvik_track::request::Request::from_json(args)?;
        let doc = self.project.document.snapshot();
        let last = i64::from(doc.frame_count.saturating_sub(1));
        if !doc.nodes.contains_key(&request.source) {
            return Err("tracking source node does not exist".into());
        }
        if request.frames.iter().any(|f| *f > last) || request.frames[0] == request.frames[1] {
            return Err(format!("start and end must differ and lie within 0..={last}").into());
        }
        if request.back_to.is_some_and(|f| f != 0) {
            return Err("live back_to must be 0: Studio tracks backward to the first frame".into());
        }
        veylvik_document::planar::validate_quad(request.surface)?;
        self.project.frame = request.frames[0];
        self.planar.configure_live(&request);
        self.run_planar(ctx);
        let running = self.planar.active.is_some();
        self.live_job_started("track_surface", running)
    }
    pub(crate) fn live_segment_subject(
        &mut self,
        args: &Value,
        ctx: &egui::Context,
    ) -> Result<Value> {
        self.live_job_ready(args)?;
        let doc = self.project.document.snapshot();
        let request = veylvik_segment::request::Request::from_json(args, doc.frame_count)?;
        if !doc.nodes.contains_key(&request.source) {
            return Err("segmentation source node does not exist".into());
        }
        if request.analysis.back_to.is_some_and(|f| f != 0) {
            return Err("live back_to must be 0: Studio tracks backward to the first frame".into());
        }
        let last = i64::from(doc.frame_count.saturating_sub(1));
        if request
            .analysis
            .frames
            .iter()
            .any(|f| !(0..=last).contains(f))
        {
            return Err(format!("frame and end must lie within 0..={last}").into());
        }
        self.subject.configure_live(&request)?;
        self.project.frame = request.analysis.frames[0];
        self.track_subject(ctx);
        let running = self.subject.active.is_some();
        self.live_job_started("segment_subject", running)
    }
    /// Called by the windows' completion handlers; ignored when no live job is running.
    pub(crate) fn finish_live_job(
        &mut self,
        tool: &str,
        outcome: std::result::Result<Value, String>,
    ) {
        if let Some(job) = &mut self.live_job
            && job.tool == tool
            && job.outcome.is_none()
        {
            job.outcome = Some(outcome);
        }
    }
    pub(crate) fn live_job_report(&self) -> Value {
        let Some(job) = &self.live_job else {
            return Value::Null;
        };
        let active = match job.tool {
            "track_surface" => self.planar.active.as_ref(),
            _ => self.subject.active.as_ref(),
        };
        let progress = |p: &crate::sequence::Progress| {
            use std::sync::atomic::Ordering::Relaxed;
            json!({"completed": p.completed.load(Relaxed), "total": p.total.load(Relaxed)})
        };
        let mut report = json!({"tool": job.tool, "session": job.session});
        match (&job.outcome, active) {
            (Some(Ok(result)), _) => {
                report["status"] = json!("done");
                report["result"] = result.clone();
            }
            (Some(Err(error)), _) => {
                report["status"] = json!("failed");
                report["error"] = json!(error);
            }
            (None, Some(p)) => {
                report["status"] = json!("running");
                report["progress"] = progress(p);
            }
            // The job ended without publishing: cancelled, or failed before completion.
            (None, None) => {
                report["status"] = json!("failed");
                report["error"] = json!(if self.error {
                    self.status.as_str()
                } else {
                    "cancelled"
                });
            }
        }
        report
    }
}
