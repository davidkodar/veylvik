//! The start screen: new compositions in one click, the projects opened lately with a picture of each, and templates. The recent list is a small file in the state directory and the pictures are PNGs in the cache directory, written and read off the UI thread.
use crate::{Leave, Studio, project::Project};
use eframe::egui;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::mpsc,
};
use veylvik_document::Time;

/// Projects remembered.
const KEPT: usize = 12;
/// Longest edge of a project's picture.
const PICTURE: u32 = 480;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Entry {
    path: PathBuf,
    /// When it was last opened or saved here (seconds since 1970).
    opened: u64,
}
/// What a click on the start screen does once unsaved work has been dealt with.
pub enum Chosen {
    Recent(PathBuf),
    /// A new composition matched to this video or image.
    FromMedia(PathBuf),
    Blank(u32, u32),
    GoldTitle,
}
#[derive(Default)]
pub struct Start {
    pub open: bool,
    pub chosen: Option<Chosen>,
    entries: Vec<Entry>,
    /// The project path last added to the list, and last photographed.
    recorded: Option<PathBuf>,
    pictured: Option<PathBuf>,
    was_dirty: bool,
    /// How many recent projects are listed; 0 until preferences say.
    pub kept: usize,
    pictures: Option<mpsc::Receiver<(PathBuf, egui::ColorImage)>>,
    textures: BTreeMap<PathBuf, egui::TextureHandle>,
    gold: Option<egui::TextureHandle>,
}
fn home(variable: &str, fallback: &str) -> Option<PathBuf> {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(fallback))
                .filter(|p| p.is_absolute())
        })
        .map(|p| p.join("veylvik"))
}
fn list_path() -> Option<PathBuf> {
    home("XDG_STATE_HOME", ".local/state").map(|p| p.join("recent.json"))
}
/// Where a project's picture is kept: named by a hash of its path.
fn picture_path(project: &Path) -> Option<PathBuf> {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in project.as_os_str().as_encoded_bytes() {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    home("XDG_CACHE_HOME", ".cache").map(|p| p.join("thumbnails").join(format!("{hash:016x}.png")))
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
/// How long ago, in words.
fn ago(then: u64) -> String {
    let seconds = now().saturating_sub(then);
    let plural = |n: u64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match seconds {
        0..=89 => "Just now".into(),
        90..=5399 => plural(seconds / 60, "minute"),
        5400..=129_599 => plural(seconds / 3600, "hour"),
        _ => plural(seconds / 86_400, "day"),
    }
}
impl Start {
    /// The project opened most recently that still exists.
    pub fn latest(&self) -> Option<PathBuf> {
        self.entries.first().map(|entry| entry.path.clone())
    }
    /// Read the recent list (at startup, before the window is shown). Projects whose
    /// files are gone are dropped.
    pub fn load(open: bool) -> Self {
        let entries = list_path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|bytes| serde_json::from_slice::<Vec<Entry>>(&bytes).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.path.is_file())
            .take(KEPT)
            .collect();
        Self {
            open,
            entries,
            ..Default::default()
        }
    }
    fn store(&self) {
        let (Some(path), Ok(bytes)) = (list_path(), serde_json::to_vec_pretty(&self.entries))
        else {
            return;
        };
        // A few hundred bytes, written whole and renamed into place off the UI thread.
        std::thread::spawn(move || {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let temporary = path.with_extension("tmp");
            if std::fs::write(&temporary, bytes).is_ok() {
                let _ = std::fs::rename(temporary, path);
            }
        });
    }
    fn remember(&mut self, path: &Path) {
        self.entries.retain(|e| e.path != path);
        self.entries.insert(
            0,
            Entry {
                path: path.to_path_buf(),
                opened: now(),
            },
        );
        self.entries
            .truncate(if self.kept > 0 { self.kept } else { KEPT });
        self.store();
    }
    /// Forget the whole list; the projects are untouched.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.textures.clear();
        self.store();
    }
    fn forget(&mut self, path: &Path) {
        self.entries.retain(|e| e.path != path);
        self.textures.remove(path);
        self.store();
    }
    /// Start reading the pictures of the listed projects on a worker.
    fn read_pictures(&mut self) {
        let paths: Vec<PathBuf> = self.entries.iter().map(|e| e.path.clone()).collect();
        let (sender, receiver) = mpsc::channel();
        self.pictures = Some(receiver);
        std::thread::spawn(move || {
            for path in paths {
                let Some(image) = picture_path(&path)
                    .and_then(|p| std::fs::read(p).ok())
                    .and_then(|bytes| image::load_from_memory(&bytes).ok())
                else {
                    continue;
                };
                let image = image.to_rgba8();
                let picture = egui::ColorImage::from_rgba_unmultiplied(
                    [image.width() as usize, image.height() as usize],
                    image.as_raw(),
                );
                if sender.send((path, picture)).is_err() {
                    return;
                }
            }
        });
    }
}
impl Studio {
    /// Show the start screen.
    pub(crate) fn show_start(&mut self) {
        self.start.open = true;
        self.start.textures.clear();
        self.start.read_pictures();
    }
    /// Keep the recent list and the open project's picture up to date. Called every frame.
    pub(crate) fn track_recent(&mut self) {
        if !self.capabilities.project_files {
            return;
        }
        let Some(path) = self.project.path.clone() else {
            return;
        };
        if self.start.recorded.as_ref() != Some(&path) {
            self.start.remember(&path);
            self.start.recorded = Some(path.clone());
            self.start.pictured = None;
        }
        // A save is the moment to take a fresh picture.
        let dirty = self.project.dirty();
        if self.start.was_dirty && !dirty {
            self.start.remember(&path);
            self.start.pictured = None;
        }
        self.start.was_dirty = dirty;
        if self.start.pictured.as_ref() == Some(&path) {
            return;
        }
        let (Some(displayed), Some(target)) = (&self.displayed, picture_path(&path)) else {
            return;
        };
        // The frame in the viewer, read back and scaled down on a worker.
        let (renderer, frame) = (self.renderer.clone(), displayed.frame.clone());
        std::thread::spawn(move || {
            let Ok(read) = renderer.read_frame(&frame) else {
                return;
            };
            let Ok(picture) = veylvik_io::preview::thumbnail(
                read.region.width,
                read.region.height,
                &read.pixels,
                PICTURE,
            ) else {
                return;
            };
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let temporary = target.with_extension("tmp");
            if std::fs::write(&temporary, &picture.png).is_ok() {
                let _ = std::fs::rename(temporary, target);
            }
        });
        self.start.pictured = Some(path);
    }
    /// Carry out a start-screen choice (after any unsaved work was saved or discarded).
    pub(crate) fn start_chosen(&mut self, ctx: &egui::Context) {
        match self.start.chosen.take() {
            Some(Chosen::Recent(path)) => self.open_path(Some(path)),
            Some(Chosen::FromMedia(path)) => self.new_from_media_path(Some(path)),
            Some(Chosen::Blank(width, height)) => {
                let (rate, frames) = (
                    self.preferences.values.new_rate,
                    self.preferences.values.new_frames,
                );
                match Time::new(rate.0, rate.1)
                    .map_err(|e| e.to_string())
                    .and_then(|rate| {
                        Project::blank(width, height, rate, frames).map_err(|e| e.to_string())
                    }) {
                    Ok(project) => self.open_project(project, ctx),
                    Err(e) => self.fail(e),
                }
            }
            Some(Chosen::GoldTitle) => {
                match Time::new(24, 1)
                    .map_err(|e| e.to_string())
                    .and_then(|rate| {
                        Project::blank(1920, 1080, rate, 240).map_err(|e| e.to_string())
                    }) {
                    Ok(project) => {
                        self.open_project(project, ctx);
                        self.add_gold_title();
                    }
                    Err(e) => self.fail(e),
                }
            }
            None => {}
        }
    }
    fn choose(&mut self, chosen: Chosen, ctx: &egui::Context) {
        self.start.chosen = Some(chosen);
        self.leave(Leave::Start, ctx);
    }
    /// The start screen, filling the window below the top bar.
    pub(crate) fn start_screen(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if let Some(receiver) = &self.start.pictures {
            while let Ok((path, picture)) = receiver.try_recv() {
                let name = format!("recent-{}", path.display());
                let texture = ctx.load_texture(name, picture, egui::TextureOptions::LINEAR);
                self.start.textures.insert(path, texture);
            }
        }
        let accent = crate::design::accent(&ctx);
        let border = crate::design::border(&ctx);
        let mut chosen = None;
        let mut forget = None;
        let mut open_file = false;
        let mut custom = false;
        let mut from_media = false;
        let mut templates = false;
        let mut close = false;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let width = ui.available_width().min(1040.0);
            let margin = ((ui.available_width() - width) * 0.5).max(0.0);
            ui.horizontal_top(|ui| {
                ui.add_space(margin);
                ui.vertical(|ui| {
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
                    ui.add_space(26.0);
                    // The mark and the name.
                    ui.horizontal(|ui| {
                        let (mark, _) = ui.allocate_exact_size(egui::Vec2::splat(52.0), egui::Sense::hover());
                        crate::icons::paint_mark(ui.painter(), mark, true);
                        ui.add_space(6.0);
                        let cap = 26.0;
                        let (word, _) = ui.allocate_exact_size(
                            egui::vec2(crate::icons::WORDMARK_WIDTH * cap, 52.0),
                            egui::Sense::hover(),
                        );
                        crate::icons::paint_wordmark(
                            ui.painter(),
                            egui::pos2(word.left(), word.center().y - cap * 0.5),
                            cap,
                            ui.visuals().strong_text_color(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(egui::Button::new("Go to the editor").min_size(egui::vec2(140.0, 32.0)))
                                .on_hover_text("Close this screen and keep the composition that is open")
                                .clicked()
                            {
                                close = true;
                            }
                        });
                    });
                    ui.add_space(10.0);
                    crate::design::heading(ui, "NEW COMPOSITION");
                    ui.horizontal_wrapped(|ui| {
                        for (name, w, h) in [
                            ("HD", 1920, 1080),
                            ("4K UHD", 3840, 2160),
                            ("Vertical", 1080, 1920),
                            ("Square", 1080, 1080),
                        ] {
                            if crate::design::format_card(ui, name, &format!("{w} × {h}"), Some([w as f32, h as f32]), false)
                                .on_hover_text("24 frames a second, ten seconds; change it later in Composition settings")
                                .clicked()
                            {
                                chosen = Some(Chosen::Blank(w, h));
                            }
                        }
                        if crate::design::format_card(ui, "Custom", "Any size and rate", None, false).clicked() {
                            custom = true;
                        }
                        ui.add_space(8.0);
                        if crate::design::option_card(ui, "From a video or image", "Match the composition to the file you pick", false).clicked() {
                            from_media = true;
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        crate::design::heading(ui, "RECENT PROJECTS");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::BOTTOM), |ui| {
                            if crate::design::primary(ui, "Open a project").clicked() {
                                open_file = true;
                            }
                        });
                    });
                    if self.start.entries.is_empty() {
                        ui.weak("Projects you open or save will be listed here, with a picture of each.");
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(14.0, 14.0);
                        for entry in &self.start.entries {
                            let size = egui::vec2(244.0, 137.0 + 52.0);
                            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
                            if !ui.is_rect_visible(rect) {
                                continue;
                            }
                            let painter = ui.painter();
                            let picture = egui::Rect::from_min_size(rect.min, egui::vec2(244.0, 137.0));
                            painter.rect_filled(rect, 8.0, ui.visuals().widgets.inactive.weak_bg_fill);
                            painter.rect_filled(picture, 8.0, egui::Color32::from_gray(10));
                            match self.start.textures.get(&entry.path) {
                                Some(texture) => {
                                    // Fit the picture inside its frame without stretching.
                                    let [w, h] = texture.size().map(|v| v as f32);
                                    let scale = (picture.width() / w).min(picture.height() / h);
                                    let fitted = egui::Rect::from_center_size(picture.center(), egui::vec2(w, h) * scale);
                                    painter.image(
                                        texture.id(),
                                        fitted,
                                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                                        egui::Color32::WHITE,
                                    );
                                }
                                None => crate::icons::paint_mark(
                                    painter,
                                    egui::Rect::from_center_size(picture.center(), egui::Vec2::splat(44.0)),
                                    true,
                                ),
                            }
                            let name = entry.path.file_stem().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                            let folder = entry
                                .path
                                .parent()
                                .and_then(|p| p.file_name())
                                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                            let fit = |text: String, limit: usize| {
                                if text.chars().count() > limit {
                                    format!("{}...", text.chars().take(limit - 3).collect::<String>())
                                } else {
                                    text
                                }
                            };
                            painter.text(
                                rect.left_top() + egui::vec2(10.0, 145.0),
                                egui::Align2::LEFT_TOP,
                                fit(name, 30),
                                egui::FontId::proportional(14.0),
                                ui.visuals().strong_text_color(),
                            );
                            painter.text(
                                rect.left_top() + egui::vec2(10.0, 166.0),
                                egui::Align2::LEFT_TOP,
                                fit(format!("{folder}  ·  {}", ago(entry.opened)), 38),
                                egui::FontId::proportional(11.5),
                                ui.visuals().weak_text_color(),
                            );
                            painter.rect_stroke(
                                rect,
                                8.0,
                                egui::Stroke::new(if response.hovered() { 1.5_f32 } else { 1.0_f32 }, if response.hovered() { accent } else { border }),
                                egui::StrokeKind::Inside,
                            );
                            let response = response.on_hover_text(entry.path.display().to_string());
                            if response.clicked() {
                                chosen = Some(Chosen::Recent(entry.path.clone()));
                            }
                            response.context_menu(|ui| {
                                if ui.button("Remove from this list").clicked() {
                                    forget = Some(entry.path.clone());
                                    ui.close();
                                }
                            });
                        }
                    });
                    ui.add_space(8.0);
                    crate::design::heading(ui, "TEMPLATES");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(14.0, 14.0);
                        let (rect, response) = ui.allocate_exact_size(egui::vec2(244.0, 137.0 + 52.0), egui::Sense::click());
                        let texture = self.start.gold.get_or_insert_with(|| {
                            let image = image::load_from_memory(include_bytes!("../assets/showcase/scene-gold_title.png"))
                                .expect("bundled scene preview")
                                .to_rgba8();
                            ctx.load_texture(
                                "start-gold-title",
                                egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.as_raw()),
                                egui::TextureOptions::LINEAR,
                            )
                        });
                        let painter = ui.painter();
                        painter.rect_filled(rect, 8.0, ui.visuals().widgets.inactive.weak_bg_fill);
                        painter.image(
                            texture.id(),
                            egui::Rect::from_min_size(rect.min, egui::vec2(244.0, 137.0)),
                            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                        painter.text(rect.left_top() + egui::vec2(10.0, 145.0), egui::Align2::LEFT_TOP, "Gold title", egui::FontId::proportional(14.0), ui.visuals().strong_text_color());
                        painter.text(rect.left_top() + egui::vec2(10.0, 166.0), egui::Align2::LEFT_TOP, "A 3D title in brushed gold, lit and ready", egui::FontId::proportional(11.5), ui.visuals().weak_text_color());
                        painter.rect_stroke(
                            rect,
                            8.0,
                            egui::Stroke::new(if response.hovered() { 1.5_f32 } else { 1.0_f32 }, if response.hovered() { accent } else { border }),
                            egui::StrokeKind::Inside,
                        );
                        if response.clicked() {
                            chosen = Some(Chosen::GoldTitle);
                        }
                        if crate::design::option_card(ui, "More templates", "Titles, looks and effects in the library", false).clicked() {
                            templates = true;
                        }
                    });
                    ui.add_space(30.0);
                });
            });
        });
        if let Some(path) = forget {
            self.start.forget(&path);
        }
        if let Some(chosen) = chosen {
            self.choose(chosen, &ctx);
        }
        if open_file {
            self.leave(Leave::Open, &ctx);
        }
        if custom {
            self.leave(Leave::New, &ctx);
        }
        if from_media {
            self.leave(Leave::NewFromMedia, &ctx);
        }
        if templates {
            self.start.open = false;
            self.show_templates();
        }
        if close {
            self.start.open = false;
        }
    }
}
