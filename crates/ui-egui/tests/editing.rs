//! Editing through the interface, checked in what is saved.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use prosecraft_core::js::{self, JsValue};
use prosecraft_ui_egui::{LibraryStore, ProjectLibrary, ProseApp};

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/../core/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[derive(Clone, Default)]
struct Memory {
    saved: Arc<Mutex<Vec<String>>>,
    ids: Arc<AtomicUsize>,
}

impl LibraryStore for Memory {
    fn save(&mut self, text: &str) -> Result<(), String> {
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
        format!("new-{}", self.ids.fetch_add(1, Ordering::SeqCst))
    }
}

impl Memory {
    fn last(&self) -> JsValue {
        let text = self
            .saved
            .lock()
            .ok()
            .and_then(|s| s.last().cloned())
            .expect("something was saved");
        js::parse(&text).expect("the saved library is JSON")
    }
}

/// The app with the fixture backup in its library, and its store.
fn app() -> (Harness<'static, ProseApp>, Memory) {
    let memory = Memory::default();
    let mut app = ProseApp::with_library(
        ProjectLibrary::new("this-device".into()),
        Box::new(memory.clone()),
    );
    app.open_backup(
        "bundle-plain.prosecraft",
        &fixture("bundle-plain.prosecraft"),
    );
    let mut harness = Harness::builder()
        .with_size([1280.0, 850.0])
        .build_ui_state(|ui, app: &mut ProseApp| app.show(ui), app);
    prosecraft_ui_egui::setup_for_tests(&harness.ctx);
    harness.run();
    (harness, memory)
}

fn project<'a>(library: &'a JsValue, id: &str) -> &'a JsValue {
    library
        .get("projects")
        .and_then(JsValue::as_array)
        .and_then(|p| {
            p.iter().find(|p| {
                p.get("id")
                    .and_then(JsValue::as_js_string)
                    .is_some_and(|i| i.eq_str(id))
            })
        })
        .expect("project")
}

fn document<'a>(project: &'a JsValue, id: &str) -> Option<&'a JsValue> {
    project
        .get("project")?
        .get("documents")?
        .as_array()?
        .iter()
        .find(|d| {
            d.get("id")
                .and_then(JsValue::as_js_string)
                .is_some_and(|i| i.eq_str(id))
        })
}

#[test]
fn a_new_chapter_and_typed_scene_are_saved_as_prosecraft_writes_them() {
    let (mut harness, memory) = app();
    // The binder's "+" (the outline has an "Add chapter" button too).
    harness
        .get_all_by_role_and_label(Role::Button, "Add chapter")
        .next()
        .expect("an add button")
        .click();
    harness.run();
    // ProseCraft's numbering counts automatically numbered chapters only; "Chapter One" isn't
    // one, so the new chapter is "Chapter 1". Its first scene is "Scene 1".
    harness.get_by_role_and_label(Role::TreeItem, "Chapter 1");
    harness
        .get_by_role_and_label(Role::TreeItem, "Scene 1")
        .click();
    harness.run();

    let body = harness.get_by_label("Scene text");
    body.focus();
    body.type_text("The lamp went out.");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness
        .get_by_label("Scene text")
        .type_text("Nobody moved.");
    // The pause after typing, then the pause before saving.
    harness.run_steps(12);

    let saved = memory.last();
    let stored = project(&saved, "project-1");
    let scene = document(stored, "new-1").expect("the new scene was saved");
    assert_eq!(
        js::stringify(scene.get("content").expect("content")),
        r#""<p>The lamp went out.</p><p>Nobody moved.</p>""#
    );
    assert_eq!(
        scene
            .get("chapterId")
            .and_then(JsValue::as_js_string)
            .map(|c| c.to_string_lossy())
            .as_deref(),
        Some("new-0")
    );
    // Stamped as ProseCraft stamps, everything else kept.
    assert!(
        stored
            .get("revision")
            .and_then(JsValue::as_number)
            .unwrap_or(0.0)
            > 1.0
    );
    let meta = stored
        .get("entities")
        .and_then(|e| e.get("documents"))
        .and_then(|d| d.get("new-1"))
        .expect("scene metadata");
    assert_eq!(
        meta.get("updatedBy")
            .and_then(JsValue::as_js_string)
            .map(|d| d.to_string_lossy())
            .as_deref(),
        Some("this-device")
    );
    assert!(
        stored
            .get("project")
            .and_then(|p| p.get("futureField"))
            .is_some()
    );
    assert!(prosecraft_core::storage::schema::is_stored_project(stored));
    harness.get_by_label("All changes saved");
}

