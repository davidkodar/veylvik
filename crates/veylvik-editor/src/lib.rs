mod about;
mod animation;
mod animation_editor;
mod asset_browser;
mod audio_preview;
mod autokey;
mod clip_source;
mod color_controls;
mod command_ui;
mod commands;
mod comparison;
mod composition;
mod control;
pub mod controls;
mod curve_preview;
mod design;
mod diagnostics;
mod display;
mod drops;
mod editable_text;
mod effects;
mod expressions;
mod exr_layers;
mod eyedropper;
mod file_results;
mod files;
mod font_browser;
mod grain_measure;
mod graph;
mod graph_layout;
mod handles;
pub use clip_source::ClipSource;
mod fill;
mod flock_bake;
mod history_log;
pub mod host;
mod host_link;
mod host_time;
mod icons;
mod inspector_tools;
mod key_timeline;
mod layer_bars;
mod layouts;
mod light_guides;
mod live_files;
mod live_jobs;
mod material_library;
mod media_controls;
mod media_import;
pub mod menu_model;
mod menus;
mod model_import;
mod models_window;
mod names;
mod node_clipboard;
mod node_finder;
mod node_palette;
mod node_thumbnails;
mod nodes;
mod nuke_import;
mod occlusion;
mod ofx_curves;
mod ofx_inspector;
/// A port's name as the node graph shows it.
pub(crate) fn graph_port_label(key: &str) -> String {
    graph::port_label(key)
}
mod pages;
mod path_edit;
mod path_handles;
mod path_pen;
mod path_transform;
mod planar;
mod playback;
mod portable;
pub mod preferences;
mod preset_library;
mod preview_images;
mod project;
mod published;
mod recovery;
mod remembered;
mod render_queue;
mod scene_focus;
mod scene_gizmo;
mod scene_guides;
mod scene_points;
mod scene_shine;
mod scene_shot;
mod scopes;
mod sequence;
mod shader_editor;
mod shape_animation;
mod shot_light;
mod showcase_gallery;
mod solve_camera;
mod solve_projection;
mod splat_import;
mod start;
mod status_bar;
mod subject;
mod surface;
mod text_effects;
mod text_handles;
mod text_outline;
mod text_viewer;
mod theme;
mod timecode;
mod timeline_markers;
mod title_styles;
mod tracking;
mod transform_handles;
mod trust;
mod vector_editor;
mod video_export;
mod viewer;
mod viewer_overlays;
mod viewport3d;
mod waveform;
use display::{Display, DisplayFrame, Paint, ViewSettings};
use eframe::{egui, egui_wgpu};
use files::{Choice, Completed, Tasks};
use project::Project;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use veylvik_document::{Command, Node, Operation};
use veylvik_preview::{GpuPreviewScheduler, PreviewTicket};
use veylvik_render::{RenderRequest, Renderer};

