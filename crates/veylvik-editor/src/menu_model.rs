//! The editor's menus as data: the editor's own bar and a host's bar show one list, built from the editor's state each time, and an item is run by its id, checked against that list.
use crate::{Leave, Studio, ViewSettings, node_palette};
use eframe::egui;
use veylvik_document::{recipes::Recipe, registry};

/// One menu of the bar: File, Edit, Add, View, Tools or Help.
#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub title: String,
    pub items: Vec<MenuItem>,
}
/// An item as the bar shows it now. An item with `children` is a submenu and is not run
/// itself; one with `checked` is a setting that is on or off.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MenuItem {
    /// Stable for an item's meaning (`"file.import_media"`, `"add.node.blur"`).
    pub id: String,
    pub label: String,
    /// The key as the editor shows it, after the user's own shortcuts.
    pub shortcut: Option<String>,
    pub enabled: bool,
    pub checked: Option<bool>,
    pub children: Vec<MenuItem>,
    /// A line goes above this item.
    pub separator_before: bool,
    /// What the item does, for a tooltip.
    pub hint: Option<String>,
}

/// Items being listed: `gap` puts a line before the next one (never first or doubled).
#[derive(Default)]
struct List {
    items: Vec<MenuItem>,
    gap: bool,
}
impl List {
    fn gap(&mut self) {
        self.gap = !self.items.is_empty();
    }
    fn push(&mut self, mut item: MenuItem) -> &mut MenuItem {
        item.separator_before = std::mem::take(&mut self.gap);
        self.items.push(item);
        self.items.last_mut().unwrap()
    }
    fn item(
        &mut self,
        id: &str,
        label: &str,
        shortcut: Option<String>,
        enabled: bool,
    ) -> &mut MenuItem {
        self.push(MenuItem {
            id: id.into(),
            label: label.into(),
            shortcut: shortcut.filter(|s| !s.is_empty()),
            enabled,
            ..Default::default()
        })
    }
    fn check(&mut self, id: &str, label: &str, on: bool, enabled: bool) -> &mut MenuItem {
        let item = self.item(id, label, None, enabled);
        item.checked = Some(on);
        item
    }
    fn sub(&mut self, id: &str, label: &str, enabled: bool, children: List) -> &mut MenuItem {
        let item = self.item(id, label, None, enabled);
        item.children = children.items;
        item
    }
}
impl MenuItem {
    fn hint(&mut self, hint: &str) -> &mut Self {
        self.hint = Some(hint.into());
        self
    }
}

