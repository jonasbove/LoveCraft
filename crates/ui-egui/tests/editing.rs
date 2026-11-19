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
        // Under a loaded CI machine a step can take a couple more frames to settle (egui
        // reports a font-provider repaint); `run` gets that room, nothing else changes.
        .with_max_steps(8)
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
fn shift_enter_breaks_the_line_inside_a_paragraph() {
    let (mut harness, memory) = app();
    harness
        .get_all_by_role_and_label(Role::Button, "Add chapter")
        .next()
        .expect("an add button")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::TreeItem, "Scene 1")
        .click();
    harness.run();
    let body = harness.get_by_label("Scene text");
    body.focus();
    body.type_text("Roses are red,");
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Enter);
    harness
        .get_by_label("Scene text")
        .type_text("violets are blue.");
    harness.run_steps(12);
    let saved = memory.last();
    let scene = document(project(&saved, "project-1"), "new-1").expect("the new scene");
    assert_eq!(
        js::stringify(scene.get("content").expect("content")),
        r#""<p>Roses are red,<br>violets are blue.</p>""#
    );
}

#[test]
fn insert_image_puts_the_picture_at_the_caret_as_prosecraft_does() {
    let (mut harness, memory) = app();
    harness
        .get_all_by_role_and_label(Role::Button, "Add chapter")
        .next()
        .expect("an add button")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::TreeItem, "Scene 1")
        .click();
    harness.run();
    let body = harness.get_by_label("Scene text");
    body.focus();
    body.type_text("The map:");
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Insert image")
        .click();
    harness.run();
    assert!(
        harness
            .state_mut()
            .take_requests()
            .contains(&prosecraft_ui_egui::HostRequest::InsertPicture)
    );
    // A 4 x 2 PNG, as the host's picker hands it over.
    let pixels = craft_image_codecs::Image::from_u8(
        4,
        2,
        craft_image_codecs::ChannelLayout::Rgba,
        vec![200; 4 * 2 * 4],
    )
    .expect("pixels");
    let png = craft_image_codecs::encode(
        &pixels,
        craft_image_codecs::Format::Png,
        &craft_image_codecs::EncodeOptions::default(),
    )
    .expect("png");
    harness.state_mut().insert_picture("Coast map.png", &png);
    harness.run();
    // Typing goes on after the picture's line break.
    harness.get_by_label("Scene text").type_text("North");
    harness.run_steps(12);
    let saved = memory.last();
    let scene = document(project(&saved, "project-1"), "new-1").expect("the new scene");
    let content = scene
        .get("content")
        .and_then(JsValue::as_js_string)
        .map(|c| c.to_string_lossy())
        .unwrap_or_default();
    assert!(
        content.starts_with(r#"<p>The map:<img src="data:image/webp;base64,"#),
        "{content}"
    );
    assert!(
        content.ends_with(r#"" alt="Coast map"><br>North</p>"#),
        "{content}"
    );
    // Read back, it is still the rich editor's (and a picture that can't be read is refused).
    assert!(prosecraft_core::richtext::RichDoc::from_html(&content).is_some());
    harness
        .state_mut()
        .insert_picture("notes.txt", b"plain text");
    harness.run();
    harness.get_by_label_contains("Use a PNG, JPG, WebP, GIF, BMP, or AVIF image.");
}

#[test]
fn a_selected_picture_is_laid_out_from_its_toolbar() {
    let (mut harness, memory) = app();
    harness
        .get_all_by_role_and_label(Role::Button, "Add chapter")
        .next()
        .expect("an add button")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::TreeItem, "Scene 1")
        .click();
    harness.run();
    let pixels = craft_image_codecs::Image::from_u8(
        40,
        20,
        craft_image_codecs::ChannelLayout::Rgba,
        vec![200; 40 * 20 * 4],
    )
    .expect("pixels");
    let png = craft_image_codecs::encode(
        &pixels,
        craft_image_codecs::Format::Png,
        &craft_image_codecs::EncodeOptions::default(),
    )
    .expect("png");
    harness.state_mut().insert_picture("Coast map.png", &png);
    harness.run();
    assert!(harness.query_by_label("Selected image tools").is_none());
    harness
        .get_by_role_and_label(Role::Image, "Coast map")
        .click();
    harness.run();
    harness.get_by_label("Medium image size");
    harness.get_by_label("Selected image tools");
    let stored = |memory: &Memory| {
        let c = content(memory, "new-1");
        let from = c.find("\" alt=").unwrap_or(0);
        let to = c[from..].find('>').map_or(c.len(), |e| from + e + 1);
        c[from..to].to_owned()
    };
    harness.get_by_label("Medium image size").click();
    harness.run_steps(12);
    assert_eq!(stored(&memory), r#"" alt="Coast map" data-size="medium">"#);
    harness
        .get_by_label("Wrap text beside right-aligned image")
        .click();
    harness.run_steps(12);
    assert_eq!(
        stored(&memory),
        r#"" alt="Coast map" data-size="medium" data-align="right" data-wrap="wrap-right">"#
    );
    harness.get_by_label("Image on its own line").click();
    harness.get_by_label("Align image center").click();
    harness.run_steps(12);
    assert_eq!(stored(&memory), r#"" alt="Coast map" data-size="medium">"#);
    // A custom width typed in.
    harness.get_by_label("Custom").click();
    harness.run();
    harness
        .get_by_label("Custom image width in percent")
        .focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run();
    harness
        .get_by_label("Custom image width in percent")
        .type_text("62");
    harness.run_steps(12);
    assert_eq!(
        stored(&memory),
        r#"" alt="Coast map" data-size="custom" data-width="62">"#
    );
    // Pixel sizes: 50% of the column, the height from the picture's 2:1.
    harness.get_by_label("Original size").click();
    harness.run_steps(12);
    assert_eq!(stored(&memory), r#"" alt="Coast map">"#);
    harness.get_by_label("50%").click();
    harness.run_steps(12);
    let sized = stored(&memory);
    let width: u32 = sized
        .split("width=\"")
        .nth(1)
        .and_then(|w| w.split('"').next())
        .and_then(|w| w.parse().ok())
        .expect("width");
    let height: u32 = sized
        .split("height=\"")
        .nth(1)
        .and_then(|w| w.split('"').next())
        .and_then(|w| w.parse().ok())
        .expect("height");
    assert!(
        width > 100 && (height as i64 - (width as i64 / 2)).abs() <= 1,
        "{sized}"
    );
    // Removed.
    harness.get_by_label("Remove image").click();
    harness.run_steps(12);
    assert!(!content(&memory, "new-1").contains("<img"));
    assert!(harness.query_by_label("Selected image tools").is_none());
}

#[test]
fn pasting_keeps_the_clipboards_formatting_and_pictures() {
    let (mut harness, memory) = app();
    let html = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let reader = html.clone();
    harness
        .state_mut()
        .set_clipboard_html(std::sync::Arc::new(move || {
            Some(reader.lock().expect("clipboard").clone()).filter(|h| !h.is_empty())
        }));
    harness
        .get_all_by_role_and_label(Role::Button, "Add chapter")
        .next()
        .expect("an add button")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::TreeItem, "Scene 1")
        .click();
    harness.run();
    harness.get_by_label("Scene text").focus();
    harness.run();
    // A page's selection: its HTML kept, cleaned.
    *html.lock().expect("clipboard") =
        r#"<meta charset="utf-8"><span style="font-size: 20px">A <b>bold</b> <a href="https://example.com/">link</a>.</span>"#.to_owned();
    harness.event(egui::Event::Paste("A bold link.".into()));
    harness.run_steps(12);
    assert_eq!(
        content(&memory, "new-1"),
        r#"<p>A <b>bold</b> <a href="https://example.com/">link</a>.</p>"#
    );
    // HTML left from an earlier copy isn't what is pasted: the text is.
    *html.lock().expect("clipboard") = "<p>Something else</p>".to_owned();
    harness.event(egui::Event::Paste(" More".into()));
    harness.run_steps(12);
    assert_eq!(
        content(&memory, "new-1"),
        r#"<p>A <b>bold</b> <a href="https://example.com/">link</a>. More</p>"#
    );
    // An image: a picture, named as the browser names a pasted image.
    let image = egui::ColorImage::new([6, 4], vec![egui::Color32::from_rgb(10, 120, 200); 24]);
    harness.event(egui::Event::PasteImage(std::sync::Arc::new(image)));
    harness.run_steps(12);
    let stored = content(&memory, "new-1");
    assert!(stored.contains(r#"" alt="image">"#), "{stored}");
}

#[test]
fn an_encrypted_backup_is_exported_with_a_passphrase() {
    let (mut harness, _memory) = app();
    harness
        .get_by_role_and_label(Role::Button, "Settings…")
        .click_accesskit();
    harness.run();
    harness
        .get_by_role_and_label(Role::Tab, "Data and sync")
        .click_accesskit();
    harness.run();
    harness.get_by_label("Export encrypted").click_accesskit();
    harness.run();
    let fill = |harness: &mut Harness<'static, ProseApp>, name: &str, text: &str| {
        let role = if name == "Passphrase hint" {
            Role::TextInput
        } else {
            Role::PasswordInput
        };
        harness.get_by_role_and_label(role, name).focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.get_by_role_and_label(role, name).type_text(text);
        harness.run();
    };
    fill(&mut harness, "Passphrase", "short");
    harness.get_by_label("Encrypt & export").click_accesskit();
    harness.run();
    harness.get_by_label("Use at least 8 characters.");
    fill(&mut harness, "Passphrase", "lighthouse keeper");
    fill(&mut harness, "Confirm passphrase", "lighthouse keepr");
    harness.get_by_label("Encrypt & export").click_accesskit();
    harness.run();
    harness.get_by_label("The passphrases do not match.");
    fill(&mut harness, "Confirm passphrase", "lighthouse keeper");
    fill(&mut harness, "Passphrase hint", "the job");
    harness.get_by_label("Encrypt & export").click_accesskit();
    let mut text = None;
    for _ in 0..600 {
        harness.step();
        if let Some(prosecraft_ui_egui::HostRequest::ExportBackup { file_name, text: t }) = harness
            .state_mut()
            .take_requests()
            .into_iter()
            .find(|r| matches!(r, prosecraft_ui_egui::HostRequest::ExportBackup { .. }))
        {
            assert_eq!(file_name, "prosecraft-encrypted.prosecraft");
            text = Some(t);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let text = text.expect("the encrypted backup");
    let bundle =
        prosecraft_core::storage::bundle::EncryptedBundle::parse(&text).expect("encrypted");
    assert_eq!(bundle.hint().as_deref(), Some("the job"));
    let (_, projects) = bundle.open(&"lighthouse keeper".into()).expect("opens");
    assert!(!projects.is_empty());
    harness.run();
    assert!(harness.query_by_label("Encrypt & export").is_none());
}

#[test]
fn the_about_window_reports_the_setup_and_never_the_writing() {
    let (mut harness, _memory) = app();
    harness
        .get_by_label(&format!("About {}", prosecraft_core::about::APP_NAME))
        .click();
    harness.run();
    let report = harness
        .get_by_role_and_label(Role::MultilineTextInput, "Diagnostics report")
        .value()
        .unwrap_or_default();
    assert!(
        report.starts_with(&format!(
            "app: {}",
            prosecraft_core::about::name_and_version()
        )),
        "{report}"
    );
    for line in [
        "projects: 2",
        "chapters: 1",
        "scenes: 2",
        "wiki pages: 1",
        "comments: 1",
        "zen mode: off",
        "proofreading: off",
    ] {
        assert!(report.lines().any(|l| l == line), "{line} in {report}");
    }
    // Nothing of the writing: not the project's name, its scenes, or the comment.
    for private in ["The L", "Opening Scene", "Mara", "Darker", "dark"] {
        assert!(!report.contains(private), "{private} in {report}");
    }
    harness.get_by_label("Copy diagnostics").click();
    harness.run();
    assert!(
        harness
            .get_all_by_role(Role::Status)
            .any(|n| n.value().as_deref() == Some("Copied"))
    );
    harness.get_by_label("Done").click();
    harness.run();
    assert!(harness.query_by_label("Copy diagnostics").is_none());
    // From Settings, as ProseCraft's About category has it.
    harness
        .get_by_role_and_label(Role::Button, "Settings…")
        .click_accesskit();
    harness.run();
    harness
        .get_by_role_and_label(Role::Tab, "About")
        .click_accesskit();
    harness.run();
    harness
        .get_by_label_contains(&format!(
            "About {} · version",
            prosecraft_core::about::APP_NAME
        ))
        .click_accesskit();
    harness.run();
    harness.get_by_label("Copy diagnostics");
}

/// Drags with the pointer from one point to another, as a person does.
fn drag(harness: &mut Harness<'static, ProseApp>, from: egui::Pos2, to: egui::Pos2) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    harness.event(egui::Event::PointerMoved(from));
    harness.step();
    harness.event(button(from, true));
    harness.step();
    for step in 1..=8 {
        harness.event(egui::Event::PointerMoved(
            from + (to - from) * (step as f32 / 8.0),
        ));
        harness.step();
    }
    harness.event(button(to, false));
    harness.run_steps(8);
}

/// The middle of a node, and a point a quarter of the way down it.
fn rows(harness: &Harness<'static, ProseApp>, name: &str) -> (egui::Pos2, egui::Pos2) {
    let r = harness.get_by_role_and_label(Role::TreeItem, name).rect();
    (
        r.center(),
        egui::pos2(r.center().x, r.top() + r.height() * 0.2),
    )
}

#[test]
fn binder_rows_are_dragged_into_place_as_prosecraft_drags_them() {
    let (mut harness, memory) = app();
    let order = |memory: &Memory, key: &str| -> Vec<String> {
        let saved = memory.last();
        project(&saved, "project-1")
            .get("project")
            .and_then(|p| p.get(key))
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .filter(|d| d.get("trashed") != Some(&JsValue::Bool(true)) && d.get("space").is_none())
            .filter_map(|d| {
                d.get("title")
                    .and_then(JsValue::as_js_string)
                    .map(|t| t.to_string_lossy())
            })
            .collect()
    };
    // A scene above the one before it.
    let (lighthouse, _) = rows(&harness, "INT. LIGHTHOUSE - NIGHT");
    let (_, opening_top) = rows(&harness, "Opening Scene");
    drag(&mut harness, lighthouse, opening_top);
    harness.run_steps(12);
    assert_eq!(
        order(&memory, "documents"),
        ["INT. LIGHTHOUSE - NIGHT", "Opening Scene"]
    );
    // A new chapter, dragged above the first.
    harness
        .get_all_by_role_and_label(Role::Button, "Add chapter")
        .next()
        .expect("add")
        .click();
    harness.run_steps(12);
    let chapters = order(&memory, "chapters");
    assert_eq!(chapters.len(), 2, "{chapters:?}");
    let (second, _) = rows(&harness, &chapters[1]);
    let (_, first_top) = rows(&harness, &chapters[0]);
    drag(&mut harness, second, first_top);
    harness.run_steps(12);
    assert_eq!(
        order(&memory, "chapters"),
        [chapters[1].clone(), chapters[0].clone()]
    );
    // A scene onto a chapter: the end of that chapter.
    let (opening, _) = rows(&harness, "Opening Scene");
    let (new_chapter, _) = rows(&harness, &chapters[1]);
    drag(&mut harness, opening, new_chapter);
    harness.run_steps(12);
    let saved = memory.last();
    let scene = document(project(&saved, "project-1"), "doc-1").expect("the opening scene");
    let new_id = project(&saved, "project-1")
        .get("project")
        .and_then(|p| p.get("chapters"))
        .and_then(JsValue::as_array)
        .and_then(|c| c.first().and_then(|c| c.get("id")).cloned());
    assert_eq!(scene.get("chapterId").cloned(), new_id);
}

/// The stored content of a document.
fn content(memory: &Memory, id: &str) -> String {
    let saved = memory.last();
    document(project(&saved, "project-1"), id)
        .and_then(|d| d.get("content"))
        .and_then(JsValue::as_js_string)
        .map(|c| c.to_string_lossy())
        .unwrap_or_default()
}

#[test]
fn undo_takes_back_typing_a_step_at_a_time_and_redo_brings_it_back() {
    let (mut harness, memory) = app();
    harness
        .get_all_by_role_and_label(Role::Button, "Add chapter")
        .next()
        .expect("an add button")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::TreeItem, "Scene 1")
        .click();
    harness.run();
    let body = harness.get_by_label("Scene text");
    body.focus();
    body.type_text("The lamp");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.get_by_label("Scene text").type_text("went out");
    harness.run();
    let undo = |harness: &mut Harness<'static, ProseApp>| {
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
    };
    // Typing is one step, the new line another (ProseCraft's grouping).
    undo(&mut harness);
    harness.run_steps(12);
    assert_eq!(content(&memory, "new-1"), "<p>The lamp</p><p><br></p>");
    undo(&mut harness);
    harness.run_steps(12);
    assert_eq!(content(&memory, "new-1"), "<p>The lamp</p>");
    harness.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::Z,
    );
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Y);
    harness.run_steps(12);
    assert_eq!(content(&memory, "new-1"), "<p>The lamp</p><p>went out</p>");
    // The toolbar's Undo, then typing: redo is gone.
    harness.get_by_role_and_label(Role::Button, "Undo").click();
    harness.run();
    harness.get_by_label("Scene text").focus();
    harness.run();
    harness.get_by_label("Scene text").type_text("!");
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Redo").click();
    harness.run_steps(12);
    let after = content(&memory, "new-1");
    assert!(
        after.contains('!') && !after.contains("went out"),
        "{after}"
    );
}

#[test]
fn add_link_links_the_selection() {
    let (mut harness, memory) = app();
    harness
        .get_all_by_role_and_label(Role::Button, "Add chapter")
        .next()
        .expect("an add button")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::TreeItem, "Scene 1")
        .click();
    harness.run();
    let body = harness.get_by_label("Scene text");
    body.focus();
    body.type_text("See the map");
    harness.run();
    for _ in 0..7 {
        harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowLeft);
    }
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Add link")
        .click();
    harness.run();
    harness
        .get_by_label("Paste a link")
        .type_text("javascript:alert(1)");
    harness.key_press(egui::Key::Enter);
    harness.run();
    harness.get_by_label("Please use a web link or email address.");
    harness.get_by_label("Paste a link").focus();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_label("Paste a link")
        .type_text("example.com/map");
    harness.key_press(egui::Key::Enter);
    harness.run_steps(12);
    assert_eq!(
        content(&memory, "new-1"),
        r#"<p>See <a href="https://example.com/map">the map</a></p>"#
    );
    assert!(harness.query_by_label("Paste a link").is_none());
}

