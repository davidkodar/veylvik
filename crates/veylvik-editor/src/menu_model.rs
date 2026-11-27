//! The editor's menus as data: the editor's own bar and a host's bar show one list, built from the editor's state each time, and an item is run by its id, checked against that list.
use crate::{Studio, ViewSettings, commands::Cmd, node_palette};
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
    /// The command the item runs, when it is one: its label, key, gate and action then
    /// all come from the command table.
    pub(crate) cmd: Option<Cmd>,
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
    /// The item for `cmd`, unless this editor does not offer it (a host's capabilities).
    fn cmd(&mut self, studio: &Studio, id: &str, cmd: Cmd) -> Option<&mut MenuItem> {
        if !studio.allows(cmd) {
            return None;
        }
        let item = self.item(
            id,
            cmd.menu_label(),
            Some(studio.key_text(cmd)),
            studio.ready_for(cmd),
        );
        item.cmd = Some(cmd);
        Some(item)
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
    fn ticked(&mut self, on: bool) -> &mut Self {
        self.checked = Some(on);
        self
    }
    fn hint(&mut self, hint: &str) -> &mut Self {
        self.hint = Some(hint.into());
        self
    }
}

impl Studio {
    /// The menus as they are now: the same items, states and keys the bar shows.
    pub(crate) fn menus(&self) -> Vec<Menu> {
        let c = &self.capabilities;
        let ready = self.ready_for(Cmd::Duplicate);
        let snapshot = self.project.document.snapshot();
        let mut file = List::default();
        file.cmd(self, "file.start", Cmd::StartScreen);
        file.gap();
        file.cmd(self, "file.new", Cmd::New);
        file.cmd(self, "file.new_from_media", Cmd::NewFromMedia);
        file.cmd(self, "file.import_nuke", Cmd::ImportNuke);
        file.cmd(self, "file.open", Cmd::Open);
        file.gap();
        file.cmd(self, "file.save", Cmd::Save);
        file.cmd(self, "file.save_as", Cmd::SaveAs);
        if let Some(host) = self.host_link.host()
            && let Some(item) = file.cmd(self, "file.update_host", Cmd::UpdateHost)
        {
            item.label = format!("Update in {host}");
        }
        if c.project_files {
            file.item("file.recover", "Recover unsaved work…", None, true);
        }
        file.gap();
        file.cmd(self, "file.import_media", Cmd::Import);
        file.cmd(self, "file.import_sequence", Cmd::ImportSequence);
        file.cmd(self, "file.reload", Cmd::Reload);
        file.gap();
        file.cmd(self, "file.project_files", Cmd::ProjectFiles);
        if c.export {
            file.item("file.render_queue", "Render queue…", None, true);
        }
        file.cmd(self, "file.export_video", Cmd::ExportVideo);
        file.cmd(self, "file.export_frame", Cmd::ExportFrame);
        file.cmd(self, "file.export_sequence", Cmd::ExportSequence);
        file.gap();
        file.cmd(self, "file.composition_settings", Cmd::CompositionSettings);
        file.gap();
        file.cmd(self, "file.quit", Cmd::Quit);

        let mut edit = List::default();
        // A host that keeps the history undoes through its own menu (Editor::undo).
        if !c.host_history {
            let history = self.project.document.history();
            if let Some(undo) = edit.cmd(self, "edit.undo", Cmd::Undo) {
                undo.enabled &= history.undo_steps > 0;
            }
            if let Some(redo) = edit.cmd(self, "edit.redo", Cmd::Redo) {
                redo.enabled &= history.redo_steps > 0;
            }
        }
        edit.cmd(self, "edit.history", Cmd::History);
        edit.gap();
        edit.item(
            "edit.apply",
            "Apply inspector changes",
            Some("Ctrl+Enter".into()),
            self.draft_dirty,
        );
        edit.item(
            "edit.revert",
            "Revert inspector changes",
            None,
            self.draft_dirty,
        );
        edit.gap();
        edit.cmd(self, "edit.group", Cmd::CreateGroup);
        edit.cmd(self, "edit.duplicate", Cmd::Duplicate);
        edit.cmd(self, "edit.duplicate_branch", Cmd::DuplicateBranch);
        edit.cmd(self, "edit.delete", Cmd::DeleteNode);
        edit.cmd(self, "edit.set_output", Cmd::SetOutput);
        edit.gap();
        edit.cmd(self, "edit.find", Cmd::FindNode);
        edit.cmd(self, "edit.marker", Cmd::AddMarker);
        edit.item("edit.published", "Settings shown on Edit…", None, true)
            .hint("Settings a host such as Klipp shows on its Edit page, without opening Veylvik");
        if let Some(item) = edit.cmd(self, "edit.auto_key", Cmd::AutoKey) {
            item.ticked(self.preferences.values.auto_key).hint(
                "On: changing an animated setting adds a keyframe at the current position. Off: such a change is a try-out until you add the keyframe",
            );
        }
        edit.gap();
        edit.cmd(self, "edit.preferences", Cmd::Preferences);

        let mut add = List::default();
        add.cmd(self, "add.library", Cmd::EffectLibrary);
        add.cmd(self, "add.search", Cmd::AddNode);
        add.gap();
        add.item(
            "add.image",
            "Image or footage…",
            Some(self.key_text(Cmd::Import)),
            ready,
        );
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
        if let Some(item) = view.cmd(self, "view.node_editor", Cmd::ToggleEditor) {
            item.ticked(!self.node_editor_hidden);
        }
        if let Some(item) = view.cmd(self, "view.layer_bars", Cmd::ToggleLayerBars) {
            item.ticked(!self.layer_bars.hidden).hint(
                "A bar for each footage clip and fade under the ruler: drag to slide, drag an end to trim",
            );
        }
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
        if let Some(item) = view.cmd(self, "view.second_viewer", Cmd::SecondViewer) {
            item.ticked(self.layouts.second_viewer).hint(
                "A second window with the same picture, for another screen. Double-click it for full screen",
            );
        }
        view.cmd(self, "view.full_screen", Cmd::ViewerFullScreen);
        if let Some(item) = view.cmd(self, "view.library", Cmd::ToggleLibrary) {
            item.ticked(self.browser.visible);
        }
        if let Some(item) = view.cmd(self, "view.inspector", Cmd::ToggleInspector) {
            item.ticked(self.inspector_visible);
        }
        view.cmd(self, "view.preview_selected", Cmd::PreviewSelected);
        view.cmd(self, "view.output", Cmd::ViewOutput);
        view.gap();
        view.cmd(self, "view.fit", Cmd::FitImage);
        view.cmd(self, "view.actual", Cmd::ActualPixels);
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
        if let Some(item) = view.cmd(self, "view.play", Cmd::Play) {
            item.label = if self.transport.playing {
                "Pause"
            } else {
                "Play"
            }
            .into();
        }
        view.gap();
        view.check(
            "view.statistics",
            "Render statistics",
            self.statistics,
            true,
        );

        let mut tools = List::default();
        tools.item("tools.subject", "Select subject…", None, !self.files.busy());
        tools.item("tools.compare_look", "Compare look…", None, true);
        tools.gap();
        tools.item("tools.models", "AI models…", None, true);

        let mut help = List::default();
        help.cmd(self, "help.commands", Cmd::CommandSearch);
        if let Some(item) = help.cmd(self, "help.shortcuts", Cmd::Shortcuts) {
            // The question mark is a fixed key of its own (Help).
            item.shortcut = Some("?".into());
        }
        help.cmd(self, "help.messages", Cmd::Messages);
        if self.allows(Cmd::About) {
            help.gap();
        }
        help.cmd(self, "help.about", Cmd::About);
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
        let menus = self.menus();
        let Some(item) = menus.iter().find_map(|menu| find(&menu.items, id)) else {
            return false;
        };
        if !item.enabled || !item.children.is_empty() {
            return false;
        }
        if let Some(cmd) = item.cmd {
            self.run(cmd, ctx);
            return true;
        }
        match id {
            "file.recover" => {
                self.recovery.refresh();
                self.recovery.open = true;
            }
            "file.render_queue" => {
                self.go_to(crate::pages::Page::Export);
                self.pages.quick = false;
            }
            "edit.apply" => {
                self.keep_tryout();
            }
            "edit.revert" => {
                self.refresh_draft();
                self.request_preview();
            }
            "edit.published" => self.published_open = true,
            "add.image" => {
                self.add_chosen(ctx, node_palette::Choice::Image);
            }
            "add.text" => {
                self.add_chosen(ctx, node_palette::Choice::Node(&registry::TEXT));
            }
            "add.text_outlines" => {
                self.add_chosen(ctx, node_palette::Choice::Text);
            }
            "view.layout.save" => {
                self.layouts.naming = Some(String::new());
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
            "view.statistics" => self.statistics = !self.statistics,
            "tools.subject" => {
                self.begin_subject();
            }
            "tools.models" => {
                self.models.show_window();
            }
            "tools.compare_look" => self.look.open = true,
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
        self.transport.pause();
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
    /// (`_ctx` is kept so hosts need not change; the menus no longer read it.)
    pub fn menus(&self, _ctx: &egui::Context) -> Vec<Menu> {
        self.studio.menus()
    }
    /// Run the item `id` of [`Editor::menus`], as a click on it would, at the editor's
    /// next frame. False, and nothing is run, when no such item is offered now or it is
    /// greyed out or a submenu.
    pub fn run_menu_item(&mut self, _ctx: &egui::Context, id: &str) -> bool {
        let found = self
            .studio
            .menus()
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
