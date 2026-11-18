//! Device sync through the Settings dialog, against an in-memory sync folder two app
//! instances share.

use std::sync::{Arc, Mutex};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use prosecraft_core::sync::memory::MemoryFs;
use prosecraft_core::sync::{DirEntry, SyncError, SyncFileSystem};
use prosecraft_ui_egui::{LibraryStore, ProjectLibrary, ProseApp};

/// A sync folder shared between the test and the app's background sync.
#[derive(Clone, Default)]
struct Shared(Arc<Mutex<MemoryFs>>);

impl Shared {
    fn with<T>(&self, f: impl FnOnce(&mut MemoryFs) -> T) -> T {
        f(&mut self.0.lock().expect("fs"))
    }
}

impl SyncFileSystem for Shared {
    fn list_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, SyncError> {
        self.with(|fs| fs.list_dir(path))
    }
    fn read_text(&mut self, path: &str) -> Result<String, SyncError> {
        self.with(|fs| fs.read_text(path))
    }
    fn write_text(&mut self, path: &str, content: &str) -> Result<(), SyncError> {
        self.with(|fs| fs.write_text(path, content))
    }
    fn create_dir(&mut self, path: &str) -> Result<(), SyncError> {
        self.with(|fs| fs.create_dir(path))
    }
    fn remove(&mut self, path: &str) -> Result<(), SyncError> {
        self.with(|fs| fs.remove(path))
    }
    fn exists(&mut self, path: &str) -> Result<bool, SyncError> {
        self.with(|fs| fs.exists(path))
    }
    fn rename(&mut self, from: &str, to: &str) -> Result<(), SyncError> {
        self.with(|fs| fs.rename(from, to))
    }
}

#[derive(Clone, Default)]
struct Store {
    folder: Shared,
    state: Arc<Mutex<Option<String>>>,
    saved: Arc<Mutex<Option<String>>>,
    /// CraftCloud's ProseCraft folder, when "installed".
    craftcloud: Option<String>,
}

impl LibraryStore for Store {
    fn save(&mut self, text: &str) -> Result<(), String> {
        *self.saved.lock().map_err(|e| e.to_string())? = Some(text.to_owned());
        Ok(())
    }
    fn now_ms(&self) -> f64 {
        1_800_000_000_000.0
    }
    fn new_id(&mut self) -> String {
        "new".into()
    }
    fn supports_sync(&self) -> bool {
        true
    }
    fn has_craftcloud(&self) -> bool {
        self.craftcloud.is_some()
    }
    fn craftcloud_folder(&mut self) -> Result<String, String> {
        self.craftcloud
            .clone()
            .ok_or_else(|| "not installed".to_owned())
    }
    fn sync_fs(&mut self, _folder: &str) -> Option<Box<dyn SyncFileSystem + Send>> {
        Some(Box::new(self.folder.clone()))
    }
    fn load_setting(&mut self, key: &str) -> Option<String> {
        (key == "prosecraft-sync-state")
            .then(|| self.state.lock().ok()?.clone())
            .flatten()
    }
    fn save_setting(&mut self, key: &str, value: Option<&str>) -> Result<(), String> {
        if key == "prosecraft-sync-state" {
            *self.state.lock().map_err(|e| e.to_string())? = value.map(str::to_owned);
        }
        Ok(())
    }
}

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/../core/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn app(store: &Store, with_projects: bool) -> Harness<'static, ProseApp> {
    let mut app = ProseApp::with_library(
        ProjectLibrary::new(
            if with_projects {
                "device-a"
            } else {
                "device-b"
            }
            .into(),
        ),
        Box::new(store.clone()),
    );
    if with_projects {
        app.open_backup(
            "bundle-plain.prosecraft",
            &fixture("bundle-plain.prosecraft"),
        );
    }
    let mut harness = Harness::builder()
        .with_size([1280.0, 850.0])
        .build_ui_state(|ui, app: &mut ProseApp| app.show(ui), app);
    prosecraft_ui_egui::setup_for_tests(&harness.ctx);
    harness.run();
    harness
}

fn status(harness: &Harness<'_, ProseApp>) -> String {
    harness
        .get_by_role(Role::Status)
        .accesskit_node()
        .label()
        .unwrap_or_default()
}