#[test]
fn app_shortcuts_and_the_shortcuts_sheet() {
    use egui::Modifiers as M;
    let (mut harness, memory) = app();
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    // Ctrl+Shift+E: the scene as text, named after its title.
    harness.key_press_modifiers(M::COMMAND | M::SHIFT, egui::Key::E);
    harness.run();
    let requests = harness.state_mut().take_requests();
    let Some(prosecraft_ui_egui::HostRequest::ExportFile {
        file_name, bytes, ..
    }) = requests.first()
    else {
        panic!("no export: {requests:?}");
    };
    assert_eq!(file_name, "opening-scene.txt");
    let text = String::from_utf8_lossy(bytes);
    assert!(text.starts_with("It was dark."), "{text}");
    assert!(!text.contains("Smith"), "a citation draws no text: {text}");
    // Ctrl+/: the sheet; Save as PDF; Escape closes it.
    harness.key_press_modifiers(M::COMMAND, egui::Key::Slash);
    harness.run();
    harness.get_by_label("Toggle focus mode");
    assert!(harness.get_all_by_label("Ctrl").count() > 5);
    harness
        .get_by_role_and_label(Role::Button, "Save as PDF")
        .click();
    harness.run();
    let requests = harness.state_mut().take_requests();
    assert!(requests.iter().any(|r| matches!(r,
        prosecraft_ui_egui::HostRequest::ExportFile { file_name, bytes, .. }
            if file_name == "prosecraft-shortcuts.pdf" && bytes.starts_with(b"%PDF-"))));
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.query_by_label("Toggle focus mode").is_none());
    // Ctrl+, Settings; Ctrl+Shift+P Publish.
    harness.key_press_modifiers(M::COMMAND, egui::Key::Comma);
    harness.run();
    harness.get_by_label("Writing");
    harness.key_press(egui::Key::Escape);
    harness.run();
    // Ctrl+N: a new scene in the first chapter.
    let scenes = |memory: &Memory| -> usize {
        project(&memory.last(), "project-1")
            .get("project")
            .and_then(|p| p.get("documents"))
            .and_then(JsValue::as_array)
            .map_or(0, <[JsValue]>::len)
    };
    harness.run_steps(12);
    let before = scenes(&memory);
    harness.key_press_modifiers(M::COMMAND, egui::Key::N);
    harness.run_steps(12);
    assert_eq!(scenes(&memory), before + 1);
    harness.key_press_modifiers(M::COMMAND | M::SHIFT, egui::Key::P);
    harness.run();
    assert_eq!(
        harness
            .get_by_role_and_label(Role::Tab, "Publish")
            .accesskit_node()
            .is_selected(),
        Some(true)
    );
}

#[test]
fn chapters_and_scenes_move_rename_and_leave_the_trash_for_good() {
    let (mut harness, memory) = app();
    let field = |memory: &Memory, key: &str, what: &str| -> Vec<String> {
        project(&memory.last(), "project-1")
            .get("project")
            .and_then(|p| p.get(key))
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .filter(|d| {
                !d.get("trashed")
                    .is_some_and(|t| matches!(t, JsValue::Bool(true)))
            })
            .map(|d| {
                d.get(what)
                    .and_then(JsValue::as_js_string)
                    .map(|s| s.to_string_lossy())
                    .unwrap_or_default()
            })
            .collect()
    };
    let menu = |harness: &mut Harness<'static, ProseApp>, row: &str, item: &str| {
        harness
            .get_by_role_and_label(Role::Button, &format!("Actions for {row}"))
            .click();
        harness.run();
        harness.get_by_role_and_label(Role::Button, item).click();
        harness.run();
    };
    harness.run_steps(12);
    let scenes_before = field(&memory, "documents", "title");
    menu(&mut harness, "INT. LIGHTHOUSE - NIGHT", "Move up");
    harness.run_steps(12);
    let scenes = field(&memory, "documents", "title");
    let at = |list: &[String], t: &str| list.iter().position(|x| x == t);
    assert!(
        at(&scenes, "INT. LIGHTHOUSE - NIGHT") < at(&scenes, "Opening Scene"),
        "{scenes:?} from {scenes_before:?}"
    );
    // A new chapter; the opening scene moves into it.
    harness
        .get_all_by_role_and_label(Role::Button, "Add chapter")
        .next()
        .expect("an add button")
        .click();
    harness.run();
    menu(&mut harness, "Opening Scene", "Move to chapter…");
    harness
        .get_by_role_and_label(Role::RadioButton, "Chapter 1")
        .click();
    harness.run_steps(12);
    let saved = memory.last();
    let opening = document(project(&saved, "project-1"), "doc-1").expect("opening scene");
    let chapters = field(&memory, "chapters", "id");
    assert_eq!(
        opening
            .get("chapterId")
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy()),
        chapters.last().cloned()
    );
    // Rename the new chapter, move it up, then to the Trash (it asks first).
    menu(&mut harness, "Chapter 1", "Rename chapter");
    harness.get_by_label("Rename Chapter 1").focus();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_label("Rename Chapter 1")
        .type_text("Part Two");
    harness.key_press(egui::Key::Enter);
    harness.run();
    menu(&mut harness, "Part Two", "Move up");
    harness.run_steps(12);
    assert_eq!(
        field(&memory, "chapters", "title")
            .first()
            .map(String::as_str),
        Some("Part Two")
    );
    menu(&mut harness, "Part Two", "Move to Trash");
    // Its own first scene and the opening scene.
    harness.get_by_label_contains("Move “Part Two” and its 2 scenes to Trash?");
    harness
        .get_all_by_role_and_label(Role::Button, "Move to Trash")
        .last()
        .expect("the confirm button")
        .click();
    harness.run_steps(12);
    assert!(!field(&memory, "chapters", "title").contains(&"Part Two".to_owned()));
    // From the Trash, for good.
    harness.get_by_label_contains("Trash (").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Permanently delete Part Two and its scenes")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Delete")
        .click();
    harness.run_steps(12);
    let saved = memory.last();
    assert!(document(project(&saved, "project-1"), "doc-1").is_none());
    let titles: Vec<String> = project(&saved, "project-1")
        .get("project")
        .and_then(|p| p.get("chapters"))
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|c| c.get("title").and_then(JsValue::as_js_string))
        .map(|t| t.to_string_lossy())
        .collect();
    assert!(!titles.contains(&"Part Two".to_owned()), "{titles:?}");
    assert_eq!(chapters.len(), 2);
}

#[test]
fn the_label_filter_project_trash_and_converting_from_the_binder() {
    let (mut harness, memory) = app();
    let menu = |harness: &mut Harness<'static, ProseApp>, row: &str| {
        harness
            .get_by_role_and_label(Role::Button, &format!("Actions for {row}"))
            .click();
        harness.run();
    };
    // Label the opening scene "Draft", then show only Draft scenes.
    menu(&mut harness, "Opening Scene");
    harness.get_by_role_and_label(Role::Button, "Draft").click();
    harness.run_steps(4);
    harness.get_by_label("Filter scenes by label").click();
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Draft").click();
    harness.run();
    harness.get_by_role_and_label(Role::TreeItem, "Opening Scene");
    assert!(
        harness
            .query_by_role_and_label(Role::TreeItem, "INT. LIGHTHOUSE - NIGHT")
            .is_none()
    );
    harness
        .get_by_role_and_label(Role::Button, "Clear label filter: Draft")
        .click();
    harness.run();
    harness.get_by_role_and_label(Role::TreeItem, "INT. LIGHTHOUSE - NIGHT");
    // Convert a scene from its menu (it asks first).
    menu(&mut harness, "Opening Scene");
    harness
        .get_by_role_and_label(Role::Button, "Convert this scene to screenplay")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Convert")
        .click();
    harness.run_steps(12);
    let saved = memory.last();
    let opening = document(project(&saved, "project-1"), "doc-1").expect("opening");
    assert_eq!(
        opening
            .get("format")
            .and_then(JsValue::as_js_string)
            .map(|f| f.to_string_lossy())
            .as_deref(),
        Some("screenplay")
    );
    // The project to the Trash (it asks first): the other one opens; back, then gone for good.
    harness
        .get_by_role_and_label(Role::Button, "Move current project to Trash")
        .click();
    harness.run();
    harness.get_by_label_contains("to Trash? You can restore it later.");
    harness
        .get_by_role_and_label(Role::Button, "Move to Trash")
        .click();
    harness.run_steps(12);
    let trashed = |memory: &Memory, id: &str| {
        project(&memory.last(), id)
            .get("project")
            .and_then(|p| p.get("trashed"))
            .is_some_and(|t| matches!(t, JsValue::Bool(true)))
    };
    assert!(trashed(&memory, "project-1"));
    harness.get_by_label_contains("Trash (").click();
    harness.run();
    harness.get_by_label_contains("Project: The Lighthouse");
    harness
        .get_by_role_and_label(Role::Button, "Restore project The Lighthouse 🌊 Keeper")
        .click();
    harness.run_steps(12);
    assert!(!trashed(&memory, "project-1"));
}

