//! Editing through the interface, checked in what is saved.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
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