impl Studio {
    /// Nothing mid-gesture or waiting in the inspector: structural edits may run.
    fn menus_ready(&self) -> bool {
        !self.draft_dirty && !self.graph.dragging() && !self.animation_editor.dragging()
    }
    /// The menus as they are now: the same items, states and keys the bar shows.
    pub(crate) fn menus(&self, ctx: &egui::Context) -> Vec<Menu> {
        let key = |default: &str| Some(crate::command_ui::menu_text(ctx, default));
        let c = &self.capabilities;
        let ready = self.menus_ready();
        let snapshot = self.project.document.snapshot();
        let mut file = List::default();
        if c.project_files {
            file.item("file.start", "Start screen", None, true);
            file.gap();
            file.item("file.new", "New composition…", key("Ctrl+N"), true);
            file.item("file.new_from_media", "New from media…", None, true);
            file.item("file.open", "Open…", key("Ctrl+O"), true);
            file.gap();
            file.item("file.save", "Save", key("Ctrl+S"), true);
            file.item("file.save_as", "Save as…", key("Ctrl+Shift+S"), true);
        }
        if let Some(host) = self.host_link.host() {
            file.item(
                "file.update_host",
                &format!("Update in {host}"),
                key("Ctrl+Shift+U"),
                true,
            );
        }
        if c.project_files {
            file.item("file.recover", "Recover unsaved work…", None, true);
        }
        file.gap();
        file.item("file.import_media", "Import media…", key("Ctrl+I"), true);
        file.item(
            "file.import_sequence",
            "Import numbered sequence…",
            None,
            true,
        );
        file.item(
            "file.reload",
            "Reload source images",
            key("Ctrl+R"),
            !self.draft_dirty,
        );
        file.gap();
        if c.project_files {
            file.item("file.project_files", "Project files…", None, true);
        }
        if c.export {
            file.item("file.render_queue", "Render queue…", None, true);
            file.item("file.export_video", "Export video…", None, true);
            file.item(
                "file.export_frame",
                "Export current preview…",
                key("Ctrl+E"),
                true,
            );
            file.item(
                "file.export_sequence",
                "Export image sequence…",
                key("Ctrl+Shift+E"),
                true,
            );
        }
        file.gap();
        file.item(
            "file.composition_settings",
            "Composition settings…",
            None,
            !self.draft_dirty,
        );
        if c.own_window {
            file.gap();
            file.item("file.quit", "Quit", key("Q"), true);
        }

        let mut edit = List::default();
        // A host that keeps the history undoes through its own menu (Editor::undo).
        if !c.host_history {
            let history = self.project.document.history();
            edit.item(
                "edit.undo",
                "Undo",
                key("Ctrl+Z"),
                ready && history.undo_steps > 0,
            );
            edit.item(
                "edit.redo",
                "Redo",
                key("Ctrl+Shift+Z"),
                ready && history.redo_steps > 0,
            );
        }
        edit.item("edit.history", "Undo history…", None, true);
        edit.gap();
        edit.item(
            "edit.apply",
            "Apply inspector changes",
            key("Ctrl+Enter"),
            self.draft_dirty,
        );
        edit.item(
            "edit.revert",
            "Revert inspector changes",
            None,
            self.draft_dirty,
        );
        edit.gap();
        edit.item("edit.group", "Create effect group…", None, ready);
        edit.item(
            "edit.duplicate",
            "Duplicate selected node",
            key("Ctrl+D"),
            ready,
        );
        edit.item(
            "edit.duplicate_branch",
            "Duplicate upstream branch",
            key("Ctrl+Shift+D"),
            ready,
        );
        edit.item("edit.delete", "Delete selected node", None, ready);
        edit.item(
            "edit.set_output",
            "Set selected node as output",
            None,
            ready,
        );
        edit.gap();
        edit.item("edit.find", "Find node…", key("Ctrl+F"), ready);
        edit.item("edit.marker", "Add a marker here", key("M"), true);
        edit.item("edit.published", "Settings shown on Edit…", None, true)
            .hint("Settings a host such as Klipp shows on its Edit page, without opening Veylvik");
        edit.check("edit.auto_key", "Automatic keyframes", self.preferences.values.auto_key, true).hint(
            "On: changing an animated setting adds a keyframe at the current position. Off: such a change is a try-out until you add the keyframe",
        );
        edit.gap();
        edit.item("edit.preferences", "Preferences…", key("Ctrl+,"), true);

        let mut add = List::default();
        add.item("add.library", "Effect library…", None, ready);
        add.item("add.search", "Search nodes…", key("Tab"), ready);
        add.gap();
        add.item("add.image", "Image or footage…", key("Ctrl+I"), ready);
        add.item("add.text", "Text", None, ready);
        add.item("add.text_outlines", "Text outlines…", None, ready);
        add.gap();
        for (n, (label, kinds)) in crate::menus::ADD_MENU.iter().enumerate() {
            let mut group = List::default();
            for kind in *kinds {
                group.item(
                    &format!("add.node.{}", kind.type_id),
                    kind.label,
                    None,
                    ready,
                );
            }
            add.sub(&format!("add.group.{n}"), label, ready, group);
        }
        add.gap();
        for recipe in Recipe::ALL {
            add.item(
                &format!("add.recipe.{}", recipe.id()),
                recipe.label(),
                None,
                ready,
            );
        }

        let mut view = List::default();
        view.check(
            "view.node_editor",
            "Node / animation editor",
            !self.node_editor_hidden,
            true,
        );
        view.check("view.layer_bars", "Layer bars on the timeline", !self.layer_bars.hidden, true)
            .hint("A bar for each footage clip and fade under the ruler: drag to slide, drag an end to trim");
        let mut layouts = List::default();
        for (n, layout) in crate::layouts::Layout::built_in().iter().enumerate() {
            layouts.item(
                &format!("view.layout.builtin.{n}"),
                &layout.name,
                None,
                true,
            );
        }
        let saved = &self.preferences.values.layouts;
        layouts.gap();
        for layout in saved {
            layouts.item(
                &format!("view.layout.saved.{}", layout.name),
                &layout.name,
                None,
                true,
            );
        }
        if !saved.is_empty() {
            let mut remove = List::default();
            for layout in saved {
                remove
                    .item(
                        &format!("view.layout.remove.{}", layout.name),
                        &layout.name,
                        None,
                        true,
                    )
                    .hint("Forget this saved layout");
            }
            layouts.sub("view.layout.remove", "Remove a saved layout", true, remove);
        }
        layouts.gap();
        layouts.item("view.layout.save", "Save current layout as…", None, true);
        view.sub("view.layout", "Layout", true, layouts);
        if c.second_viewer {
            view.check("view.second_viewer", "Viewer in its own window", self.layouts.second_viewer, true)
                .hint("A second window with the same picture, for another screen. Double-click it for full screen");
        }
        if c.full_screen {
            view.item("view.full_screen", "Viewer full screen", key("F11"), true);
        }
        view.check("view.library", "Library panel", self.browser.visible, true);
        view.check(
            "view.inspector",
            "Inspector panel",
            self.inspector_visible,
            true,
        );
        view.item("view.preview_selected", "Preview selected node", None, true);
        view.item("view.output", "View composition output", None, true);
        view.gap();
        view.item("view.fit", "Fit image", None, true);
        view.item("view.actual", "Actual pixels (100%)", None, true);
        view.check("view.alpha", "Show alpha", self.view.alpha, true);
        view.item("view.colour_management", "Colour management…", None, true);
        let mut spaces = List::default();
        use veylvik_document::color_management::WorkingSpace;
        const SPACE: &str = "Compositing and grading use this space. Materials retain their authored colour domain; output remains linear Rec.709 for the viewer and existing exports.";
        for (space, id, label) in [
            (
                WorkingSpace::LinearRec709,
                "view.working_space.rec709",
                "Linear Rec.709",
            ),
            (
                WorkingSpace::Acescg,
                "view.working_space.acescg",
                "ACEScg (AP1)",
            ),
        ] {
            spaces
                .check(id, label, snapshot.working_space == space, true)
                .hint(SPACE);
        }
        view.sub("view.working_space", "Working colour space", true, spaces);
        view.check("view.filmic", "Filmic display (ACES-style)", !snapshot.display.is_standard(), true).hint(
            "Roll highlights off smoothly in the viewer, PNG and video instead of clipping at white. Saved with the composition; EXR stays scene-linear.",
        );
        view.item("view.reset_display", "Reset display settings", None, true);
        view.gap();
        view.item(
            "view.play",
            if self.playing { "Pause" } else { "Play" },
            key("Space"),
            true,
        );
        view.gap();
        view.check(
            "view.statistics",
            "Render statistics",
            self.statistics,
            true,
        );

        let mut tools = List::default();
        tools.item("tools.subject", "Select subject…", None, !self.files.busy());
        tools.gap();
        tools.item("tools.models", "AI models…", None, true);

        let mut help = List::default();
        help.item("help.commands", "Find a command…", key("Ctrl+K"), true);
        if c.preferences {
            help.item("help.shortcuts", "Keyboard shortcuts…", key("?"), true);
        }
        help.item("help.messages", "Messages…", None, true);
        if c.about {
            help.gap();
            help.item("help.about", "About Veylvik…", None, true);
        }
        [
            ("File", file),
            ("Edit", edit),
            ("Add", add),
            ("View", view),
            ("Tools", tools),
            ("Help", help),
        ]
        .into_iter()
        .map(|(title, list)| Menu {
            title: title.into(),
            items: list.items,
        })
        .collect()
    }
    /// Does what clicking item `id` does; false (and nothing happens) when the menus have
    /// no such item now, it is a submenu, or it is greyed out.
    pub(crate) fn run_menu_item(&mut self, ctx: &egui::Context, id: &str) -> bool {
        fn find<'a>(items: &'a [MenuItem], id: &str) -> Option<&'a MenuItem> {
            items.iter().find_map(|item| {
                if item.id == id {
                    Some(item)
                } else {
                    find(&item.children, id)
                }
            })
        }
        let menus = self.menus(ctx);
        let Some(item) = menus.iter().find_map(|menu| find(&menu.items, id)) else {
            return false;
        };
        if !item.enabled || !item.children.is_empty() {
            return false;
        }
        let at = || self.graph.insertion_position(ctx);
        match id {
            "file.start" => {
                self.show_start();
            }
            "file.new" => {
                self.leave(Leave::New, ctx);
            }
            "file.new_from_media" => {
                self.leave(Leave::NewFromMedia, ctx);
            }
            "file.open" => {
                self.leave(Leave::Open, ctx);
            }
            "file.save" => {
                self.save_project(false);
            }
            "file.save_as" => {
                self.save_project(true);
            }
            "file.update_host" => {
                self.update_host();
            }
            "file.recover" => {
                self.recovery.refresh();
                self.recovery.open = true;
            }
            "file.import_media" => {
                self.import_image(at());
            }
            "file.import_sequence" => {
                self.import_media(at(), true);
            }
            "file.reload" => {
                self.reload_images();
            }
            "file.project_files" => self.portable.open = true,
            "file.render_queue" => {
                self.go_to(crate::pages::Page::Export);
                self.pages.quick = false;
            }
            "file.export_video" => {
                self.ask_export(crate::pages::ExportKind::Video);
            }
            "file.export_frame" => {
                self.export_frame();
            }
            "file.export_sequence" => {
                self.ask_export(crate::pages::ExportKind::Sequence);
            }
            "file.composition_settings" => {
                self.playing = false;
                self.composition_controls
                    .begin(self.project.document.snapshot(), false);
            }
            "file.quit" => {
                self.leave(Leave::Close, ctx);
            }
            "edit.undo" => {
                self.history(false);
            }
            "edit.redo" => {
                self.history(true);
            }
            "edit.history" => self.log.history = true,
            "edit.apply" => {
                self.keep_tryout();
            }
            "edit.revert" => {
                self.refresh_draft();
                self.request_preview();
            }
            "edit.group" => {
                self.begin_group(false);
            }
            "edit.duplicate" => {
                self.duplicate_selected(ctx);
            }
            "edit.duplicate_branch" => {
                self.duplicate_branch();
            }
            "edit.delete" => {
                self.delete_selected();
            }
            "edit.set_output" => {
                self.set_selected_output();
            }
            "edit.find" => {
                self.open_node_finder();
            }
            "edit.published" => self.published_open = true,
            "edit.marker" => {
                self.add_marker();
            }
            "edit.auto_key" => {
                self.toggle_auto_key();
            }
            "edit.preferences" => {
                self.open_preferences();
            }
            "add.library" => {
                self.open_preset_library(ctx);
            }
            "add.search" => {
                self.open_node_palette(ctx);
            }
            "add.image" => {
                self.add_chosen(ctx, node_palette::Choice::Image);
            }
            "add.text" => {
                self.add_chosen(ctx, node_palette::Choice::Node(&registry::TEXT));
            }
            "add.text_outlines" => {
                self.add_chosen(ctx, node_palette::Choice::Text);
            }
            "view.node_editor" => self.node_editor_hidden = !self.node_editor_hidden,
            "view.layer_bars" => self.layer_bars.hidden = !self.layer_bars.hidden,
            "view.layout.save" => {
                self.layouts.naming = Some(String::new());
            }
            "view.second_viewer" => self.layouts.second_viewer = !self.layouts.second_viewer,
            "view.full_screen" => {
                self.set_viewer_fullscreen(ctx, true);
            }
            "view.library" => self.browser.visible = !self.browser.visible,
            "view.inspector" => self.inspector_visible = !self.inspector_visible,
            "view.preview_selected" => {
                self.project.preview = Some(self.project.selected);
                self.request_preview();
            }
            "view.output" => {
                self.project.preview = None;
                self.request_preview();
            }
            "view.fit" => {
                self.navigation.fit();
            }
            "view.actual" => {
                self.navigation.actual_size();
            }
            "view.alpha" => self.view.alpha = !self.view.alpha,
            "view.colour_management" => crate::color_controls::open(
                ctx,
                self.project.session,
                &self.project.document.snapshot().ocio,
            ),
            "view.working_space.rec709" | "view.working_space.acescg" => {
                use veylvik_document::color_management::WorkingSpace;
                let space = if id.ends_with("acescg") {
                    WorkingSpace::Acescg
                } else {
                    WorkingSpace::LinearRec709
                };
                if self.project.document.snapshot().working_space != space {
                    self.apply_menu_command(veylvik_document::Command::WorkingSpace { space });
                }
            }
            "view.filmic" => {
                use veylvik_document::media::DisplayTransform;
                let display = if self.project.document.snapshot().display.is_standard() {
                    DisplayTransform::Filmic
                } else {
                    DisplayTransform::Standard
                };
                self.apply_menu_command(veylvik_document::Command::Display { display });
            }
            "view.reset_display" => {
                self.view = ViewSettings {
                    fast_filter: self.preferences.values.fast_view,
                    ..Default::default()
                };
            }
            "view.play" => {
                self.toggle_play();
            }
            "view.statistics" => self.statistics = !self.statistics,
            "tools.subject" => {
                self.begin_subject();
            }
            "tools.models" => {
                self.models.show_window();
            }
            "help.commands" => {
                self.run(crate::commands::Cmd::CommandSearch, ctx);
            }
            "help.shortcuts" => self.commands.editor = true,
            "help.messages" => self.log.messages = true,
            "help.about" => {
                self.about.begin();
            }
            _ => {
                if let Some(type_id) = id.strip_prefix("add.node.") {
                    let Some(kind) = crate::menus::ADD_MENU
                        .iter()
                        .flat_map(|(_, kinds)| kinds.iter().copied())
                        .find(|kind| kind.type_id == type_id)
                    else {
                        return false;
                    };
                    self.add_chosen(ctx, node_palette::Choice::Node(kind));
                } else if let Some(recipe) = id.strip_prefix("add.recipe.") {
                    let Some(recipe) = Recipe::ALL.into_iter().find(|r| r.id() == recipe) else {
                        return false;
                    };
                    self.add_chosen(ctx, node_palette::Choice::Effect(recipe));
                } else if let Some(n) = id.strip_prefix("view.layout.builtin.") {
                    let Some(layout) = n
                        .parse::<usize>()
                        .ok()
                        .and_then(|n| crate::layouts::Layout::built_in().into_iter().nth(n))
                    else {
                        return false;
                    };
                    self.apply_layout(&layout);
                } else if let Some(name) = id.strip_prefix("view.layout.saved.") {
                    let Some(layout) = self
                        .preferences
                        .values
                        .layouts
                        .iter()
                        .find(|l| l.name == name)
                        .cloned()
                    else {
                        return false;
                    };
                    self.apply_layout(&layout);
                } else if let Some(name) = id.strip_prefix("view.layout.remove.") {
                    self.preferences.values.layouts.retain(|l| l.name != name);
                    if let Err(error) = self.preferences.persist() {
                        self.fail(format!("Preferences: {error}"));
                    }
                } else {
                    return false;
                }
            }
        }
        true
    }
    fn add_chosen(&mut self, ctx: &egui::Context, choice: node_palette::Choice) {
        self.playing = false;
        self.add_from_palette(choice, self.graph.insertion_position(ctx));
    }
    fn apply_menu_command(&mut self, command: veylvik_document::Command) {
        match self.project.apply(&[command]) {
            Ok(()) => self.request_preview(),
            Err(e) => self.fail(e),
        }
    }
    /// The workspaces the editor offers here, as (id, label): Compose, Animate, 3D, and
    /// Export when the editor exports itself.
    pub(crate) fn workspaces(&self) -> Vec<(String, String)> {
        crate::pages::Page::ALL
            .into_iter()
            .filter(|page| *page != crate::pages::Page::Export || self.capabilities.export)
            .map(|page| (page.id().into(), page.label().into()))
            .collect()
    }
    pub(crate) fn set_workspace(&mut self, id: &str) -> bool {
        let Some(page) = crate::pages::Page::ALL.into_iter().find(|page| {
            page.id() == id && (*page != crate::pages::Page::Export || self.capabilities.export)
        }) else {
            return false;
        };
        self.start.open = false;
        self.go_to(page);
        true
    }
}