/// Opens Settings, enters the passphrase and syncs; returns the final status.
fn sync(harness: &mut Harness<'_, ProseApp>, passphrase: &str) -> String {
    harness
        .get_by_role_and_label(Role::Button, "Settings…")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Tab, "Data and sync")
        .click();
    harness.run();
    if harness.query_by_label("Sync passphrase").is_none() {
        harness.state_mut().set_sync_folder("/shared/sync".into());
        harness.run();
    }
    harness.get_by_label("Sync passphrase").focus();
    harness
        .get_by_label("Sync passphrase")
        .type_text(passphrase);
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Sync now")
        .click();
    // A step, not `run`: the UI repaints until the background sync is done.
    harness.step();
    // The sync runs on a background thread; wait for its result.
    for _ in 0..600 {
        harness.step();
        if status(harness) != "Syncing…" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    harness.run();
    status(harness)
}

#[test]
fn two_devices_sync_through_one_folder_from_settings() {
    let a_store = Store::default();
    let mut a = app(&a_store, true);
    let a_status = sync(&mut a, "correct horse battery");
    assert_eq!(
        a_status,
        "Created an encrypted sync file. 2 project(s) synced."
    );
    assert!(
        a_store
            .folder
            .with(|fs| fs.files().contains_key("prosecraft-sync.prosecraft"))
    );
    let state = a_store
        .state
        .lock()
        .expect("state")
        .clone()
        .expect("settings saved");
    assert!(
        state.contains("\"mode\":\"encrypted-file\"") && state.contains("\"project-1\""),
        "{state}"
    );

    // Device B: an empty library, the same folder.
    let b_store = Store {
        folder: a_store.folder.clone(),
        ..Store::default()
    };
    let mut b = app(&b_store, false);
    assert_eq!(
        sync(&mut b, "correct horse battery"),
        "Synced 2 project(s) (2 imported)."
    );
    let saved = b_store
        .saved
        .lock()
        .expect("saved")
        .clone()
        .expect("library saved");
    assert!(
        saved.contains("The Lighthouse"),
        "B's library has A's projects"
    );
}

#[test]
fn a_wrong_passphrase_says_so() {
    let store = Store::default();
    let mut a = app(&store, true);
    assert_eq!(
        sync(&mut a, "correct horse battery"),
        "Created an encrypted sync file. 2 project(s) synced."
    );
    let other = Store {
        folder: store.folder.clone(),
        ..Store::default()
    };
    let mut b = app(&other, false);
    assert_eq!(sync(&mut b, "wrong passphrase!"), "Incorrect passphrase.");
}