#[test]
fn a_screenplay_scene_stays_read_only_with_the_reason() {
    let (mut harness, _memory) = app();
    harness
        .get_by_role_and_label(Role::TreeItem, "INT. LIGHTHOUSE - NIGHT")
        .click();
    harness.run();
    assert!(
        harness.query_by_label("Scene text").is_none(),
        "no editor for content it can't write back"
    );
    harness.get_by_label_contains("content the editor can't change yet");
}

#[test]
fn typing_into_a_formatted_scene_keeps_its_emphasis_footnote_and_citation() {
    let (mut harness, memory) = app();
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    let body = harness.get_by_label("Scene text");
    body.focus();
    harness.run();
    // The caret starts at the end; add a sentence there.
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::End);
    harness.get_by_label("Scene text").type_text(" It rained.");
    harness.run_steps(12);
    let saved = memory.last();
    let scene = document(project(&saved, "project-1"), "doc-1").expect("scene");
    let content = scene
        .get("content")
        .and_then(JsValue::as_js_string)
        .map(|c| c.to_string_lossy())
        .unwrap_or_default();
    assert_eq!(
        content,
        r#"<p>It was <em>dark</em>.<sup class="footnote-ref" data-footnote-id="fn-1" contenteditable="false"></sup></p><p>Then <span class="cite-ref" data-cites="[{&quot;id&quot;:&quot;smith2020&quot;}]">(Smith 2020)</span>. It rained.</p>"#
    );
}

#[test]
fn bold_from_the_toolbar_wraps_the_selection() {
    let (mut harness, memory) = app();
    harness
        .get_all_by_role_and_label(Role::Button, "Add chapter")
        .next()
        .expect("add")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::TreeItem, "Scene 1")
        .click();
    harness.run();
    let body = harness.get_by_label("Scene text");
    body.focus();
    body.type_text("loud words");
    harness.run();
    // Select "loud" (Home, then Shift+Right four times) and press the Bold button.
    harness.key_press_modifiers(egui::Modifiers::NONE, egui::Key::Home);
    for _ in 0..4 {
        harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowRight);
    }
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Bold").click();
    harness.run_steps(12);
    let saved = memory.last();
    let scene = document(project(&saved, "project-1"), "new-1").expect("scene");
    assert_eq!(
        js::stringify(scene.get("content").expect("content")),
        r#""<p><strong>loud</strong> words</p>""#
    );
}

#[test]
fn a_scene_goes_to_the_trash_and_comes_back() {
    let (mut harness, memory) = app();
    harness
        .get_by_role_and_label(Role::TreeItem, "INT. LIGHTHOUSE - NIGHT")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Move to Trash")
        .click();
    harness.run_steps(8);
    assert!(
        harness
            .query_by_role_and_label(Role::TreeItem, "INT. LIGHTHOUSE - NIGHT")
            .is_none()
    );
    let saved = memory.last();
    let scene = document(project(&saved, "project-1"), "doc-3").expect("kept, in the trash");
    assert_eq!(scene.get("trashed"), Some(&JsValue::Bool(true)));

    harness.get_by_label_contains("Trash (").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Restore INT. LIGHTHOUSE - NIGHT")
        .click();
    harness.run_steps(8);
    harness.get_by_role_and_label(Role::TreeItem, "INT. LIGHTHOUSE - NIGHT");
}

#[test]
fn a_backup_opened_without_a_library_offers_no_editing() {
    let mut app = ProseApp::new();
    app.open_backup(
        "bundle-plain.prosecraft",
        &fixture("bundle-plain.prosecraft"),
    );
    let mut harness = Harness::builder()
        .with_size([1280.0, 850.0])
        .build_ui_state(|ui, app: &mut ProseApp| app.show(ui), app);
    prosecraft_ui_egui::setup_for_tests(&harness.ctx);
    harness.run();
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Add chapter")
            .is_none()
    );
    harness
        .get_by_role_and_label(Role::TreeItem, "Mara")
        .click();
    harness.run();
    assert!(harness.query_by_label("Scene text").is_none());
}

/// Writes a library edited through the interface to `PROSECRAFT_EXPORT_FOR_TS`, so ProseCraft's own
/// code can be run on it (see the PR). Does nothing without the variable.
#[test]
fn export_an_edited_library_for_prosecraft() {
    let Some(path) = std::env::var_os("PROSECRAFT_EXPORT_FOR_TS") else {
        return;
    };
    let (mut harness, memory) = app();
    harness
        .get_all_by_role_and_label(Role::Button, "Add chapter")
        .next()
        .expect("add")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::TreeItem, "Scene 1")
        .click();
    harness.run();
    let body = harness.get_by_label("Scene text");
    body.focus();
    body.type_text("Written in prosecraft-rs & read by ProseCraft <3");
    harness.run_steps(12);
    let text = memory
        .saved
        .lock()
        .ok()
        .and_then(|s| s.last().cloned())
        .expect("saved");
    std::fs::write(path, text).expect("write");
}

