//! The app's own library: backups are added to it and it is saved; it reopens; it exports.

use std::sync::{Arc, Mutex};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use prosecraft_ui_egui::{HostRequest, LibraryStore, ProjectLibrary, ProseApp};

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/../core/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Saves into shared memory, or fails on demand.
#[derive(Clone, Default)]
struct Memory {
    saved: Arc<Mutex<Vec<String>>>,
    fail: bool,
    ids: Arc<std::sync::atomic::AtomicUsize>,
    settings: Arc<Mutex<std::collections::HashMap<String, String>>>,
}

impl LibraryStore for Memory {
    fn load_setting(&mut self, key: &str) -> Option<String> {
        self.settings.lock().ok()?.get(key).cloned()
    }
    fn save_setting(&mut self, key: &str, value: Option<&str>) -> Result<(), String> {
        let mut settings = self.settings.lock().map_err(|e| e.to_string())?;
        match value {
            Some(v) => settings.insert(key.to_owned(), v.to_owned()),
            None => settings.remove(key),
        };
        Ok(())
    }
    fn save(&mut self, text: &str) -> Result<(), String> {
        if self.fail {
            return Err("the disk is full".into());
        }
        self.saved
            .lock()
            .map_err(|e| e.to_string())?
            .push(text.to_owned());
        Ok(())
    }
    fn now_ms(&self) -> f64 {
        1_800_000_000_000.0
    }
    fn new_id(&mut self) -> String {
        format!(
            "id-{}",
            self.ids.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        )
    }
}

fn harness(app: ProseApp) -> Harness<'static, ProseApp> {
    let harness = Harness::builder()
        .with_size([1280.0, 850.0])
        .build_ui_state(|ui, app: &mut ProseApp| app.show(ui), app);
    prosecraft_ui_egui::setup_for_tests(&harness.ctx);
    harness
}

#[test]
fn an_opened_backup_joins_the_library_which_reopens() {
    let memory = Memory::default();
    let mut app = ProseApp::with_library(
        ProjectLibrary::new("this-device".into()),
        Box::new(memory.clone()),
    );
    app.open_backup(
        "bundle-plain.prosecraft",
        &fixture("bundle-plain.prosecraft"),
    );
    let saved = memory
        .saved
        .lock()
        .unwrap()
        .last()
        .cloned()
        .expect("the library was saved");

    let reopened = ProjectLibrary::from_text(&saved).expect("the saved library reads back");
    assert_eq!(reopened.device_id, "this-device");
    assert_eq!(reopened.projects().count(), 2);
    let mut harness = harness(ProseApp::with_library(
        reopened,
        Box::new(Memory::default()),
    ));
    harness.run();
    harness.get_by_role_and_label(Role::TreeItem, "Opening Scene");
    harness.get_by_label_contains("Your library: 2 projects.");
}

#[test]
fn an_older_copy_does_not_replace_the_newer_one() {
    let memory = Memory::default();
    let mut library = ProjectLibrary::new("d".into());
    let mut newer: Vec<_> = prosecraft_ui_egui::Library::read(&fixture("bundle-plain.prosecraft"))
        .ok()
        .unwrap_or_default();
    for project in &mut newer {
        if let Some(p) = project.as_object_mut() {
            p.insert("updatedAt", prosecraft_core::js::JsValue::Number(9e12));
        }
    }
    library.merge(newer);
    let mut app = ProseApp::with_library(library, Box::new(memory.clone()));
    app.open_backup(
        "bundle-plain.prosecraft",
        &fixture("bundle-plain.prosecraft"),
    );
    assert!(
        app.state()
            .status()
            .unwrap_or_default()
            .contains("Added 0 projects")
    );
}

#[test]
fn export_hands_the_host_a_backup_that_reads_back() {
    let mut app =
        ProseApp::with_library(ProjectLibrary::new("d".into()), Box::new(Memory::default()));
    app.open_backup(
        "bundle-plain.prosecraft",
        &fixture("bundle-plain.prosecraft"),
    );
    let mut harness = harness(app);
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Export backup…")
        .click();
    harness.run();
    let requests = harness.state_mut().take_requests();
    let Some(HostRequest::ExportBackup { file_name, text }) = requests.first() else {
        panic!("no export: {requests:?}")
    };
    assert!(file_name.ends_with(".prosecraft"));
    let projects = prosecraft_ui_egui::Library::read(text)
        .ok()
        .unwrap_or_default();
    assert_eq!(projects.len(), 2);
}

