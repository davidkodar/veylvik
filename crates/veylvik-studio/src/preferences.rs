//! User preferences are separate from compositions and their undo history.
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    #[default]
    Dark,
    Light,
    Blue,
    Green,
}
impl Theme {
    pub const ALL: [Self; 5] = [
        Self::System,
        Self::Dark,
        Self::Light,
        Self::Blue,
        Self::Green,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "Follow system",
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::Blue => "Midnight Blue",
            Self::Green => "Evergreen",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Values {
    pub theme: Theme,
    pub scale: f32,
    pub restart_playback: bool,
    pub loop_playback: bool,
    pub auto_arrange: bool,
    pub hover_previews: bool,
    pub fast_view: bool,
    pub gpu_budget_mib: u64,
    pub playback_gpu_mib: u64,
    pub playback_disk_mib: u64,
    pub recovery_enabled: bool,
    pub recovery_seconds: u64,
}
impl Default for Values {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            scale: 1.,
            restart_playback: false,
            loop_playback: true,
            auto_arrange: true,
            hover_previews: true,
            fast_view: false,
            gpu_budget_mib: 2048,
            playback_gpu_mib: 512,
            playback_disk_mib: 0,
            recovery_enabled: true,
            recovery_seconds: 10,
        }
    }
}
impl Values {
    fn validate(&self) -> Result<(), String> {
        if !self.scale.is_finite()
            || !(0.75..=2.).contains(&self.scale)
            || self.playback_gpu_mib > 8192
            || self.playback_disk_mib > 65536
            || !(64..=16384).contains(&self.gpu_budget_mib)
            || !(10..=300).contains(&self.recovery_seconds)
        {
            return Err("Preferences contain out-of-range values".into());
        }
        Ok(())
    }
}
pub fn load(path: &Path) -> Result<Values, String> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Values::default()),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 65536 {
        return Err("Preferences file is too large".into());
    }
    let values: Values = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    values.validate()?;
    Ok(values)
}
fn save(path: &Path, values: &Values) -> Result<(), String> {
    values.validate()?;
    let parent = path.parent().ok_or("Missing preferences directory")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = parent.join(format!(
        ".preferences-{}.tmp",
        veylvik_document::NodeId::new_v4()
    ));
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(&serde_json::to_vec_pretty(values)?)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result.map_err(|e| e.to_string())
}
fn path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|p| p.join("veylvik/preferences.json"))
}
pub struct Preferences {
    pub values: Values,
    pub open: bool,
    draft: Values,
    section: usize,
    path: Option<PathBuf>,
    error: Option<String>,
}
impl Preferences {
    pub fn new() -> Self {
        let path = path();
        let result = path
            .as_deref()
            .ok_or("Cannot locate your configuration directory".to_owned())
            .and_then(load);
        let error = result.as_ref().err().cloned();
        let values = result.unwrap_or_default();
        Self {
            draft: values.clone(),
            values,
            open: false,
            section: 0,
            path,
            error,
        }
    }
    pub fn begin(&mut self) {
        self.draft = self.values.clone();
        self.open = true;
    }
    pub fn theme(&self) -> Theme {
        if self.open {
            self.draft.theme
        } else {
            self.values.theme
        }
    }
    pub fn persist(&mut self) -> Result<(), String> {
        save(
            self.path
                .as_deref()
                .ok_or("Cannot locate preferences file")?,
            &self.values,
        )
    }
    fn reset_section(&mut self) {
        let d = Values::default();
        match self.section {
            0 => {
                self.draft.restart_playback = d.restart_playback;
                self.draft.loop_playback = d.loop_playback;
            }
            1 => {
                self.draft.theme = d.theme;
                self.draft.scale = d.scale;
            }
            2 => self.draft.auto_arrange = d.auto_arrange,
            3 => {
                self.draft.hover_previews = d.hover_previews;
                self.draft.fast_view = d.fast_view;
                self.draft.gpu_budget_mib = d.gpu_budget_mib;
                self.draft.playback_gpu_mib = d.playback_gpu_mib;
                self.draft.playback_disk_mib = d.playback_disk_mib;
            }
            _ => {
                self.draft.recovery_enabled = d.recovery_enabled;
                self.draft.recovery_seconds = d.recovery_seconds;
            }
        }
    }
    fn section_ui(&mut self, ui: &mut egui::Ui, source: &str) {
        match self.section {
            0 => {
                ui.heading("Playback");
                ui.checkbox(
                    &mut self.draft.restart_playback,
                    "Start playback from the beginning",
                )
                .on_hover_text(
                    "Space and Play restart at the playback range’s first frame. Pausing keeps the current frame visible.",
                );
                ui.checkbox(&mut self.draft.loop_playback, "Loop playback");
                ui.weak("With looping off, playback stops on the final frame. Starting again at the end restarts from the playback range’s first frame.");
                ui.separator();
                ui.label("Space: play / pause");
                ui.label("Left / Right: step one frame");
                ui.label("Alt + Left / Right: previous / next keyframe");
            }
            1 => {
                ui.heading("Appearance");
                ui.label("Theme");
                for theme in Theme::ALL {
                    ui.selectable_value(&mut self.draft.theme, theme, theme.label());
                }
                ui.weak(if self.draft.theme == Theme::System {
                    source
                } else {
                    "Built-in palette · preview changes below"
                });
                ui.separator();
                ui.add(
                    egui::Slider::new(&mut self.draft.scale, 0.75..=2.)
                        .text("Interface scale")
                        .step_by(0.05),
                );
                ui.weak("Theme previews immediately; Cancel restores it. Interface scale applies on Apply. Themes never change rendered or exported colours.");
            }
            2 => {
                ui.heading("Workspace");
                ui.checkbox(
                    &mut self.draft.auto_arrange,
                    "Automatically arrange nodes in new/opened projects",
                );
                ui.weak("Manual node placement turns automatic arrangement off for that workspace. Changing this preference also updates the current graph.");
            }
            3 => {
                ui.heading("Preview performance");
                ui.checkbox(
                    &mut self.draft.hover_previews,
                    "Animate library previews on hover",
                );
                ui.checkbox(
                    &mut self.draft.fast_view,
                    "Use fast viewer filtering by default",
                );
                ui.weak("Fast filtering reduces viewer antialiasing cost. It never reduces export quality. Changing this preference also updates the current viewer.");
                ui.separator();
                ui.add(
                    egui::Slider::new(&mut self.draft.gpu_budget_mib, 64..=16384)
                        .text("GPU buffer budget (MiB)"),
                );
                ui.weak("Budget changes require restart. VEYLVIK_GPU_BUDGET_MIB overrides this setting. The budget covers renderer buffers, not total driver or UI memory.");
                ui.weak("Composition effects remain GPU-rendered. Shutter sampling and optical-flow quality are controlled on their respective nodes.");
                ui.separator();
                ui.label("Playback cache");
                ui.add(
                    egui::Slider::new(&mut self.draft.playback_gpu_mib, 0..=8192)
                        .text("GPU cache (MiB)"),
                );
                ui.add(
                    egui::Slider::new(&mut self.draft.playback_disk_mib, 0..=65536)
                        .text("Disk cache (MiB)"),
                );
                ui.weak("Zero disables that cache. GPU cache uses at most half the renderer budget and yields memory to rendering. Disk cache is lossless, session-only, and requires GPU readback/upload on the preview worker. It never renders effects on the CPU. Changes apply without restart.");
            }
            _ => {
                ui.heading("Recovery copies");
                ui.checkbox(
                    &mut self.draft.recovery_enabled,
                    "Keep recovery copies of unsaved edits",
                );
                ui.add_enabled(
                    self.draft.recovery_enabled,
                    egui::Slider::new(&mut self.draft.recovery_seconds, 10..=300)
                        .text("Interval (seconds)"),
                );
                ui.weak("Recovery copies do not overwrite your project. Disabling pauses future writes; existing copies remain available from File > Recover unsaved work.");
            }
        }
        ui.add_space(8.);
        if ui.button("Reset this section").clicked() {
            self.reset_section();
        }
    }
    pub fn show(&mut self, ctx: &egui::Context, source: &str) -> bool {
        if !self.open {
            return false;
        }
        let mut open = true;
        let mut applied = false;
        let mut close = false;
        egui::Window::new("Preferences")
            .id(egui::Id::new("preferences"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos(ctx.content_rect().center() - egui::vec2(350., 260.))
            .default_width(680.)
            .show(ctx, |ui| {
                let body_height = (ctx.content_rect().height() - 180.).clamp(180., 400.);
                ui.allocate_ui_with_layout(
                    egui::vec2(650., body_height),
                    egui::Layout::left_to_right(egui::Align::TOP),
                    |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(145.);
                            for (i, label) in [
                                "Playback",
                                "Appearance",
                                "Workspace",
                                "Performance",
                                "Recovery",
                            ]
                            .iter()
                            .enumerate()
                            {
                                ui.selectable_value(&mut self.section, i, *label);
                            }
                        });
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .id_salt("preferences-body")
                            .max_height(body_height)
                            .show(ui, |ui| {
                                ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                                    ui.set_width(470.);
                                    self.section_ui(ui, source);
                                });
                            });
                    },
                );
                ui.separator();
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, format!("Preferences: {error}"));
                }
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        match self
                            .path
                            .as_deref()
                            .ok_or("Cannot locate preferences file".into())
                            .and_then(|p| save(p, &self.draft))
                        {
                            Ok(()) => {
                                self.values = self.draft.clone();
                                self.error = None;
                                applied = true;
                                close = true;
                            }
                            Err(e) => self.error = Some(e),
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    ui.weak(if self.draft != self.values {
                        "Unsaved changes · click Apply to save."
                    } else {
                        "Settings are saved for this user, independent of projects."
                    });
                });
            });
        self.open = open && !close;
        applied
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_roundtrip_defaults_and_reject_invalid_files() {
        let dir = std::env::temp_dir().join(format!(
            "veylvik-prefs-{}",
            veylvik_document::NodeId::new_v4()
        ));
        let path = dir.join("preferences.json");
        assert_eq!(load(&path).unwrap(), Values::default());
        let v = Values {
            theme: Theme::Green,
            restart_playback: true,
            loop_playback: false,
            ..Default::default()
        };
        save(&path, &v).unwrap();
        assert_eq!(load(&path).unwrap(), v);
        std::fs::write(&path, b"{\"scale\":99}").unwrap();
        assert!(load(&path).is_err());
        std::fs::write(&path, b"{\"restart_playback\":true}").unwrap();
        assert!(load(&path).unwrap().loop_playback);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