#[test]
fn publish_settings_are_saved_and_plain_text_exports() {
    let (mut harness, memory) = app();
    harness.get_by_role_and_label(Role::Tab, "Publish").click();
    harness.run();
    // Only plain text is ported: with the default PDF chosen, Export is unavailable.
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Export PDF")
            .is_some()
    );
    // Below the fold: scrolled to first, as focus would.
    harness
        .get_by_role_and_label(Role::CheckBox, "Scene titles")
        .scroll_to_me();
    harness.run();
    harness
        .get_by_role_and_label(Role::CheckBox, "Scene titles")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::RadioButton, "Plain text (txt)")
        .scroll_to_me();
    harness.run();
    harness
        .get_by_role_and_label(Role::RadioButton, "Plain text (txt)")
        .click();
    harness.run_steps(12);

    let saved = memory.last();
    let options = project(&saved, "project-1")
        .get("project")
        .and_then(|p| p.get("publishOptions"))
        .expect("publishOptions were saved");
    assert_eq!(
        options.get("includeSceneTitles"),
        Some(&JsValue::Bool(true))
    );
    assert_eq!(
        options
            .get("format")
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy()),
        Some("text".to_owned())
    );

    harness
        .get_by_role_and_label(Role::Button, "Export Plain text")
        .scroll_to_me();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Export Plain text")
        .click();
    harness.run();
    let requests = harness.state_mut().take_requests();
    let Some(prosecraft_ui_egui::HostRequest::ExportFile {
        file_name,
        bytes,
        extension,
        ..
    }) = requests.first()
    else {
        panic!("an export was asked for: {requests:?}");
    };
    assert!(
        file_name.ends_with(".txt") && extension == "txt",
        "{file_name}"
    );
    let text = String::from_utf8_lossy(bytes);
    assert!(text.contains("Opening Scene\n\nIt was dark."), "{text}");
}

#[test]
fn markdown_exports_from_the_publish_view() {
    let (mut harness, _memory) = app();
    harness.get_by_role_and_label(Role::Tab, "Publish").click();
    harness.run();
    let card = harness.get_by_role_and_label(Role::RadioButton, "Markdown (md)");
    card.scroll_to_me();
    harness.run();
    harness
        .get_by_role_and_label(Role::RadioButton, "Markdown (md)")
        .click();
    harness.run_steps(4);
    let button = harness.get_by_role_and_label(Role::Button, "Export Markdown");
    button.scroll_to_me();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Export Markdown")
        .click();
    harness.run();
    let requests = harness.state_mut().take_requests();
    let Some(prosecraft_ui_egui::HostRequest::ExportFile {
        file_name, bytes, ..
    }) = requests.first()
    else {
        panic!("an export was asked for: {requests:?}");
    };
    assert!(file_name.ends_with(".md"), "{file_name}");
    let markdown = String::from_utf8_lossy(bytes);
    assert!(markdown.starts_with("---\ntitle: "), "{markdown}");
    assert!(markdown.contains("It was _dark_.[^1]"), "{markdown}");
}

#[test]
fn html_exports_from_the_publish_view() {
    let (mut harness, _memory) = app();
    harness.get_by_role_and_label(Role::Tab, "Publish").click();
    harness.run();
    for (role, name) in [
        (Role::RadioButton, "HTML (html)"),
        (Role::Button, "Export HTML"),
    ] {
        harness.get_by_role_and_label(role, name).scroll_to_me();
        harness.run();
        harness.get_by_role_and_label(role, name).click();
        harness.run_steps(4);
    }
    let requests = harness.state_mut().take_requests();
    let Some(prosecraft_ui_egui::HostRequest::ExportFile {
        file_name, bytes, ..
    }) = requests.first()
    else {
        panic!("an export was asked for: {requests:?}");
    };
    assert!(file_name.ends_with(".html"), "{file_name}");
    let html = String::from_utf8_lossy(bytes);
    assert!(html.starts_with("<!doctype html>"), "{html}");
    assert!(
        html.contains("<p>It was <em>dark</em>.<sup class=\"footnote-ref\" id=\"fnref-1\">"),
        "{html}"
    );
}

