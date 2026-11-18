//! The app shell, driven by keyboard and checked through the accessibility tree, with backups
//! that ProseCraft itself wrote (`crates/core/tests/fixtures`).

use egui::Key;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use prosecraft_ui_egui::{ProseApp, Selection, WorkspaceView};

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/../core/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn harness(app: ProseApp) -> Harness<'static, ProseApp> {
    let harness = Harness::builder()
        // Under a loaded CI machine a step can take a couple more frames to settle (egui
        // reports a font-provider repaint); `run` gets that room, nothing else changes.
        .with_max_steps(8)
        .with_size([1280.0, 850.0])
        .build_ui_state(|ui, app: &mut ProseApp| app.show(ui), app);
    // What the app does on its first frame, done before the harness's first frame too: the
    // fixtures have emoji and other characters no bundled font draws, and they must not crash.
    prosecraft_ui_egui::setup_for_tests(&harness.ctx);
    harness
}

fn opened() -> Harness<'static, ProseApp> {
    let mut app = ProseApp::new();
    app.open_backup(
        "bundle-plain.prosecraft",
        &fixture("bundle-plain.prosecraft"),
    );
    let mut harness = harness(app);
    harness.run();
    harness
}

#[test]
fn empty_app_offers_the_workspace_tabs_and_how_to_open() {
    let mut harness = harness(ProseApp::new());
    harness.run();
    // The Timeline view only once it is turned on, as in ProseCraft.
    for view in WorkspaceView::ALL {
        if view == WorkspaceView::Timeline {
            assert!(harness.query_by_label(view.label()).is_none());
        } else {
            harness.get_by_role_and_label(Role::Tab, view.label());
        }
    }
    harness.get_by_label_contains("drop a .prosecraft file");
}

#[test]
fn a_backup_fills_the_binder_as_prosecraft_does() {
    let harness = opened();
    // Live chapters and their live scenes, as tree items; the trashed chapter isn't among them.
    let chapter = harness.get_by_role_and_label(Role::TreeItem, "Chapter One");
    assert!(!chapter.is_focused());
    harness.get_by_role_and_label(Role::TreeItem, "Opening Scene");
    harness.get_by_role_and_label(Role::TreeItem, "INT. LIGHTHOUSE - NIGHT");
    assert!(
        harness
            .query_by_role_and_label(Role::TreeItem, "Chapitre deux — « voilà »")
            .is_none()
    );
    // The Wiki page in its folder.
    harness.get_by_role_and_label(Role::TreeItem, "Mara");
    harness.get_by_label_contains("Opened bundle-plain.prosecraft: 2 projects");
}

#[test]
fn choosing_a_scene_by_keyboard_opens_it_in_the_write_view() {
    let mut harness = opened();
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .focus();
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(harness.state().state().view(), WorkspaceView::Write);
    assert!(
        matches!(harness.state().state().selection(), Some(Selection::Document(id)) if id == "doc-1")
    );
    let write = harness.get_by_role_and_label(Role::Tab, "Write");
    assert!(write.accesskit_node().is_selected().unwrap_or(false));
    // The scene's text, as ProseCraft's plain text gives it.
    harness.get_by_label_contains("It was dark.");
    // Its footnote in the sidebar.
    harness
        .get_by_role_and_label(Role::Tab, "Footnotes")
        .click();
    harness.run();
    // Numbered as ProseCraft numbers it, beside its text.
    harness.get_by_label("A note with emphasis.");
    harness.get_by_role_and_label(Role::Button, "Remove footnote 1");
}

#[test]
fn outline_counts_words_like_prosecraft() {
    let harness = opened();
    // "It was dark." + "Then (Smith 2020)." + "INT. LIGHTHOUSE - NIGHT"
    harness.get_by_label_contains("10 words in 1 chapter");
}

#[test]
fn encrypted_backup_asks_for_its_passphrase() {
    let mut app = ProseApp::new();
    app.open_backup(
        "bundle-encrypted.prosecraft",
        &fixture("bundle-encrypted.prosecraft"),
    );
    assert!(app.is_locked());
    let mut harness = harness(app);
    harness.run();
    harness.get_by_label_contains("Hint: the usual");

    let field = harness.get_by_role(Role::PasswordInput);
    field.focus();
    field.type_text("wrong");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    harness.get_by_label("Incorrect passphrase.");
    assert!(harness.state().is_locked());

    harness
        .get_by_role(Role::PasswordInput)
        .type_text("correct horse battery");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert!(!harness.state().is_locked());
    harness.get_by_role_and_label(Role::TreeItem, "Opening Scene");
}

#[test]
fn escape_cancels_the_passphrase_dialog() {
    let mut app = ProseApp::new();
    app.open_backup(
        "bundle-encrypted.prosecraft",
        &fixture("bundle-encrypted.prosecraft"),
    );
    let mut harness = harness(app);
    harness.run();
    harness.key_press(Key::Escape);
    harness.run();
    assert!(!harness.state().is_locked());
    harness.get_by_label_contains("was cancelled");
}

#[test]
fn junk_is_refused_with_a_message_not_a_crash() {
    let mut app = ProseApp::new();
    app.open_backup("notes.txt", "just some notes");
    app.open_backup_bytes("image.png", &[0x89, 0x50, 0x4e, 0x47, 0xff]);
    let mut harness = harness(app);
    harness.run();
    harness.get_by_label_contains("Couldn't open image.png");
}