#[test]
fn front_and_back_matter_pages_are_added_written_moved_and_removed() {
    let (mut harness, memory) = app();
    let pages = |memory: &Memory, key: &str| -> Vec<(String, String, bool)> {
        project(&memory.last(), "project-1")
            .get("project")
            .and_then(|p| p.get("publishOptions"))
            .and_then(|o| o.get(key))
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .map(|p| {
                let s = |k: &str| {
                    p.get(k)
                        .and_then(JsValue::as_js_string)
                        .map(|v| v.to_string_lossy())
                        .unwrap_or_default()
                };
                (
                    s("title"),
                    s("body"),
                    matches!(p.get("blank"), Some(JsValue::Bool(true))),
                )
            })
            .collect()
    };
    harness.get_by_role_and_label(Role::Tab, "Publish").click();
    harness.run();
    harness.get_by_label("Front matter").scroll_to_me();
    harness.run();
    harness.get_by_label("Front matter").click();
    harness.run();
    harness.get_by_label("Front matter preset").scroll_to_me();
    harness.run();
    harness.get_by_label("Front matter preset").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Foreword")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Add page")
        .click();
    harness.run_steps(12);
    harness.get_by_label("Foreword text").focus();
    harness
        .get_by_label("Foreword text")
        .type_text("Why this book.");
    harness.get_by_label("Front matter preset").focus();
    harness.run_steps(12);
    assert_eq!(
        pages(&memory, "frontMatter"),
        [("Foreword".into(), "Why this book.".into(), false)]
    );
    harness.get_by_label("Front matter preset").scroll_to_me();
    harness.run();
    harness.get_by_label("Front matter preset").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Preface")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Add page")
        .click();
    harness.run_steps(12);
    harness
        .get_by_role_and_label(Role::Button, "Move Preface up")
        .click();
    harness.run_steps(12);
    harness
        .get_by_role_and_label(Role::Button, "Remove Foreword")
        .click();
    harness.run_steps(12);
    assert_eq!(
        pages(&memory, "frontMatter"),
        [("Preface".into(), String::new(), false)]
    );
    // A blank page at the back.
    harness.get_by_label("Back matter").scroll_to_me();
    harness.run();
    harness.get_by_label("Back matter").click();
    harness.run();
    harness.get_by_label("Back matter preset").scroll_to_me();
    harness.run();
    harness.get_by_label("Back matter preset").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Blank page")
        .click_accesskit();
    harness.run();
    harness
        .get_all_by_role_and_label(Role::Button, "Add page")
        .last()
        .expect("the back matter's Add page")
        .click();
    harness.run_steps(12);
    assert_eq!(
        pages(&memory, "backMatter"),
        [(String::new(), String::new(), true)]
    );
}

#[test]
fn a_cover_is_chosen_replaced_and_removed() {
    let (mut harness, memory) = app();
    let cover = |memory: &Memory| -> JsValue {
        project(&memory.last(), "project-1")
            .get("project")
            .and_then(|p| p.get("publishOptions"))
            .and_then(|o| o.get("cover"))
            .cloned()
            .unwrap_or(JsValue::Null)
    };
    let text = |v: &JsValue, k: &str| {
        v.get(k)
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    };
    harness.get_by_role_and_label(Role::Tab, "Publish").click();
    harness.run();
    harness.get_by_label("Cover: Image").scroll_to_me();
    harness.run();
    harness.get_by_label("Cover: Image").click_accesskit();
    harness.run_steps(12);
    // The Image tab stays chosen before there is an image, as in ProseCraft.
    assert_eq!(text(&cover(&memory), "mode"), "image");
    harness.get_by_label("No image");
    harness.get_by_label("Choose image").click_accesskit();
    harness.run();
    assert!(
        harness
            .state_mut()
            .take_requests()
            .contains(&prosecraft_ui_egui::HostRequest::ChooseCover)
    );
    // A file it can't use: ProseCraft's message, and nothing stored.
    harness.state_mut().set_cover_image("cover.gif", b"GIF89a");
    harness.run();
    harness.get_by_label("Use a PNG, JPG, or WebP image.");
    // A wide picture: cropped to a 1600 × 2400 cover.
    let mut rgba = Vec::new();
    for _ in 0..30 {
        for x in 0..60u8 {
            rgba.extend([x * 4, 90, 160, 255]);
        }
    }
    let image =
        craft_image_codecs::Image::from_u8(60, 30, craft_image_codecs::ChannelLayout::Rgba, rgba)
            .expect("image");
    let png =
        craft_image_codecs::encode(&image, craft_image_codecs::Format::Png, &Default::default())
            .expect("png");
    harness.state_mut().set_cover_image("Sea.png", &png);
    harness.run_steps(12);
    let stored = cover(&memory);
    assert_eq!(text(&stored, "mode"), "image");
    let image = text(&stored, "image");
    assert!(
        image.starts_with("data:image/webp;base64,"),
        "{}",
        &image[..30.min(image.len())]
    );
    assert!(
        harness
            .query_by_label("Use a PNG, JPG, or WebP image.")
            .is_none()
    );
    harness.get_by_label("Cover preview");
    harness.get_by_label("Replace");
    // Removed: no cover.
    harness.get_by_label("Remove").click_accesskit();
    harness.run_steps(12);
    let stored = cover(&memory);
    assert_eq!(
        (text(&stored, "mode"), text(&stored, "image")),
        ("none".to_owned(), String::new())
    );
    // Generated: a design and a palette.
    harness.get_by_label("Cover: Generated").click_accesskit();
    harness.run_steps(12);
    harness
        .get_by_role_and_label(Role::RadioButton, "Bold")
        .click_accesskit();
    harness.run_steps(12);
    harness.get_by_label("Plum palette").click_accesskit();
    harness.run_steps(12);
    let stored = cover(&memory);
    assert_eq!(
        (
            text(&stored, "mode"),
            text(&stored, "design"),
            text(&stored, "palette")
        ),
        ("generated".to_owned(), "bold".to_owned(), "plum".to_owned())
    );
}

#[test]
fn custom_styles_are_created_edited_and_deleted() {
    let (mut harness, memory) = app();
    let options = |memory: &Memory| -> JsValue {
        project(&memory.last(), "project-1")
            .get("project")
            .and_then(|p| p.get("publishOptions"))
            .cloned()
            .unwrap_or(JsValue::Null)
    };
    let text = |v: Option<&JsValue>| {
        v.and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    };
    harness.get_by_role_and_label(Role::Tab, "Publish").click();
    harness.run();
    harness.get_by_label("+ New style").scroll_to_me();
    harness.run();
    harness.get_by_label("+ New style").click();
    harness.run();
    harness.get_by_label("New style");
    harness.get_by_label("Font").click_accesskit();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Courier")
        .click_accesskit();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Create style")
        .click_accesskit();
    harness.run_steps(12);
    let saved = options(&memory);
    let style = saved
        .get("customStyles")
        .and_then(JsValue::as_array)
        .and_then(|s| s.first())
        .cloned()
        .expect("a custom style");
    assert_eq!(text(style.get("label")), "Manuscript copy");
    assert_eq!(text(style.get("fontId")), "courier");
    assert_eq!(text(saved.get("style")), text(style.get("id")));
    assert!(text(style.get("id")).starts_with("custom-"));
    // Renamed to a built-in style's name: refused; then to its own.
    harness
        .get_by_role_and_label(Role::Button, "Edit Manuscript copy")
        .click_accesskit();
    harness.run();
    harness.get_by_label("Style name").focus();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_label("Style name")
        .type_text("classic NOVEL");
    harness.run();
    harness.get_by_label("Another style already uses that name.");
    harness.get_by_label("Style name").focus();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_label("Style name").type_text("Draft");
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Save changes")
        .click_accesskit();
    harness.run_steps(12);
    let saved = options(&memory);
    assert_eq!(
        saved
            .get("customStyles")
            .and_then(JsValue::as_array)
            .map(|s| s.iter().map(|x| text(x.get("label"))).collect::<Vec<_>>()),
        Some(vec!["Draft".to_owned()])
    );
    // Deleted (asked first): Manuscript is chosen again.
    harness
        .get_by_role_and_label(Role::Button, "Delete Draft")
        .click_accesskit();
    harness.run();
    harness.get_by_label("Delete this style?");
    harness
        .get_by_role_and_label(Role::Button, "Delete")
        .click_accesskit();
    harness.run_steps(12);
    let saved = options(&memory);
    assert_eq!(text(saved.get("style")), "manuscript");
    assert_eq!(
        saved
            .get("customStyles")
            .and_then(JsValue::as_array)
            .map(<[JsValue]>::len),
        Some(0)
    );
}

#[test]
fn undo_takes_back_formatting() {
    let (mut harness, memory) = app();
    harness
        .get_all_by_role_and_label(Role::Button, "Add chapter")
        .next()
        .expect("an add button")
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
    harness.key_press_modifiers(egui::Modifiers::NONE, egui::Key::Home);
    for _ in 0..4 {
        harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowRight);
    }
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Bold").click();
    harness.run_steps(12);
    assert_eq!(
        content(&memory, "new-1"),
        "<p><strong>loud</strong> words</p>"
    );
    harness.get_by_role_and_label(Role::Button, "Undo").click();
    harness.run_steps(12);
    assert_eq!(content(&memory, "new-1"), "<p>loud words</p>");
}

#[test]
fn undo_outside_the_text_takes_back_the_last_change_on_the_page() {
    let (mut harness, memory) = app();
    let chapters = |memory: &Memory| -> usize {
        project(&memory.last(), "project-1")
            .get("project")
            .and_then(|p| p.get("chapters"))
            .and_then(JsValue::as_array)
            .map_or(0, <[JsValue]>::len)
    };
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    harness.run_steps(12);
    let before = chapters(&memory);
    harness
        .get_all_by_role_and_label(Role::Button, "Add chapter")
        .next()
        .expect("an add button")
        .click();
    harness.run_steps(12);
    assert_eq!(chapters(&memory), before + 1);
    // Nothing focused: Ctrl+Z is the page's.
    harness.ctx.memory_mut(|m| m.request_focus(egui::Id::NULL));
    harness.ctx.memory_mut(|m| {
        if let Some(id) = m.focused() {
            m.surrender_focus(id);
        }
    });
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run_steps(12);
    assert_eq!(chapters(&memory), before);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Y);
    harness.run_steps(12);
    assert_eq!(chapters(&memory), before + 1);
}