#[test]
fn epub_exports_from_the_publish_view() {
    let (mut harness, _memory) = app();
    harness.get_by_role_and_label(Role::Tab, "Publish").click();
    harness.run();
    for (role, name) in [
        (Role::RadioButton, "EPUB (epub)"),
        (Role::Button, "Export EPUB"),
    ] {
        harness.get_by_role_and_label(role, name).scroll_to_me();
        harness.run();
        harness.get_by_role_and_label(role, name).click();
        harness.run_steps(4);
    }
    let requests = harness.state_mut().take_requests();
    let Some(prosecraft_ui_egui::HostRequest::ExportFile {
        file_name,
        bytes,
        extension,
        ..
    }) = requests.first()
    else {
        panic!("an export was asked for: {requests:?}");
    };
    assert!(
        file_name.ends_with(".epub") && extension == "epub",
        "{file_name}"
    );
    assert!(bytes.starts_with(b"PK\x03\x04") && bytes[30..38] == *b"mimetype");
}

#[test]
fn a_snapshot_is_taken_restored_and_deleted() {
    let (mut harness, memory) = app();
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Tab, "Snapshots")
        .click();
    harness.run();
    harness.get_by_label("Snapshot note").focus();
    harness
        .get_by_label("Snapshot note")
        .type_text("before the rewrite");
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Take snapshot")
        .click();
    harness.run_steps(12);
    let snapshots = |memory: &Memory| -> Vec<JsValue> {
        let saved = memory.last();
        project(&saved, "project-1")
            .get("project")
            .and_then(|p| p.get("snapshots"))
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .filter(|s| {
                s.get("documentId")
                    .and_then(JsValue::as_js_string)
                    .is_some_and(|d| d.eq_str("doc-1"))
            })
            .cloned()
            .collect()
    };
    let taken = snapshots(&memory);
    let mine = taken
        .iter()
        .find(|s| {
            s.get("note")
                .and_then(JsValue::as_js_string)
                .is_some_and(|n| n.eq_str("before the rewrite"))
        })
        .cloned()
        .expect("the snapshot with its note");
    let original = mine
        .get("content")
        .and_then(JsValue::as_js_string)
        .map(|c| c.to_string_lossy())
        .expect("content");

    // Change the scene, then restore the snapshot.
    harness.get_by_label("Scene text").focus();
    harness.get_by_label("Scene text").type_text(" More.");
    harness.run_steps(12);
    let id = mine
        .get("id")
        .and_then(JsValue::as_js_string)
        .map(|c| c.to_string_lossy())
        .expect("id");
    let label = harness
        .query_all_by_role(Role::Button)
        .filter_map(|b| b.accesskit_node().label())
        .find(|l| l.starts_with("Restore snapshot from") && !l.contains("before a restore"))
        .expect("a restore button");
    harness.get_by_role_and_label(Role::Button, &label).click();
    harness.run_steps(12);
    let saved = memory.last();
    let scene = document(project(&saved, "project-1"), "doc-1").expect("scene");
    assert_eq!(
        scene
            .get("content")
            .and_then(JsValue::as_js_string)
            .map(|c| c.to_string_lossy()),
        Some(prosecraft_core::html::sanitize_html(&original)),
        "restored"
    );
    let after = snapshots(&memory);
    let before_restore = after
        .iter()
        .find(|s| {
            s.get("reason")
                .and_then(JsValue::as_js_string)
                .is_some_and(|r| r.eq_str("before-restore"))
        })
        .expect("the replaced text was kept");
    assert!(
        before_restore
            .get("content")
            .and_then(JsValue::as_js_string)
            .is_some_and(|c| c.to_string_lossy().contains("More."))
    );
    // The editor shows the restored text.
    assert!(
        !harness
            .get_by_label("Scene text")
            .value()
            .unwrap_or_default()
            .contains("More.")
    );

    // Delete the original snapshot.
    let delete = label.replacen("Restore", "Delete", 1);
    harness.get_by_role_and_label(Role::Button, &delete).click();
    harness.run_steps(12);
    assert!(!snapshots(&memory).iter().any(|s| {
        s.get("id")
            .and_then(JsValue::as_js_string)
            .is_some_and(|i| i.eq_str(&id))
    }));
}

