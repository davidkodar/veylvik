//! Match the shot: render the footage behind a lit surface off the UI thread and
//! place the key light where its light comes from.
use eframe::egui;
use std::sync::mpsc;
use veylvik_document::{
    Command, NodeId, Operation,
    optical::{ShotLight, StudioRig},
};

type Analysis = Result<(ShotLight, u32, u32), String>;
#[derive(Default)]
pub(crate) struct Pending {
    job: Option<(NodeId, mpsc::Receiver<Analysis>)>,
}
fn rig_mut(operation: &mut Operation) -> Option<&mut StudioRig> {
    match operation {
        Operation::Material(m) => Some(&mut m.studio),
        Operation::LightRig(rig) => Some(&mut rig.studio),
        _ => None,
    }
}
impl crate::Studio {
    /// The shot a rig or material sits in: the material's Environment input, any
    /// Environment of a surface the rig lights, else the composition's backdrop.
    fn shot_for(&self, target: NodeId) -> Option<NodeId> {
        let doc = self.project.document.snapshot();
        let environment = |id: &NodeId| doc.nodes.get(id)?.inputs.get("environment").copied();
        environment(&target)
            .or_else(|| {
                doc.nodes
                    .values()
                    .filter(|n| n.inputs.get("lights") == Some(&target))
                    .find_map(|n| environment(&n.id))
            })
            .or_else(|| veylvik_document::presets::background::automatic(doc))
            .filter(|id| *id != target)
    }
    pub(crate) fn match_shot(&mut self, ctx: &egui::Context) {
        if !self.apply_draft() {
            return;
        }
        let target = self.project.selected;
        let Some(shot) = self.shot_for(target) else {
            return self.fail("No shot found. Connect footage to the surface's Environment input or give the composition a backdrop.");
        };
        let plan =
            match veylvik_render::Plan::compile_output(self.project.document.snapshot(), shot) {
                Ok(plan) => plan,
                Err(e) => return self.fail(e.to_string()),
            };
        let time = match self.project.time() {
            Ok(time) => time,
            Err(e) => return self.fail(e.to_string()),
        };
        let (renderer, inputs, ctx) = (
            self.renderer.clone(),
            self.project.inputs.clone(),
            ctx.clone(),
        );
        let (width, height) = {
            let doc = self.project.document.snapshot();
            (doc.width, doc.height)
        };
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let result = renderer
                .render_frame(&plan, veylvik_render::RenderRequest::full(time), &inputs)
                .map_err(|e| e.to_string())
                .and_then(|frame| {
                    ShotLight::analyse(&frame.pixels, width, height)
                        .map(|shot| (shot, width, height))
                        .ok_or_else(|| {
                            "The shot is black at this frame; move the playhead and try again."
                                .to_owned()
                        })
                });
            let _ = send.send(result);
            ctx.request_repaint();
        });
        self.shot_light.job = Some((target, receive));
        self.status = "Matching the light to the shot…".into();
        self.error = false;
    }
    /// Apply a finished analysis as one undo step.
    pub(crate) fn shot_light_tick(&mut self) {
        let Some((target, receive)) = &self.shot_light.job else {
            return;
        };
        let target = *target;
        let Ok(result) = receive.try_recv() else {
            return;
        };
        self.shot_light.job = None;
        let (shot, width, height) = match result {
            Ok(found) => found,
            Err(e) => return self.fail(e),
        };
        if self.draft.id == target && !self.apply_draft() {
            return;
        }
        let Some(mut node) = self.project.document.snapshot().nodes.get(&target).cloned() else {
            return;
        };
        let Some(rig) = rig_mut(&mut node.operation) else {
            return;
        };
        rig.match_shot(&shot, width, height);
        match self.project.apply(&[Command::Put {
            node: Box::new(node),
        }]) {
            Ok(()) => {
                self.status = if shot.contrast >= 1.3 {
                    "Key light placed where the shot's light comes from".into()
                } else {
                    "The shot's light is soft and even: soft key above, environment matched".into()
                };
                self.refresh_draft();
                self.request_preview();
            }
            Err(e) => self.fail(e.to_string()),
        }
    }
}
