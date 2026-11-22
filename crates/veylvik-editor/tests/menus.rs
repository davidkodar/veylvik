//! A host lists the editor's menus as its bar would show them, runs an item by its id, and switches workspaces; what the editor leaves to the host is not offered.
use eframe::egui;
use veylvik_document::Time;
use veylvik_editor::menu_model::{Menu, MenuItem};
use veylvik_editor::{Capabilities, Editor, HOST_INTERFACE, LiveControl, Opens, Setup};

fn editor(context: &egui::Context, capabilities: Capabilities) -> Editor {
    let (_instance, adapter, device, queue) =
        pollster::block_on(veylvik_render::create_gpu()).unwrap();
    Editor::new(Setup {
        interface: HOST_INTERFACE,
        context: context.clone(),
        device,
        queue,
        adapter: adapter.get_info(),
        target_format: wgpu::TextureFormat::Rgba8UnormSrgb,
        opens: Opens::Composition {
            composition: Box::new(
                veylvik_document::blank(64, 36, Time::new(24, 1).unwrap()).unwrap(),
            ),
            assets: std::env::temp_dir(),
        },
        live_control: LiveControl::Off,
        capabilities,
        host: None,
        renderer: None,
    })
    .unwrap()
}
fn pass(context: &egui::Context, editor: &mut Editor) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 720.0),
        )),
        ..Default::default()
    };
    let _ = context.run_ui(input, |ui| {
        egui::CentralPanel::default().show_inside(ui, |ui| editor.ui(ui));
    });
}
fn all(items: &[MenuItem]) -> Vec<&MenuItem> {
    items
        .iter()
        .flat_map(|item| std::iter::once(item).chain(all(&item.children)))
        .collect()
}
fn find<'a>(menus: &'a [Menu], id: &str) -> Option<&'a MenuItem> {
    menus
        .iter()
        .flat_map(|menu| all(&menu.items))
        .find(|item| item.id == id)
}

#[test]
#[ignore = "requires Vulkan"]
fn a_host_lists_the_menus_and_runs_an_item_by_its_id() {
    let context = egui::Context::default();
    let mut editor = editor(&context, Capabilities::embedded());
    pass(&context, &mut editor);
    let menus = editor.menus(&context);
    let titles: Vec<&str> = menus.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(titles, ["File", "Edit", "Add", "View", "Tools", "Help"]);
    // Every item is named once, has a label, and no line comes first in a list.
    let items: Vec<&MenuItem> = menus.iter().flat_map(|m| all(&m.items)).collect();
    let mut ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    ids.sort_unstable();
    let count = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), count, "an id is used twice");
    assert!(items.iter().all(|i| !i.label.is_empty()));
    fn no_leading_line(items: &[MenuItem]) -> bool {
        items.first().is_none_or(|i| !i.separator_before)
            && items.iter().all(|i| no_leading_line(&i.children))
    }
    assert!(menus.iter().all(|m| no_leading_line(&m.items)));
    // What the host keeps is not offered: files, export, quitting, and its history.
    for gone in [
        "file.save",
        "file.open",
        "file.quit",
        "file.export_video",
        "help.about",
        "edit.undo",
        "edit.redo",
    ] {
        assert!(find(&menus, gone).is_none(), "{gone} is offered to a host");
    }
    // The Add menu's node types are there as submenus, with their keys shown.
    let blur = find(&menus, "add.node.blur").expect("blur in the Add menu");
    assert_eq!(blur.label, "Blur");
    assert!(find(&menus, "add.group.0").is_some_and(|g| !g.children.is_empty()));
    assert_eq!(
        find(&menus, "file.import_media")
            .unwrap()
            .shortcut
            .as_deref(),
        Some("Ctrl+I")
    );

    // An item runs at the next frame, and the menus show its effect.
    assert_eq!(
        find(&menus, "view.statistics").unwrap().checked,
        Some(false)
    );
    assert!(editor.run_menu_item(&context, "view.statistics"));
    assert_eq!(
        find(&editor.menus(&context), "view.statistics")
            .unwrap()
            .checked,
        Some(false)
    );
    pass(&context, &mut editor);
    assert_eq!(
        find(&editor.menus(&context), "view.statistics")
            .unwrap()
            .checked,
        Some(true)
    );
    // An unknown id, a submenu and an item not offered run nothing.
    assert!(!editor.run_menu_item(&context, "no.such.item"));
    assert!(!editor.run_menu_item(&context, "add.group.0"));
    assert!(!editor.run_menu_item(&context, "file.quit"));
    // Greyed out: nothing to apply in the inspector.
    assert!(!find(&editor.menus(&context), "edit.apply").unwrap().enabled);
    assert!(!editor.run_menu_item(&context, "edit.apply"));

    // Adding a node through the menu adds one, as one undoable change.
    let before = editor.composition().nodes.len();
    assert!(editor.run_menu_item(&context, "add.node.solid"));
    pass(&context, &mut editor);
    assert_eq!(editor.composition().nodes.len(), before + 1);
}

#[test]
#[ignore = "requires Vulkan"]
fn a_host_switches_workspaces_and_reads_the_title() {
    let context = egui::Context::default();
    let mut editor = editor(&context, Capabilities::embedded());
    pass(&context, &mut editor);
    let ids: Vec<String> = editor.workspaces().into_iter().map(|(id, _)| id).collect();
    assert_eq!(
        ids,
        ["compose", "animate", "scene"],
        "no Export page without the editor's export"
    );
    assert_eq!(editor.workspace(), "compose");
    assert!(editor.set_workspace("scene"));
    assert_eq!(editor.workspace(), "scene");
    assert!(!editor.set_workspace("export"));
    assert!(!editor.set_workspace("nowhere"));
    assert_eq!(editor.workspace(), "scene");
    editor.set_name(Some("Beach shot".into()));
    assert_eq!(editor.title(), "Beach shot");
}

#[test]
#[ignore = "requires Vulkan"]
fn undo_is_offered_when_the_editor_keeps_its_own_history() {
    let context = egui::Context::default();
    let mut editor = editor(
        &context,
        Capabilities {
            host_history: false,
            menu_bar: true,
            ..Capabilities::embedded()
        },
    );
    pass(&context, &mut editor);
    let menus = editor.menus(&context);
    let undo = find(&menus, "edit.undo").expect("undo offered");
    assert!(!undo.enabled, "nothing to undo yet");
    assert!(editor.run_menu_item(&context, "add.node.solid"));
    pass(&context, &mut editor);
    assert!(find(&editor.menus(&context), "edit.undo").unwrap().enabled);
    let added = editor.composition().nodes.len();
    assert!(editor.run_menu_item(&context, "edit.undo"));
    pass(&context, &mut editor);
    assert_eq!(editor.composition().nodes.len(), added - 1);
}