#[test]
fn a_comment_is_added_on_a_selection_resolved_and_found_again() {
    let (mut harness, memory) = app();
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    // Select "dark" in "It was dark.": Home, seven to the right, four with Shift.
    harness.get_by_label("Scene text").focus();
    harness.run();
    // To the start of the first line (focus puts the caret at the end).
    for _ in 0..3 {
        harness.key_press_modifiers(egui::Modifiers::NONE, egui::Key::ArrowUp);
    }
    harness.key_press_modifiers(egui::Modifiers::NONE, egui::Key::Home);
    for _ in 0..7 {
        harness.key_press_modifiers(egui::Modifiers::NONE, egui::Key::ArrowRight);
    }
    for _ in 0..4 {
        harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowRight);
    }
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Comment")
        .click();
    harness.run();
    harness.get_by_label("Comment text").focus();
    harness
        .get_by_label("Comment text")
        .type_text("Darker still?");
    harness.get_by_label("Author name").focus();
    harness.get_by_label("Author name").type_text("Editor");
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Add comment")
        .click();
    harness.run_steps(12);

    let saved = memory.last();
    let comments = project(&saved, "project-1")
        .get("project")
        .and_then(|p| p.get("comments"))
        .and_then(JsValue::as_array)
        .expect("comments")
        .to_vec();
    let new = comments
        .iter()
        .find(|c| {
            c.get("body")
                .and_then(JsValue::as_js_string)
                .is_some_and(|b| b.eq_str("Darker still?"))
        })
        .expect("the new comment");
    let field = |k: &str| {
        new.get(k)
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    };
    // Footnote markers and citations aren't part of the comment text, as in ProseCraft.
    assert_eq!(
        (
            field("quote"),
            field("prefix"),
            field("suffix"),
            field("author"),
            field("documentId")
        ),
        (
            "dark".into(),
            "It was ".into(),
            ".Then .".into(),
            "Editor".into(),
            "doc-1".into()
        )
    );
    assert_eq!(new.get("resolved"), Some(&JsValue::Bool(false)));

    // Resolve it, then find the passage again.
    harness
        .get_by_role_and_label(Role::Button, "Resolve comment “Darker still?” on “dark”")
        .click();
    harness.run_steps(12);
    let saved = memory.last();
    let resolved = project(&saved, "project-1")
        .get("project")
        .and_then(|p| p.get("comments"))
        .and_then(JsValue::as_array)
        .expect("comments")
        .iter()
        .any(|c| {
            c.get("body")
                .and_then(JsValue::as_js_string)
                .is_some_and(|b| b.eq_str("Darker still?"))
                && c.get("resolved") == Some(&JsValue::Bool(true))
        });
    assert!(resolved, "resolved");
    // A resolved comment is hidden until resolved ones are shown.
    harness
        .get_by_role_and_label(Role::CheckBox, "Show resolved (1)")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Go to comment “Darker still?” on “dark”")
        .click();
    harness.run_steps(3);
    // The editor's selection, as a screen reader reads it.
    let selected = harness
        .get_by_label("Scene text")
        .accesskit_node()
        .text_selection()
        .map(|range| range.text());
    assert_eq!(
        selected.as_deref(),
        Some("dark"),
        "Go to selects the passage"
    );
}

#[test]
fn find_and_replace_across_the_project() {
    let (mut harness, memory) = app();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "Find")
        .type_text("dark");
    harness.run();
    let status = |h: &Harness<'_, ProseApp>| {
        h.get_by_role(Role::Status)
            .accesskit_node()
            .label()
            .unwrap_or_default()
    };
    assert_eq!(status(&harness), "1 hit in 1 document");
    harness
        .get_by_role_and_label(Role::TextInput, "Replace")
        .focus();
    harness
        .get_by_role_and_label(Role::TextInput, "Replace")
        .type_text("dusk");
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Replace all")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Replace")
        .click();
    harness.run_steps(12);
    assert_eq!(status(&harness), "Replaced 1 match in 1 document");
    let saved = memory.last();
    let stored = project(&saved, "project-1");
    let scene = document(stored, "doc-1").expect("scene");
    let content = scene
        .get("content")
        .and_then(JsValue::as_js_string)
        .map(|c| c.to_string_lossy())
        .unwrap_or_default();
    assert!(content.starts_with("<p>It was <em>dusk</em>."), "{content}");
    let before = stored
        .get("project")
        .and_then(|p| p.get("snapshots"))
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .any(|s| {
            s.get("note")
                .and_then(JsValue::as_js_string)
                .is_some_and(|n| n.eq_str("Before replace"))
                && s.get("content")
                    .and_then(JsValue::as_js_string)
                    .is_some_and(|c| c.to_string_lossy().contains("<em>dark</em>"))
        });
    assert!(before, "a snapshot of the scene was saved first");

    // Find the new word and go to it: the scene opens with the match selected.
    harness
        .get_by_role_and_label(Role::TextInput, "Find")
        .focus();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_role_and_label(Role::TextInput, "Find")
        .type_text("dusk");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    let selected = harness
        .get_by_label("Scene text")
        .accesskit_node()
        .text_selection()
        .map(|r| r.text());
    assert_eq!(selected.as_deref(), Some("dusk"));
}

