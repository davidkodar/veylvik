//! Studio's side of the command list: pressing a command's key runs it, the shortcut editor changes keys (saved in preferences at once), and the command search finds and runs any command by name. While a new key is being recorded no command runs.
use crate::commands::{Binding, Cmd, Keys, search};
use crate::{Leave, Studio, display::Channel, pages};
use eframe::egui;

#[derive(Default)]
pub struct Commands {
    pub(crate) keys: Option<Keys>,
    pub editor: bool,
    filter: String,
    /// The command whose next key press becomes its shortcut.
    recording: Option<Cmd>,
    /// What the last change did to another command.
    notice: String,
    pub search: bool,
    query: String,
    chosen: usize,
    focus: bool,
}
impl Studio {
    /// The current keys, read from preferences the first time.
    pub(crate) fn keys(&mut self) -> &Keys {
        if self.commands.keys.is_none() {
            let keys = Keys::load(&self.preferences.values.shortcuts);
            self.commands.keys = Some(keys);
        }
        self.commands.keys.as_ref().unwrap()
    }
    fn keys_changed(&mut self) {
        let Some(keys) = &self.commands.keys else {
            return;
        };
        self.preferences.values.shortcuts = keys.saved();
        if let Err(error) = self.preferences.persist() {
            self.fail(format!("Preferences: {error}"));
        }
    }
    /// Run the commands whose keys were pressed. Called while nothing is being typed
    /// and no dialog holds the keyboard.
    pub(crate) fn command_keys(&mut self, ctx: &egui::Context) {
        if self.commands.recording.is_some() {
            return;
        }
        for (cmd, binding) in self.keys().order() {
            if binding.pressed(ctx) {
                self.run(cmd, ctx);
            }
        }
    }
    /// While a key is being recorded: take the next key press, whatever else is going on.
    pub(crate) fn record_key(&mut self, ctx: &egui::Context) {
        let Some(cmd) = self.commands.recording else {
            return;
        };
        let pressed = ctx.input_mut(|input| {
            let mut found = None;
            input.events.retain(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } if found.is_none() => {
                    found = Some((*key, *modifiers));
                    false
                }
                // Nothing else may act on keys or typed text meanwhile.
                egui::Event::Key { .. } | egui::Event::Text(_) => false,
                _ => true,
            });
            found
        });
        let Some((key, modifiers)) = pressed else {
            return;
        };
        self.commands.recording = None;
        if key == egui::Key::Escape {
            return;
        }
        let binding = Binding {
            ctrl: modifiers.ctrl || modifiers.command,
            shift: modifiers.shift,
            alt: modifiers.alt,
            key,
        };
        self.keys();
        let loser = self.commands.keys.as_mut().unwrap().set(cmd, Some(binding));
        self.commands.notice = loser.map_or_else(String::new, |other| {
            format!(
                "{} was on \"{}\", which now has no shortcut.",
                binding.shown(),
                other.label()
            )
        });
        self.keys_changed();
    }
    /// Do what `cmd` names, as its menu item would.
    /// Whether the host lets the editor do this at all: what belongs to the window, the
    /// project's files and the export is the host's when the editor is its guest.
    pub(crate) fn allows(&self, cmd: Cmd) -> bool {
        let can = &self.capabilities;
        match cmd {
            Cmd::StartScreen
            | Cmd::New
            | Cmd::NewFromMedia
            | Cmd::ImportNuke
            | Cmd::Open
            | Cmd::Save
            | Cmd::SaveAs
            | Cmd::UpdateHost
            | Cmd::ProjectFiles => can.project_files,
            Cmd::ExportVideo | Cmd::ExportFrame | Cmd::ExportSequence | Cmd::PageExport => {
                can.export
            }
            // Inside a host the press is the host's to answer (its own settings).
            Cmd::Preferences => true,
            Cmd::Shortcuts => can.preferences,
            Cmd::Quit => can.own_window,
            Cmd::ViewerFullScreen => can.full_screen,
            Cmd::SecondViewer => can.second_viewer,
            Cmd::About => can.about,
            _ => true,
        }
    }
    /// Whether `cmd` can run now: commands that change the composition wait until nothing
    /// is mid-gesture or waiting in the inspector. Keys, the command search and the menus
    /// all ask this, so a greyed-out item is exactly one that would refuse.
    pub(crate) fn ready_for(&self, cmd: Cmd) -> bool {
        let waits = matches!(
            cmd,
            Cmd::Undo
                | Cmd::Redo
                | Cmd::Duplicate
                | Cmd::DuplicateBranch
                | Cmd::DeleteNode
                | Cmd::SetOutput
                | Cmd::CreateGroup
                | Cmd::FindNode
                | Cmd::AddNode
                | Cmd::EffectLibrary
                | Cmd::Reload
                | Cmd::CompositionSettings
        );
        !waits || (!self.draft_dirty && !self.graph.dragging() && !self.animation_editor.dragging())
    }
    /// `cmd`'s key as the menus show it, after the person's own shortcuts.
    pub(crate) fn key_text(&self, cmd: Cmd) -> String {
        match &self.commands.keys {
            Some(keys) => keys.shown(cmd),
            None => Keys::load(&self.preferences.values.shortcuts).shown(cmd),
        }
    }
    pub(crate) fn run(&mut self, cmd: Cmd, ctx: &egui::Context) {
        if !self.allows(cmd) {
            return;
        }
        if !self.ready_for(cmd) {
            self.fail("Apply or revert the current parameter changes first.");
            return;
        }
        let frames = self.project.document.snapshot().frame_count;
        match cmd {
            Cmd::StartScreen => self.show_start(),
            Cmd::New => self.leave(Leave::New, ctx),
            Cmd::NewFromMedia => self.leave(Leave::NewFromMedia, ctx),
            Cmd::ImportNuke => self.leave(Leave::ImportNuke, ctx),
            Cmd::Open => self.leave(Leave::Open, ctx),
            Cmd::Save => self.save_project(false),
            Cmd::UpdateHost => self.update_host(),
            Cmd::SaveAs => self.save_project(true),
            Cmd::Import => self.import_image(self.graph.insertion_position(ctx)),
            Cmd::ImportSequence => self.import_media(self.graph.insertion_position(ctx), true),
            Cmd::Reload => self.reload_images(),
            Cmd::ProjectFiles => self.portable.open = true,
            Cmd::ExportVideo => self.ask_export(pages::ExportKind::Video),
            Cmd::ExportFrame => self.export_frame(),
            Cmd::ExportSequence => self.ask_export(pages::ExportKind::Sequence),
            Cmd::CompositionSettings => {
                self.transport.pause();
                self.composition_controls
                    .begin(self.project.document.snapshot(), false);
            }
            Cmd::Preferences => {
                if self.confirm.is_none() && !self.preferences.open {
                    self.open_preferences();
                }
            }
            Cmd::Quit => self.leave(Leave::Close, ctx),
            Cmd::Undo => self.history(false),
            Cmd::Redo => self.history(true),
            Cmd::Duplicate => self.duplicate_selected(ctx),
            Cmd::DuplicateBranch => self.duplicate_branch(),
            Cmd::DeleteNode => self.delete_selected(),
            Cmd::SetOutput => self.set_selected_output(),
            Cmd::CreateGroup => self.begin_group(false),
            Cmd::FindNode => self.open_node_finder(),
            Cmd::AutoKey => self.toggle_auto_key(),
            Cmd::CacheRange => self.cache_range(),
            Cmd::CopySettings => self.copy_settings(),
            Cmd::PasteSettings => self.paste_settings(),
            Cmd::AddNode => self.open_node_palette(ctx),
            Cmd::EffectLibrary => self.open_preset_library(ctx),
            Cmd::Play => self.toggle_play(),
            Cmd::StepBack => self.step_frame(-1),
            Cmd::StepForward => self.step_frame(1),
            Cmd::PreviousKey => self.seek_key(false),
            Cmd::NextKey => self.seek_key(true),
            Cmd::SetIn | Cmd::SetOut => {
                self.transport.pause();
                self.playback_controls.warming = None;
                if cmd == Cmd::SetIn {
                    self.playback_controls.set_in(self.project.frame, frames);
                } else {
                    self.playback_controls.set_out(self.project.frame, frames);
                }
            }
            Cmd::AddMarker => self.add_marker(),
            Cmd::PreviousMarker => self.go_to_marker(false),
            Cmd::NextMarker => self.go_to_marker(true),
            Cmd::PageCompose => self.go_to(pages::Page::Compose),
            Cmd::PageAnimate => self.go_to(pages::Page::Animate),
            Cmd::PageScene => self.go_to(pages::Page::Scene),
            Cmd::PageExport => self.go_to(pages::Page::Export),
            Cmd::ViewerFullScreen => self.set_viewer_fullscreen(ctx, !self.viewer_fullscreen),
            Cmd::ToggleEditor => self.node_editor_hidden = !self.node_editor_hidden,
            Cmd::ToggleLibrary => self.browser.visible = !self.browser.visible,
            Cmd::ToggleInspector => self.inspector_visible = !self.inspector_visible,
            Cmd::ToggleLayerBars => self.layer_bars.hidden = !self.layer_bars.hidden,
            Cmd::ToggleThumbnails => self.graph.show_thumbnails = !self.graph.show_thumbnails,
            Cmd::ToggleTimecode => {
                use crate::timecode::Display;
                let shown = &mut self.preferences.values.time_display;
                *shown = if *shown == Display::Timecode {
                    Display::Frames
                } else {
                    Display::Timecode
                };
                if let Err(error) = self.preferences.persist() {
                    self.fail(format!("Preferences: {error}"));
                }
            }
            Cmd::LayoutStandard
            | Cmd::LayoutWideViewer
            | Cmd::LayoutViewerOnly
            | Cmd::LayoutGraph => {
                let index = match cmd {
                    Cmd::LayoutStandard => 0,
                    Cmd::LayoutWideViewer => 1,
                    Cmd::LayoutViewerOnly => 2,
                    _ => 3,
                };
                self.apply_layout(&crate::layouts::Layout::built_in()[index]);
            }
            Cmd::SecondViewer => self.layouts.second_viewer = !self.layouts.second_viewer,
            Cmd::InterfaceLarger => self.scale_interface(ctx, true),
            Cmd::InterfaceSmaller => self.scale_interface(ctx, false),
            Cmd::PreviewSelected => {
                self.project.preview = Some(self.project.selected);
                self.request_preview();
            }
            Cmd::ViewOutput => {
                self.project.preview = None;
                self.request_preview();
            }
            Cmd::FitImage => self.navigation.fit(),
            Cmd::ActualPixels => self.navigation.actual_size(),
            Cmd::ChannelColour
            | Cmd::ChannelRed
            | Cmd::ChannelGreen
            | Cmd::ChannelBlue
            | Cmd::ChannelLuminance => {
                self.view.alpha = false;
                self.view.channel = match cmd {
                    Cmd::ChannelRed => Channel::Red,
                    Cmd::ChannelGreen => Channel::Green,
                    Cmd::ChannelBlue => Channel::Blue,
                    Cmd::ChannelLuminance => Channel::Luminance,
                    _ => Channel::Colour,
                };
            }
            Cmd::ChannelAlpha => self.view.alpha = true,
            Cmd::SafeAreas => self.overlays.safe = !self.overlays.safe,
            Cmd::Grid => self.overlays.grid = !self.overlays.grid,
            Cmd::Rulers => self.overlays.rulers = !self.overlays.rulers,
            Cmd::ColourReadout => self.overlays.readout = !self.overlays.readout,
            Cmd::CommandSearch => {
                self.commands.search = true;
                self.commands.query.clear();
                self.commands.chosen = 0;
                self.commands.focus = true;
            }
            Cmd::Shortcuts => self.commands.editor = true,
            Cmd::History => self.log.history = true,
            Cmd::Messages => self.log.messages = true,
            Cmd::About => self.about.begin(),
        }
    }
    /// The shortcut editor and the command search, when open.
    pub(crate) fn command_windows(&mut self, ctx: &egui::Context) {
        self.keys();
        self.shortcut_editor(ctx);
        self.command_search(ctx);
    }
    fn shortcut_editor(&mut self, ctx: &egui::Context) {
        if !self.commands.editor {
            self.commands.recording = None;
            return;
        }
        let mut open = true;
        let mut changed = false;
        egui::Window::new("Keyboard shortcuts")
            .open(&mut open)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .default_size([560.0, 620.0])
            .show(ctx, |ui| {
                let keys = self.commands.keys.as_mut().unwrap();
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.commands.filter)
                            .hint_text("Search commands")
                            .desired_width(260.0),
                    );
                    if ui
                        .add_enabled(keys.any_changed(), egui::Button::new("Reset all"))
                        .on_hover_text("Give every command its original shortcut")
                        .clicked()
                    {
                        keys.reset_all();
                        self.commands.notice.clear();
                        changed = true;
                    }
                });
                ui.weak("Click a shortcut, then press the new keys. Escape cancels.");
                if !self.commands.notice.is_empty() {
                    ui.colored_label(ui.visuals().warn_fg_color, &self.commands.notice);
                }
                ui.separator();
                let found = search(&self.commands.filter);
                egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(460.0).show(ui, |ui| {
                    let mut group = "";
                    egui::Grid::new("shortcut-grid").num_columns(3).striped(true).min_col_width(60.0).show(ui, |ui| {
                        for cmd in found {
                            if cmd.group() != group {
                                group = cmd.group();
                                ui.strong(group);
                                ui.end_row();
                            }
                            ui.label(cmd.label());
                            let recording = self.commands.recording == Some(cmd);
                            let text = if recording {
                                "Press keys".to_owned()
                            } else {
                                match keys.shown(cmd) {
                                    shown if shown.is_empty() => "None".to_owned(),
                                    shown => shown,
                                }
                            };
                            if ui
                                .add(egui::Button::new(egui::RichText::new(text).monospace()).selected(recording).min_size(egui::vec2(150.0, 0.0)))
                                .on_hover_text("Click, then press the keys for this command")
                                .clicked()
                            {
                                self.commands.recording = (!recording).then_some(cmd);
                            }
                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(keys.of(cmd).is_some(), egui::Button::new("Clear"))
                                    .on_hover_text("Leave this command without a shortcut")
                                    .clicked()
                                {
                                    keys.set(cmd, None);
                                    self.commands.notice.clear();
                                    changed = true;
                                }
                                if ui
                                    .add_enabled(keys.is_changed(cmd), egui::Button::new("Reset"))
                                    .on_hover_text("Back to the original shortcut")
                                    .clicked()
                                {
                                    self.commands.notice = keys.reset(cmd).map_or_else(String::new, |other| {
                                        format!("\"{}\" had that shortcut and now has none.", other.label())
                                    });
                                    changed = true;
                                }
                            });
                            ui.end_row();
                        }
                    });
                });
                ui.separator();
                ui.weak("Keys that belong to a tool while it is in use are fixed: Ctrl+Enter applies inspector changes, W, E and R move, rotate and scale in 3D, F frames the selection, Ctrl+C and Ctrl+V copy and paste nodes, and the question mark opens this window.");
            });
        if changed {
            self.keys_changed();
        }
        self.commands.editor = open;
    }
    fn command_search(&mut self, ctx: &egui::Context) {
        if !self.commands.search {
            return;
        }
        let mut run = None;
        let mut open = true;
        let found: Vec<Cmd> = search(&self.commands.query)
            .into_iter()
            .filter(|cmd| self.allows(*cmd))
            .collect();
        self.commands.chosen = self.commands.chosen.min(found.len().saturating_sub(1));
        egui::Window::new("Find a command")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_TOP)
            .fixed_pos(ctx.content_rect().center_top() + egui::vec2(0.0, 90.0))
            .show(ctx, |ui| {
                ui.set_width(460.0);
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.commands.query)
                        .hint_text("Type a few letters of what you want to do")
                        .desired_width(f32::INFINITY),
                );
                if std::mem::take(&mut self.commands.focus) {
                    field.request_focus();
                }
                if field.changed() {
                    self.commands.chosen = 0;
                }
                let (up, down, enter, escape) = ui.input(|i| {
                    (
                        i.key_pressed(egui::Key::ArrowUp),
                        i.key_pressed(egui::Key::ArrowDown),
                        i.key_pressed(egui::Key::Enter),
                        i.key_pressed(egui::Key::Escape),
                    )
                });
                if down && self.commands.chosen + 1 < found.len() {
                    self.commands.chosen += 1;
                }
                if up {
                    self.commands.chosen = self.commands.chosen.saturating_sub(1);
                }
                if escape {
                    self.commands.search = false;
                }
                if enter {
                    run = found.get(self.commands.chosen).copied();
                }
                ui.separator();
                let keys = self.commands.keys.as_ref().unwrap();
                egui::ScrollArea::vertical()
                    .max_height(330.0)
                    .show(ui, |ui| {
                        if found.is_empty() {
                            ui.weak("No command matches.");
                        }
                        for (row, cmd) in found.iter().enumerate() {
                            let chosen = row == self.commands.chosen;
                            let response = ui
                                .horizontal(|ui| {
                                    let response = ui.selectable_label(
                                        chosen,
                                        format!("{}  ·  {}", cmd.group(), cmd.label()),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.weak(
                                                egui::RichText::new(keys.shown(*cmd)).monospace(),
                                            );
                                        },
                                    );
                                    response
                                })
                                .inner;
                            if chosen && (up || down) {
                                response.scroll_to_me(None);
                            }
                            if response.clicked() {
                                run = Some(*cmd);
                            }
                        }
                    });
            });
        if let Some(cmd) = run {
            self.commands.search = false;
            self.run(cmd, ctx);
        } else if !open {
            self.commands.search = false;
        }
    }
}
