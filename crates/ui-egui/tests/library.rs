//! The app's own library: backups are added to it and it is saved; it reopens; it exports.

use std::sync::{Arc, Mutex};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
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
}

impl LibraryStore for Memory {
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
