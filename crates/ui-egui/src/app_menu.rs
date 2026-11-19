//! ProseCraft's application menus (its Tauri `run` menu and `update_project_menu`), drawn as the
//! Craft apps draw their menu bar: the app menu (Settings, Keyboard Shortcuts, Quit), File (New
//! Scene, New Project, Import EPUB, Project Settings, Export Scene, Publish Manuscript, and the
//! projects), Edit (Undo to Select All) and View (Typewriter Mode, Fade Other Paragraphs, Zen
//! Mode).

use craft_menu::{
    Chord, CommandRef, Item, ItemKind, ItemRole, LayoutOptions, Menu, MenuBar, MenuBarUi, MenuRole,
    Node,
};

use crate::AppState;

/// A project in File, by its id (`switch-project-<id>`).
pub(crate) const SWITCH_PROJECT: &str = "switch-project-";

fn item(id: &str, label: &str, shortcut: Option<&str>) -> Node {
    let mut item = Item::new(CommandRef::new(id), label);
    if let Some(chord) = shortcut.and_then(|s| Chord::parse(s).ok()) {
        item = item.shortcut(chord);
    }
    Node::Item(item)
}

fn check(id: &str, label: &str, on: bool) -> Node {
    let mut item = Item::new(CommandRef::new(id), label);
    item.kind = ItemKind::Check(on);
    Node::Item(item)
}

/// The menus as the app is now: the projects listed with the open one ticked, and the View
/// items checked as the writing modes are.
pub(crate) fn bar(state: &AppState) -> MenuBar {
    let app = prosecraft_core::about::APP_NAME;
    let mut file = vec![
        item("new-scene", "New Scene", Some("Cmd+N")),
        item("new-project", "New Project…", None),
        item("import-epub", "Import EPUB…", None),
        item("project-settings", "Project Settings…", None),
        item("export-document", "Export Scene…", Some("Cmd+Shift+E")),
        item(
            "publish-manuscript",
            "Publish Manuscript…",
            Some("Cmd+Shift+P"),
        ),
        Node::Separator,
    ];
    let mut projects = Item::new(CommandRef::new("projects-label"), "Projects");
    projects.enabled = false;
    file.push(Node::Item(projects));
    if let Some(library) = state.library.as_ref() {
        for (index, project) in library
            .projects()
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.view.trashed)
        {
            let mut entry = Item::new(
                CommandRef::new(format!("{SWITCH_PROJECT}{}", project.view.id)),
                project.view.name.as_str(),
            );
            entry.kind = ItemKind::Radio(index == state.active);
            file.push(Node::Item(entry));
        }
    }
    MenuBar {
        menus: vec![
            Menu::new(
                app,
                vec![
                    item("settings", "Settings…", Some("Cmd+,")),
                    item("keyboard-shortcuts", "Keyboard Shortcuts", Some("Cmd+/")),
                    item("quit", &format!("Quit {app}"), Some("Cmd+Q")),
                ],
            ),
            Menu::new("File", file),
            Menu::new(
                "Edit",
                vec![
                    item("undo", "Undo", Some("Cmd+Z")),
                    item("redo", "Redo", Some("Cmd+Shift+Z")),
                    Node::Separator,
                    item("cut", "Cut", Some("Cmd+X")),
                    item("copy", "Copy", Some("Cmd+C")),
                    item("paste", "Paste", Some("Cmd+V")),
                    item("select-all", "Select All", Some("Cmd+A")),
                ],
            ),
            Menu::new(
                "View",
                vec![
                    check(
                        "typewriter-mode",
                        "Typewriter Mode",
                        state.writing.typewriter,
                    ),
                    check(
                        "fade-other-paragraphs",
                        "Fade Other Paragraphs",
                        state.writing.fade,
                    ),
                    check("zen-mode", "Zen Mode", state.writing.zen),
                ],
            ),
        ],
        documents: Vec::new(),
    }
}

/// The bar laid out as a Mac app's menus (ProseCraft's Tauri `run` menu on macOS): the app menu
/// with Settings and Quit in their places, File, Edit, View.
pub(crate) fn mac_layout(state: &AppState) -> craft_menu::Layout {
    let mut bar = bar(state);
    for (menu, role) in bar.menus.iter_mut().zip([
        MenuRole::App,
        MenuRole::File,
        MenuRole::Edit,
        MenuRole::View,
    ]) {
        menu.role = role;
    }
    bar.tag_item(&CommandRef::new("settings"), ItemRole::Settings);
    bar.tag_item(&CommandRef::new("quit"), ItemRole::Quit);
    craft_menu::mac_layout(&bar, &LayoutOptions::new(prosecraft_core::about::APP_NAME))
}

/// What may run from the native menus this frame.
pub(crate) fn gate(ctx: &egui::Context, state: &AppState) -> craft_menu::Gate {
    craft_menu::Gate {
        modal: state.settings_open || state.encrypt_export.is_some(),
        text_editing: ctx.egui_wants_keyboard_input(),
        ..Default::default()
    }
}

/// The bar for this frame, and the commands chosen from it. A dialog greys the app's items.
pub(crate) fn show(ui: &mut egui::Ui, state: &AppState) -> Vec<CommandRef> {
    let bar = bar(state);
    let gate = craft_menu::Gate {
        modal: state.settings_open || state.encrypt_export.is_some(),
        ..Default::default()
    };
    MenuBarUi::new(&bar).gate(gate).show(ui)
}

/// The input an Edit item stands for, to the text field that had the keys: copy, cut and
/// paste as egui's own events, the others as their keys.
pub(crate) fn edit_events(id: &str, paste: impl FnOnce() -> Option<String>) -> Vec<egui::Event> {
    use egui::{Event, Key, Modifiers as M};
    let key = |key: Key, modifiers: M| {
        vec![Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }]
    };
    match id {
        "copy" => vec![Event::Copy],
        "cut" => vec![Event::Cut],
        "paste" => paste().map(Event::Paste).into_iter().collect(),
        "undo" => key(Key::Z, M::COMMAND),
        "redo" => key(Key::Z, M::COMMAND | M::SHIFT),
        "select-all" => key(Key::A, M::COMMAND),
        _ => Vec::new(),
    }
}
