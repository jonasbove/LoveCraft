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
}

fn harness(app: ProseApp) -> Harness<'static, ProseApp> {
    let harness = Harness::builder()
        .with_size([1280.0, 850.0])
        .build_ui_state(|ui, app: &mut ProseApp| app.show(ui), app);
    prosecraft_ui_egui::setup(&harness.ctx);
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