#[test]
fn the_craftcloud_folder_is_one_click_away() {
    let store = Store {
        craftcloud: Some("/home/writer/CraftCloud/ProseCraft".into()),
        ..Store::default()
    };
    let mut harness = app(&store, true);
    harness
        .get_by_role_and_label(Role::Button, "Settings…")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Tab, "Data and sync")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Use the CraftCloud folder")
        .click();
    harness.run();
    assert_eq!(
        status(&harness),
        "Folder saved. The first sync creates one encrypted sync file."
    );
    let state = store.state.lock().expect("state").clone().expect("saved");
    assert!(
        state.contains(r#""folder":"/home/writer/CraftCloud/ProseCraft""#)
            && state.contains(r#""mode":"encrypted-file""#),
        "{state}"
    );
    // Without CraftCloud the button isn't there.
    let plain = app(&Store::default(), true);
    assert!(plain.query_by_label("Use the CraftCloud folder").is_none());
}

#[test]
fn the_theme_is_chosen_in_settings_and_remembered() {
    /// Every setting saved, in order: key and value.
    type Saved = Vec<(String, Option<String>)>;
    #[derive(Clone, Default)]
    struct Settings(Arc<Mutex<Saved>>);
    struct ThemeStore(Settings);
    impl LibraryStore for ThemeStore {
        fn save(&mut self, _text: &str) -> Result<(), String> {
            Ok(())
        }
        fn now_ms(&self) -> f64 {
            0.0
        }
        fn new_id(&mut self) -> String {
            "id".into()
        }
        fn load_setting(&mut self, key: &str) -> Option<String> {
            self.0
                .0
                .lock()
                .ok()?
                .iter()
                .rev()
                .find(|(k, _)| k == key)
                .and_then(|(_, v)| v.clone())
        }
        fn save_setting(&mut self, key: &str, value: Option<&str>) -> Result<(), String> {
            self.0
                .0
                .lock()
                .map_err(|e| e.to_string())?
                .push((key.to_owned(), value.map(str::to_owned)));
            Ok(())
        }
    }
    let settings = Settings::default();
    let app = ProseApp::with_library(
        ProjectLibrary::new("d".into()),
        Box::new(ThemeStore(settings.clone())),
    );
    let mut harness = Harness::builder()
        .with_size([1280.0, 850.0])
        .build_ui_state(|ui, app: &mut ProseApp| app.show(ui), app);
    prosecraft_ui_egui::setup_for_tests(&harness.ctx);
    harness.run();
    let dark_bg = harness.ctx.global_style().visuals.panel_fill;
    harness
        .get_by_role_and_label(Role::Button, "Settings…")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::ComboBox, "Theme")
        .click();
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Light").click();
    harness.run();
    assert_eq!(
        settings.0.lock().expect("settings").last().cloned(),
        Some(("prosecraft-theme".to_owned(), Some("light".to_owned())))
    );
    assert_ne!(
        harness.ctx.global_style().visuals.panel_fill,
        dark_bg,
        "the light palette is in use"
    );
    // A new app on this device opens in the remembered theme.
    let again = ProseApp::with_library(
        ProjectLibrary::new("d".into()),
        Box::new(ThemeStore(settings)),
    );
    let mut harness = Harness::builder()
        .with_size([800.0, 600.0])
        .build_ui_state(|ui, app: &mut ProseApp| app.show(ui), again);
    prosecraft_ui_egui::setup_for_tests(&harness.ctx);
    harness.run();
    assert_ne!(harness.ctx.global_style().visuals.panel_fill, dark_bg);
}

/// Opens Settings, chooses the open folder (which asks for no passphrase) and syncs.
fn sync_open(harness: &mut Harness<'_, ProseApp>) -> String {
    harness
        .get_by_role_and_label(Role::Button, "Settings…")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Tab, "Data and sync")
        .click();
    harness.run();
    if harness
        .query_by_role_and_label(Role::Button, "Sync now")
        .is_none()
    {
        harness.state_mut().set_sync_folder("/shared/open".into());
        harness.run();
    }
    harness
        .get_by_role_and_label(Role::RadioButton, "Open folder")
        .click();
    harness.run();
    assert!(harness.query_by_label("Sync passphrase").is_none());
    harness
        .get_by_role_and_label(Role::Button, "Sync now")
        .click();
    // A step, not `run`: the UI repaints until the background sync is done.
    harness.step();
    for _ in 0..600 {
        harness.step();
        if status(harness) != "Syncing…" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    harness.run();
    let said = status(harness);
    harness.key_press(egui::Key::Escape);
    harness.run();
    said
}

#[test]
fn an_open_folder_is_written_and_an_edit_made_outside_comes_back() {
    let store = Store::default();
    let mut a = app(&store, true);
    assert_eq!(
        sync_open(&mut a),
        "Set up the open folder. 2 project(s) synced."
    );
    let path = "projects/project-1/documents/doc-1.md";
    let file = store
        .folder
        .with(|fs| fs.files().get(path).cloned())
        .expect("scene file");
    assert!(
        file.starts_with("---\nid: \"doc-1\"\n") && file.contains("\nIt was _dark_."),
        "{file}"
    );
    assert!(
        store
            .folder
            .with(|fs| fs.files().contains_key("prosecraft-folder.json"))
    );

    // Edited in another program: the next sync takes it.
    store.folder.with(|fs| {
        let edited = file.replace("It was _dark_.", "It was _very_ dark.");
        let _ = fs.write_text(path, &edited);
    });
    assert_eq!(sync_open(&mut a), "Synced 2 project(s) (1 imported).");
    let saved = store
        .saved
        .lock()
        .expect("saved")
        .clone()
        .expect("library saved");
    assert!(
        saved.contains("It was <em>very</em> dark."),
        "the edit is in the library"
    );
}
