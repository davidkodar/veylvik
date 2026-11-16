//! Measure from plate for Match grain: render the plate at the playhead off the UI thread, measure its grain and set the node's settings as one undo step.
use crate::Studio;
use eframe::egui;
use std::sync::{Arc, Mutex};
use veylvik_document::{Command, NodeId, Operation, Time};
use veylvik_track::grain::Grain;

/// A running measurement: its node, frame and result slot.
#[derive(Clone)]
struct Job {
    node: NodeId,
    time: Time,
    result: Arc<Mutex<Option<Result<Grain, String>>>>,
}
fn id() -> egui::Id {
    egui::Id::new("grain-measure")
}
impl Studio {
    pub(crate) fn measure_grain(&mut self, ctx: &egui::Context) {
        if !self.apply_draft() {
            return;
        }
        let node = self.project.selected;
        let doc = self.project.document.snapshot();
        let Some(plate) = doc
            .nodes
            .get(&node)
            .and_then(|n| n.inputs.get("plate"))
            .copied()
        else {
            return self.fail("Connect the footage to the Plate input first");
        };
        let time = match self.project.time() {
            Ok(t) => t,
            Err(e) => return self.fail(e),
        };
        let plan = match veylvik_render::Plan::compile_output(doc, plate) {
            Ok(p) => p,
            Err(e) => return self.fail(e),
        };
        let (renderer, inputs, (w, h)) = (
            self.renderer.clone(),
            self.project.inputs.clone(),
            (doc.width, doc.height),
        );
        let result = Arc::new(Mutex::new(None));
        let (slot, repaint) = (result.clone(), ctx.clone());
        std::thread::spawn(move || {
            let measured = renderer
                .render_frame(&plan, veylvik_render::RenderRequest::full(time), &inputs)
                .map_err(|e| e.to_string())
                .and_then(|frame| {
                    veylvik_track::grain::measure(&frame.pixels, w, h).ok_or_else(|| {
                        "No flat areas to measure grain in at this frame; try another frame"
                            .to_owned()
                    })
                });
            if let Ok(mut slot) = slot.lock() {
                *slot = Some(measured);
            }
            repaint.request_repaint();
        });
        ctx.data_mut(|d| d.insert_temp(id(), Job { node, time, result }));
        self.status = "Measuring the plate's grain…".into();
        self.error = false;
    }
    /// Apply a finished measurement; call every frame.
    pub(crate) fn grain_measure_tick(&mut self, ctx: &egui::Context) {
        let Some(job) = ctx.data(|d| d.get_temp::<Job>(id())) else {
            return;
        };
        let Some(result) = job.result.lock().ok().and_then(|mut r| r.take()) else {
            return;
        };
        ctx.data_mut(|d| d.remove::<Job>(id()));
        let grain = match result {
            Ok(g) => g,
            Err(e) => return self.fail(e),
        };
        let Some(mut node) = self
            .project
            .document
            .snapshot()
            .nodes
            .get(&job.node)
            .cloned()
        else {
            return;
        };
        let Operation::MatchGrain(g) = &mut node.operation else {
            return;
        };
        let set = |s: &mut veylvik_document::Scalar, v: f32| {
            veylvik_document::track::set_scalar_at(s, job.time, v)
        };
        let applied = set(&mut g.size, grain.size as f32)
            .and_then(|_| set(&mut g.shadows, grain.shadows))
            .and_then(|_| set(&mut g.midtones, grain.midtones))
            .and_then(|_| set(&mut g.highlights, grain.highlights))
            .and_then(|_| set(&mut g.red, grain.red))
            .and_then(|_| set(&mut g.blue, grain.blue))
            .and_then(|_| set(&mut g.colour, grain.colour));
        if let Err(e) = applied {
            return self.fail(e);
        }
        match self.project.apply(&[Command::Put {
            node: Box::new(node),
        }]) {
            Ok(()) => {
                self.status = format!(
                    "Grain measured: {:.3} in the midtones, size {}, {}",
                    grain.midtones,
                    grain.size,
                    if grain.colour > 0.7 {
                        "mostly grey"
                    } else {
                        "with colour"
                    }
                );
                self.error = false;
                self.refresh_draft();
                self.request_preview();
            }
            Err(e) => self.fail(e),
        }
    }
}
