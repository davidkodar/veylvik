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
/// What Studio shows when started without a project.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Launch {
    #[default]
    StartScreen,
    LastProject,
    Empty,
}
/// The frame rates offered for new compositions, as ticks over a base.
pub const RATES: [(&str, i64, u32); 8] = [
    ("23.976", 24000, 1001),
    ("24", 24, 1),
    ("25", 25, 1),
    ("29.97", 30000, 1001),
    ("30", 30, 1),
    ("50", 50, 1),
    ("59.94", 60000, 1001),
    ("60", 60, 1),
];
/// What the viewer shows behind transparent areas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backdrop {
    #[default]
    Checker,
    Black,
    Grey,
    White,
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
    /// The budget follows the card's memory; `gpu_budget_mib` is then not used.
    pub gpu_budget_auto: bool,
    pub gpu_budget_mib: u64,
    pub playback_gpu_mib: u64,
    pub playback_disk_mib: u64,
    /// Memory and disk for played pictures kept as the screen shows them.
    pub played_memory_mib: u64,
    pub played_disk_mib: u64,
    /// Memory the machine keeps available whatever the store's own limit.
    pub played_reserve_mib: u64,
    /// Make the frames of the playback range that are not kept yet while nobody is working.
    pub fill_while_idle: bool,
    /// Playing holds until the frames ahead are prepared, so it never runs unevenly.
    pub wait_for_frames: bool,
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
    /// A new composition's size, frame rate (ticks over a base) and length in frames.
    pub new_width: u32,
    pub new_height: u32,
    pub new_rate: (i64, u32),
    pub new_frames: u32,
    pub launch: Launch,
    /// A setting's first keyframe makes a smooth curve (otherwise linear).
    pub smooth_keys: bool,
    /// The hour timecode reads at the composition's first frame.
    pub timecode_hour: u32,
    /// Listen for live control (MCP) from the next start.
    pub live_control: bool,
    /// Where the playback disk cache is kept; empty for the user's cache folder.
    pub cache_folder: String,
    /// Recent projects listed on the start screen.
    pub recent_kept: u32,
    /// Undo steps kept, and the memory they may use in MiB.
    pub undo_steps: u32,
    pub undo_memory_mib: u32,
    /// Apply an inspector change by itself after a pause; otherwise on Apply or on moving on.
    pub apply_after_pause: bool,
    pub apply_delay_ms: u32,
    /// The preview quality a session starts with: 1 full, 2 half, 4 quarter.
    pub preview_quality: u32,
    pub backdrop: Backdrop,
    /// Action-safe and title-safe areas as percentages of the frame.
    pub safe_action: u32,
    pub safe_title: u32,
    /// Where Open and Save begin for a project not saved yet, and where exports begin;
    /// empty for the folder Studio was started in.
    pub projects_folder: String,
    pub exports_folder: String,
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
            gpu_budget_auto: true,
            gpu_budget_mib: 2048,
            playback_gpu_mib: 512,
            playback_disk_mib: 0,
            played_memory_mib: quarter_of_memory_mib(),
            played_disk_mib: 50 * 1024,
            played_reserve_mib: 4096,
            fill_while_idle: true,
            wait_for_frames: true,
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
            new_width: 1920,
            new_height: 1080,
            new_rate: (24, 1),
            new_frames: 240,
            launch: Launch::default(),
            smooth_keys: false,
            timecode_hour: 0,
            live_control: true,
            cache_folder: String::new(),
            recent_kept: 12,
            undo_steps: 64,
            undo_memory_mib: 128,
            apply_after_pause: true,
            apply_delay_ms: 300,
            preview_quality: 1,
            backdrop: Backdrop::default(),
            safe_action: 90,
            safe_title: 80,
            projects_folder: String::new(),
            exports_folder: String::new(),
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
            || self.played_memory_mib > 65536
            || self.played_disk_mib > 1024 * 1024
            || self.played_reserve_mib > 65536
            || !(64..=16384).contains(&self.gpu_budget_mib)
            || !(10..=300).contains(&self.recovery_seconds)
            || !(0.0..=2.0).contains(&self.audio_volume)
            || self.audio_device.as_ref().is_some_and(|d| d.len() > 512)
            || !(1..=8192).contains(&self.new_width)
            || !(1..=8192).contains(&self.new_height)
            || !(1..=1_000_000).contains(&self.new_frames)
            || !RATES
                .iter()
                .any(|(_, ticks, base)| (*ticks, *base) == self.new_rate)
            || self.projects_folder.len() > 4096
            || self.cache_folder.len() > 4096
            || self.timecode_hour > 23
            || !(1..=30).contains(&self.recent_kept)
            || !(8..=256).contains(&self.undo_steps)
            || !(16..=128).contains(&self.undo_memory_mib)
            || !(100..=3000).contains(&self.apply_delay_ms)
            || ![1, 2, 4].contains(&self.preview_quality)
            || !(50..=100).contains(&self.safe_action)
            || !(50..=100).contains(&self.safe_title)
            || self.exports_folder.len() > 4096
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
    /// The window asked to empty the playback cache, the recent list, or to show the
    /// folder preferences are kept in.
    pub want_clear_cache: bool,
    pub want_clear_recent: bool,
    pub want_folder: bool,
    /// A folder being chosen in the system's dialog, and the setting it is for.
    picking: Option<(u8, std::sync::mpsc::Receiver<Option<PathBuf>>)>,
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
            want_clear_cache: false,
            want_clear_recent: false,
            want_folder: false,
            picking: None,
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
    /// The folder preferences are kept in.
    pub fn folder(&self) -> Option<PathBuf> {
        self.path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
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
                self.draft.gpu_budget_auto = d.gpu_budget_auto;
                self.draft.gpu_budget_mib = d.gpu_budget_mib;
                self.draft.playback_gpu_mib = d.playback_gpu_mib;
                self.draft.playback_disk_mib = d.playback_disk_mib;
                self.draft.played_memory_mib = d.played_memory_mib;
                self.draft.played_disk_mib = d.played_disk_mib;
                self.draft.played_reserve_mib = d.played_reserve_mib;
                self.draft.fill_while_idle = d.fill_while_idle;
                self.draft.wait_for_frames = d.wait_for_frames;
            }
            5 => {
                self.draft.audio_enabled = d.audio_enabled;
                self.draft.audio_device = d.audio_device;
                self.draft.audio_volume = d.audio_volume;
            }
            6 => {
                self.draft.auto_key = d.auto_key;
                self.draft.spread_edits = d.spread_edits;
                self.draft.undo_steps = d.undo_steps;
                self.draft.undo_memory_mib = d.undo_memory_mib;
                self.draft.apply_after_pause = d.apply_after_pause;
                self.draft.apply_delay_ms = d.apply_delay_ms;
                self.draft.smooth_keys = d.smooth_keys;
            }
            7 => {
                self.draft.time_display = d.time_display;
                self.draft.layer_bars = d.layer_bars;
                self.draft.aids = d.aids;
                self.draft.node_thumbnails = d.node_thumbnails;
                self.draft.preview_quality = d.preview_quality;
                self.draft.backdrop = d.backdrop;
                self.draft.safe_action = d.safe_action;
                self.draft.safe_title = d.safe_title;
                self.draft.timecode_hour = d.timecode_hour;
            }
            8 => {}
            10 => {
                self.draft.live_control = d.live_control;
                self.draft.cache_folder = d.cache_folder;
                self.draft.recent_kept = d.recent_kept;
            }
            9 => {
                self.draft.new_width = d.new_width;
                self.draft.new_height = d.new_height;
                self.draft.new_rate = d.new_rate;
                self.draft.new_frames = d.new_frames;
                self.draft.launch = d.launch;
                self.draft.projects_folder = d.projects_folder;
                self.draft.exports_folder = d.exports_folder;
            }
            _ => {
                self.draft.recovery_enabled = d.recovery_enabled;
                self.draft.recovery_seconds = d.recovery_seconds;
            }
        }
    }
    fn folder_mut(&mut self, which: u8) -> &mut String {
        match which {
            0 => &mut self.draft.projects_folder,
            1 => &mut self.draft.exports_folder,
            _ => &mut self.draft.cache_folder,
        }
    }
    /// A folder setting: its path, a button that asks the system for a folder, and a
    /// note when the path is not one.
    fn folder_row(&mut self, ui: &mut egui::Ui, which: u8, label: &str, hint: &str) {
        // A folder chosen in the system's dialog arrives here.
        if let Some((target, rx)) = &self.picking
            && let Ok(chosen) = rx.try_recv()
        {
            let target = *target;
            self.picking = None;
            if let Some(path) = chosen {
                *self.folder_mut(target) = path.to_string_lossy().into_owned();
            }
        }
        let busy = self.picking.is_some();
        let mut choose = false;
        ui.horizontal(|ui| {
            ui.add_sized([104., 22.], egui::Label::new(label));
            let folder = self.folder_mut(which);
            ui.add(
                egui::TextEdit::singleline(folder)
                    .hint_text(hint)
                    .desired_width(250.),
            );
            choose = ui
                .add_enabled(!busy, egui::Button::new("Choose..."))
                .on_hover_text("Pick the folder in the system's file dialog")
                .clicked();
            let path = folder.trim();
            if !path.is_empty() && !Path::new(path).is_dir() {
                ui.colored_label(ui.visuals().warn_fg_color, "Not a folder");
            }
        });
        if choose {
            let current = PathBuf::from(self.folder_mut(which).trim());
            let start = if current.is_dir() {
                current
            } else {
                std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
            };
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = ui.ctx().clone();
            // The dialog blocks until it is answered, so it runs off the interface thread.
            std::thread::spawn(move || {
                let chosen = crate::files::select(crate::files::Choice::Directory, &start, "")
                    .ok()
                    .flatten();
                let _ = tx.send(chosen);
                ctx.request_repaint();
            });
            self.picking = Some((which, rx));
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
            9 => {
                ui.heading("New projects");
                ui.label("A new composition starts as");
                ui.horizontal(|ui| {
                    for (label, width, height) in [
                        ("HD", 1920, 1080),
                        ("UHD", 3840, 2160),
                        ("Vertical", 1080, 1920),
                        ("Square", 1080, 1080),
                    ] {
                        if ui
                            .selectable_label(
                                (self.draft.new_width, self.draft.new_height) == (width, height),
                                label,
                            )
                            .clicked()
                        {
                            (self.draft.new_width, self.draft.new_height) = (width, height);
                        }
                    }
                    ui.add(egui::DragValue::new(&mut self.draft.new_width).range(1..=8192));
                    ui.label("x");
                    ui.add(egui::DragValue::new(&mut self.draft.new_height).range(1..=8192));
                });
                let fps = self.draft.new_rate.0 as f64 / f64::from(self.draft.new_rate.1.max(1));
                ui.horizontal(|ui| {
                    ui.label("Frame rate");
                    let shown = RATES
                        .iter()
                        .find(|(_, t, b)| (*t, *b) == self.draft.new_rate)
                        .map_or("24", |(label, ..)| label);
                    egui::ComboBox::from_id_salt("new-rate")
                        .selected_text(shown)
                        .width(80.)
                        .show_ui(ui, |ui| {
                            for (label, ticks, base) in RATES {
                                if ui
                                    .selectable_label(self.draft.new_rate == (ticks, base), label)
                                    .clicked()
                                {
                                    // The length keeps its duration at the new rate.
                                    let seconds = f64::from(self.draft.new_frames) / fps;
                                    self.draft.new_rate = (ticks, base);
                                    self.draft.new_frames =
                                        ((seconds * ticks as f64 / f64::from(base)).round() as u32)
                                            .clamp(1, 1_000_000);
                                }
                            }
                        });
                    ui.label("Length");
                    let mut seconds = f64::from(self.draft.new_frames) / fps;
                    if ui
                        .add(
                            egui::DragValue::new(&mut seconds)
                                .range(0.04..=3600.0)
                                .speed(0.1)
                                .suffix(" s"),
                        )
                        .changed()
                    {
                        self.draft.new_frames =
                            ((seconds * fps).round() as u32).clamp(1, 1_000_000);
                    }
                    ui.weak(format!("{} frames", self.draft.new_frames));
                });
                ui.weak("Used by New composition and by the start screen's size cards. A composition made from media still takes the media's size and rate.");
                ui.add_space(6.);
                ui.label("When Veylvik starts without a project");
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.draft.launch,
                        Launch::StartScreen,
                        "Show the start screen",
                    );
                    ui.selectable_value(
                        &mut self.draft.launch,
                        Launch::LastProject,
                        "Open the last project",
                    );
                    ui.selectable_value(&mut self.draft.launch, Launch::Empty, "Start empty");
                });
                ui.add_space(6.);
                self.folder_row(
                    ui,
                    0,
                    "Projects folder",
                    "The folder Veylvik was started in",
                );
                self.folder_row(ui, 1, "Exports folder", "The folder Veylvik was started in");
                ui.weak("Open and Save begin in the projects folder until a project has a place of its own; export dialogs begin in the exports folder.");
            }
            10 => {
                ui.heading("System");
                ui.checkbox(
                    &mut self.draft.live_control,
                    "Listen for live control (MCP) while Veylvik is open",
                );
                ui.weak("Lets an assistant or script work in the open project on this computer. Takes effect the next time Veylvik starts; --no-control still turns it off for one session.");
                ui.add_space(6.);
                self.folder_row(ui, 2, "Cache folder", "Your cache folder");
                ui.horizontal(|ui| {
                    if ui.button("Clear the playback cache now").clicked() {
                        self.want_clear_cache = true;
                    }
                    ui.weak("Frees cached preview frames in memory and on disk.");
                });
                ui.add_space(6.);
                ui.add(
                    egui::Slider::new(&mut self.draft.recent_kept, 1..=30)
                        .text("Recent projects listed"),
                );
                if ui.button("Clear the recent list now").on_hover_text("Forgets the list on the start screen; the projects themselves are untouched").clicked() {
                    self.want_clear_recent = true;
                }
                ui.add_space(6.);
                ui.horizontal(|ui| {
                    if ui.button("Reset all preferences").on_hover_text("Every section back to its original values, including changed shortcuts and saved layouts. Nothing is saved until Apply").clicked() {
                        self.draft = Values::default();
                    }
                    if ui.button("Open the preferences folder").clicked() {
                        self.want_folder = true;
                    }
                });
                if self.draft == Values::default() && self.values != self.draft {
                    ui.colored_label(ui.visuals().warn_fg_color, "All preferences are reset in this window. Apply to keep that, Cancel to leave things as they were.");
                }
            }
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
                ui.add_space(6.);
                ui.label("An inspector change is applied");
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.draft.apply_after_pause,
                        true,
                        "After a short pause",
                    );
                    ui.selectable_value(
                        &mut self.draft.apply_after_pause,
                        false,
                        "When I press Apply",
                    );
                });
                ui.add_enabled(
                    self.draft.apply_after_pause,
                    egui::Slider::new(&mut self.draft.apply_delay_ms, 100..=3000)
                        .text("Pause (milliseconds)"),
                );
                ui.weak("Either way the viewer shows the change at once. Without the pause a change waits for Apply or Ctrl+Enter, and is applied when you move on to another frame or node.");
                ui.add_space(6.);
                ui.horizontal(|ui| {
                    ui.label("A setting's first keyframe makes a curve that is");
                    ui.selectable_value(&mut self.draft.smooth_keys, false, "Linear");
                    ui.selectable_value(&mut self.draft.smooth_keys, true, "Smooth");
                });
                ui.add_space(6.);
                ui.add(
                    egui::Slider::new(&mut self.draft.undo_steps, 8..=256).text("Undo steps kept"),
                );
                ui.add(
                    egui::Slider::new(&mut self.draft.undo_memory_mib, 16..=128)
                        .text("Memory for undo (MiB)"),
                );
                ui.weak("Lowering these forgets the oldest steps at once. Whichever limit is reached first applies.");
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
                ui.horizontal(|ui| {
                    ui.label("Timecode at the first frame");
                    ui.add(
                        egui::DragValue::new(&mut self.draft.timecode_hour)
                            .range(0..=23)
                            .custom_formatter(|v, _| format!("{:02}:00:00:00", v as u32)),
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
                ui.horizontal(|ui| {
                    ui.label("Preview quality at start");
                    for (value, label) in [(1, "Full"), (2, "Half"), (4, "Quarter")] {
                        ui.selectable_value(&mut self.draft.preview_quality, value, label);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Behind transparent areas");
                    for (value, label) in [
                        (Backdrop::Checker, "Checkerboard"),
                        (Backdrop::Black, "Black"),
                        (Backdrop::Grey, "Grey"),
                        (Backdrop::White, "White"),
                    ] {
                        ui.selectable_value(&mut self.draft.backdrop, value, label);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Safe areas");
                    ui.add(
                        egui::DragValue::new(&mut self.draft.safe_action)
                            .range(50..=100)
                            .suffix("% action"),
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.draft.safe_title)
                            .range(50..=100)
                            .suffix("% title"),
                    );
                });
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
                let card = veylvik_io::runtime::card_memory_mib();
                let automatic = veylvik_io::runtime::automatic_mib(card);
                ui.checkbox(
                    &mut self.draft.gpu_budget_auto,
                    match card {
                        Some(card) => format!("Set the GPU buffer budget from the graphics card: {automatic} MiB of its {card} MiB"),
                        None => format!("Set the GPU buffer budget automatically: {automatic} MiB (the card's memory is not known)"),
                    },
                )
                .on_hover_text("Half the card's memory, at most 8192 MiB, leaving the rest for the desktop, video decoding, AI models and other applications");
                ui.add_enabled(
                    !self.draft.gpu_budget_auto,
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
                ui.separator();
                ui.label("Played pictures");
                ui.checkbox(
                    &mut self.draft.fill_while_idle,
                    "Prepare playback while idle",
                );
                ui.checkbox(
                    &mut self.draft.wait_for_frames,
                    "Play only when the frames are ready",
                );
                ui.weak("On: pressing play holds until the frames ahead are prepared, then plays every frame at its moment. Off: it plays at once and skips frames it cannot make in time.");
                ui.weak("When you stop working for a moment, the frames of the playback range that are not kept yet are made in the background, from the playhead on, and give way the instant you do anything.");
                ui.add(
                    egui::Slider::new(&mut self.draft.played_memory_mib, 0..=65536)
                        .logarithmic(true)
                        .text("Memory (MiB)"),
                );
                ui.add(
                    egui::Slider::new(&mut self.draft.played_disk_mib, 0..=1024 * 1024)
                        .logarithmic(true)
                        .text("Disk (MiB)"),
                );
                ui.add(
                    egui::Slider::new(&mut self.draft.played_reserve_mib, 0..=65536)
                        .logarithmic(true)
                        .text("Leave free for other programs (MiB)"),
                );
                ui.weak("The memory store gives way, to disk or by letting the oldest pictures go, before the machine has less than this available, whatever its own limit.");
                ui.weak("Frames you have played are kept as the screen shows them, packed without loss, so they play again at full rate: first in memory, then on disk in the cache folder, the oldest removed first. A 4K frame takes about 17 MiB. Zero switches a store off.");
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
                                (9, "New projects"),
                                (6, "Editing"),
                                (7, "Viewer and timeline"),
                                (8, "Keyboard and layouts"),
                                (10, "System"),
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

/// A quarter of the machine's memory in MiB, or 4 GiB when that cannot be read.
fn quarter_of_memory_mib() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("MemTotal:"))
                .and_then(|rest| {
                    rest.trim()
                        .trim_end_matches("kB")
                        .trim()
                        .parse::<u64>()
                        .ok()
                })
        })
        .map_or(4096, |kib| (kib / 1024 / 4).clamp(512, 16384))
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
        // New-project settings are held to what a composition allows.
        let fine = Values {
            new_width: 3840,
            new_height: 2160,
            new_rate: (30000, 1001),
            new_frames: 900,
            launch: Launch::LastProject,
            ..Values::default()
        };
        fine.validate().unwrap();
        let back: Values = serde_json::from_str(&serde_json::to_string(&fine).unwrap()).unwrap();
        assert_eq!(back, fine);
        for bad in [
            Values {
                new_width: 0,
                ..Values::default()
            },
            Values {
                new_height: 9000,
                ..Values::default()
            },
            Values {
                new_frames: 0,
                ..Values::default()
            },
            Values {
                new_rate: (27, 1),
                ..Values::default()
            },
        ] {
            assert!(bad.validate().is_err());
        }
        for bad in [
            Values {
                undo_steps: 4,
                ..Values::default()
            },
            Values {
                undo_steps: 300,
                ..Values::default()
            },
            Values {
                undo_memory_mib: 4096,
                ..Values::default()
            },
            Values {
                apply_delay_ms: 5,
                ..Values::default()
            },
            Values {
                preview_quality: 3,
                ..Values::default()
            },
            Values {
                safe_title: 20,
                ..Values::default()
            },
        ] {
            assert!(bad.validate().is_err());
        }
        let set: Values = serde_json::from_str(
            r#"{"backdrop":"white","apply_after_pause":false,"undo_steps":200}"#,
        )
        .unwrap();
        set.validate().unwrap();
        assert_eq!(
            (set.backdrop, set.apply_after_pause, set.undo_steps),
            (Backdrop::White, false, 200)
        );
        for bad in [
            Values {
                timecode_hour: 24,
                ..Values::default()
            },
            Values {
                recent_kept: 0,
                ..Values::default()
            },
            Values {
                recent_kept: 31,
                ..Values::default()
            },
        ] {
            assert!(bad.validate().is_err());
        }
        // Resetting everything in the window and applying gives the defaults back, even
        // for settings changed elsewhere while it was open.
        let mut changed = Values::default();
        changed
            .shortcuts
            .insert("save".into(), "Ctrl+Shift+S".into());
        changed.timecode_hour = 1;
        changed.live_control = false;
        assert_eq!(
            merged(&changed, &Values::default(), &changed),
            Values::default()
        );
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
