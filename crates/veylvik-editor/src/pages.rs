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
            Self::Export => "Export the composition, and see what is rendering and done (Ctrl+4)",
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
    /// The export dialog, shown on entering the Export page and by its Export button.
    pub quick: bool,
    /// The dialog is showing every setting.
    pub more: bool,
    /// The 3D page's left panel shows the library and not the scene's contents.
    pub scene_library: bool,
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
    /// The page switch in the top bar.
    pub(crate) fn page_bar(&mut self, ui: &mut egui::Ui) {
        let accent = crate::design::accent(ui.ctx());
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_space(8.0);
        for page in Page::ALL {
            if page == Page::Export && !self.capabilities.export {
                continue;
            }
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
                // A page is a place in the editor: choosing one leaves the start screen.
                self.start.open = false;
                self.go_to(page);
            }
        }
        ui.add_space(8.0);
    }
    /// The 3D page's left panel is the scene's contents or the library: the switch
    /// between them, shown at the top of whichever is open.
    pub(crate) fn left_switch(&mut self, ui: &mut egui::Ui) {
        for (library, label, hint) in [
            (false, "Scene", "The scene's objects, cameras and lights"),
            (true, "Library", "Effects, materials, templates and media"),
        ] {
            if crate::design::chip(ui, label, self.pages.scene_library == library)
                .on_hover_text(hint)
                .clicked()
            {
                self.pages.scene_library = library;
            }
        }
    }
    /// The 3D page's left panel: what the scenes hold, to select by clicking.
    pub(crate) fn scene_outline(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("scene-outline")
            .frame(crate::design::panel(ui.ctx()))
            .default_size(250.0)
            .min_size(190.0)
            .resizable(true)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| self.left_switch(ui));
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
    /// Open the export dialog for `kind`, on the Export page where its progress shows.
    pub(crate) fn ask_export(&mut self, kind: ExportKind) {
        self.pages.export = kind;
        self.go_to(Page::Export);
        self.pages.quick = true;
    }
    /// The export dialog: what to make and a delivery to pick or keep, with every
    /// other setting behind More settings in the same dialog. Nothing is exported
    /// until Export is pressed and a destination chosen.
    pub(crate) fn quick_export(&mut self, ctx: &egui::Context) {
        if !self.pages.quick {
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
                match self.pages.export {
                    ExportKind::Video => self.video_export.quick(ui, doc),
                    ExportKind::Sequence => self.export_controls.quick(ui),
                }
                // Every other setting, here in the dialog when asked for.
                if ui
                    .add(
                        egui::Button::new(if self.pages.more {
                            "Fewer settings"
                        } else {
                            "More settings"
                        })
                        .min_size(egui::vec2(130.0, 28.0)),
                    )
                    .on_hover_text("Size, frame rate, range, sound and more")
                    .clicked()
                {
                    self.pages.more = !self.pages.more;
                }
                if self.pages.more {
                    egui::ScrollArea::vertical()
                        .max_height((ctx.content_rect().height() - 520.0).max(160.0))
                        .show(ui, |ui| match self.pages.export {
                            ExportKind::Video => self.video_export.details(ui, doc),
                            ExportKind::Sequence => {
                                self.export_controls.details(ui, doc.frame_count)
                            }
                        });
                }
                let summary = match self.pages.export {
                    ExportKind::Video => self.video_export.summary(doc),
                    ExportKind::Sequence => self
                        .export_controls
                        .summary(doc.frame_rate, doc.frame_count),
                };
                ui.add_space(6.0);
                ui.separator();
                ui.horizontal(|ui| {
                    match &summary {
                        Ok(text) => {
                            ui.label(
                                egui::RichText::new(text).color(ui.visuals().weak_text_color()),
                            );
                        }
                        Err(error) => {
                            ui.colored_label(egui::Color32::LIGHT_RED, error);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled_ui(enabled && summary.is_ok(), |ui| {
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
                            .add(egui::Button::new("Cancel").min_size(egui::vec2(90.0, 32.0)))
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
    /// The Export page's right panel: the Export button and what has been exported,
    /// is rendering and is waiting.
    pub(crate) fn export_page(&mut self, ui: &mut egui::Ui) {
        let enabled = !self.files.busy() && !self.authoring_dialog_open() && self.confirm.is_none();
        let mut action = None;
        egui::Panel::right("export-page")
            .frame(crate::design::panel(ui.ctx()))
            .default_size(560.0)
            .min_size(420.0)
            .resizable(true)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Exports");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled_ui(enabled, |ui| crate::design::primary(ui, "Export"))
                            .inner
                            .on_hover_text("Choose what to make, then where to save it")
                            .clicked()
                        {
                            self.pages.quick = true;
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
                action = self.render_queue.panel(ui, true);
            });
        match action {
            Some(crate::render_queue::Action::Frame) => self.export_frame(),
            Some(crate::render_queue::Action::Video) => self.ask_export(ExportKind::Video),
            Some(crate::render_queue::Action::Sequence) => self.ask_export(ExportKind::Sequence),
            Some(crate::render_queue::Action::Retry(job)) => self.retry_render(*job),
            None => {}
        }
    }
}