/// Run the editor as an app of its own, in its own window: Veylvik Studio.
pub fn standalone() {
    if let Err(error) = run() {
        eprintln!("Veylvik: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let mut path = None;
    // Live control is on by default at the runtime socket; --control-dir picks another
    // directory and --no-control turns it off for this session.
    let mut control_directory = None;
    let mut no_control = false;
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--control-dir"
            && control_directory.is_none()
            && !no_control
            && index + 1 < args.len()
        {
            control_directory = Some(PathBuf::from(&args[index + 1]));
            index += 2;
        } else if args[index] == "--no-control" && control_directory.is_none() && !no_control {
            no_control = true;
            index += 1;
        } else if path.is_none() && !args[index].to_string_lossy().starts_with("--") {
            path = Some(PathBuf::from(&args[index]));
            index += 1;
        } else {
            return Err(
                "usage: veylvik-studio [FILE] [--control-dir DIRECTORY | --no-control]".into(),
            );
        }
    }
    let (instance, adapter, device, queue) = pollster::block_on(veylvik_render::create_gpu())?;
    let setup = egui_wgpu::WgpuSetupExisting {
        instance,
        adapter,
        device,
        queue,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Veylvik")
            .with_app_id("veylvik")
            // Wayland compositors resolve the icon from the app ID; X11 takes it from here.
            .with_icon(Arc::new(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/veylvik-256.png"))
                    .expect("bundled icon is a valid PNG"),
            ))
            .with_inner_size([1600.0, 960.0])
            .with_min_inner_size([800.0, 600.0]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            wgpu_setup: setup.into(),
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "Veylvik",
        options,
        Box::new(move |cc| {
            let state = cc
                .wgpu_render_state
                .as_ref()
                .ok_or("Vulkan render state unavailable")?;
            let app = Studio::create(Setup {
                interface: HOST_INTERFACE,
                context: cc.egui_ctx.clone(),
                device: state.device.clone(),
                queue: state.queue.clone(),
                adapter: state.adapter.get_info(),
                target_format: state.target_format,
                opens: Opens::File(path),
                live_control: match (no_control, control_directory) {
                    (true, _) => LiveControl::Off,
                    (false, Some(directory)) => LiveControl::Directory(directory),
                    (false, None) => LiveControl::Usual,
                },
                capabilities: Capabilities::standalone(),
                host: None,
                renderer: None,
            })
            .map_err(|error| error.to_string())?;
            Ok(Box::new(app))
        }),
    )?;
    Ok(())
}
/// The version of what a host and the editor agree on: a host built for another version
/// is refused, never guessed at.
pub const HOST_INTERFACE: u32 = 1;
/// What the host lets the editor do beyond drawing in its area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// The window is the editor's own: it sets the window's look and scale, takes all
    /// its input, and may close it.
    pub own_window: bool,
    /// The editor keeps projects as files itself: start screen, new, open, recent, save,
    /// autosave and recovery. Without it the host owns the composition and its saving.
    pub project_files: bool,
    /// The editor's own export: the Export page, the render queue and exporting frames.
    pub export: bool,
    pub preferences: bool,
    pub about: bool,
    /// The viewer may take the whole window, or open in a window of its own.
    pub full_screen: bool,
    pub second_viewer: bool,
    /// The editor draws its own menu bar (with the workspaces and the title). Without it
    /// a host shows `Editor::menus`, `workspaces` and `title` in its own bar; the
    /// editor's keys still work while it has the keyboard.
    pub menu_bar: bool,
    /// The host keeps the undo history: Undo and Redo are left out of the menus, and the
    /// host undoes through `Editor::undo` / `redo` (each edit comes as one change).
    pub host_history: bool,
}
impl Capabilities {
    /// Everything: the editor as an app of its own.
    pub fn standalone() -> Self {
        Self {
            own_window: true,
            project_files: true,
            export: true,
            preferences: true,
            about: true,
            full_screen: true,
            second_viewer: true,
            menu_bar: true,
            host_history: false,
        }
    }
    /// A page inside a host that owns the window, the project and the export.
    pub fn embedded() -> Self {
        Self {
            own_window: false,
            project_files: false,
            export: false,
            preferences: false,
            about: false,
            full_screen: false,
            second_viewer: false,
            menu_bar: false,
            host_history: true,
        }
    }
}
/// A name made safe to begin a file name with: no folder separators, control characters
/// or leading dots; `None` when nothing is left.
fn safe_stem(name: &str) -> Option<String> {
    let safe: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\') || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let safe = safe.trim().trim_start_matches('.').trim();
    (!safe.is_empty()).then(|| safe.to_string())
}
#[cfg(test)]
mod stem_tests {
    #[test]
    fn a_hosts_name_begins_file_names_safely() {
        use super::safe_stem;
        assert_eq!(safe_stem("Beach at dawn").as_deref(), Some("Beach at dawn"));
        assert_eq!(
            safe_stem("take 2/3: wide").as_deref(),
            Some("take 2-3: wide")
        );
        assert_eq!(
            safe_stem("../../etc/passwd").as_deref(),
            Some("-..-etc-passwd")
        );
        assert_eq!(safe_stem("a\\b\nc").as_deref(), Some("a-b-c"));
        assert_eq!(safe_stem("  ...  "), None);
        assert_eq!(safe_stem(""), None);
    }
}
/// Read a composition from a `.veylvik` file, for a host that keeps compositions as files.
pub fn load_composition(
    path: &std::path::Path,
) -> Result<veylvik_document::Composition, Box<dyn std::error::Error>> {
    Ok(veylvik_document::Composition::load(path)?)
}
/// What the editor starts on.
pub enum Opens {
    /// A project file; none starts on the start screen.
    File(Option<PathBuf>),
    /// A composition the host owns. The editor never writes it anywhere: the host reads
    /// it back with [`Editor::composition`] when it saves. Relative paths in it, and
    /// everything the editor itself writes for it (mattes, tracks, caches), are in `assets`.
    Composition {
        composition: Box<veylvik_document::Composition>,
        assets: PathBuf,
    },
}
/// Whether the editor listens for live control (MCP), and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveControl {
    Off,
    /// At the usual runtime socket; failing to bind never stops the editor.
    Usual,
    /// In this directory; failing to bind is an error.
    Directory(PathBuf),
}
/// Everything a host hands over to have an editor: its egui context, the Vulkan device its
/// own pictures are drawn with (the editor renders on that same device, so its frames
/// never leave the GPU), and what to open.
pub struct Setup {
    /// Must equal [`HOST_INTERFACE`].
    pub interface: u32,
    pub context: egui::Context,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter: wgpu::AdapterInfo,
    /// The format of the surface the host's egui paints to.
    pub target_format: wgpu::TextureFormat,
    pub opens: Opens,
    pub live_control: LiveControl,
    pub capabilities: Capabilities,
    /// Who answers for files and for media of its own; none leaves both to the system's
    /// file dialog and the disk.
    pub host: Option<Arc<dyn host::Host>>,
    /// The host's own renderer on the same device, to draw with instead of making one: its
    /// memory budget, pipeline cache and limits are the host's. The editor only adds the 3D
    /// renderer when it has none. None makes one as a standalone editor does.
    pub renderer: Option<Arc<Renderer>>,
}
/// What the editor keeps about being a guest.
struct Guest {
    look: Arc<egui::Style>,
    /// The pass it was made in: its fonts are there from the next one.
    made: u64,
    /// The last pass it was drawn in.
    drawn: u64,
    keyboard: bool,
}
/// Veylvik's editor: viewer, node graph, inspector, timeline, library and tools, drawn
/// into whatever area it is given each frame.
pub struct Editor {
    studio: Studio,
}
impl Editor {
    /// For now the editor's fonts and look are set on the whole egui context.
    pub fn new(setup: Setup) -> Result<Self, Box<dyn std::error::Error>> {
        if setup.interface != HOST_INTERFACE {
            return Err(format!(
                "this host speaks version {} of Veylvik's host interface and this editor version {HOST_INTERFACE}",
                setup.interface
            )
            .into());
        }
        Ok(Self {
            studio: Studio::create(setup)?,
        })
    }
    /// Draw the editor into `ui`, filling it, and do one frame of its work.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let Some(guest) = self.studio.guest.as_mut() else {
            return self.studio.editor_ui(ui);
        };
        let ctx = ui.ctx().clone();
        let pass = ctx.cumulative_pass_nr();
        if pass <= guest.made {
            // Its fonts arrive with the next pass.
            ctx.request_repaint();
            return;
        }
        // A host that sets its own fonts drops the families the editor added: add them
        // again and draw from the next pass, when they are there (egui stops on a family
        // it does not know).
        if !design::guest_fonts_loaded(&ctx) {
            design::install_fonts(&ctx);
            guest.made = pass;
            ctx.request_repaint();
            return;
        }
        // Whose the keyboard is: the editor's when it comes back into view, and from then
        // on whoever was last pressed on. A press on a floating window or a menu changes
        // nothing: it belongs to whoever opened it.
        let area = ui.max_rect();
        if guest.drawn + 1 != pass {
            guest.keyboard = true;
        }
        guest.drawn = pass;
        if let Some(at) = ctx.input(|i| {
            i.pointer
                .any_pressed()
                .then(|| i.pointer.interact_pos())
                .flatten()
        }) && ctx
            .layer_id_at(at)
            .is_none_or(|layer| layer.order == egui::Order::Background)
        {
            guest.keyboard = area.contains(at);
        }
        design::set_area(&ctx, Some(area));
        // While the keyboard is the host's, the editor sees no keys, typing or pasting;
        // the host gets them back untouched.
        let withheld = (!guest.keyboard).then(|| {
            ctx.input_mut(|input| {
                let (theirs, rest) =
                    std::mem::take(&mut input.events)
                        .into_iter()
                        .partition(|event| {
                            matches!(
                                event,
                                egui::Event::Key { .. }
                                    | egui::Event::Text(_)
                                    | egui::Event::Paste(_)
                                    | egui::Event::Copy
                                    | egui::Event::Cut
                                    | egui::Event::Ime(_)
                            )
                        });
                input.events = rest;
                theirs
            })
        });
        // The editor's look is the context's only while the editor draws: its panels,
        // windows, menus and tooltips take it, and the host's own are left as they were.
        let hosts = ctx.global_style();
        ctx.set_global_style(guest.look.clone());
        ui.scope(|ui| {
            ui.set_style(ctx.global_style());
            self.studio.editor_ui(ui);
        });
        if let Some(guest) = self.studio.guest.as_mut() {
            guest.look = ctx.global_style();
        }
        ctx.set_global_style(hosts);
        if let Some(theirs) = withheld {
            let theirs: Vec<egui::Event> = theirs;
            ctx.input_mut(|input| input.events.extend(theirs));
        }
    }
    /// As a guest: whether keys, typing and pasting go to the editor (it was the last
    /// thing pressed on, or has just come into view). Always true in a window of its own.
    pub fn has_keyboard(&self) -> bool {
        self.studio
            .guest
            .as_ref()
            .is_none_or(|guest| guest.keyboard)
    }
    /// Give the keyboard to the editor, or take it for the host.
    pub fn set_keyboard(&mut self, editors: bool) {
        if let Some(guest) = self.studio.guest.as_mut() {
            guest.keyboard = editors;
        }
    }
    /// The composition as it stands, and the number that grows with every change to it.
    pub fn composition(&self) -> &veylvik_document::Composition {
        self.studio.project.document.snapshot()
    }
    pub fn revision(&self) -> u64 {
        self.studio.project.document.revision()
    }
    /// Replace what is being edited with another composition of the host's (another clip's,
    /// say). Undo history starts again.
    pub fn set_composition(
        &mut self,
        ctx: &egui::Context,
        composition: veylvik_document::Composition,
        assets: PathBuf,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.studio
            .open_project(Project::hosted(composition, assets)?, ctx);
        Ok(())
    }
    /// The names of the steps that can be undone (oldest first) and redone (nearest
    /// first), for a host that shows one undo list across its pages.
    pub fn steps(&self) -> (Vec<String>, Vec<String>) {
        self.studio.project.document.steps()
    }
    /// Name the composition, as a host names it (a clip's name): the editor's title shows
    /// it and exported files begin with it. `None` goes back to the project file's name.
    /// A new composition ([`Editor::set_composition`]) keeps the name: give each its own.
    pub fn set_name(&mut self, name: Option<String>) {
        self.studio.host_name = name.filter(|name| !name.trim().is_empty());
    }
    /// Whether the "unavailable" button was pressed since the last call: inside a host the
    /// editor does not relink media itself, so the host opens its own relink.
    pub fn take_relink_request(&mut self) -> bool {
        std::mem::take(&mut self.studio.relink_asked)
    }
    /// Use the host's settings (every Veylvik setting; see `preferences::Values`): they
    /// take effect at once, and the editor never reads or writes Studio's own file inside
    /// a host. Refused when a value is out of range.
    pub fn set_preferences(
        &mut self,
        ctx: &egui::Context,
        values: preferences::Values,
    ) -> Result<(), String> {
        values.check()?;
        let before = self.studio.preferences.values.clone();
        self.studio.preferences.values = values.clone();
        self.studio.reported_preferences = Some(values);
        // What the editor took from the settings once is taken again.
        self.studio.recalled = false;
        self.studio.commands.keys = None;
        self.studio.apply_preferences(Some(&before), ctx);
        Ok(())
    }
    /// The settings now, as the host keeps them.
    pub fn preferences(&self) -> preferences::Values {
        self.studio.preferences.values.clone()
    }
    /// The settings when the artist changed one inside the editor since the last call (a
    /// saved layout, viewer aids, shortcuts, preview quality…), for the host to keep.
    pub fn take_preferences_change(&mut self) -> Option<preferences::Values> {
        let now = &self.studio.preferences.values;
        if self.studio.reported_preferences.as_ref() == Some(now) {
            return None;
        }
        self.studio.reported_preferences = Some(now.clone());
        Some(now.clone())
    }
    /// Draw the viewer with the host's own display (its working space, tone map,
    /// transfer and viewer LUT) instead of Veylvik's clip, filmic or OCIO view, so a
    /// picture looks the same on the host's pages; played pictures are kept with it too.
    /// The single-channel, alpha and backdrop views stay the editor's. Give a new one
    /// whenever the host's display changes; `None` goes back to Veylvik's.
    pub fn set_host_display(&mut self, display: Option<Arc<dyn host::HostDisplay>>) {
        if let Some(scheduler) = &self.studio.scheduler {
            scheduler.playback_cache().set_host_display(
                display
                    .clone()
                    .map(|d| d as Arc<dyn veylvik_preview::screen::HostEncode>),
            );
        }
        self.studio.display.set_host(display.clone());
        self.studio.host_display = display;
    }
    /// Whether the artist asked for Preferences (the File menu or Ctrl+,) since the last
    /// call: inside a host the editor shows no settings of its own, so the host opens its
    /// own at Veylvik's.
    pub fn take_preferences_request(&mut self) -> bool {
        std::mem::take(&mut self.studio.preferences_asked)
    }
    /// Open the editor's keyboard shortcuts (see `preferences::Panel::take_want_shortcuts`).
    pub fn open_shortcuts(&mut self) {
        self.studio.commands.editor = true;
    }
    /// Move the playhead to `time` (composition time) and stop playing, as a host does
    /// when its page opens or another clip is picked; false while an edit is mid-gesture.
    /// The move is not reported back by [`Editor::take_time_change`].
    pub fn set_time(&mut self, time: veylvik_document::Time) -> bool {
        self.studio.host_set_time(time)
    }
    /// Where the playhead is, in composition time.
    pub fn time(&self) -> veylvik_document::Time {
        self.studio.host_time()
    }
    /// Where the playhead went since the last call, when it moved by the artist's hand
    /// or by playing (never by [`Editor::set_time`]).
    pub fn take_time_change(&mut self) -> Option<veylvik_document::Time> {
        self.studio.host_take_time_change()
    }
    /// The working range, `start` up to `end` (not included), in composition time: a
    /// clip's used part. Playing and looping stay inside it; the frames outside stay
    /// reachable as handles, shown dimmed. `None` uses the whole composition.
    pub fn set_range(&mut self, range: Option<(veylvik_document::Time, veylvik_document::Time)>) {
        self.studio.host_set_range(range);
    }
    /// The working range, from its first frame up to the frame after its last; `None`
    /// when the whole composition is used.
    pub fn range(&self) -> Option<(veylvik_document::Time, veylvik_document::Time)> {
        self.studio.host_range()
    }
    /// Keep the playback store's pictures in `folder` (a host's own cache folder, so its
    /// clear-cache removes them), or in Veylvik's usual place with `None`. Pictures kept
    /// in the place before stay there; the store starts again in the new one. Call it
    /// right after [`Editor::new`] so nothing is kept elsewhere first.
    pub fn set_playback_folder(&mut self, folder: Option<PathBuf>) {
        self.studio.playback_folder = folder;
        self.studio.configure_playback();
    }
    /// The composition's media that could not be had, by name, with the reason: for a
    /// host to show which clips are missing.
    pub fn unavailable_media(&self) -> std::collections::BTreeMap<String, String> {
        self.studio.project.unavailable_assets()
    }
    /// Every revision made since the last call, oldest first: a new step (Edit), or the
    /// editor's own undo and redo moving over its steps. For a host that keeps one list
    /// of steps across its pages; at most the last 256 are kept between calls, and a new
    /// composition (`set_composition`) starts afresh.
    pub fn take_changes(&mut self) -> Vec<veylvik_document::Change> {
        self.studio.project.document.take_changes()
    }
    /// Undo or redo one step; false when there is none or the editor is mid-gesture.
    pub fn undo(&mut self) -> bool {
        self.step(false)
    }
    pub fn redo(&mut self) -> bool {
        self.step(true)
    }
    fn step(&mut self, redo: bool) -> bool {
        let before = self.revision();
        self.studio.history(redo);
        self.revision() != before
    }
    /// The renderer the editor draws with, for a [`ClipSource`] that shares its memory
    /// budget and caches.
    pub fn renderer(&self) -> Arc<Renderer> {
        self.studio.renderer.clone()
    }
    /// Whether there is work that has not been saved.
    pub fn unsaved(&self) -> bool {
        self.studio.project.dirty() || self.studio.draft_dirty
    }
    /// Call once when the host is closing: finishes background work and lets go of the GPU.
    pub fn close(&mut self) {
        self.studio.shut_down();
    }
}
impl Studio {
    fn create(setup: Setup) -> Result<Self, Box<dyn std::error::Error>> {
        // Inside a host the settings are the host's: Studio's file is left alone.
        let preferences = if setup.capabilities.preferences {
            preferences::Preferences::new()
        } else {
            preferences::Preferences::hosted()
        };
        // Preferences may switch live control off; an explicit directory still asks for it.
        let (no_control, control_directory) = match &setup.live_control {
            LiveControl::Off => (true, None),
            LiveControl::Usual => (!preferences.values.live_control, None),
            LiveControl::Directory(directory) => (false, Some(directory.clone())),
        };
        let render_limits = veylvik_io::runtime::RenderLimits::from_env_with_default(
            (!preferences.values.gpu_budget_auto).then_some(preferences.values.gpu_budget_mib),
        )?;
        // The host answers for media before the first project asks for any.
        files::set_host(setup.host.clone());
        let project = match setup.opens {
            Opens::File(path) => Project::open(path.as_deref())?,
            Opens::Composition {
                composition,
                assets,
            } => Project::hosted(*composition, assets)?,
        };
        // A project file given at start that reads files outside its folder: ask first.
        let trust_prompt = !project.outside.is_empty();
        let capabilities = setup.capabilities;
        // A guest keeps its look to itself: the style it makes here is its own, put in
        // place only while it draws, and the host's is put back at once.
        let guest = !setup.capabilities.own_window;
        design::set_guest(&setup.context, guest);
        design::install_fonts(&setup.context);
        let hosts = setup.context.global_style();
        design::apply(&setup.context);
        let guest_look = guest.then(|| {
            let ours = setup.context.global_style();
            setup.context.set_global_style(hosts);
            (ours, setup.context.cumulative_pass_nr())
        });
        let renderer = match setup.renderer.clone() {
            // The host's renderer: one set of caches and one budget for both.
            Some(theirs) => theirs,
            None => {
                let mut renderer = Renderer::from_device(
                    setup.device.clone(),
                    setup.queue.clone(),
                    setup.adapter.clone(),
                )?;
                render_limits.apply(&mut renderer);
                // Compiled programs kept between runs: in a host, only when it names a folder.
                if setup.capabilities.own_window
                    && let Some(folder) = veylvik_render::default_pipeline_folder()
                {
                    let _ = renderer.use_pipeline_cache(&folder);
                }
                Arc::new(renderer)
            }
        };
        renderer.ensure_scene_renderer(|| {
            Arc::new(veylvik_render3d::Engine::with_cache(
                renderer.device(),
                renderer.pipeline_cache().as_ref(),
            ))
        });
        renderer.ensure_effect_host(|| Arc::new(veylvik_io::ofx::Runner::new()));
        let context = setup.context.clone();
        let scheduler = GpuPreviewScheduler::with_renderer(renderer.clone(), move || {
            context.request_repaint()
        })?;
        let display = Display::new(renderer.device(), setup.target_format);
        let draft = project.document.snapshot().nodes[&project.selected].clone();
        let context = setup.context.clone();
        let bind = |socket: &std::path::Path| {
            veylvik_control::ipc::Server::bind(socket, control::catalog(), move || {
                context.request_repaint()
            })
        };
        // An explicitly requested endpoint must work; the default one never blocks the editor.
        let (control, control_error) = if no_control {
            (None, None)
        } else if let Some(directory) = control_directory {
            (
                Some(bind(&directory.join(veylvik_control::ipc::SOCKET_NAME))?),
                None,
            )
        } else {
            match veylvik_control::ipc::default_socket().and_then(|socket| bind(&socket)) {
                Ok(server) => (Some(server), None),
                Err(error) => {
                    eprintln!("Veylvik: live control unavailable: {error}");
                    (None, Some(error.to_string()))
                }
            }
        };
        let recovery =
            recovery::Recovery::new(recovery::default_store()?, &project, setup.context.clone());
        let mut app = Studio {
            playback_controls: playback::Playback::default(),
            preferences,
            system_theme: theme::System::default(),
            control,
            control_error,
            picking: None,
            preview_images: None,
            recovery,
            project,
            renderer,
            scheduler: Some(scheduler),
            display,
            view: ViewSettings::default(),
            navigation: viewer::Navigation::default(),
            comparison: Default::default(),
            node_editor_hidden: false,
            scopes: scopes::Scopes::new(&setup.device, &setup.queue, setup.target_format),
            displayed: None,
            displayed_ticket: None,
            wanted: None,
            preview_pending: false,
            preview_started: None,
            displayed_time: None,
            pictures_shown: 0,
            fill: Default::default(),
            draft,
            draft_dirty: false,
            draft_preview_deadline: None,
            text_edits: Default::default(),
            draft_commit_deadline: None,
            fonts: font_browser::Browser::new(&setup.context),
            text_viewer: Default::default(),
            path_pen: Default::default(),
            occlusion: Default::default(),
            path_transform: Default::default(),
            title_styles: Default::default(),
            graph: graph::Graph::default(),
            effects: effects::Controls::default(),
            library: preset_library::Library::default(),
            browser: asset_browser::Browser::default(),
            inspector_visible: true,
            viewer_fullscreen: false,
            window_was_windowed: false,
            node_palette: node_palette::Controls::default(),
            node_finder: node_finder::Controls::default(),
            key_timeline: key_timeline::Keys::default(),
            animation_editor: animation_editor::Editor::default(),
            status: String::new(),
            last_render_ms: None,
            error: false,
            playing: false,
            scene_view: None,
            scene_reflections: false,
            playback: Instant::now(),
            playback_frame: 0,
            buffering: None,
            waiting: None,
            play_now: false,
            playing_behind: false,
            trust_prompt,
            playback_folder: None,
            host_name: None,
            relink_asked: false,
            reported_frame: 0,
            host_range: false,
            reported_preferences: None,
            preferences_asked: false,
            menu_runs: Vec::new(),
            published_open: false,
            host_display: None,
            viewer_canvas: None,
            files: Tasks::new(setup.context.clone()),
            export_controls: sequence::Controls::default(),
            video_export: video_export::Controls::default(),
            render_queue: render_queue::Queue::new(),
            portable: Default::default(),
            tracking: tracking::Controls::default(),
            planar: planar::Controls::default(),
            solve: solve_camera::Controls::default(),
            flock_baking: flock_bake::Baking::default(),
            live_job: None,
            start_queued: false,
            audio: Default::default(),
            waveform: Default::default(),
            layer_bars: Default::default(),
            thumbnails: Default::default(),
            commands: Default::default(),
            log: Default::default(),
            host_link: Default::default(),
            capabilities,
            guest: guest_look.map(|(look, made)| Guest {
                look,
                made,
                drawn: 0,
                keyboard: true,
            }),
            tools: Default::default(),
            layouts: Default::default(),
            recalled: false,
            overlay_hidden: None,
            tryout: None,
            keys_accepted: None,
            materials: Default::default(),
            shot_light: Default::default(),
            subject: subject::Controls::default(),
            models: models_window::ModelsWindow::default(),
            outline_controls: text_outline::Controls::default(),
            composition_controls: composition::Controls::default(),
            pages: Default::default(),
            start: start::Start::load(false),
            dropped: Default::default(),
            overlays: Default::default(),
            imported_files: 0,
            media_import: None,
            confirm: None,
            after_save: None,
            allow_close: false,
            about: about::Controls::default(),
            statistics: false,
            theme_checked: Instant::now() - Duration::from_secs(10),
            accent: egui::Color32::from_rgb(255, 214, 10),
        };
        // Opened without a file: begin on the start screen.
        app.apply_preferences(None, &setup.context);
        if app.project.path.is_none() && app.capabilities.project_files {
            app.launch(&setup.context);
        }
        app.request_preview();
        Ok(app)
    }
}
#[derive(Clone, Copy)]
enum Leave {
    /// A choice made on the start screen (`Start::chosen`).
    Start,
    Open,
    New,
    NewFromMedia,
    ImportNuke,
    Recover(veylvik_document::NodeId),
    Example(&'static str),
    Close,
}
/// Height the viewer keeps above the node and animation editor.
const VIEWER_ROOM: f32 = 220.0;
/// Below this width the editor panel draws nothing.
const EDITOR_NARROWEST: f32 = 160.0;
struct Studio {
    playback_controls: playback::Playback,
    preferences: preferences::Preferences,
    system_theme: theme::System,
    recovery: recovery::Recovery,
    control: Option<veylvik_control::ipc::Server>,
    /// Why the default live-control socket could not be opened, shown on the menu bar.
    control_error: Option<String>,
    /// Eyedropper waiting for a click or drag in the viewer.
    picking: Option<eyedropper::Picking>,
    preview_images: Option<preview_images::Worker>,
    project: Project,
    renderer: Arc<Renderer>,
    scheduler: Option<GpuPreviewScheduler>,
    display: Display,
    view: ViewSettings,
    navigation: viewer::Navigation,
    comparison: comparison::Comparison,
    scopes: scopes::Scopes,
    node_editor_hidden: bool,
    displayed: Option<Arc<DisplayFrame>>,
    displayed_ticket: Option<PreviewTicket>,
    wanted: Option<PreviewTicket>,
    preview_pending: bool,
    /// When the pending preview was submitted; the status bar names slow renders only.
    preview_started: Option<Instant>,
    displayed_time: Option<veylvik_document::Time>,
    /// Finished previews put in the viewer since the start: how smoothly it plays is
    /// this over time, whoever is asking and however often.
    pictures_shown: u64,
    fill: fill::Fill,
    draft: Node,
    draft_dirty: bool,
    draft_preview_deadline: Option<Instant>,
    draft_commit_deadline: Option<Instant>,
    text_edits: std::collections::BTreeMap<veylvik_document::NodeId, Node>,
    fonts: font_browser::Browser,
    text_viewer: text_viewer::Editor,
    path_pen: path_pen::Pen,
    occlusion: occlusion::Controls,
    path_transform: path_transform::Editor,
    title_styles: title_styles::Library,
    graph: graph::Graph,
    effects: effects::Controls,
    library: preset_library::Library,
    browser: asset_browser::Browser,
    inspector_visible: bool,
    /// The viewer alone fills the screen: panels and 3D guides are hidden (F11).
    viewer_fullscreen: bool,
    /// The window was not full screen when the viewer took it over, so it goes back.
    window_was_windowed: bool,
    node_palette: node_palette::Controls,
    node_finder: node_finder::Controls,
    key_timeline: key_timeline::Keys,
    animation_editor: animation_editor::Editor,
    status: String,
    last_render_ms: Option<f64>,
    error: bool,
    playing: bool,
    /// The free 3D view the preview films with (see `viewport3d`).
    scene_view: Option<veylvik_scene::Camera>,
    /// The 3D viewer shows where each light's reflection lands instead of the picture.
    scene_reflections: bool,
    playback: Instant,
    playback_frame: i64,
    /// Play was pressed and the frames ahead are still being prepared: the playhead
    /// holds, and since when nothing more could be prepared (to give up waiting).
    buffering: Option<Option<Instant>>,
    /// What the viewer says while playing waits, when it waits.
    waiting: Option<fill::Waiting>,
    /// For this play only: play at once, skipping frames, as the artist asked.
    play_now: bool,
    /// Playing shows every frame and has fallen behind real time: the sound waits.
    playing_behind: bool,
    /// The opened project reads files outside its folder: ask whether to allow them.
    trust_prompt: bool,
    /// Where a host wants the playback store's pictures (its own cache folder).
    playback_folder: Option<PathBuf>,
    /// The name a host gives its composition (a clip's name): the title and the stem of
    /// exported files, in place of the project file's name.
    host_name: Option<String>,
    /// The "unavailable" button was pressed inside a host, which relinks its own media.
    relink_asked: bool,
    /// The playhead's frame the host last heard of (see host_time.rs).
    reported_frame: i64,
    /// The working range is the host's (a clip's used part): frames outside are dimmed.
    host_range: bool,
    /// Preferences were asked for inside a host, which shows its own.
    preferences_asked: bool,
    /// Menu items a host asked to run (`Editor::run_menu_item`), run as the frame begins.
    menu_runs: Vec<String>,
    /// The window of settings shown on a host's Edit page is open.
    published_open: bool,
    /// The host's display, drawing the viewer as the host's own viewer draws.
    host_display: Option<Arc<dyn host::HostDisplay>>,
    /// The settings the host last gave or heard of (see `Editor::take_preferences_change`).
    reported_preferences: Option<preferences::Values>,
    /// Where the viewer's picture was last drawn.
    viewer_canvas: Option<egui::Rect>,
    files: Tasks,
    export_controls: sequence::Controls,
    video_export: video_export::Controls,
    render_queue: render_queue::Queue,
    portable: portable::Controls,
    tracking: tracking::Controls,
    planar: planar::Controls,
    solve: solve_camera::Controls,
    /// A flock being baked.
    flock_baking: flock_bake::Baking,
    live_job: Option<live_jobs::LiveJob>,
    /// A render queued over live MCP starts as soon as it is prepared.
    start_queued: bool,
    audio: audio_preview::AudioPreview,
    waveform: waveform::Waveform,
    layer_bars: layer_bars::LayerBars,
    thumbnails: node_thumbnails::Thumbnails,
    commands: command_ui::Commands,
    log: history_log::Log,
    host_link: host_link::HostLink,
    /// What the host lets the editor do.
    capabilities: Capabilities,
    /// As a guest in a host's window: the editor's own look and whether the keyboard is its.
    guest: Option<Guest>,
    tools: inspector_tools::Tools,
    layouts: layouts::Layouts,
    /// Live switches have been set from preferences once.
    recalled: bool,
    /// The node whose frame and handles in the viewer were put away by a click beside it.
    overlay_hidden: Option<veylvik_document::NodeId>,
    /// The frame of a pending try-out on an animated setting (automatic keyframes off).
    tryout: Option<i64>,
    /// Keys in the pending edit that the artist asked for or already had.
    keys_accepted: Option<usize>,
    materials: material_library::Library,
    shot_light: shot_light::Pending,
    subject: subject::Controls,
    models: models_window::ModelsWindow,
    outline_controls: text_outline::Controls,
    composition_controls: composition::Controls,
    pages: pages::Pages,
    start: start::Start,
    /// Files dropped or pasted, waiting to be taken in, and how many of this batch were.
    dropped: std::collections::VecDeque<PathBuf>,
    overlays: viewer_overlays::Overlays,
    imported_files: usize,
    media_import: Option<media_import::Pending>,
    confirm: Option<Leave>,
    after_save: Option<Leave>,
    allow_close: bool,
    about: about::Controls,
    statistics: bool,
    theme_checked: Instant,
    accent: egui::Color32,
}
impl Studio {
    fn authoring_dialog_open(&self) -> bool {
        self.preferences.open
            || self.library.open
            || self.browser.choosing()
            || self.occlusion.open
            || self.effects.open
            || self.outline_controls.open
            || self.composition_controls.open
            || self.media_import.is_some()
            || self.node_palette.open
            || self.graph.renaming()
            || self.node_finder.open
    }
    fn open_project(&mut self, project: Project, ctx: &egui::Context) {
        self.start.open = false;
        self.trust_prompt = !project.outside.is_empty();
        self.portable = portable::Controls::default();
        let recovery = recovery::Recovery::new(self.recovery.store.clone(), &project, ctx.clone());
        self.recovery.finish(true);
        self.recovery = recovery;
        self.recovery.enabled = self.preferences.values.recovery_enabled;
        self.recovery.interval = Duration::from_secs(self.preferences.values.recovery_seconds);
        self.outline_controls = text_outline::Controls::default();
        self.node_palette = node_palette::Controls::default();
        self.node_finder = node_finder::Controls::default();
        self.key_timeline = key_timeline::Keys::default();
        self.animation_editor = animation_editor::Editor::default();
        self.composition_controls = composition::Controls::default();
        self.media_import = None;
        self.export_controls = sequence::Controls::default();
        self.video_export = video_export::Controls::default();
        if let Some(p) = &self.tracking.active {
            p.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.tracking = tracking::Controls::default();
        if let Some(p) = &self.planar.active {
            p.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.planar = planar::Controls::default();
        self.occlusion = Default::default();
        self.project = project;
        self.playback_controls = playback::Playback::default();
        self.playback_controls.resolution = self.preferences.values.preview_quality;
        if let Some(scheduler) = &self.scheduler {
            scheduler
                .playback_cache()
                .set_resolution(self.playback_controls.divisor());
            scheduler.playback_cache().clear();
        }
        self.graph = graph::Graph::default();
        self.graph
            .set_auto_arrange(self.preferences.values.auto_arrange);
        self.graph.show_thumbnails = self.preferences.values.node_thumbnails;
        self.effects = effects::Controls::default();
        self.library = preset_library::Library::default();
        self.navigation.fit();
        self.comparison = Default::default();
        self.scopes.clear();
        self.displayed = None;
        self.displayed_ticket = None;
        self.displayed_time = None;
        self.refresh_draft();
        self.request_preview();
    }
    /// What to show when started without a project, as preferences say.
    fn launch(&mut self, ctx: &egui::Context) {
        let p = &self.preferences.values;
        match p.launch {
            preferences::Launch::StartScreen => self.show_start(),
            preferences::Launch::LastProject => match self.start.latest() {
                Some(path) => self.open_path(Some(path)),
                None => self.show_start(),
            },
            preferences::Launch::Empty => {
                let (width, height, rate, frames) =
                    (p.new_width, p.new_height, p.new_rate, p.new_frames);
                match veylvik_document::Time::new(rate.0, rate.1)
                    .map_err(|e| e.to_string())
                    .and_then(|rate| {
                        Project::blank(width, height, rate, frames).map_err(|e| e.to_string())
                    }) {
                    Ok(project) => self.open_project(project, ctx),
                    Err(error) => self.fail(error),
                }
            }
        }
    }
    fn apply_composition_settings(&mut self, settings: composition::Settings, ctx: &egui::Context) {
        if settings.new {
            match Project::blank(
                settings.width,
                settings.height,
                settings.frame_rate,
                settings.frame_count,
            ) {
                Ok(project) => self.open_project(project, ctx),
                Err(error) => {
                    self.composition_controls.open = true;
                    self.fail(error);
                }
            }
        } else {
            let doc = self.project.document.snapshot();
            if doc.width == settings.width
                && doc.height == settings.height
                && doc.frame_rate.ticks == settings.frame_rate.ticks
                && doc.frame_rate.rate == settings.frame_rate.rate
                && doc.frame_count == settings.frame_count
            {
                return;
            }
            let commands = match settings.commands(self.project.document.snapshot()) {
                Ok(commands) => commands,
                Err(error) => {
                    self.fail(error);
                    return;
                }
            };
            match self.project.apply(&commands) {
                Ok(()) => {
                    self.refresh_draft();
                    self.request_preview();
                }
                Err(error) => {
                    self.composition_controls.open = true;
                    self.fail(error);
                }
            }
        }
    }

    /// The viewer's settings plus the composition's display transform.
    fn effective_view(&self) -> display::ViewSettings {
        display::ViewSettings {
            filmic: self.project.document.snapshot().ocio.view.is_none()
                && !self.project.document.snapshot().display.is_standard(),
            backdrop: self.preferences.values.backdrop as u8,
            host: self
                .host_display
                .as_ref()
                .map_or(0, |display| display.identity()),
            ..self.view
        }
    }
    fn fail(&mut self, error: impl ToString) {
        self.status = error.to_string();
        self.error = true;
    }
    fn refresh_draft(&mut self) {
        self.path_transform.reset_gesture();
        self.path_pen.cancel();
        self.draft_preview_deadline = None;
        self.draft_commit_deadline = None;
        self.text_edits.clear();
        self.effects.edits.clear();
        self.effects.connections.clear();
        self.draft = self.project.document.snapshot().nodes[&self.project.selected].clone();
        self.draft_dirty = false;
    }
    fn can_change_selection(&mut self) -> bool {
        // A try-out does not hold the artist back: moving on drops it.
        if self.drop_tryout() {
            return true;
        }
        if matches!(
            self.draft.operation,
            Operation::Text(veylvik_document::nodes::Text { .. })
        ) {
            return self.apply_draft();
        }
        if self.draft_dirty {
            self.fail("Apply or revert the current parameter changes first.");
            false
        } else {
            true
        }
    }
    fn open_node_finder(&mut self) {
        if self.files.busy()
            || self.authoring_dialog_open()
            || !self.can_change_selection()
            || self.graph.dragging()
            || self.animation_editor.dragging()
        {
            return;
        }
        self.playing = false;
        self.node_finder
            .open(self.project.document.snapshot(), self.project.selected);
    }
    fn open_node_palette(&mut self, ctx: &egui::Context) {
        if self.files.busy()
            || self.authoring_dialog_open()
            || !self.can_change_selection()
            || self.graph.dragging()
            || self.animation_editor.dragging()
        {
            return;
        }
        self.playing = false;
        self.node_palette
            .open_at(self.graph.insertion_position(ctx));
    }
    fn add_from_palette(
        &mut self,
        choice: node_palette::Choice,
        position: veylvik_document::NodePosition,
    ) {
        match choice {
            node_palette::Choice::Image => self.import_image(position),
            node_palette::Choice::Text => self.outline_controls.open_at(position),
            node_palette::Choice::Effect(recipe) => {
                let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                    let input = self.project.selected;
                    let mut effect = recipe.build(self.project.document.snapshot(), input)?;
                    self.graph.place_effect(
                        self.project.document.snapshot(),
                        self.project.document.revision(),
                        input,
                        &mut effect,
                        Some(position),
                    )?;
                    self.project.insert_effect(effect)?;
                    Ok(())
                })();
                match result {
                    Ok(()) => {
                        self.refresh_draft();
                        self.request_preview();
                    }
                    Err(error) => self.fail(error),
                }
            }
            node_palette::Choice::Node(kind) => {
                let doc = self.project.document.snapshot();
                let operation = node_palette::operation(kind, doc.width, doc.height);
                if matches!(
                    operation,
                    Operation::PlanarTrack(veylvik_document::nodes::PlanarTrack { .. })
                        | Operation::PlanarWarp(veylvik_document::nodes::PlanarWarp { .. })
                ) {
                    self.begin_planar();
                    return;
                }

                let added = if matches!(
                    operation,
                    Operation::Text(veylvik_document::nodes::Text { .. })
                ) {
                    let mut text = Node::new("Text", operation, &[]);
                    text.position = Some(position);
                    let id = text.id;
                    let mut merge = Node::new(
                        "Text over scene",
                        Operation::Merge(veylvik_document::nodes::Merge {}),
                        &[("background", doc.output), ("foreground", id)],
                    );
                    merge.position = Some(veylvik_document::NodePosition {
                        x: position.x + 240.0,
                        y: position.y,
                    });
                    let output = merge.id;
                    self.project
                        .apply(&[
                            Command::Put {
                                node: Box::new(text),
                            },
                            Command::Put {
                                node: Box::new(merge),
                            },
                            Command::Output { id: output },
                        ])
                        .map(|()| {
                            self.project.selected = id;
                            self.project.preview = None;
                            id
                        })
                } else {
                    self.project.add_at(operation, Some(position))
                };
                match added {
                    Ok(_) => {
                        self.refresh_draft();
                        self.request_preview();
                        // A new LUT starts by choosing its .cube file.
                        if matches!(self.draft.operation, Operation::Lut(_)) {
                            self.replace_image();
                        }
                        // A new 3D model starts by choosing its file.
                        if matches!(self.draft.operation, Operation::Model3d(_)) {
                            self.choose_model();
                        }
                    }
                    Err(error) => self.fail(error),
                }
            }
        }
    }
    /// While the viewer shows the selected node, it goes on showing whichever node is
    /// selected: picking another one in the graph changes the picture with it.
    fn follow_selection(&mut self, ctx: &egui::Context) {
        let memory = egui::Id::new("viewer-followed-selection");
        let before: Option<veylvik_document::NodeId> = ctx.data(|d| d.get_temp(memory));
        let now = self.project.selected;
        if before != Some(now) {
            ctx.data_mut(|d| d.insert_temp(memory, now));
            if let Some(before) = before
                && self.project.preview == Some(before)
            {
                self.project.preview = Some(now);
                self.request_preview();
            }
        }
    }
    fn duplicate_selected(&mut self, ctx: &egui::Context) {
        if !self.can_change_selection() || self.graph.dragging() || self.animation_editor.dragging()
        {
            return;
        }
        self.playing = false;
        if let Some(group) = self
            .graph
            .selected_group(self.project.document.snapshot(), self.project.selected)
        {
            self.duplicate_group(group.id);
            return;
        }
        match self
            .project
            .duplicate_at(self.graph.insertion_position(ctx))
        {
            Ok(_) => {
                self.refresh_draft();
                self.request_preview();
            }
            Err(e) => self.fail(e),
        }
    }
    fn duplicate_branch(&mut self) {
        if !self.can_change_selection() || self.graph.dragging() || self.animation_editor.dragging()
        {
            return;
        }
        self.playing = false;
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let mut copy = veylvik_document::editing::copy_branch(
                self.project.document.snapshot(),
                self.project.selected,
            )?;
            self.graph.place_branch_copy(
                self.project.document.snapshot(),
                self.project.document.revision(),
                &mut copy,
            )?;
            self.project.insert_branch(copy)?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.refresh_draft();
                self.request_preview();
            }
            Err(error) => self.fail(error),
        }
    }
    fn queue_draft_preview(&mut self, ctx: &egui::Context) {
        self.draft_commit_deadline = Some(
            Instant::now()
                + Duration::from_millis(u64::from(self.preferences.values.apply_delay_ms)),
        );
        let deadline = self
            .draft_preview_deadline
            .get_or_insert_with(|| Instant::now() + Duration::from_millis(32));
        ctx.request_repaint_after(deadline.saturating_duration_since(Instant::now()));
    }
    fn request_preview(&mut self) {
        self.playback_controls.warming = None;
        self.request_preview_inner();
    }
    fn request_preview_inner(&mut self) {
        if let Some(render) = self
            .project
            .preview
            .and_then(|id| viewport3d::scene_preview(self.project.document.snapshot(), id))
        {
            self.project.preview = Some(render);
        }
        self.wanted = None;
        self.preview_pending = false;
        let result = (|| {
            let output = self
                .project
                .preview
                .unwrap_or(self.project.document.snapshot().output);
            let plan = if let Some(commands) = self.key_timeline.preview_commands() {
                let doc = self
                    .project
                    .document
                    .prepare(self.project.document.revision(), commands)?;
                std::sync::Arc::new(veylvik_render::Plan::compile_output(&doc, output)?)
            } else if self.draft_dirty {
                let doc = self
                    .project
                    .document
                    .prepare(self.project.document.revision(), &self.draft_commands())?;
                std::sync::Arc::new(veylvik_render::Plan::compile_output(&doc, output)?)
            } else {
                self.project.plan()?
            };
            let scheduler = self
                .scheduler
                .as_ref()
                .ok_or("Preview worker unavailable")?;
            scheduler.cancel()?;
            // While playing or caching a range, frames may be kept and answered as the
            // screen shows them with the viewer's look; otherwise every frame is the
            // renderer's own.
            let view = self.effective_view();
            scheduler.playback_cache().set_playing(
                (self.playing || self.playback_controls.warming.is_some()).then_some(
                    veylvik_preview::screen::Look {
                        exposure: if view.exposure.is_finite() {
                            view.exposure.clamp(-16.0, 16.0)
                        } else {
                            0.0
                        },
                        filmic: view.filmic,
                        host: view.host,
                    },
                ),
            );
            Ok::<_, Box<dyn std::error::Error>>(scheduler.submit(
                self.project.document.revision(),
                plan,
                RenderRequest {
                    scene_view: self.scene_view.filter(|_| self.viewed_scene().is_some()),
                    scene_reflections: self.scene_reflections && self.viewed_scene().is_some(),
                    ..RenderRequest::full(self.project.time()?)
                },
                self.project.inputs.clone(),
            )?)
        })();
        match result {
            Ok(ticket) => {
                self.wanted = Some(ticket);
                if !self.preview_pending {
                    self.preview_started = Some(Instant::now());
                }
                self.preview_pending = true;
                // Per-frame renders during playback must not make the status line flicker.
                if !self.playing {
                    self.status = "Rendering…".into();
                }
                self.error = false;
            }
            Err(e) => {
                self.playing = false;
                self.fail(e);
            }
        }
    }
    fn draft_commands(&self) -> Vec<Command> {
        self.effects.commands().unwrap_or_else(|| {
            std::iter::once(&self.draft)
                .chain(self.text_edits.values())
                .map(|n| Command::Put {
                    node: Box::new(n.clone()),
                })
                .chain(self.spread_commands())
                .collect()
        })
    }
    fn apply_draft(&mut self) -> bool {
        if !self.draft_dirty {
            return true;
        }
        // A try-out is never applied by moving on; only by adding its keyframe.
        if self.drop_tryout() {
            return true;
        }
        let commands = self.draft_commands();
        match self.project.apply(&commands) {
            Ok(()) => {
                self.refresh_draft();
                self.request_preview();
                true
            }
            Err(e) => {
                self.fail(e);
                false
            }
        }
    }
    fn history(&mut self, redo: bool) {
        if self.graph.dragging() {
            self.fail("Finish the graph gesture first.");
            return;
        }
        if !self.can_change_selection() {
            return;
        }
        match self.project.undo(redo) {
            Ok(()) => {
                self.refresh_draft();
                self.request_preview();
            }
            Err(e) => self.fail(e),
        }
    }
    fn save_project(&mut self, save_as: bool) {
        if self.files.busy() || !self.apply_draft() {
            self.after_save = None;
            return;
        }
        self.playing = false;
        let snapshot = self.project.save_snapshot();
        let existing = if save_as {
            None
        } else {
            self.project.path.clone()
        };
        let folder = self
            .project
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .unwrap_or(&self.project.base)
            .to_path_buf();
        let name = self
            .project
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or("untitled.veylvik".into());
        let trusted = self.project.reach == veylvik_document::access::Reach::Anywhere;
        let result = self.files.start(move || {
            let path = if let Some(path) = existing {
                path
            } else {
                let Some(path) = files::select(Choice::Save, &folder, &name)? else {
                    return Ok(Completed::cancelled());
                };
                path
            };
            let saved = snapshot.save(&path).map_err(|e| e.to_string())?;
            // The artist's own work: trusted from now on. A project held to its folder stays
            // so under its new name; saving does not allow what it reads elsewhere.
            if trusted {
                crate::trust::trust(&path);
            }
            Ok(Completed::then(move |s, ctx| s.saved(saved, ctx)))
        });
        self.file_started(result);
    }
    fn file_started(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.status = "File operation in progress…".into();
                self.error = false;
            }
            Err(e) => {
                self.after_save = None;
                self.fail(e);
            }
        }
    }
    fn leave(&mut self, action: Leave, ctx: &egui::Context) {
        if matches!(action, Leave::Close) && self.render_queue.busy() {
            self.render_queue.open = true;
            self.render_queue.pause();
            self.fail("A queued render is active. Pause the queue and finish or cancel the current job before quitting.");
            return;
        }
        if self.authoring_dialog_open() {
            self.fail("Finish or close the current authoring dialog first.");
            return;
        }
        if self.files.busy() {
            self.fail("Finish or cancel the current file operation first.");
            return;
        }
        if self.project.dirty() || self.draft_dirty {
            self.confirm = Some(action);
        } else {
            self.leave_now(action, ctx);
        }
    }
    fn leave_now(&mut self, action: Leave, ctx: &egui::Context) {
        self.playing = false;
        match action {
            Leave::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Leave::Example(id) => match Project::example(id) {
                Ok(project) => self.open_project(project, ctx),
                Err(e) => self.fail(e),
            },
            Leave::NewFromMedia => self.new_from_media(),
            Leave::ImportNuke => self.import_nuke(),
            Leave::New => self
                .composition_controls
                .begin(self.project.document.snapshot(), true),
            Leave::Recover(id) => {
                let store = self.recovery.store.clone();
                let result = self.files.start(move || {
                    Project::recover(&store, id)
                        .map(|project| Completed::then(move |s, ctx| s.open_project(project, ctx)))
                        .map_err(|e| e.to_string())
                });
                self.file_started(result);
            }
            Leave::Open => self.open_path(None),
            Leave::Start => self.start_chosen(ctx),
        }
    }
    /// Opens `path`, or asks with the file picker when `None`, on the file worker.
    pub(crate) fn open_path(&mut self, path: Option<PathBuf>) {
        let folder = self.project.base.clone();
        let result = self.files.start(move || {
            let path = match path {
                Some(path) => path,
                None => {
                    let Some(path) = files::select(Choice::Open, &folder, "")? else {
                        return Ok(Completed::cancelled());
                    };
                    path
                }
            };
            Project::open(Some(&path))
                .map(|project| Completed::then(move |s, ctx| s.open_project(project, ctx)))
                .map_err(|e| e.to_string())
        });
        self.file_started(result);
    }
    /// Samples `region` for the eyedropper on the file worker. Nodes with an image input sample
    /// what feeds them (a chroma key picks the screen, not its keyed result); generators sample
    /// the viewed image.
    fn sample_colour(&mut self, picking: eyedropper::Picking, region: veylvik_render::Region) {
        if self.files.busy() {
            self.fail("Wait for the current file task before sampling a colour.");
            return;
        }
        let time = match self.project.time() {
            Ok(time) => time,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        let document = self.project.document.snapshot().clone();
        let source = document
            .nodes
            .get(&picking.node)
            .and_then(|node| node.inputs.get("image").copied())
            .unwrap_or(self.project.preview.unwrap_or(document.output));
        let (renderer, inputs, session) = (
            self.renderer.clone(),
            self.project.inputs.clone(),
            self.project.session,
        );
        self.status = "Sampling colour…".into();
        let result = self.files.start(move || {
            let plan = veylvik_render::Plan::compile_output(&document, source)
                .map_err(|e| e.to_string())?;
            let (rgb, pixels) = eyedropper::sample(&renderer, &plan, &inputs, time, region)?;
            let sample = eyedropper::Sampled {
                session,
                node: picking.node,
                field: picking.field,
                rgb,
                pixels,
            };
            Ok(Completed::then(move |s, _| s.picked(sample)))
        });
        self.file_started(result);
    }
    fn import_text_font(&mut self, selection: font_browser::Selection) {
        if self.files.busy() {
            return;
        }
        if !matches!(selection, font_browser::Selection::Add) && !self.apply_draft() {
            return;
        }
        let mut node = self.draft.clone();
        let session = self.project.session;
        let revision = self.project.document.revision();
        let folder = self.project.base.clone();
        let time = self
            .project
            .time()
            .unwrap_or(veylvik_document::Time { ticks: 0, rate: 1 });
        let result = self.files.start(move || {
            let face = match selection {
                font_browser::Selection::Face(face) => face,
                font_browser::Selection::File | font_browser::Selection::Add => {
                    let Some(path) = files::select(Choice::Font, &folder, "")? else {
                        return Ok(Completed::cancelled());
                    };
                    if matches!(selection, font_browser::Selection::Add) {
                        let name = font_browser::install(&path)?;
                        return Ok(Completed::then(move |s, ctx| s.font_added(name, ctx)));
                    }
                    font_browser::from_file(path)
                }
            };
            let font = font_browser::load(&face)?;
            if let Operation::Text(veylvik_document::nodes::Text { font: target, .. }) =
                &mut node.operation
            {
                **target = font;
            }
            veylvik_text::outline(&node.operation, time).map_err(|e| e.to_string())?;
            let node = Box::new(node);
            Ok(
                Completed::then(move |s, _| s.text_font(node)).unless_changed(
                    session,
                    revision,
                    "Composition changed while choosing font",
                ),
            )
        });
        self.file_started(result);
    }
    fn outline_text(&mut self, mut request: veylvik_io::text::OutlineRequest) {
        if self.files.busy() || !self.apply_draft() {
            self.outline_controls.open = true;
            return;
        }
        let document = self.project.document.snapshot();
        if document.width > veylvik_render::MAX_VECTOR_DIMENSION
            || document.height > veylvik_render::MAX_VECTOR_DIMENSION
        {
            self.outline_controls.open = true;
            self.fail(
                "Text outlines currently require a canvas no larger than 4096 pixels per axis.",
            );
            return;
        }
        self.playing = false;
        let folder = self.project.base.clone();
        let position = self.outline_controls.position;
        let result = self.files.start(move || {
            let Some(path) = files::select(Choice::Font, &folder, "")? else {
                return Ok(Completed::cancelled());
            };
            request.font = path.to_str().ok_or("Font path is not UTF-8")?.into();
            let outlined =
                veylvik_io::text::outline(&folder, &request).map_err(|e| e.to_string())?;
            let mut node = Node::new(
                &request.label(),
                Operation::Path(veylvik_document::nodes::Path {
                    geometry: outlined.geometry,
                }),
                &[],
            );
            node.position = position;
            let node = Box::new(node);
            Ok(Completed::then(move |s, _| s.outlined(node)))
        });
        self.outline_controls.pending = result.is_ok();
        if result.is_err() {
            self.outline_controls.open = true;
        }
        self.file_started(result);
    }
    fn reload_images(&mut self) {
        if self.files.busy()
            || !self.can_change_selection()
            || self.graph.dragging()
            || self.animation_editor.dragging()
        {
            return;
        }
        self.playing = false;
        let composition = self.project.document.snapshot().clone();
        if !composition.nodes.values().any(|node| {
            matches!(
                node.operation,
                Operation::Image(veylvik_document::nodes::Image { .. })
                    | Operation::Footage(veylvik_document::nodes::Footage { .. })
            )
        }) {
            self.status = "No image sources to reload".into();
            self.error = false;
            return;
        }
        let base = self.project.base.clone();
        let session = self.project.session;
        let revision = self.project.document.revision();
        let previous = self.project.inputs.clone();
        let result = self.files.start(move || {
            let inputs = project::load_inputs(&composition, &base, &previous)
                .map_err(|error| error.to_string())?;
            Ok(
                Completed::then(move |s, _| s.reloaded(inputs)).unless_changed(
                    session,
                    revision,
                    "Source reload became obsolete; reload again for the current project.",
                ),
            )
        });
        self.file_started(result);
    }
    fn replace_image(&mut self) {
        if self.files.busy()
            || !self.can_change_selection()
            || self.graph.dragging()
            || self.animation_editor.dragging()
        {
            return;
        }
        if !matches!(
            self.draft.operation,
            Operation::Image(veylvik_document::nodes::Image { .. })
                | Operation::Footage(veylvik_document::nodes::Footage { .. })
                | Operation::Lut(_)
        ) {
            return;
        }
        let choice = if matches!(self.draft.operation, Operation::Lut(_)) {
            Choice::Lut
        } else {
            Choice::Import
        };
        self.playing = false;
        let folder = self.project.base.clone();
        let session = self.project.session;
        let revision = self.project.document.revision();
        let node = self.project.selected;
        let inputs = self.project.inputs.clone();
        let result = self.files.start(move || {
            let Some(path) = files::select(choice, &folder, "")? else {
                return Ok(Completed::cancelled());
            };
            let path = path.canonicalize().map_err(|error| error.to_string())?;
            let asset = path.to_str().ok_or("Image path is not UTF-8")?;
            let replaced_bytes = inputs.source_budget(asset);
            let budget = veylvik_io::runtime::source_budget()
                .saturating_sub(inputs.byte_size() - replaced_bytes)
                .min(128 * 1024 * 1024);
            let image =
                veylvik_io::footage::load(&path, budget).map_err(|error| error.to_string())?;
            Ok(
                Completed::then(move |s, _| s.replaced(node, path, image)).unless_changed(
                    session,
                    revision,
                    "Image replacement became obsolete; choose the source again.",
                ),
            )
        });
        self.file_started(result);
    }
    fn import_image(&mut self, position: veylvik_document::NodePosition) {
        self.import_media(position, false);
    }
    fn import_media(&mut self, position: veylvik_document::NodePosition, sequence: bool) {
        self.import_media_from(position, sequence, None, None);
    }
    /// Imports `path`, or asks with the file picker when `None`. `matching` decides whether
    /// an empty composition adopts the media's settings; `None` asks with the dialog.
    pub(crate) fn import_media_from(
        &mut self,
        position: veylvik_document::NodePosition,
        sequence: bool,
        path: Option<PathBuf>,
        matching: Option<bool>,
    ) {
        if self.files.busy() || !self.apply_draft() {
            return;
        }
        self.playing = false;
        let folder = self.project.base.clone();
        let session = self.project.session;
        let revision = self.project.document.revision();
        let rate = self.project.document.snapshot().frame_rate;
        let frame = {
            let doc = self.project.document.snapshot();
            (doc.width, doc.height)
        };
        let budget = veylvik_io::runtime::source_budget()
            .saturating_sub(self.project.inputs.byte_size())
            .min(128 * 1024 * 1024);
        let result = self.files.start(move || {
            let path = match path {
                Some(path) => path,
                None => {
                    let choice = if sequence {
                        Choice::ImportSequence
                    } else {
                        Choice::Import
                    };
                    match files::pick(choice, &folder, "")? {
                        None => return Ok(Completed::cancelled()),
                        Some(files::Chosen::Path(path)) => path,
                        // The host's own media: no file of ours to read, its frames
                        // come from the host.
                        Some(files::Chosen::Media { key, source, .. }) => {
                            let (path, image) = (
                                PathBuf::from(key),
                                veylvik_io::footage::Media::Source(source),
                            );
                            return Ok(Completed::then(move |s, _| {
                                s.imported(path, image, position, matching)
                            })
                            .unless_changed(
                                session,
                                revision,
                                "Import became obsolete; import again for the current composition.",
                            ));
                        }
                    }
                }
            };
            let path = path.canonicalize().map_err(|e| e.to_string())?;
            if !sequence && veylvik_io::export_audio::is_audio(&path) {
                let probe = veylvik_io::export_audio::probe(&path).map_err(|e| e.to_string())?;
                if !probe.has_audio {
                    return Err(format!("{} has no sound", path.display()));
                }
                return Ok(Completed::then(move |s, _| s.sound_file(position, path))
                    .unless_changed(
                        session,
                        revision,
                        "Import became obsolete; import again for the current composition.",
                    ));
            }
            if !sequence && veylvik_io::svg::is_svg(&path) {
                let (shapes, skipped) = veylvik_io::svg::import(&path, frame.0, frame.1, 0.8)
                    .map_err(|e| e.to_string())?;
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Drawing".into());
                return Ok(
                    Completed::then(move |s, _| s.drawing(name, shapes, skipped)).unless_changed(
                        session,
                        revision,
                        "Import became obsolete; import again for the current composition.",
                    ),
                );
            }
            let (path, image) = if sequence {
                veylvik_io::footage::import_numbered(&path, rate, budget)
                    .map_err(|e| e.to_string())?
            } else {
                let image = veylvik_io::footage::load(&path, budget).map_err(|e| e.to_string())?;
                (path, image)
            };
            Ok(
                Completed::then(move |s, _| s.imported(path, image, position, matching))
                    .unless_changed(
                        session,
                        revision,
                        "Import became obsolete; import again for the current composition.",
                    ),
            )
        });
        self.file_started(result);
    }
    fn export_frame(&mut self) {
        if self.files.busy() || !self.apply_draft() {
            return;
        }
        let time = match self.project.time() {
            Ok(time) => time,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        self.queue_export(
            render_queue::Output::Frame {
                time,
                output: self
                    .project
                    .preview
                    .unwrap_or(self.project.document.snapshot().output),
            },
            format!("frame-{:06}.png", self.project.frame),
        );
    }
    fn export_video(
        &mut self,
        mut settings: veylvik_io::video_export::Settings,
        _: &egui::Context,
    ) {
        settings.audio.base = Some(self.project.base.clone());
        let name = format!("composition.{}", settings.codec.extension());
        self.queue_export(render_queue::Output::Video(Box::new(settings)), name);
    }
    fn export_sequence(&mut self, settings: sequence::Settings, _: &egui::Context) {
        self.queue_export(
            render_queue::Output::Sequence {
                frames: settings.frames,
                format: settings.format,
                output: self
                    .project
                    .preview
                    .unwrap_or(self.project.document.snapshot().output),
            },
            settings.name,
        );
    }
    fn queue_export(&mut self, mut output: render_queue::Output, filename: String) {
        if self.files.busy() || !self.apply_draft() {
            return;
        }
        let document = self.project.document.snapshot().clone();
        let base = self.project.base.clone();
        let name = self.file_stem();
        let reach = self.project.reach.clone();
        let result = self.files.start(move || {
            let Some(mut path) = files::select(output.choice(), &base, &filename)? else {
                return Ok(Completed::cancelled());
            };
            if matches!(output, render_queue::Output::Sequence { .. }) {
                path = path.join(filename);
            } else {
                // The save dialog asked before giving back a file that exists, and
                // the user agreed: the finished render may take its place.
                output.agree_to_replace(path.exists());
            }
            render_queue::Job::prepare(name, document, base, path, output, reach)
                .map(|job| Completed::then(move |s, _| s.queued(job)))
        });
        self.file_started(result);
    }
    fn retry_render(&mut self, mut job: render_queue::Job) {
        if self.files.busy() {
            self.fail("Finish the file selection first.");
            return;
        }
        let result = self.files.start(move || {
            let filename = job
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let folder = job.path.parent().unwrap_or(&job.base);
            let Some(mut path) = files::select(job.output.choice(), folder, &filename)? else {
                return Ok(Completed::cancelled());
            };
            if matches!(job.output, render_queue::Output::Sequence { .. }) {
                // A fresh subdirectory preserves partial frames from previous attempts.
                path = path.join(format!(
                    "{}-{}",
                    filename,
                    veylvik_document::NodeId::new_v4()
                ));
            }
            job.retry_to(path)?;
            Ok(Completed::then(move |s, _| s.queued(job)))
        });
        self.file_started(result);
    }
    fn poll_files(&mut self, ctx: &egui::Context) {
        let Some(result) = self.files.poll() else {
            return;
        };
        self.portable_poll_finished(result.as_ref().err().map(String::as_str));
        self.tracking.active = None;
        self.planar.active = None;
        self.subject.active = None;
        self.solve.active = None;
        self.flock_baking.active = None;
        if self.outline_controls.pending {
            // Open again unless the outline arrives (its step closes the window).
            self.outline_controls.pending = false;
            self.outline_controls.open = true;
        }
        match result {
            Ok(done) => done.finish(self, ctx),
            Err(e) => {
                self.after_save = None;
                self.fail(e);
            }
        }
    }
    fn theme(&mut self, ctx: &egui::Context) {
        let palette = self.system_theme.resolve(self.preferences.theme(), ctx);
        if palette != theme::palette(ctx)
            || self.theme_checked.elapsed() > Duration::from_secs(3600)
        {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("veylvik-palette"), palette));
            design::apply(ctx);
            self.theme_checked = Instant::now();
        }
        self.accent = palette.accent;
    }
    /// The composition's name as the title shows it: the host's name for it, else the
    /// project file's name.
    pub(crate) fn title_name(&self) -> String {
        self.host_name.clone().unwrap_or_else(|| {
            self.project
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map_or_else(|| "Untitled".into(), |s| s.to_string_lossy().into_owned())
        })
    }
    /// The name exported files begin with: the host's name made safe for a file name,
    /// else the project file's stem.
    pub(crate) fn file_stem(&self) -> String {
        if let Some(stem) = self.host_name.as_deref().and_then(safe_stem) {
            return stem;
        }
        self.project
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map_or_else(
                || "Untitled composition".into(),
                |s| s.to_string_lossy().into_owned(),
            )
    }
    /// The playback store's limits and folder, from the preferences; a host's own
    /// folder (`Editor::set_playback_folder`) comes before both of the others.
    fn configure_playback(&self) {
        let p = &self.preferences.values;
        if let Some(scheduler) = &self.scheduler {
            let directory = std::env::var_os("XDG_CACHE_HOME")
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache"))
                })
                .map(|p| p.join("veylvik/playback"));
            // A cache folder of the artist's choice, when it exists.
            let chosen = std::path::PathBuf::from(p.cache_folder.trim());
            let directory = if let Some(theirs) = &self.playback_folder {
                Some(theirs.clone())
            } else if !p.cache_folder.trim().is_empty() && chosen.is_dir() {
                Some(chosen.join("veylvik-playback"))
            } else {
                directory
            };
            scheduler
                .playback_cache()
                .configure(veylvik_preview::playback::Settings {
                    divisor: self.playback_controls.divisor(),
                    gpu_bytes: p.playback_gpu_mib * 1024 * 1024,
                    disk_bytes: p.playback_disk_mib * 1024 * 1024,
                    directory,
                    picture_memory_bytes: p.played_memory_mib * 1024 * 1024,
                    picture_disk_bytes: p.played_disk_mib * 1024 * 1024,
                    memory_reserve_bytes: p.played_reserve_mib * 1024 * 1024,
                });
        }
    }
    fn apply_preferences(&mut self, before: Option<&preferences::Values>, ctx: &egui::Context) {
        let p = &self.preferences.values;
        if before.is_none_or(|b| b.auto_arrange != p.auto_arrange) {
            self.graph.set_auto_arrange(p.auto_arrange);
        }
        if before.is_none_or(|b| b.fast_view != p.fast_view) {
            self.view.fast_filter = p.fast_view;
        }
        self.recovery.enabled = p.recovery_enabled;
        self.recovery.interval = Duration::from_secs(p.recovery_seconds);
        self.configure_playback();
        if !design::guest(ctx) {
            ctx.set_zoom_factor(p.scale);
        }
        if before.is_none() {
            // At start only: later it is the transport's own choice.
            self.playback_controls.resolution = p.preview_quality;
        }
        files::set_default_folders(&p.projects_folder, &p.exports_folder);
        self.composition_controls.defaults =
            veylvik_document::Time::new(p.new_rate.0, p.new_rate.1)
                .ok()
                .map(|rate| (p.new_width, p.new_height, rate, p.new_frames));
        ctx.data_mut(|d| {
            d.insert_temp(egui::Id::new("animate-library-previews"), p.hover_previews)
        });
        let cache_changed = before.is_some_and(|b| {
            b.playback_gpu_mib != p.playback_gpu_mib || b.playback_disk_mib != p.playback_disk_mib
        });
        self.theme(ctx);
        if cache_changed {
            self.request_preview();
        }
    }
    fn open_preferences(&mut self) {
        // A host keeps the settings: it opens its own, at Veylvik's (see
        // `Editor::take_preferences_request`).
        if !self.capabilities.preferences {
            self.preferences_asked = true;
            return;
        }
        self.playing = false;
        if !self.error {
            self.status = "Paused".into();
        }
        self.preferences.begin();
    }

    fn inspector_actions(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.set_min_height(28.);
            ui.heading("Inspector");
            if ui
                .add(icons::Button::new(icons::Icon::Right, "Collapse inspector"))
                .clicked()
            {
                self.inspector_visible = false;
            }

            if self.draft_dirty {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(self.draft_dirty, egui::Button::new("Apply"))
                        .on_hover_text("Commit this draft as one undo step (Ctrl+Enter)")
                        .clicked()
                    {
                        self.keep_tryout();
                    }
                    if ui
                        .add_enabled(self.draft_dirty, egui::Button::new("Revert"))
                        .on_hover_text("Discard unapplied inspector changes")
                        .clicked()
                    {
                        self.refresh_draft();
                        self.request_preview();
                    }
                });
            }
        });
        let texts: Vec<_> =
            veylvik_document::presets::background::reachable(self.project.document.snapshot())
                .into_iter()
                .filter_map(|id| {
                    let node = &self.project.document.snapshot().nodes[&id];
                    if let Operation::Text(veylvik_document::nodes::Text { content, .. }) =
                        &node.operation
                    {
                        Some((id, content.chars().take(28).collect::<String>()))
                    } else {
                        None
                    }
                })
                .collect();
        let mut select = None;
        if !texts.is_empty() {
            egui::ComboBox::from_id_salt("scene-text-selector")
                .selected_text(if matches!(self.draft.operation, Operation::Text(_)) {
                    self.draft.label.as_str()
                } else {
                    "Edit scene text…"
                })
                .show_ui(ui, |ui| {
                    for (id, label) in texts {
                        if ui
                            .selectable_label(
                                self.project.selected == id,
                                if label.is_empty() {
                                    "Empty text"
                                } else {
                                    &label
                                },
                            )
                            .clicked()
                        {
                            select = Some(id);
                        }
                    }
                });
        }
        if let Some(id) = select
            && self.can_change_selection()
        {
            self.project.selected = id;
            self.project.preview = None;
            if let Some(group) = self
                .project
                .document
                .snapshot()
                .groups
                .values()
                .find(|g| g.members.contains(&id))
            {
                self.graph.expand(group.id);
            }
            self.refresh_draft();
            self.request_preview();
        }
        ui.separator();
    }
    fn inspector(&mut self, ui: &mut egui::Ui) {
        if self.effect_inspector(ui) {
            return;
        }
        self.inspector_tools(ui);
        match self.draft.operation.descriptor().output {
            veylvik_document::registry::PortKind::Scene => {
                ui.weak("3D scene element").on_hover_text("Merge into a scene and film with Render scene. Its appearance responds to the scene camera.");
            }
            veylvik_document::registry::PortKind::Image => {
                ui.weak("2D image layer").on_hover_text("Merged after Render scene, this stays fixed on screen. To place it in 3D, connect it to an Image card, or use it as a Text 3D outline or material input.");
            }
            _ => {}
        }
        if !matches!(self.draft.operation, Operation::Text(_)) {
            ui.label(
                if matches!(
                    self.draft.operation,
                    Operation::Text(veylvik_document::nodes::Text { .. })
                ) {
                    "Layer name"
                } else {
                    self.draft.operation.descriptor().label
                },
            );
            ui.add_space(8.0);
            self.draft_dirty |= ui.text_edit_singleline(&mut self.draft.label).changed();
            ui.separator();
        }
        let time = self
            .project
            .time()
            .unwrap_or(veylvik_document::Time { ticks: 0, rate: 1 });
        let mut replace_image = false;
        let mut import_font = None;
        let mut text_changed = false;
        let text_canvas = [
            self.project.document.snapshot().width as f32,
            self.project.document.snapshot().height as f32,
        ];
        let unavailable = self.project.unavailable_assets();
        let flock_status = self.flock_bake_status();
        let lit_by = self
            .draft
            .inputs
            .get("lights")
            .and_then(|id| self.project.document.snapshot().nodes.get(id))
            .map(|n| n.label.clone());
        let focus_objects = scene_focus::objects(self.project.document.snapshot());
        let scene_things = scene_focus::things(self.project.document.snapshot());
        expressions::publish(ui.ctx(), self.project.document.snapshot());
        let mut cx = nodes::InspectCx {
            node_id: self.draft.id,
            time,
            frame_rate: self.project.document.snapshot().frame_rate,
            inputs: &self.project.inputs,
            unavailable: &unavailable,
            dirty: false,
            text_changed: false,
            replace_image: false,
            save_material: false,
            environment: self.draft.inputs.contains_key("environment"),
            textured: self.draft.inputs.contains_key("texture"),
            normal_mapped: self.draft.inputs.contains_key("normal_map"),
            reflect_shot: None,
            lit_by: lit_by.as_deref(),
            share_lights: None,
            match_shot: false,
            tracked: self.draft.inputs.contains_key("track"),
            preview_soon: false,
            error: None,
            picking: self
                .picking
                .as_ref()
                .filter(|p| p.node == self.draft.id)
                .map(|p| p.field),
            pick: None,
            objects: &focus_objects,
            things: &scene_things,
            pick_focus: false,
            surround_lights: None,
            plated: self.draft.inputs.contains_key("plate"),
            measure_grain: false,
            choose_model: false,
            outline: self.draft.inputs.get("outline").copied(),
            select: None,
            flock_bake: flock_status.as_deref(),
            bake_flock: None,
            choose_splats: false,
        };
        match &mut self.draft.operation {
            op @ Operation::Text(veylvik_document::nodes::Text { .. }) => {
                let (changed, import) =
                    editable_text::controls(ui, op, time, text_canvas, &mut self.fonts);
                self.draft_dirty |= changed;
                text_changed |= changed;
                import_font = import;
            }
            op => {
                veylvik_document::each_node!(op, node => nodes::Inspect::inspect(node, ui, &mut cx))
            }
        }
        let nodes::InspectCx {
            dirty,
            text_changed: media_changed,
            replace_image: replace,
            save_material,
            reflect_shot,
            share_lights,
            match_shot,
            preview_soon,
            error,
            pick,
            pick_focus,
            surround_lights,
            measure_grain,
            choose_model,
            select: select_other,
            bake_flock,
            choose_splats,
            ..
        } = cx;
        if choose_splats {
            self.choose_splats();
        }
        match bake_flock {
            Some(true) => self.bake_flock(ui.ctx()),
            Some(false) => self.unbake_flock(),
            None => {}
        }
        if let Some(id) = select_other
            && self.can_change_selection()
        {
            self.project.selected = id;
            self.project.preview = None;
            self.refresh_draft();
            return;
        }
        if let Some(warm) = surround_lights {
            if self.draft_dirty || dirty {
                self.status = "Apply or revert the inspector before adding lights".into();
            } else {
                match veylvik_presets::surround_lights(
                    self.project.document.snapshot(),
                    self.draft.id,
                    warm,
                ) {
                    Ok(rig) => match self.project.apply(&rig.commands) {
                        Ok(_) => {
                            self.refresh_draft();
                            self.request_preview();
                            self.status = "Added surround lighting".into();
                        }
                        Err(e) => self.status = e.to_string(),
                    },
                    Err(e) => self.status = e.to_string(),
                }
            }
            return;
        }
        if choose_model {
            self.choose_model();
        }
        if measure_grain {
            self.measure_grain(ui.ctx());
        }
        if pick_focus {
            scene_focus::set_picking(ui.ctx(), true);
        }
        if match_shot {
            self.match_shot(ui.ctx());
        }
        match share_lights {
            Some(true) => self.share_material_lights(),
            Some(false) => {
                // Keep the look: the surface takes over the rig's current lights.
                let rig = self
                    .draft
                    .inputs
                    .remove("lights")
                    .and_then(|id| self.project.document.snapshot().nodes.get(&id))
                    .and_then(|n| match &n.operation {
                        Operation::LightRig(rig) => Some(rig.studio.clone()),
                        _ => None,
                    });
                if let (Some(studio), Operation::Material(m)) = (rig, &mut self.draft.operation) {
                    m.studio = studio;
                }
                self.draft_dirty = true;
            }
            None => {}
        }
        match reflect_shot {
            Some(true) => {
                // The backdrop the material sits on becomes its surroundings.
                let doc = self.project.document.snapshot();
                match veylvik_document::presets::background::automatic(doc)
                    .filter(|id| *id != self.draft.id)
                {
                    Some(backdrop) => {
                        self.draft.inputs.insert("environment".into(), backdrop);
                        self.draft_dirty = true;
                    }
                    None => self.fail(
                        "No single backdrop found. Pick the shot in the Environment input below.",
                    ),
                }
            }
            Some(false) => {
                self.draft.inputs.remove("environment");
                self.draft_dirty = true;
            }
            None => {}
        }
        if save_material && let Operation::Material(material) = &self.draft.operation {
            let label = self.draft.label.trim_start_matches("Text · ").to_owned();
            self.materials.save(&label, material);
        }
        if let Some(field) = pick {
            // A second press on the active field cancels, like Esc.
            let active = self
                .picking
                .as_ref()
                .is_some_and(|p| p.node == self.draft.id && p.field == field);
            self.picking = (!active).then(|| eyedropper::Picking::new(self.draft.id, field));
            self.status = if active {
                "Eyedropper cancelled".into()
            } else {
                "Eyedropper: click or drag in the viewer to sample a colour · Esc cancels".into()
            };
            self.error = false;
        }
        self.draft_dirty |= dirty;
        text_changed |= media_changed;
        if matches!(self.draft.operation, Operation::Text(_)) && editable_text::overview(ui) {
            let changed = self.text_appearance_overview(ui);
            self.draft_dirty |= changed;
            text_changed |= changed;
        }
        replace_image |= replace;
        if preview_soon {
            self.draft_preview_deadline
                .get_or_insert_with(|| Instant::now() + Duration::from_millis(32));
        }
        if let Some(error) = error {
            self.fail(error);
        }
        if self.draft.operation.asset().is_some() {
            if ui.button("Match composition to this media…").clicked() {
                self.match_media(self.project.selected);
            }
            if matches!(
                self.draft.operation,
                Operation::Image(veylvik_document::nodes::Image { .. })
            ) && ui.button("Enable placement controls").clicked()
            {
                let asset = self.draft.operation.asset().unwrap().to_owned();
                self.draft.operation = Operation::Footage(veylvik_document::nodes::Footage {
                    asset,
                    timing: Default::default(),
                    placement: Default::default(),
                    colour: Default::default(),
                    layer: None,
                    fallback: String::new(),
                });
                self.draft_dirty = true;
                text_changed = true;
            }
        }
        if replace_image {
            self.replace_image();
        }
        let kind = self.draft.operation.descriptor();
        for parameter in kind.parameters {
            if matches!(
                self.draft.operation,
                Operation::Text(veylvik_document::nodes::Text { .. })
            ) {
                break;
            }
            if parameter.name == "light"
                && matches!(&self.draft.operation, Operation::Shadow(s) if matches!(s.light, veylvik_document::Scalar::Constant { .. }))
            {
                continue;
            }
            // A 3D surface with a look is shaded by it, not these.
            let looked = match &self.draft.operation {
                Operation::Shape3d(s) => s.look.is_some(),
                Operation::Text3d(t) => t.look.is_some(),
                _ => false,
            };
            if looked && matches!(parameter.name, "roughness" | "metallic") {
                continue;
            }
            // A projection's hold time only with its camera held.
            if parameter.name == "hold_time"
                && matches!(self.draft.operation, Operation::CameraProjection(ref c) if !c.hold)
            {
                continue;
            }
            // A force shows the settings its kind uses.
            if let Operation::Force(force) = &self.draft.operation {
                use veylvik_document::nodes::ForceKind::*;
                let hidden = match parameter.name {
                    "direction" | "lean" => !matches!(force.kind, Wind | Vortex),
                    "scale" => force.kind != Turbulence,
                    _ => false,
                };
                if hidden {
                    continue;
                }
            }
            // Particles show the settings that apply to how they are wired.
            if kind.type_id == "particles" {
                let wired = |port: &str| self.draft.inputs.contains_key(port);
                let doc = self.project.document.snapshot();
                ui.data_mut(|d| {
                    d.insert_temp(egui::Id::new("particles-frame"), [doc.width, doc.height]);
                    d.insert_temp(egui::Id::new("particles-box"), !wired("emitter"));
                    d.insert_temp(egui::Id::new("particles-obstacle"), wired("obstacle"));
                    d.insert_temp(egui::Id::new("particles-scene"), wired("scene"));
                    d.insert_temp(egui::Id::new("particles-sprite"), wired("sprite"));
                });
                let hidden = match parameter.name {
                    "density" => !wired("emitter"),
                    "rate" | "x" | "y" | "width" | "height" => wired("emitter"),
                    "homing" => !wired("target"),
                    "inherit" => !wired("emitter") && !wired("target"),
                    "field_push" | "field_flow" => !wired("field"),
                    "bounce" => {
                        !wired("obstacle")
                            && !matches!(self.draft.operation, Operation::Particles(ref n) if n.collide && wired("scene"))
                    }
                    "spin" => !wired("sprite"),
                    // The sheet is set in the node's own controls; its rate when it plays.
                    "sprite_columns" | "sprite_rows" | "sprite_play" => true,
                    "sprite_rate" => {
                        !wired("sprite")
                            || !matches!(self.draft.operation, Operation::Particles(ref n) if n.sprite_play == veylvik_document::nodes::SpritePlay::AtRate)
                    }
                    "camera" => !wired("scene"),
                    // A flock's own settings once it is one.
                    "cohesion" | "separation" | "alignment" | "top_speed" => {
                        matches!(self.draft.operation, Operation::Particles(ref n) if matches!(n.flock_radius, veylvik_document::Scalar::Constant { value } if value <= 0.0))
                    }
                    // Offered as a choice by the node's own inspector.
                    "history" => true,
                    // A burst's own settings once it has particles.
                    "burst_speed" | "burst_spread" | "burst_life" | "burst_size" => {
                        matches!(self.draft.operation, Operation::Particles(ref n) if matches!(n.burst, veylvik_document::Scalar::Constant { value } if value < 0.5))
                    }
                    // A trail's width and the lines' look once there is a trail or lines.
                    "trail_width" => {
                        matches!(self.draft.operation, Operation::Particles(ref n) if matches!(n.trail, veylvik_document::Scalar::Constant { value } if value <= 0.0))
                    }
                    // Lighting comes from a scene; its details once particles are lit.
                    "lit" => !wired("scene"),
                    "ambient" | "scatter" => {
                        !wired("scene")
                            || matches!(self.draft.operation, Operation::Particles(ref n) if matches!(n.lit, veylvik_document::Scalar::Constant { value } if value <= 0.0))
                    }
                    "link_opacity" | "link_width" => {
                        matches!(self.draft.operation, Operation::Particles(ref n) if matches!(n.link_distance, veylvik_document::Scalar::Constant { value } if value <= 0.0))
                    }
                    _ => false,
                };
                if hidden {
                    continue;
                }
            }
            // A custom shader's values show as the controls its code declares.
            let declared = shader_editor::slot(&self.draft.operation, parameter.name);
            if kind.type_id == "shader" && declared.is_none() {
                continue;
            }
            // Map controls appear with their map.
            let needs = match parameter.name {
                "shot" => "environment",
                "normal_strength" => "normal_map",
                "height_depth" => "height_map",
                _ => "",
            };
            if kind.type_id == "material"
                && !needs.is_empty()
                && !self.draft.inputs.contains_key(needs)
            {
                continue;
            }
            if let Some(value) = self.draft.operation.scalar_mut(parameter.name) {
                let label = if let Some(slot) = &declared {
                    slot.label.clone()
                } else if kind.type_id == "material" && parameter.name == "shot" {
                    "Shot reflection".to_owned()
                } else if kind.type_id == "material" && parameter.name == "normal_strength" {
                    "Normal strength".to_owned()
                } else if kind.type_id == "material" && parameter.name == "height_depth" {
                    "Height depth (px)".to_owned()
                } else if kind.type_id == "surface_blend" {
                    match parameter.name {
                        "lighting" => "Lighting",
                        "detail" => "Texture",
                        "wear" => "Wear",
                        "threshold" => "Crack cutoff",
                        "distortion" => "Distort (px)",
                        "softness" => "Softness (px)",
                        "grain" => "Grain",
                        _ => "Opacity",
                    }
                    .to_owned()
                } else {
                    names::setting(parameter.name, parameter.unit)
                };
                let changed = animation::scalar_with_default(
                    ui,
                    &label,
                    value,
                    declared
                        .as_ref()
                        .map_or(parameter.min.unwrap()..=parameter.max.unwrap(), |slot| {
                            slot.range.clone()
                        }),
                    time,
                    declared.as_ref().map(|slot| slot.default).or_else(|| {
                        serde_json::from_value::<veylvik_document::Scalar>(
                            kind.default_operation()[parameter.name].clone(),
                        )
                        .ok()
                        .and_then(|value| value.at(time).ok())
                    }),
                );
                self.draft_dirty |= changed;
                text_changed |= changed;
            }
        }
        if text_changed || dirty {
            self.queue_draft_preview(ui.ctx());
        }
        if let Some(selection) = import_font {
            self.import_text_font(selection);
        }

        if self.draft_dirty {
            self.playing = false;
        }
        self.text_connections(ui);
        if matches!(
            self.draft.operation,
            Operation::Text(veylvik_document::nodes::Text { .. })
        ) && self.project.preview.is_none()
            && !editable_text::handles_in_scene(self.project.document.snapshot(), self.draft.id)
        {
            ui.weak("A downstream effect moves this text. Use Preview selected for source-position handles.");
        }
        ui.separator();
        let descriptor = self.draft.operation.descriptor();
        for port in descriptor.inputs {
            let optional = descriptor.is_optional(port);
            let mut current = self.draft.inputs.get(*port).copied();
            let label = current
                .and_then(|id| self.project.document.snapshot().nodes.get(&id))
                .map_or(if optional { "None" } else { "Missing" }, |n| {
                    n.label.as_str()
                })
                .to_owned();
            let before = current;
            let named = crate::ofx_inspector::port_name(&self.draft.operation, port)
                .unwrap_or_else(|| (*port).to_owned());
            egui::ComboBox::from_id_salt(("port", port))
                .selected_text(format!(
                    "{named}{}: {label}",
                    if optional { " (optional)" } else { "" }
                ))
                .show_ui(ui, |ui| {
                    if optional {
                        ui.selectable_value(&mut current, None, "None");
                    }
                    for node in self.project.document.snapshot().nodes.values() {
                        if node.id != self.draft.id {
                            ui.selectable_value(&mut current, Some(node.id), &node.label);
                        }
                    }
                });
            if current != before {
                match current {
                    Some(id) => {
                        self.draft.inputs.insert((*port).to_owned(), id);
                    }
                    None => {
                        self.draft.inputs.remove(*port);
                    }
                }
                self.draft_dirty = true;
            }
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.label("Viewer");
            if ui
                .add(
                    egui::Button::new("Selected node")
                        .selected(self.project.preview == Some(self.project.selected))
                        .min_size(egui::vec2(0.0, 26.0)),
                )
                .clicked()
            {
                self.project.preview = Some(self.project.selected);
                self.request_preview();
            }
            if ui
                .add(
                    egui::Button::new("Composition")
                        .selected(self.project.preview.is_none())
                        .min_size(egui::vec2(0.0, 26.0)),
                )
                .clicked()
            {
                self.project.preview = None;
                self.request_preview();
            }
        });
        ui.menu_button("Layer tools", |ui| {
            if matches!(self.draft.operation,Operation::Text(_)) {ui.label("Layer name");self.draft_dirty |= ui.text_edit_singleline(&mut self.draft.label).changed();ui.separator();}

        self.surface_controls(ui);
        self.occlusion_controls(ui);
        if ui
            .add_enabled(!self.files.busy(), egui::Button::new("Track motion…"))
            .on_hover_text(
                "Track a feature and attach editable movement to text, a mask or a layer",
            )
            .clicked()
        {
            self.begin_tracking();
        }
        if ui
            .button("Track surface…")
            .on_hover_text("Track perspective motion and attach or stabilize any layer")
            .clicked()
        {
            self.begin_planar();
        }
        if ui
            .button("Solve camera…")
            .on_hover_text("Find the real camera of this footage so 3D objects stay put in the shot")
            .clicked()
        {
            self.begin_solve_camera();
        }
        if ui
            .button("Select subject…")
            .on_hover_text("Click a person or object to cut it out, track it through the shot, replace the background or put layers behind it")
            .clicked()
        {
            self.begin_subject();
        }
        });
        ui.menu_button("Node actions...", |ui| {
            if ui.button("Copy settings").clicked() {
                self.copy_settings();
                ui.close();
            }
            let from = self.copied_from().map(str::to_owned);
            if ui
                .add_enabled(
                    from.is_some(),
                    egui::Button::new(match &from {
                        Some(from) => format!("Paste settings from {from}"),
                        None => "Paste settings".to_owned(),
                    }),
                )
                .on_hover_text("Onto every selected node: all settings for the same kind of node, the shared ones for another kind")
                .clicked()
            {
                self.paste_settings();
                ui.close();
            }
            ui.separator();
            if ui.button("Set as output").clicked() {
                self.set_selected_output();
                ui.close();
            }
            if ui.button("Duplicate node").clicked() {
                self.duplicate_selected(ui.ctx());
                ui.close();
            }
            if ui
                .button("Duplicate branch")
                .on_hover_text("Copy this node and all upstream dependencies (Ctrl+Shift+D)")
                .clicked()
            {
                self.duplicate_branch();
                ui.close();
            }
            ui.separator();
            if ui.button("Delete node").clicked() {
                self.delete_selected();
                ui.close();
            }
        });
        ui.add_space(15.0);
        ui.weak("Parameter changes become one undo step when applied.");
    }
    fn set_selected_output(&mut self) {
        if !self.apply_draft() {
            return;
        }
        match self.project.apply(&[Command::Output {
            id: self.project.selected,
        }]) {
            Ok(()) => {
                self.project.preview = None;
                self.request_preview();
            }
            Err(error) => self.fail(error),
        }
    }
    fn delete_selected(&mut self) {
        if !self.can_change_selection() {
            return;
        }
        match self.project.apply(&[Command::Remove {
            id: self.project.selected,
        }]) {
            Ok(()) => {
                self.refresh_draft();
                self.request_preview();
            }
            Err(error) => self.fail(error),
        }
    }
    fn sync_key_timeline(&mut self) {
        let mut drafts = Vec::new();
        if self.draft_dirty {
            drafts.push(self.draft.clone());
            drafts.extend(self.text_edits.values().cloned());
            for ((id, control), value) in &self.effects.edits {
                if let Some(control) = self
                    .project
                    .document
                    .snapshot()
                    .groups
                    .get(id)
                    .and_then(|g| g.controls.get(control))
                    && let Ok(scalar) =
                        serde_json::from_value::<veylvik_document::Scalar>(value.clone())
                {
                    let index = drafts
                        .iter()
                        .position(|n| n.id == control.node)
                        .unwrap_or_else(|| {
                            drafts.push(
                                self.project.document.snapshot().nodes[&control.node].clone(),
                            );
                            drafts.len() - 1
                        });
                    if let Some(target) = drafts[index].operation.scalar_mut(&control.parameter) {
                        *target = scalar;
                    }
                }
            }
        }
        self.key_timeline.sync_live(
            self.project.document.snapshot(),
            self.project.document.revision(),
            self.project.selected,
            &drafts,
        );
    }
    fn seek_key(&mut self, forward: bool) {
        if !self.apply_draft() {
            return;
        }
        self.sync_key_timeline();
        if let Some(frame) = self.key_timeline.adjacent(self.project.frame, forward) {
            self.project.frame = frame;
            self.playing = false;
            self.request_preview();
        }
    }
    fn step_frame(&mut self, direction: i64) {
        if !self.apply_draft() {
            return;
        }
        self.playing = false;
        self.project.frame = (self.project.frame + direction).clamp(
            0,
            i64::from(
                self.project
                    .document
                    .snapshot()
                    .frame_count
                    .saturating_sub(1),
            ),
        );
        self.request_preview();
    }
    fn timeline(&mut self, ui: &mut egui::Ui) {
        if let Some(time) = ui.ctx().data_mut(|d| {
            let id = egui::Id::new("parameter-key-seek");
            let time = d.get_temp::<veylvik_document::Time>(id);
            d.remove::<veylvik_document::Time>(id);
            time
        }) && self.apply_draft()
        {
            let doc = self.project.document.snapshot();
            if let Some(frame) = key_timeline::frame(time, doc.frame_rate, doc.frame_count) {
                self.project.frame = frame;
                self.playing = false;
                self.request_preview();
            }
        }
        self.sync_key_timeline();
        let count = self.project.document.snapshot().frame_count;
        let end_frame = (i64::from(count) - 1).max(self.project.frame);
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(!self.animation_editor.dragging(), |ui| {
                if ui.selectable_label(!self.node_editor_hidden && !self.animation_editor.visible, "Nodes").clicked() {self.node_editor_hidden=false;self.animation_editor.visible=false;}
                if ui.selectable_label(!self.node_editor_hidden && self.animation_editor.visible, "Animation").clicked() {self.node_editor_hidden=false;self.animation_editor.visible=true;}
                if ui.button(if self.node_editor_hidden {"Show editor"} else {"Hide editor"}).on_hover_text("Collapse the node/animation editor while retaining playback controls and the time ruler").clicked() {self.node_editor_hidden = !self.node_editor_hidden;}

            });
            if ui
                .add(icons::Button::new(
                    if self.playing {
                        icons::Icon::Pause
                    } else {
                        icons::Icon::Play
                    },
                    if self.playing { "Pause" } else { "Play" },
                ))
                .clicked()
            {
                self.toggle_play();
            }
            if ui
                .add(
                    icons::Button::new(icons::Icon::Loop, "Loop playback")
                        .selected(self.preferences.values.loop_playback),
                )
                .on_hover_text("Loop playback · saved in Preferences")
                .clicked()
            {
                self.preferences.values.loop_playback = !self.preferences.values.loop_playback;
                if let Err(error) = self.preferences.persist() {
                    self.fail(error);
                }
            }
            if ui
                .add(crate::icons::Button::new(
                    crate::icons::Icon::PreviousFrame,
                    "Previous frame",
                ))
                .on_hover_text("Previous frame · Left arrow")
                .clicked()
            {
                self.step_frame(-1);
            }
            if ui
                .add(crate::icons::Button::new(
                    crate::icons::Icon::NextFrame,
                    "Next frame",
                ))
                .on_hover_text("Next frame · Right arrow")
                .clicked()
            {
                self.step_frame(1);
            }
            let mut frame = self.project.frame;
            let rate = self.project.document.snapshot().frame_rate;
            let timecode = self.preferences.values.time_display == timecode::Display::Timecode;
            // Reserve the widest text the field can show, rather than the current
            // value's width. Keep the same bounds while typing and dragging.
            let number_font = ui.style().drag_value_text_style.resolve(ui.style());
            let prefix_font = egui::TextStyle::Button.resolve(ui.style());
            let colour = ui.visuals().text_color();
            let digit_width = ('0'..='9')
                .map(|digit| {
                    ui.painter()
                        .layout_no_wrap(digit.to_string(), number_font.clone(), colour)
                        .size()
                        .x
                })
                .fold(0.0_f32, f32::max);
            let frame_width = if timecode {
                // Eight digits and three separators.
                digit_width * 8.0
                    + 3.0
                        * ui.painter()
                            .layout_no_wrap(";".into(), number_font.clone(), colour)
                            .size()
                            .x
                            .max(digit_width * 0.5)
                    + 2.0 * ui.spacing().button_padding.x
                    + 8.0
            } else {
                ui.painter()
                    .layout_no_wrap("Frame ".into(), prefix_font, colour)
                    .size()
                    .x
                    + digit_width * count.to_string().len().max(4) as f32
                    + 2.0 * ui.spacing().button_padding.x
                    + 4.0
            };
            let changed = ui
                .scope(|ui| {
                    ui.spacing_mut().interact_size.x = frame_width;
                    let field = egui::DragValue::new(&mut frame).range(0..=end_frame);
                    let field = if timecode {
                        field
                            .custom_formatter(move |value, _| timecode::format(value as i64, rate))
                            .custom_parser(move |text| timecode::parse(text, rate).map(|f| f as f64))
                    } else {
                        field.prefix("Frame ")
                    };
                    ui.add(field)
                        .on_hover_text(if timecode {
                            "Current position: drag to scrub, or click and type a timecode (5:12 is five seconds and twelve frames) or a frame number"
                        } else {
                            "Current frame: drag to scrub or click to type a frame number"
                        })
                        .changed()
                })
                .inner;
            if changed && self.apply_draft() {
                self.project.frame = frame;
                self.playing = false;
                self.request_preview();
            }
            if timecode {
                ui.label(format!("of {}", timecode::duration(i64::from(count), rate)));
            } else {
                ui.label(format!("of {count} frames"));
            }
            let fps = rate.ticks as f64 / f64::from(rate.rate.max(1));
            ui.weak(if (fps - fps.round()).abs() < 1e-9 {
                format!("{fps:.0} fps")
            } else {
                format!("{fps:.3} fps")
            });
            // Timecode or frame numbers, for every position shown.
            if ui
                .selectable_label(false, if timecode { "Timecode" } else { "Frames" })
                .on_hover_text("Show positions as timecode or as frame numbers")
                .clicked()
            {
                self.preferences.values.time_display = if timecode {
                    timecode::Display::Frames
                } else {
                    timecode::Display::Timecode
                };
                if let Err(error) = self.preferences.persist() {
                    self.fail(format!("Preferences: {error}"));
                }
            }
            self.key_timeline.scope_controls(ui);
        });
        self.playback_toolbar(ui);
        self.sync_key_timeline();
        let cache = self
            .scheduler
            .as_ref()
            .map(|s| s.playback_cache().status())
            .unwrap_or_default();
        self.key_timeline.playback_range = self
            .playback_controls
            .enabled
            .then(|| self.playback_controls.range(count));
        self.key_timeline.dim_outside = self.host_range && self.playback_controls.enabled;
        self.key_timeline.cached_gpu = cache
            .gpu_frames
            .iter()
            .chain(&cache.ready_frames)
            .filter_map(|t| {
                key_timeline::frame(*t, self.project.document.snapshot().frame_rate, count)
            })
            .collect();
        self.key_timeline.cached_disk = cache
            .disk_frames
            .iter()
            .filter_map(|t| {
                key_timeline::frame(*t, self.project.document.snapshot().frame_rate, count)
            })
            .collect();
        let doc = self.project.document.snapshot();
        if cache.revision != self.project.document.revision()
            || cache.output != self.project.preview.unwrap_or(doc.output)
            || cache.size
                != [
                    doc.width.div_ceil(self.playback_controls.divisor()),
                    doc.height.div_ceil(self.playback_controls.divisor()),
                ]
            || self.draft_dirty
        {
            self.key_timeline.cached_gpu.clear();
            self.key_timeline.cached_disk.clear();
        }
        let actions = ui
            .horizontal(|ui| {
                self.key_timeline.waveform = self.waveform.peaks.clone();
                self.key_timeline.timecode =
                    self.preferences.values.time_display == timecode::Display::Timecode;
                self.key_timeline.show(
                    ui,
                    self.project.frame,
                    self.project.document.snapshot(),
                    !self.draft_dirty && !self.playing,
                )
            })
            .inner;
        let mut actions = actions;
        // Layer bars line up under the ruler; they need the editor's room.
        if !self.node_editor_hidden
            && let Some(span) = self.key_timeline.span
        {
            actions.extend(self.layer_bars.show(
                ui,
                self.project.document.snapshot(),
                span,
                self.project.selected,
                self.project.frame,
                !self.draft_dirty && !self.playing,
            ));
        }
        for action in actions {
            match action {
                key_timeline::Action::Select(id) => {
                    if self.can_change_selection() {
                        self.project.selected = id;
                        self.refresh_draft();
                    }
                }
                key_timeline::Action::Seek(frame) => {
                    if self.apply_draft() {
                        self.project.frame = frame;
                        self.playing = false;
                        self.request_preview();
                    }
                }
                key_timeline::Action::Commit(commands) => match self.project.apply(&commands) {
                    Ok(()) => {
                        self.refresh_draft();
                        self.request_preview();
                    }
                    Err(e) => {
                        self.request_preview();
                        self.fail(e);
                    }
                },
                key_timeline::Action::Preview => self.request_preview(),
                key_timeline::Action::Open(id) => {
                    if self.can_change_selection() {
                        self.project.selected = id;
                        self.refresh_draft();
                        self.animation_editor.visible = true;
                    }
                }
                key_timeline::Action::Error(error) => self.fail(error),
            }
        }
        self.planar_quality(ui);
    }
    fn animation_workspace(&mut self, ui: &mut egui::Ui) {
        let ready = !self.draft_dirty && !self.playing;
        if !ready {
            ui.weak("Pause playback and apply or revert inspector changes to edit animation.");
        }
        let mut actions = Vec::new();
        ui.add_enabled_ui(ready, |ui| {
            actions = self.animation_editor.show(
                ui,
                self.project.document.snapshot(),
                self.project.document.revision(),
                self.project.selected,
                self.project.frame,
            );
        });
        for action in actions {
            match action {
                animation_editor::Action::Batch { revision, edits } => {
                    if revision != self.project.document.revision() {
                        self.fail("Animation changed during the edit; please try again.");
                        continue;
                    }
                    let commands: Vec<_> = edits
                        .into_iter()
                        .map(|(id, parameter, scalar)| Command::SetParameter {
                            id,
                            parameter,
                            value: serde_json::to_value(scalar).expect("validated scalar"),
                        })
                        .collect();
                    match self.project.apply(&commands) {
                        Ok(()) => {
                            self.refresh_draft();
                            self.request_preview();
                        }
                        Err(e) => self.fail(e),
                    }
                }
                animation_editor::Action::Select(id) => {
                    self.project.selected = id;
                    self.refresh_draft();
                }
                animation_editor::Action::Seek(frame) => {
                    self.project.frame = frame;
                    self.playing = false;
                    self.request_preview();
                }
                animation_editor::Action::Commit {
                    revision,
                    node,
                    parameter,
                    scalar,
                } => {
                    if revision != self.project.document.revision() {
                        self.fail("Animation changed during the edit; please try again.");
                        continue;
                    }
                    let command = Command::SetParameter {
                        id: node,
                        parameter,
                        value: serde_json::to_value(scalar).expect("finite validated scalar"),
                    };
                    match self.project.apply(&[command]) {
                        Ok(()) => {
                            self.refresh_draft();
                            self.request_preview();
                        }
                        Err(error) => self.fail(error),
                    }
                }
            }
        }
    }
    fn toggle_play(&mut self) {
        if !self.apply_draft() {
            return;
        }
        self.path_pen.cancel();
        self.playing = !self.playing;
        self.playback_controls.warming = None;
        let range = self
            .playback_controls
            .range(self.project.document.snapshot().frame_count);
        if self.playing
            && (self.preferences.values.restart_playback
                || self.project.frame < range.0
                || self.project.frame >= range.1)
        {
            self.project.frame = range.0;
            self.request_preview();
        }
        if !self.error {
            self.status = if self.playing { "Playing" } else { "Paused" }.into();
        }
        self.playback = Instant::now();
        self.playback_frame = self.project.frame;
    }
    fn graph(&mut self, ui: &mut egui::Ui) {
        let actions = self.graph.show(
            ui,
            self.project.document.snapshot(),
            self.project.document.revision(),
            self.project.selected,
            self.accent,
        );
        if let Some(rect) = self.graph.canvas_rect() {
            self.library_drop_target(ui, rect, true);
        }
        for action in actions {
            match action {
                graph::Action::Copy => self.copy_graph_nodes(ui.ctx()),
                graph::Action::Paste { text, position } => self.paste_graph_nodes(&text, position),
                graph::Action::AddAt(position) => {
                    if !self.files.busy()
                        && !self.authoring_dialog_open()
                        && self.can_change_selection()
                        && !self.graph.dragging()
                    {
                        self.playing = false;
                        self.node_palette.open_at(position);
                    }
                }
                graph::Action::Node { id, action } => {
                    if !self.project.document.snapshot().nodes.contains_key(&id) {
                        continue;
                    }
                    if !self.can_change_selection() {
                        continue;
                    }
                    self.project.selected = id;
                    self.refresh_draft();
                    match action {
                        graph::NodeAction::MatchMedia => self.match_media(id),
                        graph::NodeAction::Preview => {
                            self.project.preview = Some(id);
                            self.request_preview();
                        }
                        graph::NodeAction::Composition => {
                            self.project.preview = None;
                            self.request_preview();
                        }
                        graph::NodeAction::Duplicate => self.duplicate_selected(ui.ctx()),
                        graph::NodeAction::DuplicateBranch => self.duplicate_branch(),
                        graph::NodeAction::Output => self.set_selected_output(),
                        graph::NodeAction::Delete => self.delete_selected(),
                        graph::NodeAction::CreateGroup => self.begin_group(false),
                        graph::NodeAction::EditGroup => self.begin_group(true),
                        graph::NodeAction::Rename(label) => {
                            let command = if let Some(group) = self
                                .graph
                                .selected_group(self.project.document.snapshot(), id)
                            {
                                let mut group = group.clone();
                                group.label = label;
                                Command::PutGroup {
                                    group: Box::new(group),
                                }
                            } else {
                                let mut node = self.draft.clone();
                                node.label = label;
                                Command::Put {
                                    node: Box::new(node),
                                }
                            };
                            match self.project.apply(&[command]) {
                                Ok(()) => self.refresh_draft(),
                                Err(e) => self.fail(e),
                            }
                        }
                        graph::NodeAction::Bypass => {
                            let mut node = self.draft.clone();
                            node.bypass = !node.bypass;
                            match self.project.apply(&[Command::Put {
                                node: Box::new(node),
                            }]) {
                                Ok(()) => {
                                    self.refresh_draft();
                                    self.request_preview();
                                }
                                Err(e) => self.fail(e),
                            }
                        }
                    }
                }
                graph::Action::Group => self.begin_group(false),
                graph::Action::CloseGroup(id) => {
                    if self.can_change_selection() {
                        self.graph.collapse(id);
                        self.project.selected = self.project.document.snapshot().groups[&id].output;
                        self.refresh_draft();
                    }
                }
                graph::Action::OpenGroup(id) => {
                    if self.can_change_selection() {
                        self.graph.expand(id);
                    }
                }
                graph::Action::Find => self.open_node_finder(),
                graph::Action::Add => self.open_node_palette(ui.ctx()),
                graph::Action::Select { id, preview } => {
                    if self.can_change_selection() {
                        self.overlay_hidden = None;
                        self.project.selected = id;
                        self.refresh_draft();
                        if preview {
                            self.project.preview = Some(id);
                            self.request_preview();
                        }
                    }
                }
                graph::Action::Disconnect { target, port } => {
                    if self.can_change_selection() {
                        match self.project.disconnect(target, &port) {
                            Ok(true) => {
                                self.refresh_draft();
                                self.request_preview();
                            }
                            Ok(false) => {}
                            Err(error) => self.fail(error),
                        }
                    }
                }
                graph::Action::Connect {
                    source,
                    target,
                    port,
                    group,
                } => {
                    if self.can_change_selection() {
                        let result = if group {
                            self.project
                                .apply(&[Command::ConnectGroup {
                                    id: target,
                                    input: port,
                                    source,
                                }])
                                .map(|_| true)
                        } else {
                            self.project.connect(source, target, &port)
                        };
                        match result {
                            Ok(true) => {
                                self.refresh_draft();
                                self.request_preview();
                            }
                            Ok(false) => {}
                            Err(error) => self.fail(error),
                        }
                    }
                }
                graph::Action::Annotate { id, tag, note } => {
                    self.annotate(&[Command::Annotate { id, tag, note }]);
                }
                graph::Action::Backdrops(backdrops) => {
                    self.annotate(&[Command::Backdrops { backdrops }]);
                }
                graph::Action::Place { id, position } => self.place(&[Command::Place {
                    id,
                    position: Some(position),
                }]),
                graph::Action::Arrange(positions) => self.place(
                    &positions
                        .into_iter()
                        .map(|(id, position)| Command::Place {
                            id,
                            position: Some(position),
                        })
                        .collect::<Vec<_>>(),
                ),
            }
        }
    }
    /// Tags, notes and backdrops: one undo step, and nothing to render again.
    fn annotate(&mut self, commands: &[Command]) {
        if !self.can_change_selection() {
            return;
        }
        match self.project.apply(commands) {
            Ok(()) => self.refresh_draft(),
            Err(error) => self.fail(error),
        }
    }
    fn place(&mut self, commands: &[Command]) {
        if let Err(e) = self.project.apply(commands) {
            self.fail(e);
            return;
        }
        // Keep an unapplied parameter draft while accepting its new layout.
        self.draft.position = self.project.document.snapshot().nodes[&self.draft.id].position;
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        let before = self.preferences.values.clone();
        self.preferences.audio_status = self.audio.status();
        if self.preferences.show(ctx, &self.system_theme.label) {
            self.apply_preferences(Some(&before), ctx);
            self.recall();
        }
        if std::mem::take(&mut self.preferences.want_shortcuts) {
            self.commands.editor = true;
        }
        if std::mem::take(&mut self.preferences.want_clear_cache) {
            if let Some(scheduler) = &self.scheduler {
                scheduler.playback_cache().clear();
            }
            self.request_preview();
            self.status = "Playback cache cleared".into();
            self.error = false;
        }
        if std::mem::take(&mut self.preferences.want_clear_recent) {
            self.start.clear();
            self.status = "Recent list cleared".into();
            self.error = false;
        }
        if std::mem::take(&mut self.preferences.want_folder) {
            match self.preferences.folder() {
                Some(folder) => {
                    if let Err(error) = std::process::Command::new("xdg-open").arg(&folder).spawn()
                    {
                        self.fail(format!("Could not open {}: {error}", folder.display()));
                    }
                }
                None => self.fail("Preferences have no folder on this system."),
            }
        }
        self.theme(ctx);
        self.effect_dialog(ctx);
        self.preset_library(ctx);
        self.save_effect_window(ctx);
        self.library_drop_options(ctx);
        self.materials.dialogs(ctx);
        if let Some(e) = self.materials.error.take() {
            self.fail(e);
        }
        if let Some(action) = self.confirm {
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center())
                .show(ctx, |ui| {
                    ui.label("Save your work before continuing, or discard it.");
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            self.confirm = None;
                            self.after_save = Some(action);
                            self.save_project(false);
                        }
                        if ui.button("Discard changes").clicked() {
                            self.confirm = None;
                            self.leave_now(action, ctx);
                        }
                        if ui.button("Keep editing").clicked() {
                            self.confirm = None;
                        }
                    });
                });
        }
        diagnostics::show(ctx, &mut self.statistics, &self.renderer);
        self.about.show(ctx, &self.renderer);
        self.command_windows(ctx);
        self.log_windows(ctx);
        self.layout_windows(ctx);
        self.published_window(ctx);
        self.second_viewer(ctx);
        self.layout_tick(ctx);
    }
}
impl eframe::App for Studio {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // File dialogs belong to this window.
        files::offer_parent(frame);
        self.editor_ui(ui);
    }
    fn on_exit(&mut self) {
        self.shut_down();
    }
}
impl Studio {
    /// One frame of the editor, drawn into `ui`.
    fn editor_ui(&mut self, ui: &mut egui::Ui) {
        self.track_recent();
        let files_ctx = ui.ctx().clone();
        self.run_asked_menu_items(&files_ctx);
        self.record_key(&files_ctx);
        self.gather_files(&files_ctx);
        self.take_next_file(&files_ctx);
        self.drop_hint(&files_ctx);
        let ctx = ui.ctx().clone();
        self.theme(&ctx);
        self.trust_dialog(&ctx);
        self.scene_record_tick(&ctx);
        // eframe creates the root UI before our live palette update.
        ui.set_style(ctx.global_style());
        self.path_pen.shortcuts(&ctx);
        self.occlusion_tick();
        self.poll_files(&ctx);
        self.portable_window(&ctx);
        if self.render_queue.will_start() {
            self.playback_controls.warming = None;
            if let Some(s) = &self.scheduler {
                s.playback_cache().clear();
            }
        }
        let exporting = self.render_queue.busy();
        self.render_queue.tick(&self.renderer, &ctx);
        if exporting && !self.render_queue.busy() && self.status == "Exporting" {
            self.status = "Ready".into();
        }
        if let Some(action) = self.render_queue.show(&ctx) {
            match action {
                render_queue::Action::Frame => self.export_frame(),
                render_queue::Action::Video => self.ask_export(pages::ExportKind::Video),
                render_queue::Action::Sequence => self.ask_export(pages::ExportKind::Sequence),
                render_queue::Action::Retry(job) => self.retry_render(*job),
            }
        }
        self.audio_tick();
        self.waveform_tick(&ctx);
        self.thumbnails_tick(&ctx);
        self.materials.poll(&ctx);
        self.shot_light_tick();
        self.grain_measure_tick(&ctx);
        light_guides::begin_frame(&ctx);
        self.fonts.poll(&ctx);
        self.remember_tick();
        self.log_tick();
        self.link_tick(&ctx);
        self.follow_selection(&ctx);
        self.autokey_tick(&ctx);
        self.tryout_bar(&ctx);
        if self.draft_dirty
            && self.tryout.is_none()
            && self.preferences.values.apply_after_pause
            && self
                .draft_commit_deadline
                .is_some_and(|t| Instant::now() >= t)
            && !ctx.egui_wants_keyboard_input()
            && !ctx.input(|i| i.pointer.any_down())
        {
            self.draft_commit_deadline = None;
            self.apply_draft();
        }
        if let Some(deadline) = self.draft_preview_deadline {
            if Instant::now() >= deadline && !self.preview_pending {
                self.draft_preview_deadline = None;
                if self.draft_dirty {
                    self.request_preview();
                }
            } else {
                ctx.request_repaint_after(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .max(Duration::from_millis(8)),
                );
            }
        }
        // A host that owns the composition keeps its own safety copies.
        if self.capabilities.project_files
            && let Some(error) = self.recovery.tick(&self.project)
        {
            self.fail(error);
        }
        for _ in 0..8 {
            let Some(request) = self.control.as_ref().and_then(|server| server.try_recv()) else {
                break;
            };
            control::dispatch(self, request, &ctx);
        }
        ctx.request_repaint_after(Duration::from_secs(2));
        if let Some(scheduler) = &self.scheduler {
            match scheduler.try_take() {
                Ok(Some(result)) if self.wanted == Some(result.ticket) => {
                    self.preview_pending = false;
                    match result.frame {
                        Ok(frame) => {
                            let flow_report = frame.flow_report();
                            self.displayed = Some(self.display.frame_as(
                                &self.renderer,
                                frame,
                                self.effective_view(),
                                viewer::ViewTransform::default(),
                                result.screen_ready,
                            ));
                            self.displayed_ticket = Some(result.ticket);
                            self.pictures_shown += 1;
                            self.displayed_time = Some(result.request.time);
                            if !self.error && !self.files.busy() {
                                self.last_render_ms = Some(result.elapsed.as_secs_f64() * 1000.);
                                self.status = if let Some(report) = flow_report {
                                    if report.scene_cut {
                                        "Flow: cut detected, using source frame".into()
                                    } else {
                                        format!(
                                            "Flow: {:3.0}% source fallback",
                                            report.fallback_fraction * 100.
                                        )
                                    }
                                } else {
                                    if self.playing { "Playing" } else { "Ready" }.into()
                                };
                            }
                        }
                        Err(e) => {
                            self.playing = false;
                            self.fail(e);
                        }
                    }
                }
                Err(e) => {
                    self.preview_pending = false;
                    self.playing = false;
                    self.fail(e);
                }
                _ => {}
            }
        }
        self.fill_tick(&ctx);
        // Playing is over and the viewer still holds a played picture: ask for the frame
        // itself, so values read off it are the renderer's.
        if !self.playing
            && self.playback_controls.warming.is_none()
            && !self.preview_pending
            && self.displayed.as_ref().is_some_and(|shown| shown.baked)
        {
            self.request_preview();
        }
        if let Some(warm) = self.playback_controls.warming {
            if self.error {
                self.playback_controls.warming = None;
            } else if !self.preview_pending {
                if self.project.frame < warm.end {
                    self.project.frame += 1;
                    self.request_preview_inner();
                } else {
                    self.project.frame = warm.restore.clamp(
                        0,
                        i64::from(
                            self.project
                                .document
                                .snapshot()
                                .frame_count
                                .saturating_sub(1),
                        ),
                    );
                    self.request_preview();
                }
            }
            if self.playback_controls.warming.is_some() {
                self.status = format!("Caching frame {} / {}", self.project.frame, warm.end);
            }
        }
        self.poll_planar_preview();
        if !self.playing {
            self.buffering = None;
            self.waiting = None;
            self.play_now = false;
            self.fill.clear_since();
        }
        // Playing shows every frame at its moment or it does not move: while frames
        // ahead are still being prepared the playhead holds, and goes when they are there.
        self.playing_behind &= self.playing;
        if self.playing && self.hold_for_frames(&ctx) {
            self.playback = Instant::now();
            self.playback_frame = self.project.frame;
        } else if self.playing {
            let fps = self.project.document.snapshot().frame_rate;
            let elapsed =
                (self.playback.elapsed().as_secs_f64() * fps.ticks as f64 / fps.rate as f64) as i64;
            let range = self
                .playback_controls
                .range(self.project.document.snapshot().frame_count);
            let looping = self.preferences.values.loop_playback;
            let (mut frame, mut finished) =
                playback::frame(self.playback_frame, elapsed, range, looping);
            // Every frame: never past the next one; when the clock is ahead, the next
            // frame starts the clock again, so playing runs slower and skips nothing.
            if self.preferences.values.every_frame && !self.preview_pending {
                let (step, behind) =
                    playback::every_frame((frame, finished), self.project.frame, range, looping);
                self.playing_behind = behind;
                if behind {
                    (frame, finished) = step;
                    self.playback = Instant::now();
                    self.playback_frame = frame;
                }
            }
            // Playback must let an active frame finish before submitting again.
            // Explicit edits and scrubbing still replace obsolete work immediately.
            if frame != self.project.frame && !self.preview_pending {
                self.project.frame = frame;
                self.request_preview();
            }
            if finished && !self.preview_pending {
                self.playing = false;
                if !self.error {
                    self.status = "Ready".into();
                }
            }
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        // Consume early, apply after widgets have processed this frame's text edits.
        let apply_inspector = !self.files.busy()
            && !self.authoring_dialog_open()
            && self.confirm.is_none()
            && !self.graph.dragging()
            && !self.animation_editor.dragging()
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Enter));
        if !self.files.busy()
            && !self.authoring_dialog_open()
            && self.confirm.is_none()
            && (!ctx.egui_wants_keyboard_input()
                || self.animation_editor.has_focus(&ctx)
                || self.key_timeline.has_focus(&ctx))
            && !self.animation_editor.dragging()
            && !self.key_timeline.dragging()
        {
            self.command_keys(&ctx);
            if ctx.input(|i| {
                i.events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Text(s) if s == "?"))
            }) {
                self.commands.editor = !self.commands.editor;
            }
        }
        // F11 shows the viewer alone on the whole screen; F11 or Escape brings the rest back.
        let leave = self.viewer_fullscreen
            && self.picking.is_none()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if leave {
            self.set_viewer_fullscreen(&ctx, false);
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.leave(Leave::Close, &ctx);
        }
        ui.add_enabled_ui(
            !self.files.busy() && !self.authoring_dialog_open() && self.confirm.is_none(),
            |ui| {
                if !self.viewer_fullscreen {
                if self.capabilities.menu_bar {
                    egui::Panel::top("menu_bar").show_inside(ui, |ui| self.menu_bar(ui));
                }
                egui::Panel::bottom("status").show_inside(ui, |ui| self.status_bar(ui));
                // The start screen takes the whole window below the top bar.
                if self.start.open {
                    egui::CentralPanel::default()
                        .frame(design::panel(ui.ctx()))
                        .show_inside(ui, |ui| self.start_screen(ui));
                    return;
                }
                // Compose and Animate keep the library; 3D shares the place between the
                // scene's contents and the library; Export leaves the room to the picture.
                match self.pages.page {
                    pages::Page::Compose | pages::Page::Animate => self.asset_browser(ui),
                    pages::Page::Scene if self.pages.scene_library => self.asset_browser(ui),
                    pages::Page::Scene => self.scene_outline(ui),
                    pages::Page::Export => {}
                }
                if self.pages.page == pages::Page::Export {
                    self.export_page(ui);
                } else if self.inspector_visible {
                (match self.layouts.inspector_width() {
                    Some(width) => egui::Panel::right("inspector").exact_size(width),
                    None => egui::Panel::right("inspector").default_size(414.0).min_size(340.0),
                }).frame(design::panel(ui.ctx()))
                    .resizable(true)
                    .show_inside(ui, |ui| {
                        self.inspector_actions(ui);
                        if matches!(self.draft.operation, Operation::Text(veylvik_document::nodes::Text { .. }))
                            && self
                                .graph
                                .selected_group(
                                    self.project.document.snapshot(),
                                    self.project.selected,
                                )
                                .is_none()
                            && editable_text::header(ui, &mut self.draft.operation)
                        {
                            self.draft_dirty = true;
                            self.playing = false;
                            self.queue_draft_preview(ui.ctx());
                        }
                        egui::ScrollArea::vertical()
                            .id_salt(("inspector-body", editable_text::body_id(ui)))
                            .show(ui, |ui| self.inspector(ui));
                    });
                } else {
                    egui::Panel::right("inspector-tool-rail").exact_size(76.).frame(design::panel(ui.ctx())).show_inside(ui,|ui| {
                        if ui.add(icons::Button::new(icons::Icon::Inspector,"Inspector").caption()).clicked() {self.inspector_visible=true;}
                    });
                }
                let editor_hidden_at_start=self.node_editor_hidden;
                // The viewer always keeps this much height, whatever the editor wants.
                let room = ui.available_height() - VIEWER_ROOM;
                (if editor_hidden_at_start {
                    egui::Panel::bottom("transport-only").exact_size(if self.waveform.peaks.is_some() { 152.0 } else { 126.0 }).resizable(false)
                } else if self.pages.page == pages::Page::Animate {
                    // Animate gives the curves most of the window.
                    egui::Panel::bottom("graph-animate").default_size(560.0_f32.min(room.max(200.0))).min_size(200.0).max_size(room.max(200.0)).resizable(true)
                } else {
                    match self.layouts.editor_height() {
                        Some(height) => egui::Panel::bottom("graph").exact_size(height.min(room.max(130.0))).resizable(true),
                        None => egui::Panel::bottom("graph").default_size((ctx.content_rect().height() * 0.42).max(420.0).min(room.max(130.0))).min_size(130.0).max_size(room.max(130.0)).resizable(true),
                    }
                }).frame(design::panel(ui.ctx()))
                    .show_inside(ui, |ui| {
                        // In a window too narrow to show it, the transport would wrap into a
                        // tall column and the panel would grow to hold it, and stay that tall.
                        if ui.available_width() < EDITOR_NARROWEST {
                            // Hold the panel's height, or it would shrink to nothing and stay so.
                            ui.allocate_space(ui.available_size());
                            return;
                        }
                        ui.add_enabled_ui(!self.animation_editor.dragging(), |ui| {
                            self.timeline(ui)
                        });
                        if !editor_hidden_at_start {
                            ui.separator();
                            if self.animation_editor.visible {self.animation_workspace(ui);} else {self.graph(ui);}
                        }
                    });
                }
                egui::CentralPanel::default().frame(if self.viewer_fullscreen { egui::Frame::NONE.fill(egui::Color32::BLACK) } else { design::panel(ui.ctx()) }).show_inside(ui, |ui| {
                    // Full screen is the picture alone: no toolbars, no tool hints.
                    if !self.viewer_fullscreen {
                    ui.horizontal_wrapped(|ui| {
                        ui.heading("Viewer");
                        if matches!(self.draft.operation, Operation::Text(veylvik_document::nodes::Text { .. })) {
                            ui.menu_button("Text tools", |ui| {
                            if ui
                                .selectable_label(self.text_viewer.active, "Edit text")
                                .clicked()
                            {
                                self.text_viewer.toggle(self.draft.id);
                            }
                            if ui
                                .checkbox(&mut self.text_viewer.layout, "Layout box")
                                .changed()
                                && self.text_viewer.layout
                                && let Operation::Text(veylvik_document::nodes::Text {
                                    wrap_width,
                                    box_height,
                                    typography,
                                    ..
                                }) = &mut self.draft.operation
                                && let Ok(time) = self.project.time()
                                && (wrap_width.at(time).unwrap_or(0.0) == 0.0
                                    || box_height.at(time).unwrap_or(0.0) == 0.0)
                            {
                                *wrap_width =
                                    (self.project.document.snapshot().width as f32 * 0.8).into();
                                *box_height =
                                    (self.project.document.snapshot().height as f32 * 0.4).into();
                                typography.anchor = Some([0.5, 0.5]);
                                self.draft_dirty = true;
                                self.queue_draft_preview(ui.ctx());
                            }
                            ui.checkbox(&mut self.text_viewer.guides, "Safe guides");
                            ui.checkbox(&mut self.text_viewer.snap, "Snap");
                            });
                        }
                        // Says what the picture is: the composition, the selected node, or
                        // another node that is being looked at while something else is selected.
                        let viewing = match self.project.preview {
                            None => "Composition output".to_owned(),
                            Some(id) if id == self.project.selected => "Selected node".to_owned(),
                            Some(id) => self
                                .project
                                .document
                                .snapshot()
                                .nodes
                                .get(&id)
                                .map_or("Another node".to_owned(), |n| format!("Viewing {}", n.label)),
                        };
                        ui.weak(viewing);
                        ui.separator();
                        self.channel_choice(ui);
                        self.overlays_menu(ui);
                        ui.checkbox(&mut self.view.fast_filter, "Fast view").on_hover_text("Basic bilinear filtering instead of antialiased downscaling. Off by default; does not change exports.");
                        ui.add_enabled_ui(!self.view.alpha, |ui| {
                            ui.label("View exposure");
                            ui.add(
                                egui::DragValue::new(&mut self.view.exposure)
                                    .speed(0.1)
                                    .range(-16.0..=16.0)
                                    .suffix(" stops"),
                            );
                        });
                        if ui.button("Fit").clicked() {
                            self.navigation.fit();
                            self.comparison.fit();
                        }
                        if ui
                            .button("100%")
                            .on_hover_text("One image pixel per physical display pixel")
                            .clicked()
                        {
                            self.navigation.actual_size();
                            self.comparison.actual_size();
                        }
                        if ui.button("Reset view").clicked() {
                            self.view = ViewSettings {fast_filter:self.preferences.values.fast_view,..Default::default()};
                            // Everything about the view: where the picture sits and its size too.
                            self.navigation.fit();
                            self.comparison.fit();
                        }
                    });
                    self.comparison_toolbar(ui);
                    self.scene_toolbar(ui);
                    self.scopes_panel(ui);
                    if self.occlusion_pending() {
                        ui.horizontal_wrapped(|ui| {
                            ui.strong("Outline foreground object");
                            self.path_pen.toolbar(ui);
                        });
                    } else if matches!(self.draft.operation, Operation::Path(veylvik_document::nodes::Path { .. })) {
                        ui.horizontal_wrapped(|ui| {
                            if matches!(self.draft.operation, Operation::Path(veylvik_document::nodes::Path { .. }))
                                && !self.playing
                                && ui.add_enabled_ui(!matches!(&self.draft.operation,Operation::Path(veylvik_document::nodes::Path { geometry }) if !geometry.keys.is_empty()), |ui| self.path_pen.toolbar(ui)).inner
                            {
                                self.path_transform.active = false;
                                self.project.preview = Some(self.draft.id);
                                self.request_preview();
                            }
                            if matches!(self.draft.operation, Operation::Path(veylvik_document::nodes::Path { .. }))
                                && !self.playing
                                && !self.path_pen.active
                                && self.path_transform.toolbar(ui)
                            {
                                self.project.preview = if veylvik_document::occlusion::find(
                                    self.project.document.snapshot(),
                                    self.draft.id,
                                )
                                .is_some_and(|s| {
                                    s.output == self.project.document.snapshot().output
                                }) {
                                    None
                                } else {
                                    Some(self.draft.id)
                                };
                                self.request_preview();
                            }
                        });
                    }
                    }
                    if self.comparison.active() {
                        self.comparison_view(ui);
                    } else if let Some(frame) = self.displayed.clone() {
                        let image = egui::vec2(
                            self.project.document.snapshot().width as f32,
                            self.project.document.snapshot().height as f32,
                        );
                        let (canvas, response) = ui.allocate_exact_size(
                            (ui.available_size() - egui::vec2(0.0, if self.scopes.visible { 6.0 } else { 0.0 })).max(egui::vec2(1.0, 1.0)),
                            egui::Sense::click_and_drag(),
                        );
                        // A click that reached the picture itself, not a handle drawn over it.
                        let clicked_at = response.clicked().then(|| response.interact_pointer_pos()).flatten();
                        let before_navigation = self.navigation.transform(canvas, image, ctx.pixels_per_point());
                        let navigating_3d = self.navigating_3d(&ctx);
                        let guides_response = response.clone();
                        // Handles take drags that start on them; other drags move the view.
                        let handled = navigating_3d
                            && self.gizmo_input(ui, &response, canvas, before_navigation);
                        if navigating_3d && !handled && self.scene_interact(ui, &response, canvas) {
                            self.request_preview();
                        }
                        let text_hit = if !navigating_3d && !light_guides::active(&ctx) && response.clicked()
                            && !self.text_viewer.active && self.picking.is_none()
                            && !self.tracking.open && !self.planar.open && !self.subject.open && !self.path_pen.active {
                            response.interact_pointer_pos().and_then(|pointer| {
                                let [ox,oy,sx,sy]=before_navigation.0;
                                let local=pointer-canvas.min-egui::vec2(canvas.width()*ox,canvas.height()*oy);
                                self.project.time().ok().and_then(|time| text_viewer::pick(self.project.document.snapshot(),self.project.preview.unwrap_or(self.project.document.snapshot().output),time,[local.x/(canvas.width()*sx/image.x),local.y/(canvas.height()*sy/image.y)]))
                            })
                        } else {None};
                        if let Some(id)=text_hit
                            && (id==self.draft.id || self.can_change_selection()) {
                            if id!=self.draft.id { self.project.selected=id; self.refresh_draft(); }
                            if response.double_clicked() { self.playing=false; self.text_viewer.open(id); }
                        }
                        if !navigating_3d
                            && !self.text_viewer.active
                            && !light_guides::active(&ctx)
                            && self.picking.is_none()
                            && !self.tracking.open && !self.planar.open && !self.subject.open
                            && !self.path_pen.active
                            && !(self.path_transform.active
                                && matches!(self.draft.operation, Operation::Path(veylvik_document::nodes::Path { .. }))
                                && ui.input(|i| i.pointer.primary_down()))
                            && !(response.double_clicked()
                                && matches!(self.draft.operation, Operation::Text(veylvik_document::nodes::Text { .. })))
                        {
                            self.navigation.interact(ui, &response, canvas, image);
                        }
                        let transform =
                            self.navigation
                                .transform(canvas, image, ctx.pixels_per_point());
                        // egui keeps an open tooltip while the pointer stays inside the
                        // widget, even under a window opened later; require the viewer to
                        // be the top layer so dialogs over it never carry its hint.
                        let viewer_on_top = ctx
                            .pointer_hover_pos()
                            .is_some_and(|pos| ctx.layer_id_at(pos) == Some(response.layer_id));
                        // Nothing over the picture when it is shown alone.
                        if viewer_on_top
                            && !self.viewer_fullscreen
                            && !matches!(self.draft.operation, Operation::Text(_))
                        {
                            // At the pointer: below the viewer it would cover the transport.
                            response.on_hover_text_at_pointer(format!(
                                "{:.0}% · scroll to zoom · drag to pan · double-click to fit",
                                self.navigation.scale(canvas, image, ctx.pixels_per_point())
                                    * 100.0
                            ));
                        }
                        if frame.settings != self.effective_view() || frame.transform != transform {
                            self.displayed = Some(self.display.frame_as(
                                &self.renderer,
                                frame.frame.clone(),
                                self.effective_view(),
                                transform,
                                frame.baked,
                            ));
                        }
                        ui.painter().with_clip_rect(canvas).add(
                            egui_wgpu::Callback::new_paint_callback(
                                canvas,
                                Paint(self.displayed.as_ref().unwrap().clone()),
                            ),
                        );
                        self.viewer_canvas = Some(canvas);
                        self.wait_panel(&ctx);
                        if !self.viewer_fullscreen {
                            let pointer = ctx.pointer_hover_pos().filter(|pos| viewer_on_top && canvas.contains(*pos));
                            self.viewer_overlays(ui, canvas, transform, image, &frame.frame, pointer);
                        }
                        // Nothing made yet: say where to begin, on the empty picture itself.
                        if self.project.is_empty() {
                            ui.painter().with_clip_rect(canvas).text(
                                canvas.center(),
                                egui::Align2::CENTER_CENTER,
                                "Start from the Add menu (a text, footage, a shape),\ndrop or paste a file here, or pick a look in the library.",
                                egui::FontId::proportional(15.0),
                                ui.visuals().weak_text_color(),
                            );
                        }
                        self.light_overlay(ui, canvas, transform, image);
                        if navigating_3d {
                            self.scene_guides(ui, &guides_response, canvas, transform);
                        }
                        let label = format!(
                            "{} × {} · {:.0}%",
                            image.x as u32,
                            image.y as u32,
                            self.navigation.scale(canvas, image, ctx.pixels_per_point()) * 100.0
                        );
                        let painter = ui.painter().with_clip_rect(canvas);
                        let galley = painter.layout_no_wrap(
                            label,
                            egui::FontId::monospace(12.0),
                            egui::Color32::WHITE,
                        );
                        let position = canvas.right_bottom() - egui::vec2(8.0, 8.0) - galley.size();
                        // Full screen shows the picture and nothing else.
                        if !self.viewer_fullscreen {
                            painter.rect_filled(
                                egui::Rect::from_min_size(position, galley.size()).expand(3.0),
                                2.0,
                                egui::Color32::from_black_alpha(190),
                            );
                            painter.galley(position, galley, egui::Color32::WHITE);
                        }
                        // A pending frame must never change the canvas geometry.
                        // During playback, retaining the previous frame is normal.
                        if self.displayed_ticket != self.wanted && (!self.playing || self.error) {
                            viewer::paint_status(
                                ui,
                                canvas,
                                if self.error {
                                    "Showing previous frame · preview failed"
                                } else {
                                    "Showing previous frame · rendering"
                                },
                            );
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) && self.picking.take().is_some() {
                            self.status = "Eyedropper cancelled".into();
                        }
                        if let Some(picking) = &mut self.picking
                            && let Some(region) = picking.overlay(ui, canvas, transform, frame.frame.region())
                        {
                            let picking = self.picking.take().unwrap();
                            self.sample_colour(picking, region);
                        }
                        self.occlusion_canvas(ui, canvas, transform, image);
                        self.library_drop_target(ui,canvas,false);
                        if self.picking.is_none()
                            && !self.occlusion_pending()
                            && let Operation::Path(veylvik_document::nodes::Path { geometry }) = &self.draft.operation
                            && let Some(geometry) =
                                self.path_pen.show(ui, canvas, transform, image, geometry)
                        {
                            self.draft.operation = Operation::Path(veylvik_document::nodes::Path { geometry });
                            self.draft_dirty = true;
                            self.apply_draft();
                        }
                        if self.planar.open && self.planar.source == self.project.preview && self.displayed_ticket == self.wanted {
                            self.planar.overlay(ui, canvas, transform, self.project.frame, self.project.document.snapshot().frame_rate);
                        }
                        if self.subject.open
                            && self.subject.source == self.project.preview
                            && self.displayed_ticket == self.wanted
                            && self.subject.overlay(ui, canvas, transform, self.project.frame)
                        {
                            self.preview_subject(&ctx);
                        }
                        if self.tracking.open && self.tracking.source == self.project.preview && self.displayed_ticket == self.wanted {
                            self.tracking.overlay(ui, canvas, transform, image, self.project.frame, self.project.document.snapshot().frame_rate);
                        }
                        let root = self
                            .project
                            .preview
                            .unwrap_or(self.project.document.snapshot().output);
                        // Text boxes and shape handles are drawn in the flat 2D frame: over a
                        // free 3D view they would sit in the wrong place, so they wait for
                        // the scene camera.
                        let free_3d = self.viewed_scene().is_some() && self.free_view(&ctx).is_some();
                        // Clicking the picture away from the selected layer puts its frame and
                        // handles away; clicking the layer (or selecting its node) brings them back.
                        if let Some(point) = clicked_at
                            && !free_3d
                            && !navigating_3d
                            && self.picking.is_none()
                            && !self.path_pen.active
                            && !self.tracking.open && !self.planar.open && !self.subject.open
                            && !matches!(self.draft.operation, Operation::Path(_))
                        {
                            let is_text = matches!(self.draft.operation, Operation::Text(_));
                            let on_it = is_text && self.text_viewer.covers(canvas, transform, image, point);
                            if self.overlay_hidden == Some(self.draft.id) {
                                if on_it || !is_text {
                                    self.overlay_hidden = None;
                                }
                            } else if !on_it && !self.text_viewer.active {
                                self.overlay_hidden = Some(self.draft.id);
                                self.status = if is_text {
                                    "Frame hidden: click the text to show it again".into()
                                } else {
                                    "Handles hidden: click the picture to show them again".into()
                                };
                            }
                        }
                        if free_3d
                            && matches!(
                                self.draft.operation,
                                Operation::Text(veylvik_document::nodes::Text { .. }) | Operation::Path(veylvik_document::nodes::Path { .. })
                            )
                        {
                            ui.painter().with_clip_rect(canvas).text(
                                canvas.left_top() + egui::vec2(10.0, 8.0),
                                egui::Align2::LEFT_TOP,
                                "Free 3D view: click Scene camera to move or resize this on the picture",
                                egui::FontId::proportional(12.0),
                                egui::Color32::from_rgb(235, 215, 160),
                            );
                        }
                        if !self.playing
                            && !free_3d
                            && !light_guides::active(&ctx)
                            && self.picking.is_none()
                            && self.overlay_hidden != Some(self.draft.id)
                            && !self.tracking.open && !self.planar.open && !self.subject.open
                            && (self.displayed_ticket == self.wanted
                                || matches!(
                                    self.draft.operation,
                                    Operation::Text(veylvik_document::nodes::Text { .. }) | Operation::Path(veylvik_document::nodes::Path { .. })
                                ))
                            && (self.text_viewer.active || root == self.draft.id
                                // Particles and the forces on them have handles in the frame's
                                // pixels, which line up wherever their particles are merged.
                                || (matches!(self.draft.operation, Operation::Particles(_) | Operation::Force(_))
                                    && veylvik_document::occlusion::depends_on(
                                        self.project.document.snapshot(),
                                        root,
                                        self.draft.id,
                                    ))
                                || veylvik_document::occlusion::find(
                                    self.project.document.snapshot(),
                                    self.draft.id,
                                )
                                .is_some_and(|setup| setup.output == root)
                                || (matches!(self.draft.operation, Operation::Text(veylvik_document::nodes::Text { .. }))
                                    && self.project.preview.is_none()
                                    && editable_text::handles_in_scene(
                                        self.project.document.snapshot(),
                                        self.draft.id,
                                    )))
                            && let Ok(time) = self.project.time()
                        {
                            if let Operation::Path(veylvik_document::nodes::Path { geometry }) = &self.draft.operation { shape_animation::ghosts(ui,canvas,transform,image,geometry,time); }
                            let is_text = matches!(self.draft.operation, Operation::Text(veylvik_document::nodes::Text { .. }));
                            if is_text
                                && self.text_viewer.show(
                                    ui,
                                    canvas,
                                    transform,
                                    image,
                                    self.draft.id,
                                    &mut self.draft.operation,
                                    time,
                                )
                            {
                                self.draft_dirty = true;
                                self.queue_draft_preview(ui.ctx());
                            }
                            let transform_mask = self.path_transform.active
                                && matches!(self.draft.operation, Operation::Path(veylvik_document::nodes::Path { .. }));
                            if transform_mask && !self.path_pen.active
                                && let Operation::Path(veylvik_document::nodes::Path { geometry }) = &mut self.draft.operation {
                                match self.path_transform.show_at(ui,canvas,transform,image,geometry,time) {
                                    Ok(true) => {
                                        self.draft_dirty = true;
                                        self.draft_preview_deadline.get_or_insert_with(|| Instant::now()+Duration::from_millis(32));
                                        ui.ctx().request_repaint_after(Duration::from_millis(32));
                                    },
                                    Err(error) => self.fail(error),
                                    _ => {}
                                }
                            }
                            if !transform_mask
                                && !self.path_pen.active
                                && (!is_text
                                    || (!self.text_viewer.active && !self.text_viewer.layout
                                        && (root == self.draft.id || editable_text::handles_in_scene(self.project.document.snapshot(),self.draft.id))))
                            {
                                match handles::show(
                                    ui,
                                    canvas,
                                    transform,
                                    image,
                                    &mut self.draft.operation,
                                    time,
                                ) {
                                    Ok(changed) => {
                                        if changed
                                            && self.text_viewer.snap
                                            && ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::new("text-box-moving")).unwrap_or(false))
                                            && matches!(
                                                self.draft.operation,
                                                Operation::Text(veylvik_document::nodes::Text { .. })
                                            )
                                        {
                                            let threshold = 6.0
                                                / (canvas.width() * transform.0[2] / image.x)
                                                    .max(0.001);
                                            for (name, center) in
                                                [("x", image.x * 0.5), ("y", image.y * 0.5)]
                                            {
                                                if let Some(value) =
                                                    self.draft.operation.scalar_mut(name)
                                                    && let Ok(current) = value.at(time)
                                                    && (current - center).abs() < threshold
                                                {
                                                    let _ = handles::edit(
                                                        value,
                                                        time,
                                                        center - current,
                                                        -32768.0,
                                                        32768.0,
                                                    );
                                                }
                                            }
                                        }
                                        self.draft_dirty |= changed;
                                        if changed
                                            && matches!(
                                                self.draft.operation,
                                                Operation::Path(veylvik_document::nodes::Path { .. })
                                            )
                                        {
                                            self.draft_preview_deadline.get_or_insert_with(|| {
                                                Instant::now() + Duration::from_millis(32)
                                            });
                                            ui.ctx()
                                                .request_repaint_after(Duration::from_millis(32));
                                        }
                                        if changed
                                            && matches!(
                                                self.draft.operation,
                                                Operation::Text(veylvik_document::nodes::Text { .. })
                                            )
                                        {
                                            self.queue_draft_preview(ui.ctx());
                                        }
                                    }
                                    Err(error) => self.fail(error),
                                }
                            }
                        }
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label(if self.error {
                                "Preview unavailable"
                            } else {
                                "Preparing preview…"
                            });
                        });
                    }
                });
            },
        );
        if apply_inspector
            && !self.files.busy()
            && !self.authoring_dialog_open()
            && self.confirm.is_none()
            && !self.graph.dragging()
            && !self.animation_editor.dragging()
            && self.keep_tryout()
        {
            ctx.memory_mut(|memory| {
                if let Some(focused) = memory.focused() {
                    memory.surrender_focus(focused);
                }
            });
        }
        if let Some(id) = self.node_finder.show(&ctx) {
            if self.project.document.snapshot().nodes.contains_key(&id) {
                self.project.selected = id;
                self.project.preview = Some(id);
                if let Some(group) = self
                    .project
                    .document
                    .snapshot()
                    .groups
                    .values()
                    .find(|g| g.output == id)
                {
                    self.graph.collapse(group.id);
                }
                self.graph.focus(
                    self.project.document.snapshot(),
                    self.project.document.revision(),
                    id,
                );
                self.refresh_draft();
                self.request_preview();
            } else {
                self.fail("The selected node no longer exists");
            }
        }
        if let Some((choice, position)) = self.node_palette.show(&ctx) {
            self.add_from_palette(choice, position);
        }
        self.media_import_dialog(&ctx);
        self.occlusion_dialog(&ctx);
        self.dialogs(&ctx);
        if let Some(settings) =
            color_controls::show(&ctx, self.project.session, &self.project.inputs)
        {
            match self
                .project
                .apply(&[veylvik_document::Command::OcioViews { settings }])
            {
                Ok(()) => self.request_preview(),
                Err(e) => self.fail(e),
            }
        }
        self.quick_export(&ctx);
        if let Some(settings) = self.composition_controls.show(&ctx) {
            self.apply_composition_settings(settings, &ctx);
        }
        if let Some(id) = self.recovery.show(
            &ctx,
            !self.files.busy() && self.confirm.is_none() && !self.authoring_dialog_open(),
        ) {
            self.leave(Leave::Recover(id), &ctx);
        }
        if let Some(request) = self
            .outline_controls
            .show(&ctx, !self.files.busy() && self.confirm.is_none())
        {
            self.outline_text(request);
        }
        self.tracking_window(&ctx);
        self.planar_window(&ctx);
        self.solve_camera_window(&ctx);
        self.subject_window(&ctx);
        self.models.show(&ctx);
        if let Some(settings) = self.video_export.show(
            &ctx,
            self.project.document.snapshot(),
            !self.files.busy() && !self.authoring_dialog_open() && self.confirm.is_none(),
        ) {
            self.export_video(settings, &ctx);
        }

        if let Some(settings) = self.export_controls.show(
            &ctx,
            self.project.document.snapshot().frame_rate,
            self.project.document.snapshot().frame_count,
            !self.files.busy() && !self.authoring_dialog_open() && self.confirm.is_none(),
        ) {
            self.export_sequence(settings, &ctx);
        }
    }
    fn shut_down(&mut self) {
        // Programs compiled this session, for the next one.
        let _ = self.renderer.save_pipeline_cache();
        self.recovery
            .finish(self.allow_close || (!self.project.dirty() && !self.draft_dirty));
        self.control.take();
        self.scheduler.take();
        self.preview_images.take();
        self.displayed.take();
        let _ = self.renderer.device().poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(30)),
        });
    }
}