#[test]
fn a_screenplay_scene_opens_in_the_screenplay_editor() {
    let (mut harness, _memory) = app();
    harness
        .get_by_role_and_label(Role::TreeItem, "INT. LIGHTHOUSE - NIGHT")
        .click();
    harness.run();
    harness.get_by_label("Scene text").focus();
    harness.run();
    assert_eq!(
        harness
            .get_by_label("Element type")
            .accesskit_node()
            .value()
            .as_deref(),
        Some("Scene heading")
    );
    harness.get_by_label_contains("1 page · ~1 min");
    assert!(
        harness
            .query_by_label_contains("content the editor can't change yet")
            .is_none()
    );
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

/// Moves the pointer along the scene's first lines until `found` turns up.
fn hover_until(
    harness: &mut Harness<'static, ProseApp>,
    found: impl Fn(&Harness<'static, ProseApp>) -> bool,
) -> bool {
    let rect = harness.get_by_label("Scene text").rect();
    let mut y = rect.top() + 4.0;
    while y < rect.top() + 120.0 {
        let mut x = rect.left() + 2.0;
        while x < rect.right().min(rect.left() + 400.0) {
            harness.hover_at(egui::pos2(x, y));
            harness.run_steps(2);
            if found(harness) {
                return true;
            }
            x += 6.0;
        }
        y += 8.0;
    }
    false
}

#[test]
fn comments_and_wiki_mentions_are_highlighted_with_cards_on_hover() {
    let bundle = fixture("bundle-plain.prosecraft").replace(
        "<p>It was <em>dark</em>.",
        "<p>Mara said it was <em>dark</em>.",
    );
    let memory = Memory::default();
    let mut app =
        ProseApp::with_library(ProjectLibrary::new("test".into()), Box::new(memory.clone()));
    app.open_backup("bundle-plain.prosecraft", &bundle);
    let mut harness = Harness::builder()
        .with_max_steps(8)
        .with_size([1280.0, 850.0])
        .build_ui_state(|ui, app: &mut ProseApp| app.show(ui), app);
    prosecraft_ui_egui::setup_for_tests(&harness.ctx);
    harness.run();
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    // The Wiki article "Mara" is named: hovering it shows the article.
    assert!(hover_until(&mut harness, |h| h
        .query_by_label("Wiki article Mara")
        .is_some()));
    harness.get_by_label_contains("Notes about Mara.");
    // The comment on "dark": its card, then Resolve.
    assert!(hover_until(&mut harness, |h| h
        .query_by_label("Comment on this text")
        .is_some()));
    harness.get_by_label("Darker?");
    harness.get_by_label("Resolve").click();
    harness.run_steps(8);
    assert!(harness.query_by_label("Comment on this text").is_none());
    assert!(!hover_until(&mut harness, |h| h
        .query_by_label("Comment on this text")
        .is_some()));
}

#[test]
fn a_backup_opened_without_a_library_offers_no_editing() {
    let mut app = ProseApp::new();
    app.open_backup(
        "bundle-plain.prosecraft",
        &fixture("bundle-plain.prosecraft"),
    );
    let mut harness = Harness::builder()
        // Under a loaded CI machine a step can take a couple more frames to settle (egui
        // reports a font-provider repaint); `run` gets that room, nothing else changes.
        .with_max_steps(8)
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
fn a_generated_cover_is_shown_and_exported() {
    let (mut harness, _memory) = app();
    harness.get_by_role_and_label(Role::Tab, "Publish").click();
    harness.run();
    harness.get_by_label("Cover: Generated").scroll_to_me();
    harness.run();
    harness.get_by_label("Cover: Generated").click_accesskit();
    harness.run_steps(12);
    harness.get_by_label("Generated cover preview");
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
    let Some(prosecraft_ui_egui::HostRequest::ExportFile { bytes, .. }) = requests.first() else {
        panic!("an export was asked for: {requests:?}");
    };
    let html = String::from_utf8_lossy(bytes);
    // The Ink palette's colour behind the drawn cover, first in the book.
    let book = html.find("<div class=\"book\">").expect("book");
    assert!(
        html[book..].starts_with(
            "<div class=\"book\">\n<section class=\"cover-page\" style=\"background:#141813\" aria-label=\"Cover\"><img src=\"data:image/png;base64,"
        ),
        "{}",
        &html[book..book + 200]
    );
}

#[test]
fn the_preview_shows_each_part_on_its_pages() {
    let (mut harness, _memory) = app();
    harness.set_size(egui::vec2(1700.0, 1000.0));
    harness.get_by_role_and_label(Role::Tab, "Publish").click();
    harness.run_steps(4);
    // The title page first, on the PDF's first page.
    harness.get_by_label_contains("Paginated preview of Title Page. Section 1 of 3.");
    harness.get_by_label("Page 1");
    assert!(harness.query_by_label("Page 2").is_none());
    harness.get_by_label("Next section").click();
    harness.run_steps(4);
    harness.get_by_label_contains("Paginated preview of Chapter One. Section 2 of 3.");
    harness.get_by_label("Page 2");
    assert!(harness.query_by_label("Page 1").is_none());
    // Straight to the bibliography from the navigator.
    harness
        .get_by_role_and_label(Role::Button, "Bibliography")
        .click();
    harness.run_steps(4);
    harness.get_by_label_contains("Paginated preview of Bibliography. Section 3 of 3.");
    assert!(
        harness
            .get_by_label("Next section")
            .accesskit_node()
            .is_disabled()
    );
    // Formats that aren't paged say so.
    harness
        .get_by_role_and_label(Role::RadioButton, "EPUB (epub)")
        .click();
    harness.run_steps(4);
    harness.get_by_label_contains("Previewing Bibliography.");
}

/// Wiki files on a pretend device: what was copied in, read and deleted.
#[derive(Clone, Default)]
struct PretendWikiFiles(std::rc::Rc<std::cell::RefCell<Vec<String>>>);

impl prosecraft_ui_egui::WikiFileStore for PretendWikiFiles {
    fn import(&self, root: &str, folder: &str, key: &str, source: &str) -> Result<u64, String> {
        if source.ends_with("locked.pdf") {
            return Err("The selected file could not be read.".into());
        }
        self.0
            .borrow_mut()
            .push(format!("import {root}|{folder}|{key}"));
        Ok(2048)
    }
    fn read_markdown(&self, _: &str, _: &str, key: &str) -> Result<String, String> {
        self.0.borrow_mut().push(format!("read {key}"));
        Ok("# Harbour notes\n\nThe *tide* turns at six.".into())
    }
    fn file_path(&self, root: &str, folder: &str, key: &str) -> Result<String, String> {
        Ok(format!("{root}/{folder}/{key}"))
    }
    fn read(&self, _: &str, _: &str, key: &str, _: u64) -> Result<Vec<u8>, String> {
        if key.ends_with(".pdf") {
            // A real PDF: ProseCraft's shortcuts sheet, as the app writes it.
            return prosecraft_core::shortcuts::pdf(false).map_err(|e| e.to_string());
        }
        Ok(b"# Harbour notes\n\nThe *tide* turns at six.".to_vec())
    }
    fn delete(&self, _: &str, _: &str, key: &str) -> Result<(), String> {
        self.0.borrow_mut().push(format!("delete {key}"));
        Ok(())
    }
    fn prepare_folder(&self, root: &str, folder: &str) -> Result<String, String> {
        Ok(format!("{root}/{folder}"))
    }
    fn open(&self, target: &str) -> Result<(), String> {
        self.0.borrow_mut().push(format!("open {target}"));
        Ok(())
    }
    fn reveal(&self, path: &str) -> Result<(), String> {
        self.0.borrow_mut().push(format!("reveal {path}"));
        Ok(())
    }
}

#[test]
fn wiki_links_and_files_are_added_opened_and_removed() {
    let (mut harness, memory) = app();
    let device = PretendWikiFiles::default();
    harness.state_mut().set_wiki_files(Box::new(device.clone()));
    let items = |memory: &Memory| -> Vec<JsValue> {
        project(&memory.last(), "project-1")
            .get("project")
            .and_then(|p| p.get("researchItems"))
            .and_then(JsValue::as_array)
            .map(<[JsValue]>::to_vec)
            .unwrap_or_default()
    };
    let text = |v: &JsValue, k: &str| {
        v.get(k)
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    };
    // A web link: the address made https, titled by its site.
    harness.get_by_label("Add to Wiki").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Add web link")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "Web address")
        .type_text("ftp://nope");
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Add link")
        .click();
    harness.run();
    harness.get_by_label("Enter a valid http or https address.");
    harness
        .get_by_role_and_label(Role::TextInput, "Web address")
        .focus();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_role_and_label(Role::TextInput, "Web address")
        .type_text("www.example.org/harbour");
    harness.run();
    harness
        .get_by_role_and_label(Role::MultilineTextInput, "Description")
        .focus();
    harness.run();
    harness
        .get_by_role_and_label(Role::MultilineTextInput, "Description")
        .type_text("Tide tables");
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Add link")
        .click();
    harness.run_steps(12);
    let link = items(&memory)
        .into_iter()
        .find(|i| text(i, "kind") == "link")
        .expect("a link");
    assert_eq!(text(&link, "url"), "https://www.example.org/harbour");
    assert_eq!(text(&link, "title"), "example.org");
    assert_eq!(text(&link, "description"), "Tide tables");
    harness.get_by_label("Link added.");
    harness.get_by_label("example.org").click();
    harness.run();
    assert!(harness.state_mut().take_requests().contains(
        &prosecraft_ui_egui::HostRequest::OpenUrl("https://www.example.org/harbour".into())
    ));
    // Files: the folder is asked for first, then the files; one can't be read.
    harness.get_by_label("Add to Wiki").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Add file")
        .click();
    harness.run();
    assert_eq!(
        harness.state_mut().take_requests(),
        [prosecraft_ui_egui::HostRequest::ChooseWikiRoot { add_to: Some(None) }]
    );
    harness.state_mut().set_wiki_root("/wiki", Some(None));
    assert_eq!(
        harness.state_mut().take_requests(),
        [prosecraft_ui_egui::HostRequest::PickWikiFiles { folder: None }]
    );
    harness.state_mut().add_wiki_files(
        None,
        &[
            "/home/me/Harbour notes.md".into(),
            "/home/me/locked.pdf".into(),
        ],
    );
    harness.run_steps(12);
    harness.get_by_label("Added 1 file. The selected file could not be read.");
    let file = items(&memory)
        .into_iter()
        .find(|i| text(i, "kind") == "file")
        .expect("a file");
    let key = text(&file, "storageKey");
    assert!(
        key.starts_with("research/") && key.ends_with("/Harbour notes.md"),
        "{key}"
    );
    assert_eq!(text(&file, "mimeType"), "text/markdown");
    assert_eq!(file.get("size").and_then(JsValue::as_number), Some(2048.0));
    let folder = text(
        project(&memory.last(), "project-1")
            .get("project")
            .expect("project"),
        "researchFolderName",
    );
    assert!(folder.ends_with(")") && folder.contains(" ("), "{folder}");
    assert!(
        device
            .0
            .borrow()
            .contains(&format!("import /wiki|{folder}|{key}"))
    );
    // A Markdown file opens in the preview.
    harness.get_by_label("Harbour notes.md").click();
    harness.run();
    let preview = harness.get_by_label("Markdown preview");
    assert!(
        preview
            .accesskit_node()
            .value()
            .is_some_and(|v| v.contains("The tide turns at six.")),
        "{:?}",
        preview.accesskit_node().value()
    );
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.query_by_label("Markdown preview").is_none());
    // Removed, asked first: gone from the Wiki and deleted from the folder.
    harness
        .get_by_role_and_label(Role::Button, "Actions for Harbour notes.md")
        .click();
    harness.run();
    harness.get_by_label("Remove and delete file").click();
    harness.run();
    harness.get_by_label_contains("The file will be deleted from the project folder.");
    harness
        .get_by_role_and_label(Role::Button, "Remove")
        .click();
    harness.run_steps(12);
    assert!(items(&memory).iter().all(|i| text(i, "kind") != "file"));
    assert!(device.0.borrow().contains(&format!("delete {key}")));
}

#[test]
fn proofreading_underlines_and_fixes_a_misspelling() {
    let (mut harness, memory) = app();
    // Settings → Proofreading: on.
    harness
        .get_by_role_and_label(Role::Button, "Settings…")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Tab, "Proofreading")
        .click();
    harness.run();
    harness.get_by_label("Check spelling and grammar").click();
    harness.run();
    assert!(harness.get_all_by_label("English dialect").count() >= 1);
    harness.key_press(egui::Key::Escape);
    harness.run();
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    let body = harness.get_by_label("Scene text");
    body.focus();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_label("Scene text")
        .type_text("Teh lamp went out.");
    harness.run();
    // The pause after typing, then the check on its thread.
    for _ in 0..400 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    harness.run();
    // Settings says what was found.
    harness
        .get_by_role_and_label(Role::Button, "Settings…")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Tab, "Proofreading")
        .click();
    harness.run();
    assert!(harness.get_all_by_role(Role::Status).any(
        |n| n.accesskit_node().value().as_deref() == Some("1 suggestion in the current scene.")
    ));
    harness.key_press(egui::Key::Escape);
    harness.run();
    // A click on the misspelled word opens its suggestions.
    let text = harness.get_by_label("Scene text").rect();
    let at = text.left_top() + egui::vec2(8.0, 12.0);
    harness.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.step();
    }
    harness.run();
    harness.get_by_label("Proofreading suggestion");
    harness.get_by_label_contains("Did you mean");
    harness.get_by_role_and_label(Role::Button, "The").click();
    harness.run_steps(12);
    assert!(harness.query_by_label("Proofreading suggestion").is_none());
    let saved = memory.last();
    let scene = document(project(&saved, "project-1"), "doc-1").expect("scene");
    assert_eq!(
        js::stringify(scene.get("content").expect("content")),
        r#""<p>The lamp went out.</p>""#
    );
}

#[test]
fn equations_and_calculations_are_inserted_from_the_toolbar() {
    let (mut harness, memory) = app();
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    let body = harness.get_by_label("Scene text");
    body.focus();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_label("Scene text").type_text("Energy");
    harness.run();
    // An empty calculation is refused.
    harness.get_by_label("Insert calculation").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Insert")
        .click();
    harness.run();
    harness.get_by_label("Add a calculation first.");
    harness
        .get_by_role_and_label(Role::Button, "Cancel")
        .click();
    harness.run();
    // A block equation after the line.
    harness.get_by_label("Insert equation").click();
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Block").click();
    harness
        .get_by_role_and_label(Role::MultilineTextInput, "LaTeX")
        .focus();
    harness.run();
    harness
        .get_by_role_and_label(Role::MultilineTextInput, "LaTeX")
        .type_text("E = mc^2");
    harness.run();
    harness.get_by_label("Preview");
    harness
        .get_by_role_and_label(Role::Button, "Insert")
        .click();
    harness.run_steps(12);
    assert!(harness.query_by_label("Insert equation").is_some());
    assert!(
        harness
            .query_by_role_and_label(Role::MultilineTextInput, "LaTeX")
            .is_none()
    );
    let saved = memory.last();
    let scene = document(project(&saved, "project-1"), "doc-1").expect("scene");
    assert_eq!(
        js::stringify(scene.get("content").expect("content")),
        r#""<p>Energy</p><div class=\"math-block\" data-tex=\"E = mc^2\"></div><p><br></p>""#
    );
}

