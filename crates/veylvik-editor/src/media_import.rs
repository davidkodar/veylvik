//! Media matching is explicit, cancellable, and committed with import as one undo step.
use crate::{
    Studio,
    composition::Settings,
    files::{self, Choice, Completed},
    project::Project,
};
use eframe::egui;
use std::path::PathBuf;
use veylvik_document::{NodeId, NodePosition, Time};
use veylvik_io::footage::Media;
use veylvik_render::SourceInfo;
type Import = (PathBuf, Media, NodePosition);
pub struct Pending {
    session: NodeId,
    revision: u64,
    info: SourceInfo,
    import: Option<Import>,
    match_rate: bool,
    match_duration: bool,
    scale_scene: bool,
}
impl Pending {
    pub fn new(project: &Project, info: SourceInfo, import: Option<Import>) -> Self {
        Self {
            session: project.session,
            revision: project.document.revision(),
            info,
            import,
            match_rate: false,
            match_duration: false,
            scale_scene: false,
        }
    }
}
fn duration_frames(duration: Time, rate: Time) -> u32 {
    let numerator = i128::from(duration.ticks) * i128::from(rate.ticks);
    let denominator = i128::from(duration.rate) * i128::from(rate.rate);
    ((numerator + denominator - 1) / denominator).clamp(1, u32::MAX as i128) as u32
}
/// The current composition's settings matched to `info` (size, rate and duration), in place.
pub(crate) fn matched_current(doc: &veylvik_document::Composition, info: &SourceInfo) -> Settings {
    matched(Settings::current(doc), info, true, true)
}
fn matched(mut settings: Settings, info: &SourceInfo, rate: bool, duration: bool) -> Settings {
    settings.width = info.width;
    settings.height = info.height;
    if rate && let Some(value) = info.frame_rate {
        settings.frame_rate = value;
    }
    if duration && let Some(value) = info.duration {
        settings.frame_count = duration_frames(value, settings.frame_rate);
    }
    settings
}
impl Studio {
    pub(crate) fn finish_import(
        &mut self,
        path: PathBuf,
        image: Media,
        position: NodePosition,
        settings: Option<Settings>,
    ) {
        match self.project.import(&path, image, position, settings) {
            Ok(()) => {
                self.refresh_draft();
                self.request_preview();
            }
            Err(error) => self.fail(error),
        }
    }
    pub(crate) fn new_from_media(&mut self) {
        self.new_from_media_path(None);
    }
    /// Creates a composition matched to `path`, or asks with the file picker when `None`.
    pub(crate) fn new_from_media_path(&mut self, path: Option<PathBuf>) {
        let folder = self.project.base.clone();
        let session = self.project.session;
        let revision = self.project.document.revision();
        let result = self.files.start(move || {
            let path = match path {
                Some(path) => path,
                None => {
                    let Some(path) = files::select(Choice::Import, &folder, "")? else {
                        return Ok(Completed::cancelled());
                    };
                    path
                }
            };
            let path = path.canonicalize().map_err(|e| e.to_string())?;
            let image =
                veylvik_io::footage::load(&path, 128 * 1024 * 1024).map_err(|e| e.to_string())?;
            let info = image.info().ok_or("Media has no size metadata")?;
            let settings = matched(
                Settings {
                    width: 1920,
                    height: 1080,
                    frame_rate: Time { ticks: 24, rate: 1 },
                    frame_count: veylvik_document::DEFAULT_FRAME_COUNT,
                    new: true,
                    display: Default::default(),
                    scale_scene: false,
                },
                &info,
                true,
                true,
            );
            let mut project = Project::blank(
                settings.width,
                settings.height,
                settings.frame_rate,
                settings.frame_count,
            )
            .map_err(|e| e.to_string())?;
            project
                .import(&path, image, NodePosition { x: 0.0, y: 0.0 }, None)
                .map_err(|e| e.to_string())?;
            Ok(
                Completed::then(move |s, ctx| s.open_project(project, ctx)).unless_changed(
                    session,
                    revision,
                    "Composition changed while opening media. Please try again.",
                ),
            )
        });
        self.file_started(result);
    }
    pub(crate) fn match_media(&mut self, id: NodeId) {
        if self.authoring_dialog_open() || !self.apply_draft() {
            return;
        }
        let info = self
            .project
            .document
            .snapshot()
            .nodes
            .get(&id)
            .and_then(|n| n.operation.asset())
            .and_then(|asset| self.project.inputs.info(asset));
        if let Some(info) = info {
            self.media_import = Some(Pending::new(&self.project, info, None));
        } else {
            self.fail("Load the source before matching composition settings.");
        }
    }
    pub(crate) fn media_import_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut pending) = self.media_import.take() else {
            return;
        };
        if pending.session != self.project.session
            || pending.revision != self.project.document.revision()
        {
            self.fail("Composition changed while matching media. Please try again.");
            return;
        }
        let doc = self.project.document.snapshot();
        let mut open = true;
        let mut action = None;
        let mut cancel = false;
        egui::Window::new(if pending.import.is_some() {
            "Match imported media?"
        } else {
            "Match composition to media"
        })
        .open(&mut open)
        .collapsible(false)
        .resizable(false)

            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
            ui.set_width(520.0_f32.min(ctx.content_rect().width() - 40.0));
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            crate::design::heading(ui, "SIZES");
            ui.horizontal(|ui| {
                crate::design::format_card(
                    ui,
                    "Composition",
                    &format!("{} × {}", doc.width, doc.height),
                    Some([doc.width as f32, doc.height as f32]),
                    false,
                );
                crate::design::format_card(
                    ui,
                    "Media",
                    &format!("{} × {}", pending.info.width, pending.info.height),
                    Some([pending.info.width as f32, pending.info.height as f32]),
                    true,
                );
            });
            crate::design::heading(ui, "ALSO MATCH");
            if let Some(rate) = pending.info.frame_rate {
                ui.checkbox(
                    &mut pending.match_rate,
                    format!(
                        "Also match frame rate ({:.3} fps)",
                        rate.seconds().unwrap_or_default()
                    ),
                );
            }
            if let Some(duration) = pending.info.duration {
                ui.checkbox(
                    &mut pending.match_duration,
                    format!(
                        "Also match duration ({:.3} s)",
                        duration.seconds().unwrap_or_default()
                    ),
                );
            }
            if pending.import.is_none() {
                ui.checkbox(
                    &mut pending.scale_scene,
                    "Scale scene controls proportionally to fit new canvas",
                );
                ui.weak("Unchecked changes canvas bounds only. Media keeps its placement mode; animation times stay in seconds.");
            } else {
                ui.weak("Keep settings fits and centres the media in the current canvas.");
            }
            ui.add_space(6.0);
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::design::primary(ui, "Match the media").clicked() {
                    action = Some(true);
                }
                if pending.import.is_some()
                    && ui
                        .add(egui::Button::new("Keep my settings").min_size(egui::vec2(120.0, 32.0)))
                        .clicked()
                {
                    action = Some(false);
                }
                if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(80.0, 32.0))).clicked() {
                    cancel = true;
                }
            });
        });
        if !open || cancel {
            self.status = "Media matching cancelled".into();
            self.error = false;
            return;
        }
        if let Some(match_size) = action {
            let settings = match_size.then(|| {
                let mut settings = matched(
                    Settings::current(doc),
                    &pending.info,
                    pending.match_rate,
                    pending.match_duration,
                );
                settings.scale_scene = pending.scale_scene;
                settings
            });
            if let Some((path, image, position)) = pending.import.take() {
                self.finish_import(path, image, position, settings);
            } else if let Some(settings) = settings {
                self.apply_composition_settings(settings, ctx);
            }
        } else {
            self.media_import = Some(pending);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matching_rate_and_duration_are_independent_and_round_up() {
        let doc = veylvik_document::blank(1920, 1080, Time::new(24, 1).unwrap()).unwrap();
        let info = SourceInfo {
            width: 3840,
            height: 2160,
            frame_rate: Some(Time::new(30000, 1001).unwrap()),
            duration: Some(Time::new(1001, 1000).unwrap()),
            frames: Some(30),
            description: String::new(),
        };
        let size = matched(Settings::current(&doc), &info, false, false);
        assert_eq!([size.width, size.height], [3840, 2160]);
        assert_eq!(size.frame_count, doc.frame_count);
        assert_eq!(size.frame_rate.ticks, 24);
        assert_eq!(matched(size, &info, false, true).frame_count, 25);
        assert_eq!(matched(size, &info, true, true).frame_count, 30);
    }
}
