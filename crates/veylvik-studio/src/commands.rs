//! Every menu-level action as one list of commands: each has a name, a group and a default key. Keys are looked up here, can be changed in the shortcut editor (kept in preferences), and any command can be found and run by name. Keys that belong to a tool while it is active stay with that tool.
use eframe::egui;
use std::collections::BTreeMap;

macro_rules! commands {
    ($($variant:ident, $id:literal, $group:literal, $label:literal, $default:literal;)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub enum Cmd { $($variant),* }
        impl Cmd {
            pub const ALL: &'static [Cmd] = &[$(Cmd::$variant),*];
            /// The name a changed key is saved under; never reuse one for another command.
            pub fn id(self) -> &'static str { match self { $(Cmd::$variant => $id),* } }
            pub fn group(self) -> &'static str { match self { $(Cmd::$variant => $group),* } }
            pub fn label(self) -> &'static str { match self { $(Cmd::$variant => $label),* } }
            fn default_text(self) -> &'static str { match self { $(Cmd::$variant => $default),* } }
        }
    };
}
commands! {
    StartScreen, "start_screen", "File", "Start screen", "";
    New, "new", "File", "New composition", "Ctrl+N";
    NewFromMedia, "new_from_media", "File", "New from media", "";
    Open, "open", "File", "Open", "Ctrl+O";
    Save, "save", "File", "Save", "Ctrl+S";
    SaveAs, "save_as", "File", "Save as", "Ctrl+Shift+S";
    Import, "import", "File", "Import media", "Ctrl+I";
    ImportSequence, "import_sequence", "File", "Import numbered sequence", "";
    Reload, "reload", "File", "Reload source images", "Ctrl+R";
    ProjectFiles, "project_files", "File", "Project files", "";
    ExportVideo, "export_video", "File", "Export video", "";
    ExportFrame, "export_frame", "File", "Export current preview", "Ctrl+E";
    ExportSequence, "export_sequence", "File", "Export image sequence", "Ctrl+Shift+E";
    CompositionSettings, "composition_settings", "File", "Composition settings", "";
    Preferences, "preferences", "File", "Preferences", "Ctrl+Comma";
    Quit, "quit", "File", "Quit", "Q";
    Undo, "undo", "Edit", "Undo", "Ctrl+Z";
    Redo, "redo", "Edit", "Redo", "Ctrl+Shift+Z";
    History, "history", "Edit", "Undo history", "";
    Duplicate, "duplicate", "Edit", "Duplicate selected node", "Ctrl+D";
    DuplicateBranch, "duplicate_branch", "Edit", "Duplicate upstream branch", "Ctrl+Shift+D";
    DeleteNode, "delete_node", "Edit", "Delete selected node", "";
    SetOutput, "set_output", "Edit", "Set selected node as output", "";
    CreateGroup, "create_group", "Edit", "Create effect group", "";
    FindNode, "find_node", "Edit", "Find node", "Ctrl+F";
    AutoKey, "auto_key", "Edit", "Automatic keyframes on or off", "";
    AddNode, "add_node", "Add", "Search and add a node", "Tab";
    EffectLibrary, "effect_library", "Add", "Effect library", "";
    Play, "play", "Playback", "Play or pause", "Space";
    StepBack, "step_back", "Playback", "Previous frame", "Left";
    StepForward, "step_forward", "Playback", "Next frame", "Right";
    PreviousKey, "previous_key", "Playback", "Previous key of the selected node", "Alt+Left";
    NextKey, "next_key", "Playback", "Next key of the selected node", "Alt+Right";
    SetIn, "set_in", "Playback", "Set the range's start here", "I";
    SetOut, "set_out", "Playback", "Set the range's end here", "O";
    AddMarker, "add_marker", "Playback", "Add a marker here", "M";
    PreviousMarker, "previous_marker", "Playback", "Previous marker", "Comma";
    NextMarker, "next_marker", "Playback", "Next marker", "Period";
    PageCompose, "page_compose", "View", "Compose page", "Ctrl+1";
    PageAnimate, "page_animate", "View", "Animate page", "Ctrl+2";
    PageScene, "page_scene", "View", "3D page", "Ctrl+3";
    PageExport, "page_export", "View", "Export page", "Ctrl+4";
    ViewerFullScreen, "viewer_full_screen", "View", "Viewer full screen", "F11";
    ToggleEditor, "toggle_editor", "View", "Show or hide the node and animation editor", "";
    ToggleLibrary, "toggle_library", "View", "Show or hide the library panel", "";
    ToggleInspector, "toggle_inspector", "View", "Show or hide the inspector panel", "";
    ToggleLayerBars, "toggle_layer_bars", "View", "Show or hide layer bars on the timeline", "";
    ToggleThumbnails, "toggle_thumbnails", "View", "Show or hide thumbnails on nodes", "";
    ToggleTimecode, "toggle_timecode", "View", "Switch between timecode and frame numbers", "";
    PreviewSelected, "preview_selected", "View", "Preview the selected node", "";
    ViewOutput, "view_output", "View", "View the composition output", "";
    FitImage, "fit_image", "View", "Fit the image in the viewer", "";
    ActualPixels, "actual_pixels", "View", "Show the image at 100%", "";
    ChannelColour, "channel_colour", "View", "Viewer: show colour", "";
    ChannelRed, "channel_red", "View", "Viewer: show the red channel", "";
    ChannelGreen, "channel_green", "View", "Viewer: show the green channel", "";
    ChannelBlue, "channel_blue", "View", "Viewer: show the blue channel", "";
    ChannelLuminance, "channel_luminance", "View", "Viewer: show the luminance", "";
    ChannelAlpha, "channel_alpha", "View", "Viewer: show the alpha", "";
    SafeAreas, "safe_areas", "View", "Viewer: safe areas", "";
    Grid, "grid", "View", "Viewer: grid", "";
    Rulers, "rulers", "View", "Viewer: rulers", "";
    ColourReadout, "colour_readout", "View", "Viewer: colour under the pointer", "";
    CommandSearch, "command_search", "Help", "Find a command", "Ctrl+P";
    Shortcuts, "shortcuts", "Help", "Keyboard shortcuts", "";
    Messages, "messages", "Help", "Messages", "";
    About, "about", "Help", "About Veylvik", "";
}
/// A key with its modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: egui::Key,
}
impl Binding {
    /// "Ctrl+Shift+S": modifiers in that order, then the key's name.
    pub fn parse(text: &str) -> Option<Self> {
        let mut binding = Self {
            ctrl: false,
            shift: false,
            alt: false,
            key: egui::Key::Escape,
        };
        let mut parts = text.split('+').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_some() {
                match part {
                    "Ctrl" if !binding.ctrl => binding.ctrl = true,
                    "Shift" if !binding.shift => binding.shift = true,
                    "Alt" if !binding.alt => binding.alt = true,
                    _ => return None,
                }
            } else {
                binding.key = egui::Key::from_name(part)?;
            }
        }
        (binding.key != egui::Key::Escape).then_some(binding)
    }
    /// The form saved in preferences; `parse` reads it back.
    pub fn text(&self) -> String {
        self.written(self.key.name())
    }
    /// The form shown to people.
    pub fn shown(&self) -> String {
        self.written(match self.key {
            egui::Key::ArrowLeft => "Left",
            egui::Key::ArrowRight => "Right",
            egui::Key::ArrowUp => "Up",
            egui::Key::ArrowDown => "Down",
            egui::Key::Comma => ",",
            egui::Key::Period => ".",
            key => key.name(),
        })
    }
    fn written(&self, key: &str) -> String {
        let mut text = String::new();
        for (held, name) in [
            (self.ctrl, "Ctrl+"),
            (self.shift, "Shift+"),
            (self.alt, "Alt+"),
        ] {
            if held {
                text.push_str(name);
            }
        }
        text + key
    }
    fn modifiers(&self) -> egui::Modifiers {
        let mut modifiers = egui::Modifiers::NONE;
        if self.ctrl {
            modifiers |= egui::Modifiers::CTRL;
        }
        if self.shift {
            modifiers |= egui::Modifiers::SHIFT;
        }
        if self.alt {
            modifiers |= egui::Modifiers::ALT;
        }
        modifiers
    }
    fn count(&self) -> usize {
        usize::from(self.ctrl) + usize::from(self.shift) + usize::from(self.alt)
    }
    /// Whether it was pressed this frame; the press is used up.
    pub fn pressed(&self, ctx: &egui::Context) -> bool {
        ctx.input_mut(|i| i.consume_key(self.modifiers(), self.key))
    }
}
/// The key of every command: its default unless changed.
#[derive(Debug, Clone, PartialEq)]
pub struct Keys {
    changed: BTreeMap<Cmd, Option<Binding>>,
}
impl Keys {
    /// From what preferences hold: command id to key text, empty for no key. Entries
    /// that name no command or no key are ignored.
    pub fn load(saved: &BTreeMap<String, String>) -> Self {
        let mut changed = BTreeMap::new();
        for cmd in Cmd::ALL {
            if let Some(text) = saved.get(cmd.id()) {
                if text.is_empty() {
                    changed.insert(*cmd, None);
                } else if let Some(binding) = Binding::parse(text) {
                    changed.insert(*cmd, Some(binding));
                }
            }
        }
        let mut keys = Self { changed };
        keys.tidy();
        keys
    }
    pub fn saved(&self) -> BTreeMap<String, String> {
        self.changed
            .iter()
            .map(|(cmd, binding)| {
                (
                    cmd.id().to_owned(),
                    binding.map_or_else(String::new, |b| b.text()),
                )
            })
            .collect()
    }
    fn default(cmd: Cmd) -> Option<Binding> {
        Binding::parse(cmd.default_text())
    }
    fn tidy(&mut self) {
        self.changed
            .retain(|cmd, binding| *binding != Self::default(*cmd));
    }
    pub fn of(&self, cmd: Cmd) -> Option<Binding> {
        self.changed
            .get(&cmd)
            .copied()
            .unwrap_or_else(|| Self::default(cmd))
    }
    pub fn is_changed(&self, cmd: Cmd) -> bool {
        self.changed.contains_key(&cmd)
    }
    pub fn any_changed(&self) -> bool {
        !self.changed.is_empty()
    }
    /// What `cmd`'s key reads as, or nothing.
    pub fn shown(&self, cmd: Cmd) -> String {
        self.of(cmd).map_or_else(String::new, |b| b.shown())
    }
    /// The command that has `binding` now.
    pub fn holder(&self, binding: Binding) -> Option<Cmd> {
        Cmd::ALL
            .iter()
            .copied()
            .find(|cmd| self.of(*cmd) == Some(binding))
    }
    /// Give `cmd` this key (or none). A command that had the key loses it and is returned.
    pub fn set(&mut self, cmd: Cmd, binding: Option<Binding>) -> Option<Cmd> {
        let loser = binding
            .and_then(|b| self.holder(b))
            .filter(|other| *other != cmd);
        if let Some(loser) = loser {
            self.changed.insert(loser, None);
        }
        self.changed.insert(cmd, binding);
        self.tidy();
        loser
    }
    /// Back to the default; a command holding that default key loses it and is returned.
    pub fn reset(&mut self, cmd: Cmd) -> Option<Cmd> {
        self.set(cmd, Self::default(cmd))
    }
    pub fn reset_all(&mut self) {
        self.changed.clear();
    }
    /// Commands with a key, those with more modifiers first, so Ctrl+Shift+S is tried
    /// before Ctrl+S.
    pub fn order(&self) -> Vec<(Cmd, Binding)> {
        let mut bound: Vec<(Cmd, Binding)> = Cmd::ALL
            .iter()
            .filter_map(|cmd| self.of(*cmd).map(|b| (*cmd, b)))
            .collect();
        bound.sort_by_key(|(_, binding)| std::cmp::Reverse(binding.count()));
        bound
    }
    /// Default key text to current key text, for menus written with the defaults.
    pub fn menu_texts(&self) -> BTreeMap<String, String> {
        Cmd::ALL
            .iter()
            .filter_map(|cmd| Self::default(*cmd).map(|b| (b.shown(), self.shown(*cmd))))
            .collect()
    }
}
/// The commands matching every word of `query`, in list order.
pub fn search(query: &str) -> Vec<Cmd> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    Cmd::ALL
        .iter()
        .copied()
        .filter(|cmd| {
            let text = format!("{} {}", cmd.group(), cmd.label()).to_lowercase();
            words.iter().all(|word| text.contains(word))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_command_is_distinct_and_default_keys_are_valid_and_unique() {
        let mut ids = std::collections::BTreeSet::new();
        let mut keys = std::collections::BTreeSet::new();
        for cmd in Cmd::ALL {
            assert!(ids.insert(cmd.id()), "{} is used twice", cmd.id());
            assert!(!cmd.label().is_empty() && !cmd.group().is_empty());
            let text = cmd.default_text();
            if !text.is_empty() {
                let binding = Binding::parse(text).unwrap_or_else(|| panic!("{text} is not a key"));
                assert_eq!(binding.text(), text, "written the way it is read");
                assert!(keys.insert(text), "{text} is the default of two commands");
            }
        }
        assert_eq!(ids.len(), Cmd::ALL.len());
    }
    #[test]
    fn keys_parse_strictly_change_with_conflicts_and_save_only_what_changed() {
        for bad in [
            "",
            "Ctrl+",
            "Ctrl+Ctrl+S",
            "Shift",
            "Hyper+S",
            "Ctrl+Nonsense",
            "S+Ctrl",
            "Escape",
        ] {
            assert!(Binding::parse(bad).is_none(), "{bad}");
        }
        let save = Binding::parse("Ctrl+S").unwrap();
        assert!(save.ctrl && !save.shift && !save.alt && save.key == egui::Key::S);
        assert_eq!(Binding::parse("Alt+Left").unwrap().shown(), "Alt+Left");
        assert_eq!(Binding::parse("Ctrl+Comma").unwrap().shown(), "Ctrl+,");
        let mut keys = Keys::load(&BTreeMap::new());
        assert!(!keys.any_changed() && keys.saved().is_empty());
        assert_eq!(keys.shown(Cmd::Save), "Ctrl+S");
        assert_eq!(keys.shown(Cmd::StartScreen), "");
        // Taking another command's key leaves that command without one.
        assert_eq!(keys.set(Cmd::ExportVideo, Some(save)), Some(Cmd::Save));
        assert_eq!(keys.of(Cmd::Save), None);
        assert_eq!(keys.holder(save), Some(Cmd::ExportVideo));
        let saved = keys.saved();
        assert_eq!(saved.len(), 2);
        assert_eq!(
            (saved["export_video"].as_str(), saved["save"].as_str()),
            ("Ctrl+S", "")
        );
        assert_eq!(Keys::load(&saved), keys);
        // Putting Save back takes the key from the command that had borrowed it.
        assert_eq!(keys.reset(Cmd::Save), Some(Cmd::ExportVideo));
        // Export video has no key by default, so everything is back as it was.
        assert_eq!(keys.of(Cmd::ExportVideo), None);
        assert!(!keys.any_changed());
        // More modifiers are tried first; unknown or broken saved entries are ignored.
        let order = keys.order();
        let at = |cmd| order.iter().position(|(c, _)| *c == cmd).unwrap();
        assert!(at(Cmd::SaveAs) < at(Cmd::Save) && at(Cmd::Redo) < at(Cmd::Undo));
        let odd: BTreeMap<String, String> = [
            ("no_such".into(), "Ctrl+J".into()),
            ("save".into(), "Banana".into()),
        ]
        .into();
        assert!(!Keys::load(&odd).any_changed());
        assert_eq!(keys.menu_texts()["Ctrl+S"], "Ctrl+S");
        keys.set(Cmd::Save, None);
        assert_eq!(keys.menu_texts()["Ctrl+S"], "");
    }
    #[test]
    fn search_matches_every_word_in_a_group_or_a_name() {
        assert_eq!(search("save as"), vec![Cmd::SaveAs]);
        assert!(search("EXPORT").contains(&Cmd::ExportVideo) && search("export").len() >= 4);
        assert_eq!(search("viewer red"), vec![Cmd::ChannelRed]);
        assert!(search("zzz").is_empty());
        assert_eq!(search("").len(), Cmd::ALL.len());
    }
}