#[test]
fn a_pane_edits_its_document_as_prosecrafts_tile_editor_does() {
    let (mut harness, memory) = app();
    harness.set_size(egui::vec2(1700.0, 1000.0));
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    harness.get_by_label("Open split view").click();
    harness.run();
    // The Wiki page Mara, edited beside the scene.
    harness.get_by_label("Show in this pane").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "  Mara")
        .click();
    harness.run();
    let title = harness.get_by_role_and_label(Role::TextInput, "Title");
    title.focus();
    harness.run();
    harness.key_press(egui::Key::End);
    harness
        .get_by_role_and_label(Role::TextInput, "Title")
        .type_text(" Vale");
    harness.run_steps(6);
    let text = harness.get_by_label("Document text");
    text.focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::End);
    harness.get_by_label("Document text").type_text(" More.");
    // Sent once typing pauses.
    harness.step();
    std::thread::sleep(std::time::Duration::from_millis(350));
    harness.run_steps(12);
    let saved = memory.last();
    let wiki = document(project(&saved, "project-1"), "doc-2").expect("the wiki page");
    let field = |name: &str| {
        wiki.get(name)
            .and_then(JsValue::as_js_string)
            .map(|c| c.to_string_lossy())
            .unwrap_or_default()
    };
    assert_eq!(field("title"), "Mara Vale");
    assert_eq!(field("content"), "<p>Notes about Mara. 𝔐𝔞𝔯𝔞 More.</p>");
}

#[test]
fn a_scene_pane_writes_its_chapter_back_and_leaves_text_it_cannot_hold_alone() {
    // Mara's page as bare text, which the editor can't hold.
    let bundle = fixture("bundle-plain.prosecraft")
        .replace("<p>Notes about Mara. 𝔐𝔞𝔯𝔞</p>", "Loose notes about Mara");
    let memory = Memory::default();
    let mut app =
        ProseApp::with_library(ProjectLibrary::new("test".into()), Box::new(memory.clone()));
    app.open_backup("bundle-plain.prosecraft", &bundle);
    let mut harness = Harness::builder()
        .with_max_steps(8)
        .with_size([1700.0, 1000.0])
        .build_ui_state(|ui, app: &mut ProseApp| app.show(ui), app);
    prosecraft_ui_egui::setup_for_tests(&harness.ctx);
    harness.run();
    let stored = |memory: &Memory, id: &str| -> String {
        let saved = memory.last();
        document(project(&saved, "project-1"), id)
            .and_then(|d| d.get("content"))
            .and_then(JsValue::as_js_string)
            .map(|c| c.to_string_lossy())
            .unwrap_or_default()
    };
    harness
        .get_by_role_and_label(Role::TreeItem, "Mara")
        .click();
    harness.run();
    harness.get_by_label("Open split view").click();
    harness.run();
    // The other scene's pane: the whole chapter, both scenes, editable.
    harness.get_by_label("Show in this pane").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "  INT. LIGHTHOUSE - NIGHT")
        .click();
    harness.run();
    harness.run_steps(12);
    let opening = stored(&memory, "doc-1");
    // Each scene its own editor: the second one's.
    assert_eq!(harness.get_all_by_label("Document text").count(), 2);
    harness
        .get_all_by_label("Document text")
        .last()
        .expect("the second scene")
        .focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::End);
    harness
        .get_all_by_label("Document text")
        .last()
        .expect("the second scene")
        .type_text(" LATER");
    harness.step();
    std::thread::sleep(std::time::Duration::from_millis(350));
    harness.run_steps(12);
    assert_eq!(
        stored(&memory, "doc-1"),
        opening,
        "the first scene is as it was"
    );
    assert!(
        stored(&memory, "doc-3").contains("LATER"),
        "{}",
        stored(&memory, "doc-3")
    );
    // Mara, beside a scene: shown, not edited.
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    harness.get_by_label("Show in this pane").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "  Mara")
        .click();
    harness.run();
    assert!(harness.query_by_label("Document text").is_none());
    harness.get_by_label("Open in the editor to change this text.");
    assert_eq!(stored(&memory, "doc-2"), "Loose notes about Mara");
}

#[test]
fn the_split_view_shows_scenes_and_wiki_files_beside_the_editor() {
    let (mut harness, _memory) = app();
    harness.set_size(egui::vec2(1700.0, 1000.0));
    harness
        .state_mut()
        .set_wiki_files(Box::new(PretendWikiFiles::default()));
    harness.state_mut().set_wiki_root("/wiki", None);
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    harness.get_by_label("Open split view").click();
    harness.run();
    harness.get_by_label_contains("Pick a scene, wiki page or file above");
    // Another scene, beside the editor.
    harness.get_by_label("Show in this pane").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "  INT. LIGHTHOUSE - NIGHT")
        .click();
    harness.run();
    // Not the open scene: editable, as ProseCraft's tile editor.
    assert_eq!(harness.get_all_by_label("Document text").count(), 1);
    // Swapped: the editor has that scene, the pane the one that was open.
    harness
        .get_by_label("Edit this in the main editor and show the current text here")
        .click();
    harness.run_steps(6);
    assert!(harness.query_by_label("Scene text").is_some());
    // Another pane, then closed again.
    harness.get_by_label("Add another pane").click();
    harness.run();
    assert_eq!(harness.get_all_by_label("Close this pane").count(), 2);
    harness
        .get_all_by_label("Close this pane")
        .last()
        .expect("pane")
        .click_accesskit();
    harness.run();
    assert_eq!(harness.get_all_by_label("Close this pane").count(), 1);
    // A wiki file beside: a PDF's pages.
    harness.state_mut().add_wiki_files(
        None,
        &[
            "/home/me/Map.pdf".into(),
            "/home/me/Harbour notes.md".into(),
        ],
    );
    harness.run_steps(6);
    harness
        .get_by_role_and_label(Role::Button, "Actions for Map.pdf")
        .click();
    harness.run();
    harness.get_by_label("Open beside").click();
    harness.run_steps(6);
    harness.get_by_label("Page 1");
    harness.get_by_label("Close split view").click();
    harness.run();
    assert!(harness.query_by_label("Close this pane").is_none());
    // Alt+Click on a scene opens it beside, as ProseCraft's shortcut sheet says.
    harness
        .get_by_role_and_label(Role::TreeItem, "INT. LIGHTHOUSE - NIGHT")
        .click_modifiers(egui::Modifiers::ALT);
    harness.run_steps(4);
    assert_eq!(harness.get_all_by_label("Close this pane").count(), 1);
    assert_eq!(harness.get_all_by_label("Pane text").count(), 1);
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
fn docx_exports_from_the_publish_view() {
    let (mut harness, _memory) = app();
    harness.get_by_role_and_label(Role::Tab, "Publish").click();
    harness.run();
    for (role, name) in [
        (Role::RadioButton, "DOCX (docx)"),
        (Role::Button, "Export DOCX"),
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
        kind,
        extension,
    }) = requests.first()
    else {
        panic!("an export was asked for: {requests:?}");
    };
    assert!(
        file_name.ends_with(".docx") && extension == "docx" && kind == "DOCX",
        "{file_name}"
    );
    // A ZIP package holding the Word document.
    let holds = |name: &[u8]| bytes.windows(name.len()).any(|w| w == name);
    assert!(bytes.starts_with(b"PK\x03\x04"));
    assert!(holds(b"word/document.xml") && holds(b"word/footnotes.xml"));
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
    // A fixed number of frames: under a loaded test run this can take more than `run`'s four
    // to settle.
    harness.run_steps(8);
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

/// Settings → Writing → "Show Timeline view" (off by default, as in ProseCraft).
fn enable_timeline(harness: &mut Harness<'static, ProseApp>) {
    assert!(harness.query_by_label("Timeline").is_none());
    harness
        .get_by_role_and_label(Role::Button, "Settings…")
        .click();
    harness.run();
    harness.get_by_label("Writing").click();
    harness.run();
    harness.get_by_label("Show Timeline view").click();
    harness.run();
    harness.key_press(egui::Key::Escape);
    harness.run();
}

#[test]
fn zen_mode_hides_the_interface_while_typing_and_focus_mode_hides_it_until_left() {
    let (mut harness, _) = app();
    harness
        .get_by_role_and_label(Role::Button, "Settings…")
        .click();
    harness.run();
    harness.get_by_label("Writing").click();
    harness.run();
    harness
        .get_by_label("Hide the interface while typing (Zen mode)")
        .click();
    harness.run();
    harness.key_press(egui::Key::Escape);
    harness.run();
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    harness.get_by_label("Scene text").focus();
    harness.run();
    harness.get_by_label("Scene text").type_text(" More.");
    harness.run();
    // The binder, the side panel and the view tabs are gone; the text stays.
    assert!(harness.query_by_label("Opening Scene").is_none());
    assert!(harness.query_by_label("Write").is_none());
    harness.get_by_label("Scene text");
    harness.key_press(egui::Key::Escape);
    harness.run();
    harness.get_by_label("Opening Scene");
    // Focus mode: Ctrl+Shift+Enter hides it all at once, with a hint, until left.
    harness.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::Enter,
    );
    harness.run();
    assert!(harness.query_by_label("Opening Scene").is_none());
    harness.get_by_label_contains("Focus mode · Ctrl+Shift+Enter or Escape to leave");
    harness.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::Enter,
    );
    harness.run();
    harness.get_by_label("Opening Scene");
}

