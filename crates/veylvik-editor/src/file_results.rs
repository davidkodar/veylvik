//! What the editor's own file jobs do when they finish, on the UI thread; each runs only for the project it was started for (`files::Completed` checks that once).
use crate::{Studio, eyedropper, media_import, pages, project::Saved};
use eframe::egui;
use std::path::PathBuf;
use veylvik_document::{Command, NodeId, NodePosition};
use veylvik_io::footage::Media;

impl Studio {
    pub(crate) fn saved(&mut self, saved: Saved, ctx: &egui::Context) {
        self.status = format!("Saved {}", saved.stamp.path().display());
        self.error = false;
        self.project.mark_saved(saved);
        if let Some(action) = self.after_save.take() {
            self.leave_now(action, ctx);
        }
    }
    pub(crate) fn queued(&mut self, job: crate::render_queue::Job) {
        match self.render_queue.add(job) {
            Ok(()) => {
                let asked = std::mem::take(&mut self.start_queued);
                if asked || self.pages.page == pages::Page::Export {
                    // From the Export page, or asked for over live control, a chosen
                    // destination renders at once, with its progress shown there.
                    self.render_queue.start();
                    self.render_queue.open = false;
                    self.status = "Exporting".into();
                } else {
                    self.status = "Added snapshot to render queue".into();
                }
                self.error = false;
            }
            Err(e) => self.fail(e),
        }
    }
    pub(crate) fn exported(&mut self, path: PathBuf) {
        self.status = format!("Exported {}", path.display());
        self.error = false;
    }
    pub(crate) fn live_done(&mut self, tool: &'static str, result: serde_json::Value) {
        self.finish_live_job(tool, Ok(result));
        self.status = "Done".into();
        self.error = false;
    }
    pub(crate) fn font_added(&mut self, name: String, ctx: &egui::Context) {
        self.fonts.added(ctx);
        self.status = format!("Added {name}. Choose it under Added fonts.");
        self.error = false;
    }
    pub(crate) fn text_font(&mut self, node: Box<veylvik_document::Node>) {
        match self.project.apply(&[Command::Put { node }]) {
            Ok(()) => {
                self.refresh_draft();
                self.request_preview();
            }
            Err(e) => self.fail(e),
        }
    }
    pub(crate) fn outlined(&mut self, node: Box<veylvik_document::Node>) {
        self.outline_controls.open = false;
        let id = node.id;
        match self.project.apply(&[Command::Put { node }]) {
            Ok(()) => {
                self.project.selected = id;
                self.project.preview = Some(id);
                self.refresh_draft();
                self.request_preview();
            }
            Err(error) => {
                self.outline_controls.open = true;
                self.fail(error);
            }
        }
    }
    pub(crate) fn preset_listing(&mut self, listing: veylvik_io::presets::Listing) {
        self.library.listing = listing;
        self.status = "Effect library updated".into();
        self.error = false;
    }
    pub(crate) fn picked(&mut self, sample: eyedropper::Sampled) {
        if sample.session == self.project.session
            && sample.node == self.draft.id
            && eyedropper::apply(&mut self.draft.operation, sample.field, sample.rgb)
        {
            self.draft_dirty = true;
            self.playing = false;
            let [r, g, b] = sample.rgb;
            self.status = format!(
                "Sampled R {r:.4} G {g:.4} B {b:.4} (scene-linear) from {} pixels · Apply to keep",
                sample.pixels
            );
            self.error = false;
        } else {
            self.fail("The sampled colour no longer matches the selected node; pick again.");
        }
    }
    pub(crate) fn sound_file(&mut self, position: NodePosition, path: PathBuf) {
        self.project.allow_added(&path);
        let mut node = veylvik_document::Node::new(
            path.file_name().and_then(|s| s.to_str()).unwrap_or("Sound"),
            veylvik_document::Operation::Sound(veylvik_document::nodes::Sound {
                file: path.to_string_lossy().into_owned(),
                timing: Default::default(),
                gain: 0.0.into(),
            }),
            &[],
        );
        node.position = Some(position);
        let id = node.id;
        match self.project.apply(&[Command::Put {
            node: Box::new(node),
        }]) {
            Ok(()) => {
                self.project.selected = id;
                self.status = "Sound added: it plays from the first frame; drag its bar on the timeline (View > Layer bars) to place it".into();
                self.error = false;
                self.refresh_draft();
            }
            Err(error) => self.fail(error),
        }
    }
    pub(crate) fn drawing(
        &mut self,
        name: String,
        shapes: Vec<veylvik_io::svg::Shape>,
        skipped: usize,
    ) {
        let (commands, report) =
            veylvik_io::svg::commands(self.project.document.snapshot(), &name, &shapes);
        match self.project.apply(&commands) {
            Ok(()) => {
                if let Some(id) = report["result"].as_str().and_then(|id| id.parse().ok()) {
                    self.project.selected = id;
                    self.project.preview = Some(id);
                }
                self.status = format!(
                    "Imported {name}: {} shapes{}. Each shape's outline is a Path node: connect it to 3D text to extrude it",
                    shapes.len(),
                    if skipped > 0 {
                        format!(" ({skipped} parts such as text or pictures left out)")
                    } else {
                        String::new()
                    }
                );
                self.error = false;
                self.refresh_draft();
                self.request_preview();
            }
            Err(error) => self.fail(error),
        }
    }
    /// Media read for import; `matching` says whether an empty composition adopts the
    /// media's settings, `None` asks the person.
    pub(crate) fn imported(
        &mut self,
        path: PathBuf,
        image: Media,
        position: NodePosition,
        matching: Option<bool>,
    ) {
        self.project.allow_added(&path);
        if let Some(matching) = matching {
            let settings = matching
                .then(|| image.info())
                .flatten()
                .map(|info| media_import::matched_current(self.project.document.snapshot(), &info));
            self.finish_import(path, image, position, settings);
        } else if self.project.is_empty()
            && image.info().is_some_and(|i| {
                let doc = self.project.document.snapshot();
                i.width != doc.width
                    || i.height != doc.height
                    || i.frame_rate
                        .is_some_and(|r| r.compare(doc.frame_rate).is_ok_and(|o| !o.is_eq()))
            })
        {
            self.status = "Choose whether to match the imported media".into();
            self.error = false;
            self.media_import = image.info().map(|info| {
                media_import::Pending::new(&self.project, info, Some((path, image, position)))
            });
        } else {
            self.finish_import(path, image, position, None);
        }
    }
    pub(crate) fn reloaded(&mut self, inputs: veylvik_render::ImageInputs) {
        self.project.inputs = inputs;
        self.request_preview();
    }
    pub(crate) fn replaced(&mut self, node: NodeId, path: PathBuf, image: Media) {
        match self.project.replace_image(node, &path, image) {
            Ok(()) => {
                self.refresh_draft();
                self.request_preview();
            }
            Err(error) => self.fail(error),
        }
    }
}