#[test]
fn goals_are_set_and_writing_is_recorded_as_activity() {
    let (mut harness, memory) = app();
    harness
        .get_by_role_and_label(Role::TextInput, "Word count goal")
        .focus();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_role_and_label(Role::TextInput, "Word count goal")
        .type_text("60000");
    harness.key_press(egui::Key::Enter);
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "Chapter One word goal")
        .focus();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_role_and_label(Role::TextInput, "Chapter One word goal")
        .type_text("3000");
    harness.key_press(egui::Key::Enter);
    harness.run_steps(12);
    let saved = memory.last();
    let data = project(&saved, "project-1")
        .get("project")
        .cloned()
        .expect("data");
    assert_eq!(
        data.get("projectGoal").and_then(JsValue::as_number),
        Some(60000.0)
    );
    assert_eq!(
        data.get("chapterGoals")
            .and_then(|g| g.get("chapter-1"))
            .and_then(JsValue::as_number),
        Some(3000.0)
    );

    // Writing three words is recorded for today (the test store's clock: 2027-01-15).
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    harness.get_by_label("Scene text").focus();
    harness
        .get_by_label("Scene text")
        .type_text(" Three more words");
    harness.run_steps(12);
    let saved = memory.last();
    let day = project(&saved, "project-1")
        .get("project")
        .and_then(|p| p.get("activity"))
        .and_then(|a| a.get("2027-01-15"))
        .cloned()
        .expect("today's activity");
    assert_eq!(
        day.get("wordsAdded").and_then(JsValue::as_number),
        Some(3.0)
    );
    assert!(
        day.get("edits")
            .and_then(JsValue::as_number)
            .is_some_and(|e| e >= 1.0)
    );
}

#[test]
fn timeline_moments_are_added_edited_moved_and_removed() {
    let (mut harness, memory) = app();
    harness.get_by_role_and_label(Role::Tab, "Timeline").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "+ Add moment")
        .click();
    harness.run_steps(2);
    // The new moment's title has the focus: type into it and leave with Enter.
    let title = harness.get_by_role_and_label(Role::TextInput, "Title for timeline moment 2");
    assert!(title.is_focused(), "the new moment's title is focused");
    title.type_text("The storm");
    harness.key_press(egui::Key::Enter);
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "Date for The storm")
        .focus();
    harness
        .get_by_role_and_label(Role::TextInput, "Date for The storm")
        .type_text("Day 3");
    harness.key_press(egui::Key::Enter);
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Move The storm earlier")
        .click();
    harness.run_steps(12);
    let nodes = |memory: &Memory| -> Vec<(String, String, String)> {
        let saved = memory.last();
        project(&saved, "project-1")
            .get("project")
            .and_then(|p| p.get("timelineNodes"))
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .map(|n| {
                let s = |k: &str| {
                    n.get(k)
                        .and_then(JsValue::as_js_string)
                        .map(|v| v.to_string_lossy())
                        .unwrap_or_default()
                };
                (s("id"), s("title"), s("date"))
            })
            .collect()
    };
    let after = nodes(&memory);
    assert_eq!(
        (
            after[0].1.as_str(),
            after[0].2.as_str(),
            after[1].1.as_str()
        ),
        ("The storm", "Day 3", "Day One"),
        "{after:?}"
    );
    harness
        .get_by_role_and_label(Role::Button, "Remove The storm")
        .click();
    harness.run_steps(12);
    assert_eq!(
        nodes(&memory)
            .iter()
            .map(|n| n.1.clone())
            .collect::<Vec<_>>(),
        ["Day One"]
    );
}

