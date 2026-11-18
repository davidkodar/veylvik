//! Pages: the one window arranged for one job at a time. Compose is the full editor; Animate gives the timeline and curves the room; 3D gives it to the viewer with the scene's contents listed; Export puts the delivery settings beside the picture. Switching away from Compose remembers its editor state and returning restores it.
use crate::Studio;
use eframe::egui;
use veylvik_document::{NodeId, Operation};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Page {
    #[default]
    Compose,
    Animate,
    Scene,
    Export,
}
impl Page {
    pub const ALL: [Self; 4] = [Self::Compose, Self::Animate, Self::Scene, Self::Export];
    pub fn label(self) -> &'static str {
        match self {
            Self::Compose => "Compose",
            Self::Animate => "Animate",
            Self::Scene => "3D",
            Self::Export => "Export",
        }
    }
    fn hint(self) -> &'static str {
        match self {
            Self::Compose => "Build the picture: nodes, library and inspector (Ctrl+1)",
            Self::Animate => "Keys and curves with room to work (Ctrl+2)",
            Self::Scene => {
                "The viewer large, with the scene's objects, cameras and lights listed (Ctrl+3)"
            }
            Self::Export => "Delivery settings beside the picture, and the render queue (Ctrl+4)",
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ExportKind {
    #[default]
    Video,
    Sequence,
}
#[derive(Default)]
pub struct Pages {
    pub page: Page,
    pub export: ExportKind,
    /// Compose's editor state while another page is shown: (curves shown, editor hidden).
    compose: Option<(bool, bool)>,
    /// The quick export dialog, shown on entering the Export page.
    pub quick: bool,
}
impl Studio {
    /// Show `page`, arranging the editor for it.
    pub(crate) fn go_to(&mut self, page: Page) {
        let from = self.pages.page;
        if from == page {
            return;
        }
        if from == Page::Compose {
            self.pages.compose = Some((self.animation_editor.visible, self.node_editor_hidden));
        }
        match page {
            Page::Compose => {
                if let Some((curves, hidden)) = self.pages.compose.take() {
                    self.animation_editor.visible = curves;
                    self.node_editor_hidden = hidden;
                }
            }
            Page::Animate => {
                self.animation_editor.visible = true;
                self.node_editor_hidden = false;
            }
            Page::Scene => self.node_editor_hidden = true,
            Page::Export => {
                self.node_editor_hidden = true;
                self.pages.quick = true;
            }
        }
        self.pages.page = page;
    }
    /// Ctrl+1 to Ctrl+4 switch pages.
    pub(crate) fn page_shortcuts(&mut self, ctx: &egui::Context) {
        for (key, page) in [
            (egui::Key::Num1, Page::Compose),
            (egui::Key::Num2, Page::Animate),
            (egui::Key::Num3, Page::Scene),
            (egui::Key::Num4, Page::Export),
        ] {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, key)) {
                self.go_to(page);
            }
        }
    }
    /// The page switch in the top bar.
    pub(crate) fn page_bar(&mut self, ui: &mut egui::Ui) {
        let accent = crate::design::accent(ui.ctx());
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_space(8.0);
        for page in Page::ALL {
            let on = self.pages.page == page;
            let text = egui::RichText::new(page.label()).size(15.5);
            let button = if on {
                egui::Button::new(text.strong().color(accent))
                    .fill(accent.gamma_multiply(0.2))
                    .stroke(egui::Stroke::new(1.5_f32, accent))
            } else {
                egui::Button::new(text.color(ui.visuals().strong_text_color()))
                    .fill(egui::Color32::TRANSPARENT)
            };
            if ui
                .add(button.corner_radius(9.0).min_size(egui::vec2(112.0, 34.0)))
                .on_hover_text(page.hint())
                .clicked()
            {
                self.go_to(page);
            }
        }
        ui.add_space(8.0);
    }
    /// The 3D page's left panel: what the scenes hold, to select by clicking.
    pub(crate) fn scene_outline(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("scene-outline")
            .frame(crate::design::panel(ui.ctx()))
            .default_size(250.0)
            .min_size(190.0)
            .resizable(true)
            .show_inside(ui, |ui| {
                ui.heading("Scene");
                let doc = self.project.document.snapshot();
                let group = |operation: &Operation| match operation {
                    Operation::Shape3d(_)
                    | Operation::Text3d(_)
                    | Operation::Model3d(_)
                    | Operation::ImageCard(_)
                    | Operation::PointCloud(_) => Some(0),
                    Operation::Camera3d(_) => Some(1),
                    Operation::Light3d(_) | Operation::Environment3d(_) => Some(2),
                    Operation::Transform3d(_) | Operation::MergeScenes(_) | Operation::RenderScene(_) => Some(3),
                    _ => None,
                };
                let mut rows: [Vec<(NodeId, String)>; 4] = Default::default();
                for node in doc.nodes.values() {
                    if let Some(g) = group(&node.operation) {
                        rows[g].push((node.id, node.label.clone()));
                    }
                }
                let mut select = None;
                if rows.iter().all(Vec::is_empty) {
                    ui.add_space(8.0);
                    ui.weak("Nothing in 3D yet. Add shapes, 3D text, a camera and lights from Add, 3D, or start from the Gold title in the library's Templates.");
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (title, rows) in ["OBJECTS", "CAMERAS", "LIGHTS", "STRUCTURE"].into_iter().zip(&mut rows) {
                        if rows.is_empty() {
                            continue;
                        }
                        rows.sort_by_key(|a| a.1.to_lowercase());
                        crate::design::heading(ui, title);
                        for (id, label) in rows.iter() {
                            if ui
                                .selectable_label(self.project.selected == *id, label)
                                .clicked()
                            {
                                select = Some(*id);
                            }
                        }
                    }
                });
                if let Some(id) = select
                    && self.can_change_selection()
                {
                    self.project.selected = id;
                    self.refresh_draft();
                }
            });
    }
    /// The dialog that greets the Export page: pick a delivery or keep the default,
    /// and export; More settings leaves the full page.
    pub(crate) fn quick_export(&mut self, ctx: &egui::Context) {
        if self.pages.page != Page::Export || !self.pages.quick {
            return;
        }
        let enabled = !self.files.busy() && !self.authoring_dialog_open() && self.confirm.is_none();
        let mut open = true;
        let mut close = false;
        let mut video = None;
        let mut sequence = None;
        egui::Window::new("Export")
            .id(egui::Id::new("quick-export"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.set_width(622.0_f32.min(ctx.content_rect().width() - 40.0));
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                let doc = self.project.document.snapshot();
                crate::design::heading(ui, "WHAT TO MAKE");
                ui.horizontal(|ui| {
                    for (kind, label, hint) in [
                        (ExportKind::Video, "Video", "One MP4 file, with sound"),
                        (
                            ExportKind::Sequence,
                            "Image sequence",
                            "A folder of numbered images, with transparency or full HDR",
                        ),
                    ] {
                        if crate::design::chip(ui, label, self.pages.export == kind)
                            .on_hover_text(hint)
                            .clicked()
                        {
                            self.pages.export = kind;
                        }
                    }
                });
                let ready = match self.pages.export {
                    ExportKind::Video => {
                        self.video_export.quick(ui, doc);
                        self.video_export.request(doc).is_some()
                    }
                    ExportKind::Sequence => {
                        self.export_controls.quick(ui);
                        self.export_controls
                            .request(doc.frame_rate, doc.frame_count)
                            .is_some()
                    }
                };
                ui.add_space(6.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("The whole composition, at its own frame rate")
                            .color(ui.visuals().weak_text_color()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled_ui(enabled && ready, |ui| {
                                crate::design::primary(ui, "Export")
                            })
                            .inner
                            .on_hover_text("Choose where to save, then it renders")
                            .clicked()
                        {
                            match self.pages.export {
                                ExportKind::Video => video = self.video_export.request(doc),
                                ExportKind::Sequence => {
                                    sequence = self
                                        .export_controls
                                        .request(doc.frame_rate, doc.frame_count)
                                }
                            }
                            close = true;
                        }
                        if ui
                            .add(
                                egui::Button::new("More settings")
                                    .min_size(egui::vec2(120.0, 32.0)),
                            )
                            .on_hover_text(
                                "Size, frame rate, range, sound and more, on the page behind",
                            )
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        if !open || close {
            self.pages.quick = false;
        }
        if let Some(settings) = video {
            self.export_video(settings, ctx);
        }
        if let Some(settings) = sequence {
            self.export_sequence(settings, ctx);
        }
    }
    /// The Export page's right panel: delivery settings and the queue.
    pub(crate) fn export_page(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let enabled = !self.files.busy() && !self.authoring_dialog_open() && self.confirm.is_none();
        let mut video = None;
        let mut sequence = None;
        egui::Panel::right("export-page")
            .frame(crate::design::panel(ui.ctx()))
            .default_size(660.0)
            .min_size(520.0)
            .resizable(true)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Export");
                    ui.add_space(12.0);
                    for (kind, label) in [
                        (ExportKind::Video, "Video"),
                        (ExportKind::Sequence, "Image sequence"),
                    ] {
                        if crate::design::chip(ui, label, self.pages.export == kind).clicked() {
                            self.pages.export = kind;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Exporting always goes through the dialog first: keep the
                        // default or choose how, then export.
                        if ui
                            .add_enabled_ui(enabled, |ui| crate::design::primary(ui, "Export"))
                            .inner
                            .on_hover_text("Choose what to make, then where to save it")
                            .clicked()
                        {
                            self.pages.quick = true;
                        }
                        if ui
                            .button("Render queue")
                            .on_hover_text("What is waiting, rendering and done")
                            .clicked()
                        {
                            self.render_queue.open = true;
                        }
                        if ui
                            .button("This frame")
                            .on_hover_text("Save the frame in the viewer as an image (Ctrl+E)")
                            .clicked()
                        {
                            self.export_frame();
                        }
                    });
                });
                ui.separator();
                // What is rendering, or how the last export went.
                if let Some((file, share, phase)) = self.render_queue.rendering() {
                    ui.add(
                        egui::ProgressBar::new(share)
                            .text(format!(
                                "Rendering {file}  {}  {:.0}%",
                                phase,
                                share * 100.0
                            ))
                            .desired_height(22.0),
                    );
                    ctx.request_repaint_after_secs(0.2);
                } else if let Some((path, ok, detail)) = self.render_queue.finished() {
                    if ok {
                        ui.colored_label(
                            egui::Color32::from_rgb(140, 210, 150),
                            format!("Saved {}", path.display()),
                        );
                    } else {
                        ui.colored_label(
                            egui::Color32::LIGHT_RED,
                            format!("{} did not finish: {detail}", path.display()),
                        );
                    }
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let doc = self.project.document.snapshot();
                    match self.pages.export {
                        ExportKind::Video => video = self.video_export.panel(ui, doc, enabled),
                        ExportKind::Sequence => {
                            sequence = self.export_controls.panel(
                                ui,
                                doc.frame_rate,
                                doc.frame_count,
                                enabled,
                            )
                        }
                    }
                });
            });
        if let Some(settings) = video {
            self.export_video(settings, &ctx);
        }
        if let Some(settings) = sequence {
            self.export_sequence(settings, &ctx);
        }
    }
}