impl crate::Editor {
    /// The editor's menus as its own bar would show them now, for a host that shows them
    /// in its bar (with `Capabilities::menu_bar` off). Build it again each frame the host
    /// draws its menus: items are greyed out, ticked and named by the editor's state.
    pub fn menus(&self, ctx: &egui::Context) -> Vec<Menu> {
        self.studio.menus(ctx)
    }
    /// Run the item `id` of [`Editor::menus`], as a click on it would, at the editor's
    /// next frame. False, and nothing is run, when no such item is offered now or it is
    /// greyed out or a submenu.
    pub fn run_menu_item(&mut self, ctx: &egui::Context, id: &str) -> bool {
        let found = self
            .studio
            .menus(ctx)
            .iter()
            .any(|menu| runnable(&menu.items, id));
        if found {
            self.studio.menu_runs.push(id.to_owned());
        }
        found
    }
    /// The workspaces (pages) as (id, label), for a host's own switcher.
    pub fn workspaces(&self) -> Vec<(String, String)> {
        self.studio.workspaces()
    }
    /// The workspace shown now.
    pub fn workspace(&self) -> String {
        self.studio.pages.page.id().into()
    }
    /// Show workspace `id`; false for an id [`Editor::workspaces`] does not offer.
    pub fn set_workspace(&mut self, id: &str) -> bool {
        self.studio.set_workspace(id)
    }
    /// The document's name as the editor's bar shows it (the host's name when it gave
    /// one with `set_name`); [`Editor::unsaved`] says whether it has changes.
    pub fn title(&self) -> String {
        self.studio.title_name()
    }
}
fn runnable(items: &[MenuItem], id: &str) -> bool {
    items.iter().any(|item| {
        (item.id == id && item.enabled && item.children.is_empty()) || runnable(&item.children, id)
    })
}

impl Studio {
    /// Items a host asked for, run as the editor's frame begins.
    pub(crate) fn run_asked_menu_items(&mut self, ctx: &egui::Context) {
        for id in std::mem::take(&mut self.menu_runs) {
            self.run_menu_item(ctx, &id);
        }
    }
}
