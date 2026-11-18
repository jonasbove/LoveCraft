//! Renders the app's views offscreen to PNG files, to look at them next to ProseCraft's.
//!
//! Ignored by default (it needs a GPU or a software renderer). Run it with
//! `PROSECRAFT_RENDER_DIR=/some/dir CRAFT_FONTS_DIR=/path/to/craft-fonts \
//!  cargo test -p prosecraft-ui-egui --test render -- --ignored`.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use prosecraft_ui_egui::{LibraryStore, ProjectLibrary, ProseApp};

/// Keeps nothing; the renders only look.
struct Nowhere;
impl LibraryStore for Nowhere {
    fn save(&mut self, _text: &str) -> Result<(), String> {
        Ok(())
    }
    fn now_ms(&self) -> f64 {
        1_800_000_000_000.0
    }
    fn new_id(&mut self) -> String {
        // Distinct ids, as the real store's UUIDs are.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        format!(
            "new-{}",
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    }
    fn supports_sync(&self) -> bool {
        true
    }
}

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/../core/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
#[ignore = "renders with wgpu; run on demand"]
fn render_views() {
    let Some(dir) = std::env::var_os("PROSECRAFT_RENDER_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let mut app = ProseApp::with_library(ProjectLibrary::new("render".into()), Box::new(Nowhere));
    app.open_backup(
        "bundle-plain.prosecraft",
        &fixture("bundle-plain.prosecraft"),
    );
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .build_ui_state(|ui, app: &mut ProseApp| app.show(ui), app);
    prosecraft_ui_egui::setup(&harness.ctx);
    harness.run();
    let save = |harness: &mut Harness<'_, ProseApp>, name: &str| {
        harness.run();
        let image = harness.render().unwrap_or_else(|e| panic!("render: {e}"));
        image
            .save(dir.join(format!("{name}.png")))
            .unwrap_or_else(|e| panic!("save: {e}"));
    };
    save(&mut harness, "outline");
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    save(&mut harness, "write-scene");
    harness.get_by_role_and_label(Role::Tab, "Comments").click();
    save(&mut harness, "comments");
    harness
        .get_by_role_and_label(Role::Tab, "Snapshots")
        .click();
    save(&mut harness, "snapshots");
    harness.get_by_role_and_label(Role::Tab, "Timeline").click();
    save(&mut harness, "timeline");
    harness.get_by_role_and_label(Role::Tab, "Write").click();
    harness.get_by_role_and_label(Role::Tab, "Progress").click();
    harness
        .get_by_role_and_label(Role::Button, "Search project")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "Find")
        .type_text("the");
    save(&mut harness, "find");
    harness.key_press(egui::Key::Escape);
    harness
        .get_by_role_and_label(Role::TreeItem, "Chapter One")
        .click();
    save(&mut harness, "write-chapter");
    harness.get_by_role_and_label(Role::Tab, "Publish").click();
    save(&mut harness, "publish");
    harness
        .get_by_role_and_label(Role::Button, "Settings…")
        .click();
    harness
        .state_mut()
        .set_sync_folder("/home/writer/Nextcloud/ProseCraft".into());
    save(&mut harness, "settings");
    harness
        .get_by_role_and_label(Role::ComboBox, "Theme")
        .click();
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Light").click();
    harness.run();
    harness.key_press(egui::Key::Escape);
    harness.run();
    harness.get_by_role_and_label(Role::Tab, "Write").click();
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    save(&mut harness, "write-light");

    // A new project from an empty library.
    let empty = ProseApp::with_library(ProjectLibrary::new("render".into()), Box::new(Nowhere));
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .build_ui_state(|ui, app: &mut ProseApp| app.show(ui), empty);
    prosecraft_ui_egui::setup(&harness.ctx);
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Create project")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::RadioButton, "Fantasy")
        .click();
    save(&mut harness, "starter");
    harness
        .get_by_role_and_label(Role::Button, "Create")
        .click();
    save(&mut harness, "project-settings");
    harness
        .get_by_role_and_label(Role::Button, "Manage labels…")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Add label")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Delete label Final")
        .click();
    save(&mut harness, "labels");
}