#[test]
fn wiki_folders_and_pages_are_added_renamed_and_removed() {
    let (mut harness, memory) = app();
    let data = |memory: &Memory| {
        project(&memory.last(), "project-1")
            .get("project")
            .cloned()
            .expect("data")
    };
    harness
        .get_by_role_and_label(Role::Button, "Add to Wiki")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Add folder")
        .click();
    harness.run_steps(3);
    // The new folder's name is being edited.
    let name = harness.get_by_role_and_label(Role::TextInput, "Folder name");
    name.focus();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_role_and_label(Role::TextInput, "Folder name")
        .type_text("People");
    harness.key_press(egui::Key::Enter);
    harness.run_steps(12);
    let folders = data(&memory)
        .get("researchFolders")
        .and_then(JsValue::as_array)
        .map(<[JsValue]>::to_vec)
        .unwrap_or_default();
    let people = folders
        .iter()
        .find(|f| {
            f.get("title")
                .and_then(JsValue::as_js_string)
                .is_some_and(|t| t.eq_str("People"))
        })
        .cloned()
        .expect("the folder is named");
    let people_id = people
        .get("id")
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .expect("id");

    // A page in it, from the folder's menu: it opens in the editor.
    harness
        .get_by_role_and_label(Role::Button, "Actions for folder People")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Add page")
        .click();
    harness.run_steps(12);
    assert!(
        harness.query_by_label("Scene text").is_some(),
        "the new page is open"
    );
    let docs = data(&memory)
        .get("documents")
        .and_then(JsValue::as_array)
        .map(<[JsValue]>::to_vec)
        .unwrap_or_default();
    let page = docs.last().cloned().expect("a page");
    assert_eq!(
        page.get("folderId")
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .as_deref(),
        Some(people_id.as_str())
    );
    assert_eq!(
        page.get("title")
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .as_deref(),
        Some("Untitled page")
    );

    // Removing the folder (confirmed) moves its page to the Wiki root; nothing is deleted.
    harness
        .get_by_role_and_label(Role::Button, "Actions for folder People")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Remove folder")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Remove")
        .click();
    harness.run_steps(12);
    let after = data(&memory);
    assert!(
        !after
            .get("researchFolders")
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .any(|f| f
                .get("id")
                .and_then(JsValue::as_js_string)
                .is_some_and(|i| i.eq_str(&people_id)))
    );
    let page_after = after
        .get("documents")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .last()
        .cloned()
        .expect("the page is still there");
    assert!(
        page_after.get("folderId").is_none(),
        "unfiled at the Wiki root"
    );
}

#[test]
fn labels_are_applied_and_managed() {
    let (mut harness, memory) = app();
    let data = |memory: &Memory| {
        project(&memory.last(), "project-1")
            .get("project")
            .cloned()
            .expect("data")
    };
    let label_of = |data: &JsValue, key: &str, id: &str| -> Option<String> {
        data.get(key)
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .find(|i| {
                i.get("id")
                    .and_then(JsValue::as_js_string)
                    .is_some_and(|v| v.eq_str(id))
            })
            .and_then(|i| i.get("labelId"))
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
    };
    // A scene's menu applies a label.
    harness
        .get_by_role_and_label(Role::Button, "Actions for Opening Scene")
        .click();
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Draft").click();
    harness.run_steps(12);
    assert_eq!(
        label_of(&data(&memory), "documents", "doc-1").as_deref(),
        Some("label-1")
    );
    assert!(
        harness.query_all_by_label("Label: Draft").count() == 2,
        "the scene shows its label, as its chapter does"
    );

    // Manage labels… from Project settings: delete Draft (used by a scene and a chapter), add one.
    harness
        .get_by_role_and_label(Role::Button, "Project settings")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Manage labels…")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Delete label Draft")
        .click();
    harness.run();
    assert!(
        harness
            .query_by_label_contains("Delete label “Draft”? 1 scene and 1 chapter will lose it.")
            .is_some()
    );
    harness
        .get_by_role_and_label(Role::Button, "Delete label")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Add label")
        .click();
    harness.run_steps(2);
    assert!(
        harness
            .get_by_role_and_label(Role::TextInput, "Name for label 1")
            .is_focused(),
        "the new label's name is focused"
    );
    harness
        .get_by_role_and_label(Role::RadioButton, "blue")
        .click();
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Save").click();
    harness.run_steps(12);
    let after = data(&memory);
    assert_eq!(
        label_of(&after, "documents", "doc-1"),
        None,
        "the deleted label is cleared from the scene"
    );
    assert_eq!(
        label_of(&after, "chapters", "chapter-1"),
        None,
        "and from the chapter"
    );
    let labels = after
        .get("labels")
        .and_then(JsValue::as_array)
        .map(<[JsValue]>::to_vec)
        .unwrap_or_default();
    assert_eq!(labels.len(), 1);
    let text = |k: &str| {
        labels[0]
            .get(k)
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    };
    assert_eq!(
        (text("name").as_str(), text("color").as_str()),
        ("New label", "blue")
    );
    assert!(text("id").starts_with("label-"), "{}", text("id"));
}