#[test]
fn timeline_moments_are_added_edited_moved_and_removed() {
    let (mut harness, memory) = app();
    enable_timeline(&mut harness);
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

    // `startStoryboardPan`: a drag on the background pans with the Pan tool, not with Select.
    let handle_name = "Move Opening. Use arrow keys to nudge the card, or drag to position it.";
    let on_screen = |harness: &Harness<'static, ProseApp>| {
        harness
            .get_by_role_and_label(Role::Button, handle_name)
            .rect()
            .center()
    };
    let canvas = harness.get_by_label("Freeform storyboard").rect();
    let empty = canvas.left_bottom() + egui::vec2(30.0, -30.0);
    let pan = |harness: &mut Harness<'static, ProseApp>| {
        harness.event(egui::Event::PointerMoved(empty));
        harness.event(egui::Event::PointerButton {
            pos: empty,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run();
        for step in 1..=4 {
            harness.event(egui::Event::PointerMoved(
                empty + egui::vec2(10.0, -5.0) * step as f32,
            ));
            harness.run();
        }
        harness.event(egui::Event::PointerButton {
            pos: empty + egui::vec2(40.0, -20.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(6);
    };
    let still = on_screen(&harness);
    pan(&mut harness);
    assert_eq!(on_screen(&harness), still, "Select doesn't pan");
    harness
        .get_by_role_and_label(Role::Button, "Pan tool")
        .click();
    harness.run();
    pan(&mut harness);
    let moved = on_screen(&harness) - still;
    assert!(
        (moved.x - 40.0).abs() < 1.0 && (moved.y + 20.0).abs() < 1.0,
        "{moved:?}"
    );
}

#[test]
fn storyboard_drawings_are_drawn_moved_labelled_and_deleted() {
    let (mut harness, memory) = app();
    let drawings = |memory: &Memory| -> Vec<JsValue> {
        project(&memory.last(), "project-1")
            .get("project")
            .and_then(|p| p.get("storyboardDrawings"))
            .and_then(JsValue::as_array)
            .map(<[JsValue]>::to_vec)
            .unwrap_or_default()
    };
    let field = |d: &JsValue, k: &str| match d.get(k) {
        Some(JsValue::Number(n)) => n.to_string(),
        Some(JsValue::String(s)) => s.to_string_lossy(),
        _ => String::new(),
    };
    let drag = |harness: &mut Harness<'static, ProseApp>, from: egui::Pos2, to: egui::Pos2| {
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
    };
    harness
        .get_by_role_and_label(Role::Tab, "Storyboard")
        .click();
    harness.run();
    let canvas = harness.get_by_label("Freeform storyboard").rect();
    let origin = canvas.max - egui::vec2(400.0, 300.0);

    // A box, dragged out from any corner.
    harness
        .get_by_role_and_label(Role::Button, "Box tool")
        .click();
    harness.run();
    drag(&mut harness, origin + egui::vec2(120.0, 90.0), origin);
    let boxed = drawings(&memory)
        .into_iter()
        .find(|d| field(d, "type") == "box")
        .expect("a box");
    assert_eq!(
        (
            field(&boxed, "type"),
            field(&boxed, "width"),
            field(&boxed, "height")
        ),
        ("box".into(), "120".into(), "90".into())
    );

    // An arrow; then, with Select, nudge it and delete it from the keyboard.
    harness
        .get_by_role_and_label(Role::Button, "Arrow tool")
        .click();
    harness.run();
    drag(
        &mut harness,
        origin + egui::vec2(200.0, 200.0),
        origin + egui::vec2(300.0, 200.0),
    );
    let arrow = drawings(&memory)
        .into_iter()
        .rfind(|d| field(d, "type") == "arrow")
        .expect("an arrow");
    let length = field(&arrow, "endX").parse::<f64>().expect("x")
        - field(&arrow, "startX").parse::<f64>().expect("x");
    assert_eq!(length, 100.0);
    harness
        .get_by_role_and_label(Role::Button, "Select tool")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Select arrow diagram element")
        .focus();
    harness.run();
    harness.key_press(egui::Key::ArrowDown);
    harness.run_steps(12);
    let nudged = drawings(&memory)
        .into_iter()
        .find(|d| field(d, "type") == "arrow")
        .expect("an arrow");
    assert_eq!(
        field(&nudged, "startY").parse::<f64>().expect("y")
            - field(&arrow, "startY").parse::<f64>().expect("y"),
        12.0
    );
    harness.key_press(egui::Key::Delete);
    harness.run_steps(12);
    assert!(
        !drawings(&memory)
            .iter()
            .any(|d| field(d, "type") == "arrow"),
        "the arrow is deleted"
    );

    // A text label: placed by a click, typed into, kept; an empty one goes.
    harness
        .get_by_role_and_label(Role::Button, "Text tool")
        .click();
    harness.run();
    harness.get_by_label("Freeform storyboard").rect();
    let at = origin + egui::vec2(20.0, 150.0);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(12);
    assert!(
        harness
            .get_by_role_and_label(Role::MultilineTextInput, "Diagram text")
            .is_focused(),
        "the new label is being edited"
    );
    harness
        .get_by_role_and_label(Role::MultilineTextInput, "Diagram text")
        .type_text("Act I");
    harness.run();
    harness.key_press(egui::Key::Escape);
    harness.run_steps(12);
    let text = drawings(&memory)
        .into_iter()
        .find(|d| field(d, "type") == "text")
        .expect("a label");
    assert_eq!(
        (
            field(&text, "text"),
            field(&text, "fontSize"),
            field(&text, "width")
        ),
        ("Act I".into(), "14".into(), "180".into())
    );

    let at = origin + egui::vec2(220.0, 20.0);
    harness.event(egui::Event::PointerMoved(at));
    harness.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(12);
    assert_eq!(
        drawings(&memory)
            .iter()
            .filter(|d| field(d, "type") == "text")
            .count(),
        2
    );
    harness.key_press(egui::Key::Escape);
    harness.run_steps(12);
    assert_eq!(
        drawings(&memory)
            .iter()
            .filter(|d| field(d, "type") == "text")
            .count(),
        1,
        "the empty label is removed"
    );
    // With Select, dragging a box moves it (it doesn't pan the canvas).
    harness
        .get_by_role_and_label(Role::Button, "Select tool")
        .click();
    harness.run();
    let before = drawings(&memory)
        .into_iter()
        .find(|d| field(d, "type") == "box")
        .expect("box");
    drag(
        &mut harness,
        origin + egui::vec2(60.0, 45.0),
        origin + egui::vec2(90.0, 55.0),
    );
    let after = drawings(&memory)
        .into_iter()
        .find(|d| field(d, "type") == "box")
        .expect("box");
    let num = |d: &JsValue, k: &str| field(d, k).parse::<f64>().expect("number");
    assert_eq!(
        (
            num(&after, "x") - num(&before, "x"),
            num(&after, "y") - num(&before, "y")
        ),
        (30.0, 10.0)
    );
}

#[test]
fn footnotes_are_inserted_written_removed_and_cleaned_up() {
    let (mut harness, memory) = app();
    let doc = |memory: &Memory| -> JsValue {
        project(&memory.last(), "project-1")
            .get("project")
            .and_then(|p| p.get("documents"))
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .find(|d| {
                d.get("id")
                    .and_then(JsValue::as_js_string)
                    .is_some_and(|i| i.eq_str("doc-1"))
            })
            .cloned()
            .expect("doc-1")
    };
    let content = |d: &JsValue| {
        d.get("content")
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    };
    let defs = |d: &JsValue| -> Vec<(String, String)> {
        d.get("footnotes")
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .map(|f| {
                let s = |k: &str| {
                    f.get(k)
                        .and_then(JsValue::as_js_string)
                        .map(|v| v.to_string_lossy())
                        .unwrap_or_default()
                };
                (s("id"), s("html"))
            })
            .collect()
    };
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Tab, "Footnotes")
        .click();
    harness.run();
    // Insert footnote at the caret (the end): its editor opens as footnote 2.
    harness.get_by_label("Scene text").focus();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Insert footnote")
        .click();
    harness.run_steps(4);
    assert!(
        harness.query_by_label("Footnote 2").is_some(),
        "the editor is open on footnote 2"
    );
    assert!(harness.get_by_label("Footnote text").is_focused());
    harness
        .get_by_label("Footnote text")
        .type_text("Second note.");
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Done").click();
    harness.run_steps(12);
    let after = doc(&memory);
    let notes = defs(&after);
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert_eq!(notes[1].1, "Second note.");
    let new_id = notes[1].0.clone();
    assert!(new_id.starts_with("fn-") && new_id.len() == 9, "{new_id}");
    assert!(content(&after).ends_with(&format!(r#".<sup class="footnote-ref" data-footnote-id="{new_id}" contenteditable="false"></sup></p>"#)), "{}", content(&after));

    // Remove footnote 1 from the Footnotes tab: marker and definition go.
    harness
        .get_by_role_and_label(Role::Button, "Remove footnote 1")
        .click();
    harness.run_steps(12);
    let after = doc(&memory);
    assert!(!content(&after).contains("fn-1"), "{}", content(&after));
    assert_eq!(
        defs(&after).iter().map(|d| d.0.clone()).collect::<Vec<_>>(),
        std::slice::from_ref(&new_id)
    );

    // Deleting the marker in the text leaves an unused definition; Clean up drops it.
    let at = content(&after);
    assert!(at.contains(&new_id));
    harness.get_by_label("Scene text").focus();
    harness.run();
    // (The caret is where the removed footnote was: go to the end of the text.)
    harness.key_press(egui::Key::ArrowDown);
    harness.key_press(egui::Key::End);
    harness.key_press(egui::Key::Backspace);
    harness.run_steps(12);
    harness
        .get_by_role_and_label(Role::Button, "Clean up 1 orphan")
        .click();
    harness.run_steps(12);
    let after = doc(&memory);
    assert!(
        !content(&after).contains("footnote-ref"),
        "{}",
        content(&after)
    );
    assert!(after.get("footnotes").is_none(), "no definitions are left");
}

#[test]
fn a_footnote_keeps_its_formatting_when_edited() {
    let (mut harness, memory) = app();
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Tab, "Footnotes")
        .click();
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Edit").click();
    harness.run_steps(3);
    assert!(harness.query_by_label("Footnote 1").is_some());
    let text = harness.get_by_label("Footnote text");
    assert_eq!(
        text.accesskit_node().value().as_deref(),
        Some("A note with emphasis.")
    );
    text.type_text(" More.");
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Done").click();
    harness.run_steps(12);
    let saved = memory.last();
    let html = project(&saved, "project-1")
        .get("project")
        .and_then(|p| p.get("documents"))
        .and_then(JsValue::as_array)
        .and_then(|d| d.first().cloned())
        .and_then(|d| {
            d.get("footnotes")
                .and_then(JsValue::as_array)
                .and_then(|f| f.first().cloned())
        })
        .and_then(|f| {
            f.get("html")
                .and_then(JsValue::as_js_string)
                .map(|s| s.to_string_lossy())
        });
    assert_eq!(
        html.as_deref(),
        Some("A note with <em>emphasis</em>. More.")
    );
}

#[test]
fn a_fountain_screenplay_joins_the_open_chapter() {
    let (mut harness, memory) = app();
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    harness.state_mut().import_fountain(
        "pilot.fountain",
        "Title: The Pilot\n\nINT. LIGHTHOUSE - NIGHT\n\nMARA\n(quietly)\nIs **anyone** there?\n",
    );
    harness.run_steps(12);
    harness.get_by_label_contains("Imported “The Pilot” as a screenplay scene.");
    let saved = memory.last();
    let docs = project(&saved, "project-1")
        .get("project")
        .and_then(|p| p.get("documents"))
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .to_vec();
    let scene = docs.last().cloned().expect("the new scene");
    let s = |k: &str| {
        scene
            .get(k)
            .and_then(JsValue::as_js_string)
            .map(|v| v.to_string_lossy())
            .unwrap_or_default()
    };
    assert_eq!(
        (
            s("title").as_str(),
            s("chapterId").as_str(),
            s("format").as_str()
        ),
        ("The Pilot", "chapter-1", "screenplay")
    );
    assert_eq!(
        s("content"),
        r#"<p data-sp="scene-heading">INT. LIGHTHOUSE - NIGHT</p><p data-sp="character">MARA</p><p data-sp="parenthetical">(quietly)</p><p data-sp="dialogue">Is <strong>anyone</strong> there?</p>"#
    );
    // Without a title page, the file name names it.
    harness
        .state_mut()
        .import_fountain("Second Act.txt", "EXT. SHORE - DAY\n\nWaves.\n");
    harness.run_steps(12);
    harness.get_by_label_contains("Imported “Second Act” as a screenplay scene.");
}

#[test]
fn a_screenplay_is_written_element_by_element() {
    let (mut harness, memory) = app();
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    harness.state_mut().import_fountain(
        "pilot.fountain",
        "INT. LIGHTHOUSE - NIGHT\n\nMARA\nIs anyone there?\n",
    );
    harness.run_steps(4);
    let content = |memory: &Memory| -> String {
        let saved = memory.last();
        let docs = project(&saved, "project-1")
            .get("project")
            .and_then(|p| p.get("documents"))
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .to_vec();
        docs.last()
            .and_then(|d| d.get("content"))
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    };
    let element = |harness: &Harness<'static, ProseApp>| {
        harness
            .get_by_label("Element type")
            .accesskit_node()
            .value()
            .map(|v| v.to_string())
    };
    // The caret at the end: on the dialogue.
    harness.get_by_label("Scene text").focus();
    harness.run();
    harness.run();
    assert_eq!(element(&harness).as_deref(), Some("Dialogue"));
    // Enter after dialogue: a character cue, upper-cased as it is typed; Enter: dialogue.
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(element(&harness).as_deref(), Some("Character"));
    harness.get_by_label("Scene text").type_text("bob");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(element(&harness).as_deref(), Some("Dialogue"));
    harness.get_by_label("Scene text").type_text("Hi.");
    harness.run();
    // Enter, Enter: character, then (empty) action; Tab on empty action: character again.
    harness.key_press(egui::Key::Enter);
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(element(&harness).as_deref(), Some("Action"));
    harness.key_press(egui::Key::Tab);
    harness.run();
    assert_eq!(element(&harness).as_deref(), Some("Character"));
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Tab);
    harness.run();
    assert_eq!(element(&harness).as_deref(), Some("Action"));
    // A typed scene heading is recognized and upper-cased.
    harness
        .get_by_label("Scene text")
        .type_text("ext. shore - day");
    harness.run_steps(12);
    assert_eq!(element(&harness).as_deref(), Some("Scene heading"));
    assert_eq!(
        content(&memory),
        r#"<p data-sp="scene-heading">INT. LIGHTHOUSE - NIGHT</p><p data-sp="character">MARA</p><p data-sp="dialogue">Is anyone there?</p><p data-sp="character">BOB</p><p data-sp="dialogue">Hi.</p><p data-sp="scene-heading">EXT. SHORE - DAY</p>"#
    );
    // The Screenplay bar changes the element under the caret.
    harness.get_by_label("Element type").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Transition")
        .click();
    harness.run_steps(12);
    assert!(
        content(&memory).ends_with(r#"<p data-sp="transition">EXT. SHORE - DAY</p>"#),
        "{}",
        content(&memory)
    );
}

#[test]
fn fountain_exports_from_the_publish_view_of_a_project_with_a_screenplay() {
    let (mut harness, _memory) = app();
    harness.get_by_role_and_label(Role::Tab, "Publish").click();
    harness.run();
    for (role, name) in [
        (Role::RadioButton, "Fountain (fountain)"),
        (Role::Button, "Export Fountain"),
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
    assert!(file_name.ends_with(".fountain"), "{file_name}");
    let fountain = String::from_utf8_lossy(bytes);
    assert!(fountain.starts_with("Title: The Lighthouse"), "{fountain}");
    assert!(
        fountain.contains("\n\nINT. LIGHTHOUSE - NIGHT\n"),
        "{fountain}"
    );
}

#[test]
fn a_prose_scene_of_a_screenplay_project_converts_with_a_snapshot() {
    let (mut harness, memory) = app();
    // Make the project a screenplay project.
    harness
        .get_by_role_and_label(Role::Button, "Project settings")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::ComboBox, "Project type")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Screenplay")
        .click();
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Save").click();
    harness.run_steps(4);
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Convert this scene to screenplay")
        .click();
    harness.run();
    harness.get_by_label_contains("Convert “Opening Scene” to screenplay format? A snapshot of the prose version will be saved first.");
    harness
        .get_by_role_and_label(Role::Button, "Convert")
        .click();
    harness.run_steps(12);
    let saved = memory.last();
    let data = project(&saved, "project-1")
        .get("project")
        .cloned()
        .expect("data");
    let doc = data
        .get("documents")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .find(|d| {
            d.get("id")
                .and_then(JsValue::as_js_string)
                .is_some_and(|i| i.eq_str("doc-1"))
        })
        .cloned()
        .expect("doc-1");
    let s = |v: &JsValue, k: &str| {
        v.get(k)
            .and_then(JsValue::as_js_string)
            .map(|t| t.to_string_lossy())
            .unwrap_or_default()
    };
    assert_eq!(s(&doc, "format"), "screenplay");
    assert!(s(&doc, "content").starts_with(r#"<p data-sp="scene-heading">INT./EXT. LOCATION - DAY</p><p data-sp="action">It was <em>dark</em>.</p>"#), "{}", s(&doc, "content"));
    let snapshot = data
        .get("snapshots")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .last()
        .cloned()
        .expect("a snapshot");
    assert_eq!(
        (
            s(&snapshot, "reason").as_str(),
            s(&snapshot, "note").as_str()
        ),
        ("before-restore", "Before screenplay conversion")
    );
    assert!(
        s(&snapshot, "content").contains("It was <em>dark</em>"),
        "the prose is kept"
    );
    // Now the scene is written in the screenplay editor.
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Convert this scene to screenplay")
            .is_none()
    );
    harness.get_by_label("Element type");
}

#[test]
fn a_scene_is_written_in_markdown_and_back_in_rich_text() {
    let (mut harness, memory) = app();
    harness
        .get_all_by_role_and_label(Role::Button, "Add chapter")
        .next()
        .expect("an add button")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::TreeItem, "Scene 1")
        .click();
    harness.run();
    // No Markdown editor until it is enabled in Settings → Writing.
    assert!(harness.query_by_label("Markdown editor").is_none());
    harness
        .get_by_role_and_label(Role::Button, "Settings…")
        .click();
    harness.run();
    harness.get_by_label("Writing").click();
    harness.run();
    harness.get_by_label("Enable Markdown editor").click();
    harness.run();
    harness.key_press(egui::Key::Escape);
    harness.run();

    harness.get_by_label("Markdown editor").click();
    harness.run_steps(2);
    let source = harness.get_by_label("Markdown source");
    source.focus();
    source.type_text("# Lighthouse");
    harness.key_press(egui::Key::Enter);
    harness.key_press(egui::Key::Enter);
    harness
        .get_by_label("Markdown source")
        .type_text("The *lamp* went out[^n].");
    harness.key_press(egui::Key::Enter);
    harness.key_press(egui::Key::Enter);
    harness
        .get_by_label("Markdown source")
        .type_text("[^n]: At midnight.");
    harness.run_steps(12);

    let saved = memory.last();
    let scene = document(project(&saved, "project-1"), "new-1").expect("saved scene");
    let text = |key: &str| scene.get(key).map(js::stringify).unwrap_or_default();
    assert_eq!(
        text("content"),
        r#""<h1>Lighthouse</h1>\n<p>The <em>lamp</em> went out<sup class=\"footnote-ref\" data-footnote-id=\"n\" contenteditable=\"false\"></sup>.</p>\n""#
    );
    assert_eq!(
        text("markdownSource"),
        r##""# Lighthouse\n\nThe *lamp* went out[^n].\n\n[^n]: At midnight.""##
    );
    assert_eq!(text("footnotes"), r#"[{"id":"n","html":"At midnight."}]"#);
    harness.get_by_label("5 words");

    // Back in rich text, the scene shows what the Markdown made; a rich edit drops the source.
    harness
        .get_by_role_and_label(Role::Button, "Rich text editor")
        .click();
    harness.run_steps(2);
    let body = harness.get_by_label("Scene text");
    assert!(
        body.accesskit_node()
            .value()
            .is_some_and(|v| v.starts_with("Lighthouse\nThe lamp went out"))
    );
    body.focus();
    harness.key_press(egui::Key::End);
    harness.get_by_label("Scene text").type_text("!");
    harness.run_steps(12);
    let saved = memory.last();
    let scene = document(project(&saved, "project-1"), "new-1").expect("saved scene");
    assert!(scene.get("markdownSource").is_none());
}

#[test]
fn the_citation_style_and_bibliography_are_chosen_in_publish() {
    let (mut harness, memory) = app();
    harness.get_by_role_and_label(Role::Tab, "Publish").click();
    harness.run();
    // ProseCraft's Citations section: the project's one source counted.
    harness.get_by_label("1 SOURCE");
    harness
        .get_by_role_and_label(Role::ComboBox, "Citation style")
        .scroll_to_me();
    harness.run();
    harness
        .get_by_role_and_label(Role::ComboBox, "Citation style")
        .click();
    harness.run();
    harness.get_by_role_and_label(Role::Button, "IEEE").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::CheckBox, "Include bibliography")
        .scroll_to_me();
    harness.run();
    harness
        .get_by_role_and_label(Role::CheckBox, "Include bibliography")
        .click();
    harness.run_steps(12);
    let saved = memory.last();
    let options = project(&saved, "project-1")
        .get("project")
        .and_then(|p| p.get("publishOptions"))
        .expect("publishOptions were saved");
    assert_eq!(options.get("citationStyle"), Some(&JsValue::str("ieee")));
    assert_eq!(
        options.get("includeBibliography"),
        Some(&JsValue::Bool(false))
    );
}

/// Steps until a label containing `text` shows (an answer from another thread arriving).
fn wait_for(harness: &mut Harness<'static, ProseApp>, text: &str) {
    for _ in 0..300 {
        harness.step();
        if harness.query_by_label_contains(text).is_some() {
            harness.run();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("{text:?} never showed");
}

#[test]
fn sources_come_from_zotero() {
    let (mut harness, memory) = app();
    let answer = r#"[{"id":"http://zotero.org/users/local/0/items/K8M9P2Q1","type":"book","title":"Strukturwandel der Öffentlichkeit","author":[{"family":"Habermas","given":"Jürgen"}],"issued":{"date-parts":[[1962]]}},{"id":"http://zotero.org/users/local/0/items/ATT00001","itemType":"attachment","title":"Full Text PDF"}]"#;
    let asked = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
    let log = std::sync::Arc::clone(&asked);
    harness
        .state_mut()
        .set_zotero(std::sync::Arc::new(move |path: &str| {
            log.lock().expect("log").push(path.to_owned());
            match path {
                "/connector/ping" => Ok("Zotero is running".to_owned()),
                "/api/users/0/items?q=habermas&format=csljson&limit=25" => Ok(answer.to_owned()),
                _ => Err("HTTP 404".to_owned()),
            }
        }));
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Sources")
        .click();
    wait_for(&mut harness, "Connected");
    harness
        .get_by_role_and_label(Role::TextInput, "Search Zotero")
        .focus();
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "Search Zotero")
        .type_text(" habermas ");
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Search")
        .click();
    wait_for(&mut harness, "Strukturwandel");
    // The attachment isn't a source.
    assert!(harness.query_by_label_contains("Full Text PDF").is_none());
    harness.get_by_label_contains("1962 · @K8M9P2Q1");
    harness
        .get_by_role_and_label(Role::Button, "Add to this project")
        .click();
    harness.run();
    harness.get_by_label("Added “Strukturwandel der Öffentlichkeit” to this project.");
    harness.get_by_label("In project");
    // No Better BibTeX: its picker isn't offered again.
    harness
        .get_by_role_and_label(Role::Button, "Pick in Zotero…")
        .click();
    for _ in 0..100 {
        harness.step();
        if harness.query_by_label("Pick in Zotero…").is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(harness.query_by_label("Pick in Zotero…").is_none());
    harness.run_steps(12);
    let saved = memory.last();
    let citations = project(&saved, "project-1")
        .get("project")
        .and_then(|p| p.get("citations"))
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .to_vec();
    let added = citations
        .iter()
        .find(|c| {
            c.get("id")
                .and_then(JsValue::as_js_string)
                .is_some_and(|i| i.eq_str("K8M9P2Q1"))
        })
        .expect("the Zotero source was saved");
    assert_eq!(
        added
            .get("provider")
            .and_then(JsValue::as_js_string)
            .map(|p| p.to_string_lossy()),
        Some("zotero".to_owned())
    );
    assert_eq!(
        *asked.lock().expect("log"),
        [
            "/connector/ping",
            "/api/users/0/items?q=habermas&format=csljson&limit=25",
            "/better-bibtex/cayw?format=json"
        ]
    );
}

#[test]
fn zotero_not_running_says_how_to_reach_it() {
    let (mut harness, _) = app();
    harness
        .state_mut()
        .set_zotero(std::sync::Arc::new(|_: &str| {
            Err("Zotero is not running (connection refused on 127.0.0.1:23119).".to_owned())
        }));
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Sources")
        .click();
    wait_for(&mut harness, "Not reachable");
    harness.get_by_label_contains("Zotero is not reachable on this computer.");
    // "Not running" isn't repeated as an error under the hint.
    assert!(
        harness
            .query_by_label_contains("Start Zotero on this computer and try again")
            .is_none()
    );
}

#[test]
fn sources_are_added_imported_and_deleted() {
    let (mut harness, memory) = app();
    let ids = |memory: &Memory| -> Vec<String> {
        project(&memory.last(), "project-1")
            .get("project")
            .and_then(|p| p.get("citations"))
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(|i| i.get("id").and_then(JsValue::as_js_string))
            .map(|s| s.to_string_lossy())
            .collect()
    };
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Sources")
        .click();
    harness.run();
    harness.get_by_label_contains("Cited in 1 document");
    harness.get_by_label("1 source in project");

    // Add a book by hand: its key comes from the author and year.
    harness
        .get_by_role_and_label(Role::Button, "Add manually")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "Title")
        .focus();
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "Title")
        .type_text("Tides and Lights");
    harness
        .get_by_role_and_label(
            Role::TextInput,
            "Authors (Last, First; separated by semicolons)",
        )
        .focus();
    harness.run();
    harness
        .get_by_role_and_label(
            Role::TextInput,
            "Authors (Last, First; separated by semicolons)",
        )
        .type_text("Müller, Hans");
    harness
        .get_by_role_and_label(Role::TextInput, "Year")
        .focus();
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "Year")
        .type_text("2019");
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Add source")
        .click();
    harness.run_steps(12);
    assert_eq!(ids(&memory), ["smith2020", "mueller2019"]);

    // Import a BibTeX file: an id already in the library is renamed, as ProseCraft does.
    harness.state_mut().import_citation_file(
        "refs.bib",
        "@book{smith2020, title={Lighthouses Revisited}}\n@article{roe2001, title={Fog}, author={Roe, R.}, journal={J}, year={2001}}",
    );
    harness.run_steps(12);
    harness.get_by_label_contains("Imported 2 sources from refs.bib.");
    harness
        .get_by_label_contains("Duplicate citation id \"smith2020\" renamed to \"smith2020-2\".");
    assert_eq!(
        ids(&memory),
        ["smith2020", "mueller2019", "smith2020-2", "roe2001"]
    );

    // The search narrows the list.
    harness
        .get_by_role_and_label(Role::TextInput, "Search sources")
        .focus();
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "Search sources")
        .type_text("fog");
    harness.run();
    assert!(harness.query_by_label("Delete Lighthouses").is_none());
    assert!(harness.query_by_label("Delete Fog").is_some());
    harness
        .get_by_role_and_label(Role::TextInput, "Search sources")
        .focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.key_press(egui::Key::Backspace);
    harness.run();

    // Deleting a cited source warns first.
    harness
        .get_by_role_and_label(Role::Button, "Delete Lighthouses")
        .click();
    harness.run();
    harness.get_by_label_contains("Deleting this source will leave unresolved citations");
    harness
        .get_by_role_and_label(Role::Button, "Delete anyway")
        .click();
    harness.run_steps(12);
    assert_eq!(ids(&memory), ["mueller2019", "smith2020-2", "roe2001"]);
    harness.get_by_role_and_label(Role::Button, "Done").click();
    harness.run();
    assert!(harness.query_by_label("Search sources").is_none());
}