#[test]
fn a_failed_save_is_reported_not_hidden() {
    let memory = Memory {
        fail: true,
        ..Memory::default()
    };
    let mut app = ProseApp::with_library(ProjectLibrary::new("d".into()), Box::new(memory));
    app.open_backup(
        "bundle-plain.prosecraft",
        &fixture("bundle-plain.prosecraft"),
    );
    assert!(
        app.state()
            .status()
            .unwrap_or_default()
            .contains("couldn't save your library: the disk is full")
    );
}

#[test]
fn a_new_project_starts_from_a_wiki_template() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    #[derive(Clone, Default)]
    struct Store(Arc<Mutex<Option<String>>>, Arc<AtomicUsize>);
    impl prosecraft_ui_egui::LibraryStore for Store {
        fn save(&mut self, text: &str) -> Result<(), String> {
            *self.0.lock().map_err(|e| e.to_string())? = Some(text.to_owned());
            Ok(())
        }
        fn now_ms(&self) -> f64 {
            1_800_000_000_000.0
        }
        fn new_id(&mut self) -> String {
            format!("id-{}", self.1.fetch_add(1, Ordering::SeqCst))
        }
    }
    let store = Store::default();
    let app = ProseApp::with_library(
        prosecraft_ui_egui::ProjectLibrary::new("me".into()),
        Box::new(store.clone()),
    );
    let mut harness = Harness::builder()
        .with_size([1280.0, 850.0])
        .build_ui_state(|ui, app: &mut ProseApp| app.show(ui), app);
    prosecraft_ui_egui::setup_for_tests(&harness.ctx);
    harness.run();
    // An empty library offers to create a project.
    harness
        .get_by_role_and_label(Role::Button, "Create project")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::RadioButton, "Fantasy")
        .click();
    harness.run();
    // Add one more module, then create.
    harness
        .get_by_role_and_label(Role::Button, "Religions")
        .scroll_to_me();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Religions")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Create")
        .click();
    harness.run();
    let saved = store
        .0
        .lock()
        .expect("saved")
        .clone()
        .expect("the library was saved");
    let library = prosecraft_core::js::parse(&saved).expect("json");
    let project = &library
        .get("projects")
        .and_then(|p| p.as_array())
        .expect("projects")[0];
    let data = project.get("project").expect("data");
    assert_eq!(
        data.get("name")
            .and_then(|n| n.as_js_string())
            .map(|s| s.to_string_lossy())
            .as_deref(),
        Some("Untitled Project")
    );
    let folders: Vec<String> = data
        .get("researchFolders")
        .and_then(|f| f.as_array())
        .expect("folders")
        .iter()
        .map(|f| {
            f.get("title")
                .and_then(|t| t.as_js_string())
                .map(|s| s.to_string_lossy())
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(
        folders,
        [
            "Characters",
            "Places",
            "Magic",
            "Cultures",
            "Creatures",
            "History & Timeline",
            "Items",
            "Religions"
        ]
    );
    // The new project is open at its first scene, in the Write view.
    harness.get_by_role_and_label(Role::TreeItem, "Scene 1");
    assert!(harness.query_by_label("Scene text").is_some());

    // Project Settings opens next, as in ProseCraft: name the project and add an author.
    let name = harness.get_by_role_and_label(Role::TextInput, "Project name");
    name.focus();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_role_and_label(Role::TextInput, "Project name")
        .type_text("The Long Night");
    harness
        .get_by_role_and_label(Role::TextInput, "Author name")
        .focus();
    harness
        .get_by_role_and_label(Role::TextInput, "Author name")
        .type_text("  A. Writer ");
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Save").click();
    // The library is written after the pause that follows a change.
    harness.run_steps(12);
    let saved = store.0.lock().expect("saved").clone().expect("saved");
    let library = prosecraft_core::js::parse(&saved).expect("json");
    let data = library
        .get("projects")
        .and_then(|p| p.as_array())
        .expect("projects")[0]
        .get("project")
        .cloned()
        .expect("data");
    let field = |k: &str| {
        data.get(k)
            .and_then(|v| v.as_js_string())
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    };
    assert_eq!(
        (field("name"), field("author"), field("kind")),
        ("The Long Night".into(), "A. Writer".into(), "novel".into())
    );
    // The gear opens it again.
    harness
        .get_by_role_and_label(Role::Button, "Project settings")
        .click();
    harness.run();
    assert_eq!(
        harness
            .get_by_role_and_label(Role::TextInput, "Project name")
            .value()
            .as_deref(),
        Some("The Long Night")
    );
}

/// A book from the EPUB import fixtures.
fn epub(name: &str) -> Vec<u8> {
    use base64::Engine;
    let all = prosecraft_core::js::parse(&fixture("epub-import.json")).expect("json");
    let case = all
        .get("cases")
        .and_then(prosecraft_core::js::JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .find(|c| {
            c.get("name")
                .and_then(prosecraft_core::js::JsValue::as_js_string)
                .is_some_and(|n| n.eq_str(name))
        })
        .cloned()
        .expect("case");
    let text = case
        .get("epub")
        .and_then(prosecraft_core::js::JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .expect("base64")
}

#[test]
fn an_imported_epub_becomes_a_project_that_opens() {
    let memory = Memory::default();
    let app = ProseApp::with_library(
        ProjectLibrary::new("this-device".into()),
        Box::new(memory.clone()),
    );
    let mut harness = harness(app);
    harness.run();
    // Settings → Data and sync asks the host for a book.
    harness
        .get_by_role_and_label(Role::Button, "Settings…")
        .click();
    harness.run();
    harness.get_by_label("Data and sync").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Import EPUB…")
        .click();
    harness.run();
    assert!(
        harness
            .state_mut()
            .take_requests()
            .contains(&HostRequest::ImportEpub)
    );
    harness.key_press(egui::Key::Escape);
    harness.run();

    harness
        .state_mut()
        .import_epub("book.epub", &epub("nav-and-metadata"));
    harness.run_steps(3);
    harness.get_by_label_contains("Imported “A Test Book” — 2 chapter(s), 2 scene(s).");
    // The chapter, and its first scene titled after it (as ProseCraft names them).
    assert_eq!(
        harness
            .query_all_by_role_and_label(Role::TreeItem, "Chapter Two")
            .count(),
        2
    );
    // Opened at its first scene, with the heading made the chapter's title.
    assert_eq!(
        harness
            .get_by_label("Scene text")
            .accesskit_node()
            .value()
            .as_deref(),
        Some("First scene.")
    );
    let saved = memory.saved.lock().unwrap().last().cloned().expect("saved");
    assert!(saved.contains("A Test Book") && saved.contains("Ada Lovelace"));

    // A book that can't be imported says why.
    harness.state_mut().import_epub("drm.epub", &epub("drm"));
    harness.run();
    harness.get_by_label_contains("This EPUB is DRM-protected and can't be imported.");
}

#[test]
fn a_scrivener_project_is_previewed_then_imported() {
    use base64::Engine;
    let all = prosecraft_core::js::parse(&fixture("scrivener-import.json")).expect("json");
    let case = all
        .get("scrivener")
        .and_then(prosecraft_core::js::JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .find(|c| {
            c.get("name")
                .and_then(prosecraft_core::js::JsValue::as_js_string)
                .is_some_and(|n| n.eq_str("wiki"))
        })
        .cloned()
        .expect("case");
    let text = |v: &prosecraft_core::js::JsValue, k: &str| {
        v.get(k)
            .and_then(prosecraft_core::js::JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    };
    let entries: Vec<(String, Vec<u8>)> = case
        .get("files")
        .and_then(prosecraft_core::js::JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .map(|f| {
            (
                text(f, "path"),
                base64::engine::general_purpose::STANDARD
                    .decode(text(f, "bytes"))
                    .expect("base64"),
            )
        })
        .collect();
    let memory = Memory::default();
    let app = ProseApp::with_library(
        ProjectLibrary::new("this-device".into()),
        Box::new(memory.clone()),
    );
    let mut harness = harness(app);
    harness.run();
    harness
        .state_mut()
        .import_scrivener_folder("Lighthouse.scriv", entries);
    harness.run();
    // The preview: its name (from the folder) and what it will create.
    let name = harness.get_by_label("Project name");
    assert_eq!(name.accesskit_node().value().as_deref(), Some("Lighthouse"));
    harness.get_by_label("• 6 wiki page(s)");
    harness.get_by_label("• 3 wiki folder(s)");
    harness.get_by_label("• 1 file(s) such as PDFs and images were skipped");
    harness.get_by_label("• 1 item(s) have notes that are not imported");
    harness.get_by_label("Project name").focus();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_label("Project name").type_text("The Keeper");
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Import")
        .click();
    harness.run_steps(3);
    harness
        .get_by_label_contains("Imported “The Keeper”: 1 chapter(s), 1 scene(s), 6 wiki page(s).");
    harness.get_by_role_and_label(Role::TreeItem, "Mara");
    let saved = memory.saved.lock().unwrap().last().cloned().expect("saved");
    assert!(saved.contains("The Keeper") && saved.contains("Mara text."));

    // A file that is not a zip says so.
    harness.state_mut().import_scrivener_zip(b"not a zip");
    harness.run();
    harness.get_by_label_contains("Scrivener import: this file is not a readable zip archive.");
}

#[test]
fn a_composed_starter_is_saved_as_my_template_and_deleted() {
    let memory = Memory::default();
    let app = ProseApp::with_library(
        ProjectLibrary::new("this-device".into()),
        Box::new(memory.clone()),
    );
    let mut first = harness(app);
    first.run();
    first
        .get_by_role_and_label(Role::Button, "Create project")
        .click();
    first.run();
    first
        .get_by_role_and_label(Role::RadioButton, "Crime")
        .click();
    first.run();
    // A preset's own folders are "Template content".
    first.get_by_label("Template content");
    first.get_by_label("Template name").focus();
    first.get_by_label("Template name").type_text("My fantasy");
    first.run();
    first
        .get_by_role_and_label(Role::Button, "Save as my template")
        .scroll_to_me();
    first.run();
    first
        .get_by_role_and_label(Role::Button, "Save as my template")
        .click();
    first.run_steps(3);
    first.get_by_label("Saved as \"My fantasy\".");
    first.get_by_role_and_label(Role::RadioButton, "My fantasy");
    let saved = |memory: &Memory| {
        memory
            .settings
            .lock()
            .unwrap()
            .get("prosecraft-custom-starters")
            .cloned()
            .unwrap_or_default()
    };
    assert!(
        saved(&memory).contains(r#""name":"My fantasy""#),
        "{}",
        saved(&memory)
    );

    // Another app on this device has it too.
    let other = ProseApp::with_library(
        ProjectLibrary::new("this-device".into()),
        Box::new(memory.clone()),
    );
    let mut second = harness(other);
    second.run();
    second
        .get_by_role_and_label(Role::Button, "Create project")
        .click();
    second.run();
    second
        .get_by_role_and_label(Role::RadioButton, "My fantasy")
        .click();
    second.run();

    // Deleted (asked first), it is gone.
    first
        .get_by_role_and_label(Role::Button, "Delete My fantasy")
        .scroll_to_me();
    first.run();
    first
        .get_by_role_and_label(Role::Button, "Delete My fantasy")
        .click();
    first.run();
    first.get_by_label_contains(
        "Delete the template “My fantasy”? Projects created from it keep their pages.",
    );
    first.get_by_role_and_label(Role::Button, "Delete").click();
    first.run_steps(3);
    assert!(
        first
            .query_by_role_and_label(Role::RadioButton, "My fantasy")
            .is_none()
    );
    assert_eq!(saved(&memory), "[]");
}
