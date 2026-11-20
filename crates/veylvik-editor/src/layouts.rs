//! Layouts: which panels are open and how large they are, four built in and up to sixteen saved by name in preferences. And the viewer in a window of its own: the same picture, fitted, for a second screen; it shows the result only and edits nothing.
use crate::{Studio, display::Paint, viewer::ViewTransform};
use eframe::{egui, egui_wgpu};
use serde::{Deserialize, Serialize};

/// Layouts a person may save, and characters in a layout's name.
pub const MAX_SAVED: usize = 16;
pub const MAX_NAME: usize = 40;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub name: String,
    pub library: bool,
    pub inspector: bool,
    /// The node and animation editor under the viewer.
    pub editor: bool,
    pub library_width: f32,
    pub inspector_width: f32,
    pub editor_height: f32,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            name: "Standard".into(),
            library: true,
            inspector: true,
            editor: true,
            library_width: 326.0,
            inspector_width: 414.0,
            editor_height: 420.0,
        }
    }
}
impl Layout {
    /// The layouts that come with Studio.
    pub fn built_in() -> [Self; 4] {
        let standard = Self::default();
        [
            standard.clone(),
            Self {
                name: "Wide viewer".into(),
                library: false,
                editor_height: 240.0,
                ..standard.clone()
            },
            Self {
                name: "Viewer only".into(),
                library: false,
                inspector: false,
                editor: false,
                ..standard.clone()
            },
            Self {
                name: "Graph".into(),
                library: false,
                editor_height: 760.0,
                ..standard
            },
        ]
    }
    /// Sizes within what the panels allow, and a usable name.
    pub fn tidy(mut self) -> Option<Self> {
        self.name = self
            .name
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_NAME)
            .collect();
        let sizes = [self.library_width, self.inspector_width, self.editor_height];
        if self.name.is_empty() || sizes.iter().any(|size| !size.is_finite()) {
            return None;
        }
        self.library_width = self.library_width.clamp(230.0, 420.0);
        self.inspector_width = self.inspector_width.clamp(340.0, 900.0);
        self.editor_height = self.editor_height.clamp(130.0, 2000.0);
        Some(self)
    }
}
/// `saved` with `layout` added, replacing one of the same name; the oldest goes when full.
pub fn with(saved: &[Layout], layout: Layout) -> Vec<Layout> {
    let mut next: Vec<Layout> = saved
        .iter()
        .filter(|l| l.name != layout.name)
        .cloned()
        .collect();
    next.push(layout);
    if next.len() > MAX_SAVED {
        next.remove(0);
    }
    next
}
/// The largest rectangle of `aspect` (width over height) centred in `room`.
pub(crate) fn fitted(room: egui::Rect, aspect: f32) -> egui::Rect {
    let size = if room.width() / room.height().max(1.0) > aspect {
        egui::vec2(room.height() * aspect, room.height())
    } else {
        egui::vec2(room.width(), room.width() / aspect.max(1e-6))
    };
    egui::Rect::from_center_size(room.center(), size)
}
#[derive(Default)]
pub struct Layouts {
    /// Panel sizes to impose, and for how many more frames.
    forced: Option<(Layout, u8)>,
    naming: Option<String>,
    /// The viewer is also shown in a window of its own.
    pub second_viewer: bool,
    second_fullscreen: bool,
    second: Option<std::sync::Arc<crate::display::DisplayFrame>>,
}
impl Layouts {
    /// The size a panel must take this frame, while a layout is being applied.
    pub fn library_width(&self) -> Option<f32> {
        self.forced.as_ref().map(|(l, _)| l.library_width)
    }
    pub fn inspector_width(&self) -> Option<f32> {
        self.forced.as_ref().map(|(l, _)| l.inspector_width)
    }
    pub fn editor_height(&self) -> Option<f32> {
        self.forced.as_ref().map(|(l, _)| l.editor_height)
    }
}
fn panel_size(ctx: &egui::Context, name: &str, fallback: f32, horizontal: bool) -> f32 {
    egui::containers::panel::PanelState::load(ctx, egui::Id::new(name)).map_or(fallback, |state| {
        if horizontal {
            state.size().x
        } else {
            state.size().y
        }
    })
}
impl Studio {
    pub(crate) fn apply_layout(&mut self, layout: &Layout) {
        self.browser.visible = layout.library;
        self.inspector_visible = layout.inspector;
        self.node_editor_hidden = !layout.editor;
        // Two frames: the panels take the size, then remember it as their own.
        self.layouts.forced = Some((layout.clone(), 2));
        self.status = format!("Layout: {}", layout.name);
        self.error = false;
    }
    /// Count down an applied layout. Called once a frame, after the panels were drawn.
    pub(crate) fn layout_tick(&mut self, ctx: &egui::Context) {
        if let Some((_, frames)) = &mut self.layouts.forced {
            *frames -= 1;
            if *frames == 0 {
                self.layouts.forced = None;
            }
            ctx.request_repaint();
        }
    }
    fn current_layout(&self, ctx: &egui::Context, name: &str) -> Layout {
        let standard = Layout::default();
        Layout {
            name: name.to_owned(),
            library: self.browser.visible,
            inspector: self.inspector_visible,
            editor: !self.node_editor_hidden,
            library_width: panel_size(ctx, "asset-library", standard.library_width, true),
            inspector_width: panel_size(ctx, "inspector", standard.inspector_width, true),
            editor_height: panel_size(ctx, "graph", standard.editor_height, false),
        }
    }
    /// The Layout submenu of the View menu.
    pub(crate) fn layout_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Layout", |ui| {
            let mut chosen = None;
            for layout in Layout::built_in() {
                if ui.button(&layout.name).clicked() {
                    chosen = Some(layout);
                }
            }
            let saved = self.preferences.values.layouts.clone();
            if !saved.is_empty() {
                ui.separator();
            }
            let mut removed = None;
            for layout in &saved {
                ui.horizontal(|ui| {
                    if ui.button(&layout.name).clicked() {
                        chosen = Some(layout.clone());
                    }
                    if ui
                        .small_button("Remove")
                        .on_hover_text("Forget this saved layout")
                        .clicked()
                    {
                        removed = Some(layout.name.clone());
                    }
                });
            }
            ui.separator();
            if ui.button("Save current layout as...").clicked() {
                self.layouts.naming = Some(String::new());
                ui.close();
            }
            if let Some(layout) = chosen {
                self.apply_layout(&layout);
                ui.close();
            }
            if let Some(name) = removed {
                self.preferences.values.layouts.retain(|l| l.name != name);
                if let Err(error) = self.preferences.persist() {
                    self.fail(format!("Preferences: {error}"));
                }
            }
        });
        let mut second = self.layouts.second_viewer;
        if self.capabilities.second_viewer
            && ui
                .checkbox(&mut second, "Viewer in its own window")
            .on_hover_text("A second window with the same picture, for another screen. Double-click it for full screen")
            .changed()
        {
            self.layouts.second_viewer = second;
        }
    }
    /// The small window that asks for a new layout's name.
    pub(crate) fn layout_windows(&mut self, ctx: &egui::Context) {
        let Some(mut name) = self.layouts.naming.take() else {
            return;
        };
        let (mut open, mut save) = (true, false);
        egui::Window::new("Save layout")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.label("Name this arrangement of panels");
                let field = ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .char_limit(MAX_NAME)
                        .desired_width(240.0),
                );
                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
                field.request_focus();
                ui.horizontal(|ui| {
                    save |= ui
                        .add_enabled(!name.trim().is_empty(), egui::Button::new("Save"))
                        .clicked();
                });
                if Layout::built_in().iter().any(|l| l.name == name.trim()) {
                    ui.weak("A built-in layout has this name; yours will be listed under it.");
                }
            });
        if save {
            if let Some(layout) = self.current_layout(ctx, &name).tidy() {
                self.preferences.values.layouts = with(&self.preferences.values.layouts, layout);
                match self.preferences.persist() {
                    Ok(()) => self.status = format!("Saved the layout {}", name.trim()),
                    Err(error) => self.fail(format!("Preferences: {error}")),
                }
            }
        } else if open {
            self.layouts.naming = Some(name);
        }
    }
    /// Make the whole interface larger or smaller, within what preferences allow.
    pub(crate) fn scale_interface(&mut self, ctx: &egui::Context, larger: bool) {
        let scale = &mut self.preferences.values.scale;
        *scale = ((*scale + if larger { 0.1 } else { -0.1 }) * 20.0).round() / 20.0;
        *scale = scale.clamp(0.75, 2.0);
        // The whole window's scale is its owner's.
        if !crate::design::guest(ctx) {
            ctx.set_zoom_factor(*scale);
        }
        self.status = format!("Interface scale {:.0}%", *scale * 100.0);
        self.error = false;
        if let Err(error) = self.preferences.persist() {
            self.fail(format!("Preferences: {error}"));
        }
    }
    /// The viewer's picture in a window of its own, while that is switched on.
    pub(crate) fn second_viewer(&mut self, ctx: &egui::Context) {
        if !self.layouts.second_viewer {
            self.layouts.second = None;
            return;
        }
        let Some(shown) = self.displayed.clone() else {
            return;
        };
        let settings = self.effective_view();
        // One display frame for this window, remade only when the picture or how it is
        // shown changes.
        let current = self.layouts.second.as_ref().is_some_and(|frame| {
            std::sync::Arc::ptr_eq(&frame.frame, &shown.frame) && frame.settings == settings
        });
        if !current {
            self.layouts.second = Some(self.display.frame_as(
                &self.renderer,
                shown.frame.clone(),
                settings,
                ViewTransform::default(),
                shown.baked,
            ));
        }
        let frame = self.layouts.second.clone().unwrap();
        let doc = self.project.document.snapshot();
        let aspect = doc.width as f32 / doc.height.max(1) as f32;
        let (mut close, mut toggle) = (false, false);
        let fullscreen = self.layouts.second_fullscreen;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("veylvik-second-viewer"),
            egui::ViewportBuilder::default()
                .with_title("Veylvik viewer")
                .with_inner_size([960.0, 540.0])
                .with_fullscreen(fullscreen),
            |ui, class| {
                let mut body =
                    |ui: &mut egui::Ui| {
                        let (room, response) =
                            ui.allocate_exact_size(ui.available_size(), egui::Sense::click());
                        ui.painter().rect_filled(room, 0.0, egui::Color32::BLACK);
                        let picture = fitted(room, aspect);
                        ui.painter().with_clip_rect(room).add(
                            egui_wgpu::Callback::new_paint_callback(picture, Paint(frame.clone())),
                        );
                        toggle |= response.double_clicked();
                        response
                            .on_hover_text("Double-click or F11 for full screen; Escape leaves it");
                    };
                if class == egui::ViewportClass::EmbeddedWindow {
                    // No separate windows here: the toolkit shows a floating panel inside
                    // the main window, and this fills it.
                    ui.set_min_size(egui::vec2(480.0, 270.0));
                    body(ui);
                } else {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show_inside(ui, |ui| body(ui));
                    let ctx = ui.ctx();
                    close |= ctx.input(|i| i.viewport().close_requested());
                    toggle |= ctx.input(|i| i.key_pressed(egui::Key::F11));
                    if fullscreen && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        toggle = true;
                    }
                }
            },
        );
        if toggle {
            self.layouts.second_fullscreen = !fullscreen;
        }
        if close {
            self.layouts.second_viewer = false;
            self.layouts.second_fullscreen = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layouts_are_tidied_replaced_by_name_and_bounded() {
        let names: Vec<String> = Layout::built_in().into_iter().map(|l| l.name).collect();
        assert_eq!(names, ["Standard", "Wide viewer", "Viewer only", "Graph"]);
        for layout in Layout::built_in() {
            assert_eq!(
                layout.clone().tidy(),
                Some(layout),
                "built-in layouts are within the limits"
            );
        }
        // Names are trimmed and cut, sizes pulled within what the panels allow.
        let wild = Layout {
            name: format!("  {}\n", "n".repeat(60)),
            library_width: 5.0,
            inspector_width: 5000.0,
            editor_height: -3.0,
            ..Layout::default()
        }
        .tidy()
        .unwrap();
        assert_eq!(wild.name.len(), MAX_NAME);
        assert_eq!(
            (wild.library_width, wild.inspector_width, wild.editor_height),
            (230.0, 900.0, 130.0)
        );
        assert!(
            Layout {
                name: "   ".into(),
                ..Layout::default()
            }
            .tidy()
            .is_none()
        );
        assert!(
            Layout {
                editor_height: f32::NAN,
                ..Layout::default()
            }
            .tidy()
            .is_none()
        );
        // Saving under an existing name replaces it; the seventeenth pushes the oldest out.
        let named = |name: &str, width| Layout {
            name: name.into(),
            inspector_width: width,
            ..Layout::default()
        };
        let mut saved = with(&[], named("Grade", 400.0));
        saved = with(&saved, named("Titles", 500.0));
        saved = with(&saved, named("Grade", 600.0));
        assert_eq!(
            saved
                .iter()
                .map(|l| (l.name.as_str(), l.inspector_width))
                .collect::<Vec<_>>(),
            [("Titles", 500.0), ("Grade", 600.0)]
        );
        for i in 0..20 {
            saved = with(&saved, named(&format!("L{i}"), 400.0));
        }
        assert_eq!(saved.len(), MAX_SAVED);
        assert_eq!(saved.last().unwrap().name, "L19");
        assert!(!saved.iter().any(|l| l.name == "Titles"));
        // Old preferences without the newer fields still load.
        let partial: Layout = serde_json::from_str(r#"{"name":"Mine","library":false}"#).unwrap();
        assert_eq!(
            (
                partial.name.as_str(),
                partial.library,
                partial.inspector_width
            ),
            ("Mine", false, 414.0)
        );
    }
    #[test]
    fn the_picture_is_fitted_and_centred_in_the_second_window() {
        let room = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1000.0, 1000.0));
        let wide = fitted(room, 2.0);
        assert_eq!(
            (wide.width(), wide.height(), wide.center()),
            (1000.0, 500.0, room.center())
        );
        let tall = fitted(
            egui::Rect::from_min_size(egui::pos2(10.0, 0.0), egui::vec2(400.0, 100.0)),
            1.0,
        );
        assert_eq!(
            (tall.width(), tall.height(), tall.center().x),
            (100.0, 100.0, 210.0)
        );
    }
}