#[test]
fn storyboard_cards_are_added_edited_nudged_and_removed() {
    let (mut harness, memory) = app();
    let data = |memory: &Memory| {
        project(&memory.last(), "project-1")
            .get("project")
            .cloned()
            .expect("data")
    };
    let card = |data: &JsValue, kind: &str| -> Option<JsValue> {
        data.get("storyboardCards")
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .find(|c| {
                c.get("kind")
                    .and_then(JsValue::as_js_string)
                    .is_some_and(|k| k.eq_str(kind))
            })
            .cloned()
    };
    let num = |c: &JsValue, k: &str| match c.get(k) {
        Some(JsValue::Number(n)) => *n,
        _ => f64::NAN,
    };
    harness
        .get_by_role_and_label(Role::Tab, "Storyboard")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Add Chapter One near the canvas center")
        .click();
    harness.run_steps(12);
    let added = card(&data(&memory), "chapter").expect("a chapter card");
    let s = |c: &JsValue, k: &str| {
        c.get(k)
            .and_then(JsValue::as_js_string)
            .map(|v| v.to_string_lossy())
            .unwrap_or_default()
    };
    assert_eq!(
        (s(&added, "title").as_str(), s(&added, "sourceId").as_str()),
        ("Chapter One", "chapter-1")
    );
    let (x, y) = (num(&added, "x"), num(&added, "y"));

    // The note of a chapter card is the chapter's summary.
    let note =
        harness.get_by_role_and_label(Role::MultilineTextInput, "Chapter summary for Chapter One");
    note.focus();
    harness.run();
    harness
        .get_by_role_and_label(Role::MultilineTextInput, "Chapter summary for Chapter One")
        .type_text("The storm breaks.");
    harness.run();
    harness.key_press(egui::Key::Escape);
    harness.run_steps(12);
    let after = data(&memory);
    let chapter = after
        .get("chapters")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .find(|c| s(c, "id") == "chapter-1")
        .cloned()
        .expect("chapter");
    assert!(
        s(&chapter, "summary").ends_with("The storm breaks."),
        "{}",
        s(&chapter, "summary")
    );
    assert_eq!(
        s(&card(&after, "chapter").expect("card"), "note"),
        s(&chapter, "summary")
    );

    // Arrow keys nudge a focused card by 24.
    harness
        .get_by_role_and_label(
            Role::Button,
            "Move Chapter One. Use arrow keys to nudge the card, or drag to position it.",
        )
        .focus();
    harness.run();
    harness.key_press(egui::Key::ArrowRight);
    harness.run_steps(12);
    let moved = card(&data(&memory), "chapter").expect("card");
    assert_eq!((num(&moved, "x"), num(&moved, "y")), (x + 24.0, y));

    harness
        .get_by_role_and_label(Role::Button, "Remove Chapter One from storyboard")
        .click();
    harness.run_steps(12);
    assert!(
        card(&data(&memory), "chapter").is_none(),
        "the card is removed"
    );
    assert!(
        card(&data(&memory), "scene").is_some(),
        "the other cards stay"
    );

    // Dragging a Story element onto the canvas adds its card where it is dropped.
    let from = harness
        .get_by_role_and_label(Role::Button, "Drag Mara onto the storyboard canvas")
        .rect()
        .center();
    let canvas = harness.get_by_label("Freeform storyboard").rect();
    let to = canvas.center();
    harness.event(egui::Event::PointerMoved(from));
    harness.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
    for step in 1..=5 {
        harness.event(egui::Event::PointerMoved(
            from + (to - from) * (step as f32 / 5.0),
        ));
        harness.run();
    }
    harness.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(12);
    let dropped = card(&data(&memory), "Wiki").expect("the dropped wiki card");
    assert_eq!(s(&dropped, "title"), "Mara");
    assert!(
        num(&dropped, "x") > 0.0 && num(&dropped, "y") > 0.0,
        "placed on the canvas"
    );
    // A card dragged by its handle moves by as much.
    let before = card(&data(&memory), "scene").expect("scene card");
    let handle = harness
        .get_by_role_and_label(
            Role::Button,
            "Move Opening. Use arrow keys to nudge the card, or drag to position it.",
        )
        .rect()
        .center();
    harness.event(egui::Event::PointerMoved(handle));
    harness.event(egui::Event::PointerButton {
        pos: handle,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
    for step in 1..=4 {
        harness.event(egui::Event::PointerMoved(
            handle + egui::vec2(12.0, 6.0) * step as f32,
        ));
        harness.run();
    }
    harness.event(egui::Event::PointerButton {
        pos: handle + egui::vec2(48.0, 24.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(12);
    let after = card(&data(&memory), "scene").expect("scene card");
    assert_eq!(
        (
            num(&after, "x") - num(&before, "x"),
            num(&after, "y") - num(&before, "y")
        ),
        (48.0, 24.0)
    );
}
