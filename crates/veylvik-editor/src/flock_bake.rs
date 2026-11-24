//! Baking a flock: its whole simulation written to a file beside the project on the file worker, then read instead of simulated, so an approved flock stays exactly as it is.
use crate::{Studio, files::Completed, sequence::Progress};
use eframe::egui;
use std::sync::{Arc, atomic::Ordering};
use veylvik_document::{Command, NodeId, Operation, Time};

/// A bake under way: its node and progress.
#[derive(Default)]
pub(crate) struct Baking {
    pub(crate) active: Option<(NodeId, Arc<Progress>)>,
}
/// A finished bake, to point its node at on the UI thread.
pub struct Baked {
    node: NodeId,
    /// The file, as the node names it (relative to the project).
    name: String,
    file: std::path::PathBuf,
    frames: u32,
}
fn is_flock(operation: &Operation) -> bool {
    matches!(operation, Operation::Particles(p)
        if !matches!(p.flock_radius, veylvik_document::Scalar::Constant { value } if value <= 0.0))
}
impl Studio {
    /// What the selected flock's bake is, in a line (None: not a flock).
    pub(crate) fn flock_bake_status(&self) -> Option<String> {
        if !is_flock(&self.draft.operation) {
            return None;
        }
        if let Some((node, progress)) = &self.flock_baking.active
            && *node == self.draft.id
        {
            return Some(format!(
                "Baking… {} of {} frames",
                progress.completed.load(Ordering::Relaxed),
                progress.total.load(Ordering::Relaxed)
            ));
        }
        let Operation::Particles(p) = &self.draft.operation else {
            return None;
        };
        if p.bake.is_empty() {
            return Some("Simulated: it may settle differently on another card or driver. Bake it to keep it as it is.".into());
        }
        let doc = self.project.document.snapshot();
        let now = veylvik_render::Renderer::flock_key(doc, self.draft.id);
        match self.project.inputs.flock(&p.bake) {
            None => Some(format!(
                "The bake {} cannot be read: it is simulated",
                p.bake
            )),
            Some(bake) if Some(bake.key()) != now => Some(
                "Its settings changed since it was baked: it is simulated until you bake it again"
                    .into(),
            ),
            Some(bake) => Some(format!(
                "Baked: {} frames, read from {}",
                bake.frames().end - bake.frames().start,
                p.bake
            )),
        }
    }
    /// Bakes the selected flock over the whole composition.
    pub(crate) fn bake_flock(&mut self, ctx: &egui::Context) {
        if self.flock_baking.active.is_some() {
            return self.fail("A flock is being baked already");
        }
        let Some(project) = self.project.path.clone() else {
            return self.fail("Save the project first: the bake is written beside it");
        };
        let doc = self.project.document.snapshot().clone();
        let node = self.draft.id;
        let label = doc
            .nodes
            .get(&node)
            .map_or("flock".to_string(), |n| n.label.clone());
        let stem = project
            .file_stem()
            .map_or("project".into(), |s| s.to_string_lossy().into_owned());
        let safe: String = label
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let name = format!("{stem}.{safe}.veyflock");
        let folder = project
            .parent()
            .map_or_else(|| self.project.base.clone(), |p| p.to_path_buf());
        let file = folder.join(&name);
        let plan = match veylvik_render::Plan::compile_output(&doc, node) {
            Ok(plan) => plan,
            Err(e) => return self.fail(e.to_string()),
        };
        let Some(key) = veylvik_render::Renderer::flock_key(&doc, node) else {
            return self.fail("The flock is missing");
        };
        let header = veylvik_io::flock::Header {
            key,
            frame_rate: doc.frame_rate,
            first: 0,
            frames: doc.frame_count,
            particles: veylvik_render::flock_capacity(doc.width, doc.height),
        };
        let renderer = self.renderer.clone();
        let progress = Arc::new(Progress::default());
        progress.total.store(doc.frame_count, Ordering::Relaxed);
        let worker = progress.clone();
        let ctx = ctx.clone();
        let (session, rate) = (self.project.session, doc.frame_rate);
        let work = self.files.start(move || {
            veylvik_io::flock::write(&file, header, |k| {
                if worker.cancel.load(Ordering::Relaxed) {
                    return Err("Baking was stopped".into());
                }
                let time = Time::at_frame(i64::from(k), rate).map_err(|e| e.to_string())?;
                let states = renderer
                    .flock_states(&plan, node, time)
                    .map_err(|e| e.to_string())?;
                worker.completed.store(k + 1, Ordering::Relaxed);
                ctx.request_repaint();
                Ok(states)
            })
            .map(|()| {
                let baked = Baked {
                    node,
                    name,
                    file,
                    frames: header.frames,
                };
                // A bake for a project that is no longer open is simply not read.
                Completed::then(move |s, _| s.flock_baked(baked)).in_session(session, "")
            })
        });
        if work.is_ok() {
            self.flock_baking.active = Some((node, progress));
        }
        self.file_started(work);
    }
    /// A finished bake: the node reads it from now on.
    pub(crate) fn flock_baked(&mut self, baked: Baked) {
        match veylvik_io::flock::Bake::open(&baked.file) {
            Ok(bake) => self
                .project
                .inputs
                .insert_flock(baked.name.clone(), Arc::new(bake)),
            Err(e) => return self.fail(e),
        }
        let doc = self.project.document.snapshot();
        let Some(mut node) = doc.nodes.get(&baked.node).cloned() else {
            return;
        };
        if let Operation::Particles(p) = &mut node.operation {
            p.bake = baked.name.clone();
        }
        match self.project.apply(&[Command::Put {
            node: Box::new(node),
        }]) {
            Ok(_) => {
                self.refresh_draft();
                self.request_preview();
                self.status = format!("Baked {} frames to {}", baked.frames, baked.name);
            }
            Err(e) => self.fail(e.to_string()),
        }
    }
    /// Back to simulating the selected flock (the file is left where it is).
    pub(crate) fn unbake_flock(&mut self) {
        let doc = self.project.document.snapshot();
        let Some(mut node) = doc.nodes.get(&self.draft.id).cloned() else {
            return;
        };
        if let Operation::Particles(p) = &mut node.operation {
            p.bake.clear();
        }
        match self.project.apply(&[Command::Put {
            node: Box::new(node),
        }]) {
            Ok(_) => {
                self.refresh_draft();
                self.request_preview();
            }
            Err(e) => self.fail(e.to_string()),
        }
    }
}