#[test]
fn citations_are_inserted_with_the_picker() {
    let (mut harness, memory) = app();
    let content = |memory: &Memory| -> String {
        project(&memory.last(), "project-1")
            .get("project")
            .and_then(|p| p.get("documents"))
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .find(|d| {
                d.get("id")
                    .and_then(JsValue::as_js_string)
                    .is_some_and(|i| i.eq_str("doc-1"))
            })
            .and_then(|d| d.get("content"))
            .and_then(JsValue::as_js_string)
            .map(|c| c.to_string_lossy())
            .unwrap_or_default()
    };
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    // The caret at the end of the text, then Insert citation.
    harness.get_by_label("Scene text").focus();
    harness.run();
    harness.key_press(egui::Key::End);
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Insert citation")
        .click();
    harness.run_steps(3);
    assert!(
        harness
            .get_by_role_and_label(Role::TextInput, "Search citation sources")
            .is_focused(),
        "the picker opens with its search focused"
    );
    // Nothing chosen yet: Insert is disabled.
    assert!(
        harness
            .get_by_role_and_label(Role::Button, "Insert")
            .accesskit_node()
            .is_disabled()
    );
    harness
        .get_by_role_and_label(Role::TextInput, "Search citation sources")
        .type_text("light");
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Lighthouses")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "Locator")
        .focus();
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "Locator")
        .type_text("42");
    harness.run_steps(2);
    // The preview, in the publish style.
    harness.get_by_label_contains("(Smith, 2020, p. 42)");
    harness
        .get_by_role_and_label(Role::Button, "Insert")
        .click();
    harness.run_steps(12);
    assert!(
        harness.query_by_label("Search citation sources").is_none(),
        "the picker closed"
    );
    let html = content(&memory);
    assert!(
        html.contains(r#"<span class="cite-ref" data-cites="[{&quot;id&quot;:&quot;smith2020&quot;,&quot;locator&quot;:&quot;42&quot;,&quot;label&quot;:&quot;page&quot;}]" contenteditable="false"></span>"#),
        "{html}"
    );

    // Ctrl+Alt+C opens it too; Escape closes it without a change.
    harness.get_by_label("Scene text").focus();
    harness.run();
    harness.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::ALT,
        egui::Key::C,
    );
    harness.run_steps(3);
    assert!(harness.query_by_label("Search citation sources").is_some());
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(harness.query_by_label("Search citation sources").is_none());
    assert_eq!(content(&memory), html);

    // A change to the scene while the picker is open (a footnote here) closes it: its place in
    // the text is gone, so nothing is inserted there.
    harness.get_by_label("Scene text").focus();
    harness.run();
    harness.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::ALT,
        egui::Key::C,
    );
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::Button, "Lighthouses")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Insert footnote")
        .click();
    harness.run_steps(12);
    assert!(
        harness.query_by_label("Search citation sources").is_none(),
        "the picker closed"
    );
    let after = content(&memory);
    assert_eq!(
        after.matches("cite-ref").count(),
        html.matches("cite-ref").count()
    );
    assert!(after.matches("footnote-ref").count() > html.matches("footnote-ref").count());
}

