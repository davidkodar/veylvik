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
/// The viewer aids that are switched on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Aids {
    pub safe: bool,
    pub centre: bool,
    pub thirds: bool,
    pub grid: bool,
    pub rulers: bool,
    pub readout: bool,
}
/// What Apply saves: every setting the window changed since it opened, and for the
/// rest whatever is current, so a change made elsewhere meanwhile is not undone.
pub(crate) fn merged(opened: &Values, draft: &Values, current: &Values) -> Values {
    let object = |values: &Values| match serde_json::to_value(values) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => Default::default(),
    };
    let (opened, draft, mut result) = (object(opened), object(draft), object(current));
    for (name, value) in draft {
        if opened.get(&name) != Some(&value) {
            result.insert(name, value);
        }
    }
    serde_json::from_value(serde_json::Value::Object(result)).unwrap_or_else(|_| current.clone())
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
    /// Play the composition's sound during playback.
    pub audio_enabled: bool,
    /// Output device by name; `None` follows the system default (PipeWire's choice).
    pub audio_device: Option<String>,
    /// Playback gain, 0..=2 (1 = as recorded).
    pub audio_volume: f32,
    /// Positions in time as timecode or as frame numbers.
    pub time_display: crate::timecode::Display,
    /// A small picture of each node's output on its tile in the graph.
    pub node_thumbnails: bool,
    /// Changing an animated setting adds a keyframe by itself.
    pub auto_key: bool,
    /// An inspector change reaches the other selected nodes of the same kind.
    pub spread_edits: bool,
    /// Bars for footage and fades under the timeline's ruler.
    pub layer_bars: bool,
    /// The viewer aids left on.
    pub aids: Aids,
    /// Panel arrangements saved by name.
    pub layouts: Vec<crate::layouts::Layout>,
    /// Changed shortcuts: command id to key, empty for none.
    pub shortcuts: std::collections::BTreeMap<String, String>,
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
            audio_enabled: true,
            audio_device: None,
            audio_volume: 1.,
            time_display: Default::default(),
            node_thumbnails: false,
            auto_key: false,
            spread_edits: true,
            layer_bars: true,
            aids: Aids::default(),
            layouts: Vec::new(),
            shortcuts: Default::default(),
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
            || !(0.0..=2.0).contains(&self.audio_volume)
            || self.audio_device.as_ref().is_some_and(|d| d.len() > 512)
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
    /// The values when the window opened, to tell what it changed.
    opened: Values,
    /// The window asked for the shortcut editor.
    pub want_shortcuts: bool,
    section: usize,
    path: Option<PathBuf>,
    error: Option<String>,
    /// Output devices, read when the Audio section opens (enumeration can take a moment).
    audio_devices: Option<Vec<veylvik_audio::Device>>,
    /// A device scan in progress; ALSA enumeration stays off the UI thread.
    audio_scan: Option<std::sync::mpsc::Receiver<Vec<veylvik_audio::Device>>>,
    /// The device in use, set by the editor each frame.
    pub audio_status: String,
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
            opened: values.clone(),
            want_shortcuts: false,
            values,
            open: false,
            section: 0,
            path,
            error,
            audio_devices: None,
            audio_scan: None,
            audio_status: String::new(),
        }
    }
    pub fn begin(&mut self) {
        self.draft = self.values.clone();
        self.opened = self.values.clone();
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
            5 => {
                self.draft.audio_enabled = d.audio_enabled;
                self.draft.audio_device = d.audio_device;
                self.draft.audio_volume = d.audio_volume;
            }
            6 => {
                self.draft.auto_key = d.auto_key;
                self.draft.spread_edits = d.spread_edits;
            }
            7 => {
                self.draft.time_display = d.time_display;
                self.draft.layer_bars = d.layer_bars;
                self.draft.aids = d.aids;
                self.draft.node_thumbnails = d.node_thumbnails;
            }
            8 => {}
            _ => {
                self.draft.recovery_enabled = d.recovery_enabled;
                self.draft.recovery_seconds = d.recovery_seconds;
            }
        }
    }
    fn audio_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Audio");
        ui.checkbox(&mut self.draft.audio_enabled, "Play sound during playback")
            .on_hover_text(
                "Footage sound is mixed exactly like video export and follows the playhead.",
            );
        if let Some(scan) = &self.audio_scan
            && let Ok(found) = scan.try_recv()
        {
            self.audio_devices = Some(found);
            self.audio_scan = None;
        }
        if self.audio_devices.is_none() && self.audio_scan.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = ui.ctx().clone();
            let _ = std::thread::Builder::new()
                .name("audio-devices".into())
                .spawn(move || {
                    let _ = tx.send(veylvik_audio::devices());
                    ctx.request_repaint();
                });
            self.audio_scan = Some(rx);
        }
        let scanning = self.audio_devices.is_none();
        let devices = self.audio_devices.clone().unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label("Output device");
            let selected = match &self.draft.audio_device {
                None => "System default".to_owned(),
                Some(name) => devices
                    .iter()
                    .find(|d| &d.name == name)
                    .map_or(name.clone(), |d| d.label.clone()),
            };
            egui::ComboBox::from_id_salt("audio-device")
                .selected_text(selected)
                .width(300.)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.draft.audio_device, None, "System default")
                        .on_hover_text("Follow the system's output, as set in PipeWire or your sound settings. Recommended.");
                    for device in devices.iter() {
                        let tip = if device.direct {
                            format!("{} · opens the hardware directly, bypassing PipeWire; other apps may lose sound on it while Veylvik plays", device.detail)
                        } else {
                            device.detail.clone()
                        };
                        ui.selectable_value(
                            &mut self.draft.audio_device,
                            Some(device.name.clone()),
                            &device.label,
                        )
                        .on_hover_text(tip);
                    }
                });
        });
        if scanning {
            ui.weak("Looking for devices…");
        }
        let missing = !scanning
            && self
                .draft
                .audio_device
                .as_ref()
                .is_some_and(|name| !devices.iter().any(|d| &d.name == name));
        if missing {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "This device is not connected; playback stays silent until it returns.",
            );
        }
        if ui.small_button("Refresh devices").clicked() {
            self.audio_devices = None;
        }
        ui.horizontal(|ui| {
            ui.label("Volume");
            ui.add(
                egui::Slider::new(&mut self.draft.audio_volume, 0.0..=2.0)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.)),
            );
        });
        ui.separator();
        ui.label("In use");
        ui.weak(if self.audio_status.is_empty() {
            "Not opened yet; it opens on first playback."
        } else {
            self.audio_status.as_str()
        });
        ui.weak(format!("Audio system: {}", veylvik_audio::backend()));
    }
    fn section_ui(&mut self, ui: &mut egui::Ui, source: &str) {
        match self.section {
            5 => self.audio_ui(ui),
            6 => {
                ui.heading("Editing");
                ui.checkbox(&mut self.draft.auto_key, "Automatic keyframes");
                ui.weak("On: changing an animated setting adds a keyframe at the current position. Off: such a change is a try-out until you choose Add keyframe. Settings without keyframes are never animated by changing them.");
                ui.add_space(6.);
                ui.checkbox(
                    &mut self.draft.spread_edits,
                    "Apply inspector changes to all selected nodes of the same kind",
                );
                ui.weak("Only the settings you change are carried over. The inspector shows how many nodes a change reaches.");
            }
            7 => {
                ui.heading("Viewer and timeline");
                ui.label("Show positions in time as");
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.draft.time_display,
                        crate::timecode::Display::Timecode,
                        "Timecode",
                    );
                    ui.selectable_value(
                        &mut self.draft.time_display,
                        crate::timecode::Display::Frames,
                        "Frame numbers",
                    );
                });
                ui.checkbox(
                    &mut self.draft.layer_bars,
                    "Layer bars under the timeline's ruler",
                );
                ui.add_space(6.);
                ui.label("Viewer aids left on");
                ui.horizontal_wrapped(|ui| {
                    let aids = &mut self.draft.aids;
                    ui.checkbox(&mut aids.safe, "Safe areas");
                    ui.checkbox(&mut aids.centre, "Centre");
                    ui.checkbox(&mut aids.thirds, "Thirds");
                    ui.checkbox(&mut aids.grid, "Grid");
                    ui.checkbox(&mut aids.rulers, "Rulers");
                    ui.checkbox(&mut aids.readout, "Colour under the pointer");
                });
                ui.weak("The aids are drawn over the viewer only and are never part of a render.");
                ui.add_space(6.);
                ui.checkbox(
                    &mut self.draft.node_thumbnails,
                    "Thumbnails on nodes in the graph",
                );
                ui.weak("Made one node at a time while Studio is idle; each is a full render of that node's branch.");
            }
            8 => {
                ui.heading("Keyboard and layouts");
                ui.label(match self.values.shortcuts.len() {
                    0 => "Every command has its original shortcut.".to_owned(),
                    n => format!("{n} command(s) have a changed shortcut."),
                });
                if ui.button("Keyboard shortcuts...").clicked() {
                    self.want_shortcuts = true;
                }
                ui.add_space(6.);
                ui.label(match self.values.layouts.len() {
                    0 => "No layouts saved yet. Save one from View > Layout.".to_owned(),
                    n => format!("{n} saved layout(s), listed under View > Layout."),
                });
                return;
            }
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
                            // Indices are stable section ids; the list order is what users see.
                            for (i, label) in [
                                (0, "Playback"),
                                (5, "Audio"),
                                (1, "Appearance"),
                                (2, "Workspace"),
                                (3, "Performance"),
                                (4, "Recovery"),
                                (6, "Editing"),
                                (7, "Viewer and timeline"),
                                (8, "Keyboard and layouts"),
                            ] {
                                ui.selectable_value(&mut self.section, i, label);
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
                            .and_then(|p| save(p, &merged(&self.opened, &self.draft, &self.values)))
                        {
                            Ok(()) => {
                                self.values = merged(&self.opened, &self.draft, &self.values);
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
    fn applying_keeps_what_was_changed_elsewhere_while_the_window_was_open() {
        let opened = Values::default();
        // In the window: a new scale and automatic keyframes on.
        let mut draft = opened.clone();
        draft.scale = 1.5;
        draft.auto_key = true;
        // Meanwhile, elsewhere: a shortcut, a saved layout, thumbnails and frame numbers.
        let mut current = opened.clone();
        current
            .shortcuts
            .insert("save".into(), "Ctrl+Shift+S".into());
        current.layouts.push(crate::layouts::Layout::default());
        current.node_thumbnails = true;
        current.time_display = crate::timecode::Display::Frames;
        let result = merged(&opened, &draft, &current);
        assert_eq!((result.scale, result.auto_key), (1.5, true));
        assert_eq!(result.shortcuts, current.shortcuts);
        assert_eq!(result.layouts.len(), 1);
        assert!(result.node_thumbnails);
        assert_eq!(result.time_display, crate::timecode::Display::Frames);
        // Changed in both places: the window, which is what Apply was pressed on, wins.
        current.auto_key = false;
        current.scale = 0.9;
        assert_eq!(merged(&opened, &draft, &current).scale, 1.5);
        // Nothing changed in the window: nothing is touched.
        assert_eq!(merged(&opened, &opened, &current), current);
        // Preferences written before the newer settings existed still load, with defaults.
        let old: Values = serde_json::from_str(r#"{"scale":1.25,"loop_playback":false}"#).unwrap();
        assert_eq!(
            (old.scale, old.spread_edits, old.layer_bars, old.aids),
            (1.25, true, true, Aids::default())
        );
    }
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