#[test]
fn a_citation_is_changed_and_removed_from_its_picker() {
    let (mut harness, memory) = app();
    let content = |memory: &Memory| -> String {
        project(&memory.last(), "project-1")
            .get("project")
            .and_then(|p| p.get("documents"))
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .find(|d| {
                d.get("id")
                    .and_then(JsValue::as_js_string)
                    .is_some_and(|i| i.eq_str("doc-1"))
            })
            .and_then(|d| d.get("content"))
            .and_then(JsValue::as_js_string)
            .map(|c| c.to_string_lossy())
            .unwrap_or_default()
    };
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    // Click along the text until a citation's label is hit: its picker opens to edit it.
    let text = harness.get_by_label("Scene text").rect();
    let click = |harness: &mut Harness<'static, ProseApp>, pos: egui::Pos2| {
        harness.drag_at(pos);
        harness.run();
        harness.drop_at(pos);
        harness.run_steps(3);
    };
    let mut chip = None;
    'scan: for dy in (8..120).step_by(8) {
        for dx in (10..400).step_by(10) {
            let pos = text.left_top() + egui::vec2(dx as f32, dy as f32);
            click(&mut harness, pos);
            if harness.query_by_label("Search citation sources").is_some() {
                chip = Some(pos);
                break 'scan;
            }
        }
    }
    let chip = chip.expect("a click on the citation opens its picker");
    assert!(
        harness.query_by_label("Lighthouses").is_some(),
        "the picker lists the library"
    );
    harness.get_by_label("Remove Smith (2020)");
    harness
        .get_by_role_and_label(Role::TextInput, "Locator")
        .focus();
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "Locator")
        .type_text("7");
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Update")
        .click();
    harness.run_steps(12);
    assert!(
        content(&memory).contains(
            r#"data-cites="[{&quot;id&quot;:&quot;smith2020&quot;,&quot;locator&quot;:&quot;7&quot;,&quot;label&quot;:&quot;page&quot;}]""#
        ),
        "{}",
        content(&memory)
    );

    // Its picker again (the label is a little wider now): Remove takes it out.
    click(&mut harness, chip);
    assert!(harness.query_by_label("Search citation sources").is_some());
    harness
        .get_by_role_and_label(Role::Button, "Remove")
        .click();
    harness.run_steps(12);
    assert!(
        !content(&memory).contains("cite-ref"),
        "{}",
        content(&memory)
    );
    assert!(content(&memory).contains("Then"));
}

#[test]
fn a_footnote_cites_a_source() {
    let (mut harness, memory) = app();
    let footnotes = |memory: &Memory| -> String {
        project(&memory.last(), "project-1")
            .get("project")
            .and_then(|p| p.get("documents"))
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .find(|d| {
                d.get("id")
                    .and_then(JsValue::as_js_string)
                    .is_some_and(|i| i.eq_str("doc-1"))
            })
            .and_then(|d| d.get("footnotes"))
            .map(js::stringify)
            .unwrap_or_default()
    };
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    harness.get_by_label("Scene text").focus();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Insert footnote")
        .click();
    harness.run_steps(4);
    harness.get_by_label("Footnote text").type_text("See ");
    harness.run();
    // The footnote's own Insert citation (the scene's toolbar has one too).
    harness
        .query_all_by_role_and_label(Role::Button, "Insert citation")
        .last()
        .expect("the footnote's Insert citation")
        .click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::Button, "Lighthouses")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Insert")
        .click();
    harness.run_steps(3);
    assert!(harness.query_by_label("Search citation sources").is_none());
    assert!(
        harness.query_by_label("Footnote text").is_some(),
        "the footnote editor stays open"
    );

    // Escape with the picker open closes only the picker.
    harness
        .query_all_by_role_and_label(Role::Button, "Insert citation")
        .last()
        .expect("the footnote's Insert citation")
        .click();
    harness.run_steps(3);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(harness.query_by_label("Search citation sources").is_none());
    assert!(harness.query_by_label("Footnote text").is_some());

    // The caret is after the citation: typing continues there.
    harness
        .get_by_label("Footnote text")
        .type_text(" for more.");
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Done").click();
    harness.run_steps(12);
    let saved = footnotes(&memory);
    assert!(
        saved.contains(r#"See <span class=\"cite-ref\" data-cites=\"[{&quot;id&quot;:&quot;smith2020&quot;}]\" contenteditable=\"false\"></span> for more."#),
        "{saved}"
    );
}

#[test]
fn a_scene_is_written_among_the_other_scenes_of_its_chapter() {
    let (mut harness, memory) = app();
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    // ProseCraft's chapter view: the title is the chapter's, and the chapter's other scene
    // shows below the open one.
    let title = harness.get_by_label("Chapter title");
    assert_eq!(
        title.accesskit_node().value().as_deref(),
        Some("Chapter One")
    );
    assert!(harness.query_by_label("Scene title").is_none());
    assert_eq!(harness.query_all_by_label("Scene text").count(), 1);
    // A click in the other scene opens it there.
    harness
        .get_by_label("Scene INT. LIGHTHOUSE - NIGHT")
        .click();
    harness.run();
    assert!(
        harness
            .query_by_label("Scene INT. LIGHTHOUSE - NIGHT")
            .is_none()
    );
    harness.get_by_label("Scene Opening Scene");
    harness.get_by_label("Element type");
    // Typing goes to the scene now open; the first keeps its text.
    harness.get_by_label("Scene text").focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::End);
    harness.get_by_label("Scene text").type_text(" Rain.");
    harness.run_steps(12);
    let saved = memory.last();
    let stored = project(&saved, "project-1");
    let text = |id: &str| {
        document(stored, id)
            .and_then(|d| d.get("content"))
            .and_then(JsValue::as_js_string)
            .map(|c| c.to_string_lossy())
            .unwrap_or_default()
    };
    assert!(text("doc-3").contains("RAIN."), "{}", text("doc-3"));
    assert!(!text("doc-1").to_uppercase().contains("RAIN."));
}

#[test]
fn a_scene_is_renamed_from_the_binder() {
    let (mut harness, memory) = app();
    harness
        .get_by_role_and_label(Role::Button, "Actions for Opening Scene")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Rename scene")
        .click();
    harness.run();
    let field = harness.get_by_label("Rename Opening Scene");
    field.focus();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_label("Rename Opening Scene")
        .type_text("  Dawn  ");
    harness.key_press(egui::Key::Enter);
    harness.run_steps(12);
    harness.get_by_role_and_label(Role::TreeItem, "Dawn");
    let saved = memory.last();
    let scene = document(project(&saved, "project-1"), "doc-1").expect("scene");
    assert_eq!(
        scene
            .get("title")
            .and_then(JsValue::as_js_string)
            .map(|t| t.to_string_lossy())
            .as_deref(),
        Some("Dawn")
    );
}

#[test]
fn enter_on_an_empty_paragraph_starts_a_scene_and_backspace_joins_it_back() {
    let (mut harness, memory) = app();
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    harness.get_by_label("Scene text").focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::End);
    // An empty paragraph, then Enter on it: a new scene after this one.
    harness.key_press(egui::Key::Enter);
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run_steps(4);
    harness.get_by_role_and_label(Role::TreeItem, "Untitled scene");
    harness.get_by_label("Scene Opening Scene");
    harness.get_by_label("Scene text").type_text("Next.");
    harness.run_steps(12);
    let saved = memory.last();
    let stored = project(&saved, "project-1");
    let ids: Vec<String> = stored
        .get("project")
        .and_then(|p| p.get("documents"))
        .and_then(JsValue::as_array)
        .map(|d| {
            d.iter()
                .filter_map(|d| d.get("id").and_then(JsValue::as_js_string))
                .map(|i| i.to_string_lossy())
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(ids, ["doc-1", "new-0", "doc-2", "doc-3"]);
    let content = |stored: &JsValue, id: &str| {
        document(stored, id)
            .and_then(|d| d.get("content"))
            .and_then(JsValue::as_js_string)
            .map(|c| c.to_string_lossy())
            .unwrap_or_default()
    };
    assert_eq!(content(stored, "new-0"), "<p>Next.</p>");
    // Saved through ProseCraft's sanitizer, which empties a citation marker's stale label.
    assert!(
        content(stored, "doc-1").ends_with(r#"contenteditable="false"></span>.</p>"#),
        "{}",
        content(stored, "doc-1")
    );

    // Backspace at the start of the new scene joins it back.
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Home);
    harness.run();
    harness.key_press(egui::Key::Backspace);
    harness.run_steps(12);
    assert!(harness.query_by_label("Scene Untitled scene").is_none());
    let saved = memory.last();
    let stored = project(&saved, "project-1");
    assert!(content(stored, "doc-1").ends_with("</span>.</p><p>Next.</p>"));
    let joined = document(stored, "new-0").expect("kept in the trash");
    assert_eq!(joined.get("trashed"), Some(&JsValue::Bool(true)));
    // The caret is at the join: typing continues the joined text.
    harness.get_by_label("Scene text").type_text("More ");
    harness.run_steps(12);
    let saved = memory.last();
    assert!(
        content(project(&saved, "project-1"), "doc-1").ends_with("<p>More Next.</p>"),
        "{}",
        content(project(&saved, "project-1"), "doc-1")
    );
}

#[test]
fn a_chapter_is_written_in_markdown_with_scene_break_lines() {
    let (mut harness, memory) = app();
    harness
        .get_all_by_role_and_label(Role::Button, "Add chapter")
        .next()
        .expect("an add button")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Actions for Chapter 1")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Add scene")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::TreeItem, "Scene 1")
        .click();
    harness.run();
    harness.get_by_label("Scene text").focus();
    harness.run();
    harness.get_by_label("Scene text").type_text("First.");
    harness.run_steps(12);
    harness
        .get_by_role_and_label(Role::Button, "Settings…")
        .click();
    harness.run();
    harness.get_by_label("Writing").click();
    harness.run();
    harness.get_by_label("Enable Markdown editor").click();
    harness.run();
    harness.key_press(egui::Key::Escape);
    harness.run();
    harness.get_by_label("Markdown editor").click();
    harness.run_steps(2);

    // The chapter's two scenes, with a marker line between them.
    let source = harness.get_by_label("Markdown source");
    assert_eq!(
        source.accesskit_node().value().as_deref(),
        Some("First.\n\n<!-- scene-break -->\n\n")
    );
    harness.get_by_label("Chapter title");
    source.focus();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::End);
    harness
        .get_by_label("Markdown source")
        .type_text("Second *one*.");
    harness.run_steps(12);
    let saved = memory.last();
    let stored = project(&saved, "project-1");
    let text = |id: &str, key: &str| {
        document(stored, id)
            .and_then(|d| d.get(key))
            .map(js::stringify)
            .unwrap_or_default()
    };
    assert_eq!(text("new-1", "markdownSource"), r#""First.""#);
    assert_eq!(text("new-2", "markdownSource"), r#""Second *one*.""#);
    assert_eq!(
        text("new-2", "content"),
        r#""<p>Second <em>one</em>.</p>\n""#
    );
    // The source stays as typed while it's written back.
    assert_eq!(
        harness
            .get_by_label("Markdown source")
            .accesskit_node()
            .value()
            .as_deref(),
        Some("First.\n\n<!-- scene-break -->\n\nSecond *one*.")
    );
}

#[test]
fn pdf_exports_from_the_publish_view() {
    let (mut harness, _memory) = app();
    harness.get_by_role_and_label(Role::Tab, "Publish").click();
    harness.run();
    for (role, name) in [
        (Role::RadioButton, "PDF (pdf)"),
        (Role::Button, "Export PDF"),
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
        file_name.ends_with(".pdf") && extension == "pdf",
        "{file_name}"
    );
    assert!(bytes.starts_with(b"%PDF-"));
    // The fixture's title has a character outside Windows-1252: it is said, not garbled.
    let notes: Vec<String> = harness
        .query_all_by_role(Role::Alert)
        .filter_map(|n| n.accesskit_node().label())
        .collect();
    assert!(
        notes.iter().any(|n| n.contains("came out as “?”")),
        "{notes:?}"
    );
}

#[test]
fn a_screenplay_pdf_exports_from_the_publish_view() {
    let (mut harness, _memory) = app();
    harness.get_by_role_and_label(Role::Tab, "Publish").click();
    harness.run();
    for (role, name) in [
        (Role::RadioButton, "Screenplay PDF (pdf)"),
        (Role::Button, "Export Screenplay PDF"),
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
    assert!(file_name.ends_with(".pdf"), "{file_name}");
    assert!(bytes.starts_with(b"%PDF-"));
}
