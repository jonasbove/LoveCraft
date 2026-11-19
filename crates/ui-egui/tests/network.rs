//! The Network view, the character card, the story data panel and the graph pane (ProseCraft#31),
//! driven through the interface and checked in what is saved.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use prosecraft_core::js::{self, JsObject, JsValue};
use prosecraft_ui_egui::{LibraryStore, ProjectLibrary, ProseApp};

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
    /// A document of project-1 as last saved.
    fn document(&self, id: &str) -> JsValue {
        let text = self
            .saved
            .lock()
            .ok()
            .and_then(|s| s.last().cloned())
            .expect("saved");
        let library = js::parse(&text).expect("json");
        library
            .get("projects")
            .and_then(JsValue::as_array)
            .and_then(|p| p.first())
            .and_then(|p| p.get("project"))
            .and_then(|p| p.get("documents"))
            .and_then(JsValue::as_array)
            .and_then(|d| {
                d.iter().find(|d| {
                    d.get("id")
                        .and_then(JsValue::as_js_string)
                        .is_some_and(|i| i.eq_str(id))
                })
            })
            .cloned()
            .expect("document")
    }
}

fn doc(id: &str, title: &str, extra: &[(&str, JsValue)]) -> JsValue {
    let mut o = JsObject::new();
    o.insert("id", JsValue::str(id));
    o.insert("title", JsValue::str(title));
    for (k, v) in extra {
        o.insert(*k, v.clone());
    }
    JsValue::Object(o)
}

/// The plain bundle with a Characters folder holding Mara and Jonah, and three scenes in which
/// both appear.
fn bundle() -> String {
    bundle_with(0)
}

/// The bundle with `extra` more character pages, unrelated.
fn bundle_with(extra: usize) -> String {
    let path = format!(
        "{}/../core/tests/fixtures/bundle-plain.prosecraft",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut bundle = js::parse(&std::fs::read_to_string(path).expect("fixture")).expect("json");
    let project = bundle
        .as_object_mut()
        .and_then(|b| b.get_mut("projects"))
        .and_then(|p| match p {
            JsValue::Array(a) => a.first_mut(),
            _ => None,
        })
        .and_then(|p| p.as_object_mut())
        .and_then(|p| p.get_mut("project"))
        .and_then(JsValue::as_object_mut)
        .expect("project");
    project.insert(
        "researchFolders",
        js::parse(r#"[{"id":"folder-1","title":"Characters","parentId":null}]"#).expect("folders"),
    );
    let Some(JsValue::Array(documents)) = project.get_mut("documents") else {
        panic!("documents");
    };
    documents.push(doc(
        "doc-jonah",
        "Jonah",
        &[
            ("space", JsValue::str("Wiki")),
            ("folderId", JsValue::str("folder-1")),
            ("content", JsValue::str("<p>A sailor.</p>")),
        ],
    ));
    for n in 1..=extra {
        documents.push(doc(
            &format!("doc-extra-{n}"),
            &format!("Extra {n}"),
            &[
                ("space", JsValue::str("Wiki")),
                ("folderId", JsValue::str("folder-1")),
                ("content", JsValue::str("<p>Someone.</p>")),
                // A line of friends, laid out at another angle by each layout seed.
                (
                    "meta",
                    js::parse(&if n < extra {
                        format!(
                            r#"{{"relations":[{{"id":"r-{n}","to":"doc-extra-{}","type":"friend"}}]}}"#,
                            n + 1
                        )
                    } else {
                        "{}".to_owned()
                    })
                    .expect("meta"),
                ),
            ],
        ));
    }
    for n in 1..=3 {
        documents.push(doc(
            &format!("scene-{n}"),
            &format!("Together {n}"),
            &[
                ("chapterId", JsValue::str("chapter-1")),
                ("content", JsValue::str("<p>Mara and Jonah talk.</p>")),
            ],
        ));
    }
    js::stringify(&bundle)
}

fn app() -> (Harness<'static, ProseApp>, Memory) {
    app_with(bundle())
}

fn app_with(bundle: String) -> (Harness<'static, ProseApp>, Memory) {
    let memory = Memory::default();
    let mut app = ProseApp::with_library(
        ProjectLibrary::new("this-device".into()),
        Box::new(memory.clone()),
    );
    app.open_backup("network.prosecraft", &bundle);
    let mut harness = Harness::builder()
        .with_max_steps(8)
        .with_size([1500.0, 1000.0])
        .build_ui_state(|ui, app: &mut ProseApp| app.show(ui), app);
    prosecraft_ui_egui::setup_for_tests(&harness.ctx);
    harness.run();
    (harness, memory)
}

fn relations(memory: &Memory, id: &str) -> Vec<JsValue> {
    memory
        .document(id)
        .get("meta")
        .and_then(|m| m.get("relations"))
        .and_then(JsValue::as_array)
        .map(<[JsValue]>::to_vec)
        .unwrap_or_default()
}

fn text(v: &JsValue, k: &str) -> String {
    v.get(k)
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default()
}

#[test]
fn relations_are_added_edited_and_deleted_in_the_network() {
    let (mut harness, memory) = app();
    harness.get_by_role_and_label(Role::Tab, "Network").click();
    harness.run();
    // Both characters are pages of the graph.
    harness.get_by_label("Mara, Character, 1 link");
    harness.get_by_label("Jonah, Character, 1 link");
    harness
        .get_all_by_label("+ Add relation")
        .next()
        .expect("header button")
        .click();
    harness.run();
    // From is the first page by title (Jonah); To is chosen.
    harness
        .get_by_role_and_label(Role::ComboBox, "To")
        .click_accesskit();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Mara")
        .click_accesskit();
    harness.run();
    harness
        .get_all_by_label("Friend")
        .last()
        .expect("type chip")
        .click_accesskit();
    harness.run();
    harness
        .get_all_by_label("+ Add relation")
        .last()
        .expect("panel button")
        .click();
    harness.run_steps(12);
    let list = relations(&memory, "doc-jonah");
    assert_eq!(list.len(), 1);
    assert_eq!(
        (text(&list[0], "to"), text(&list[0], "type")),
        ("doc-2".to_owned(), "friend".to_owned())
    );
    // The new relation is selected: give it a label and a tension.
    harness.get_by_label("Delete relation");
    let label = harness.get_by_role_and_label(Role::TextInput, "Label");
    label.focus();
    label.type_text("Old shipmates");
    harness.key_press(egui::Key::Enter);
    harness.run();
    harness.get_by_label("Tension 3").click();
    harness.run_steps(12);
    let list = relations(&memory, "doc-jonah");
    assert_eq!(text(&list[0], "label"), "Old shipmates");
    assert_eq!(
        list[0].get("tension").and_then(JsValue::as_number),
        Some(3.0)
    );
    harness.get_by_label("Delete relation").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Delete")
        .click();
    harness.run_steps(12);
    assert!(relations(&memory, "doc-jonah").is_empty());
    assert!(
        memory.document("doc-jonah").get("meta").is_none(),
        "an empty meta is removed"
    );
}

#[test]
fn a_suggestion_becomes_a_relation_or_is_dismissed() {
    let (mut harness, memory) = app();
    harness.get_by_role_and_label(Role::Tab, "Network").click();
    harness.run();
    harness.get_by_label("Suggestions").click();
    harness.run();
    harness.get_by_label("Mara and Jonah");
    harness.get_by_label("Appear together in 3 scenes · no relation yet");
    harness
        .get_by_role_and_label(Role::Button, "Accept")
        .click();
    harness.run();
    harness
        .get_all_by_label("Rival")
        .last()
        .expect("chip")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Create relation")
        .click();
    harness.run_steps(12);
    let list = relations(&memory, "doc-2");
    assert_eq!(list.len(), 1, "stored on the first page of the pair");
    assert_eq!(
        (text(&list[0], "to"), text(&list[0], "type")),
        ("doc-jonah".to_owned(), "rival".to_owned())
    );
}

#[test]
fn the_character_card_applies_a_template_and_stores_values() {
    let (mut harness, memory) = app();
    harness.get_by_role_and_label(Role::Tab, "Network").click();
    harness.run();
    harness.get_by_label("Cards").click();
    harness.run();
    harness
        .get_all_by_label("Jonah")
        .find(|n| n.accesskit_node().role() != Role::TreeItem)
        .expect("Jonah's card")
        .click();
    harness.run();
    // ProseCraft's panel: 12 points in from the window's right and bottom and below the menu
    // bar, 380 wide, its fields inside it (it was a short window that cut them off). The
    // window's outline (its stroke) is outside its accessible bounds.
    let close = harness
        .get_by_role_and_label(Role::Button, "Close character card")
        .rect();
    let panel = harness.get_by_label("Jonah character card").rect();
    assert!(panel.contains_rect(close), "{close:?} in {panel:?}");
    let menu = harness.get_by_label("File").rect();
    assert!(
        panel.top() > menu.bottom() && panel.top() - menu.bottom() <= 24.0,
        "{panel:?} below the menu bar ({menu:?})"
    );
    assert!((panel.bottom() - (1000.0 - 12.0)).abs() <= 2.0, "{panel:?}");
    assert!((panel.right() - (1500.0 - 12.0)).abs() <= 2.0, "{panel:?}");
    assert!((panel.width() - 380.0).abs() <= 2.0, "{panel:?}");
    harness
        .get_by_role_and_label(Role::ComboBox, "Template")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Thriller")
        .click();
    harness.run_steps(12);
    assert_eq!(
        text(
            memory.document("doc-jonah").get("meta").expect("meta"),
            "sheetTemplate"
        ),
        "thriller"
    );
    let age = harness.get_by_role_and_label(Role::TextInput, "Age");
    age.focus();
    age.type_text("forty");
    harness.key_press(egui::Key::Enter);
    harness.run();
    harness.get_by_label("Enter a number.");
    let role = harness.get_by_role_and_label(Role::TextInput, "Role");
    assert!(
        panel.contains_rect(role.rect()),
        "{:?} in {panel:?}",
        role.rect()
    );
    role.focus();
    role.type_text("Navigator");
    harness.key_press(egui::Key::Enter);
    harness.run_steps(12);
    let meta = memory
        .document("doc-jonah")
        .get("meta")
        .cloned()
        .expect("meta");
    assert_eq!(text(meta.get("sheet").expect("sheet"), "role"), "Navigator");
    assert!(meta.get("sheet").and_then(|s| s.get("age")).is_none());
}

#[test]
fn story_data_confirms_a_mentioned_character() {
    let (mut harness, memory) = app();
    harness
        .get_by_role_and_label(Role::TreeItem, "Together 1")
        .click();
    harness.run();
    harness.get_by_label("Story data").click();
    harness.run();
    harness.get_by_label("Jonah auto").click();
    harness.run_steps(12);
    let meta = memory
        .document("scene-1")
        .get("meta")
        .cloned()
        .expect("meta");
    assert_eq!(
        js::stringify(meta.get("characters").expect("characters")),
        r#"["doc-jonah"]"#
    );
}

#[test]
fn the_graph_pane_shows_the_scenes_characters() {
    let (mut harness, _memory) = app();
    harness
        .get_by_role_and_label(Role::TreeItem, "Together 2")
        .click();
    harness.run();
    harness.get_by_label("Open split view").click();
    harness.run();
    harness.get_by_label("Show in this pane").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "  Characters graph")
        .click_accesskit();
    harness.run_steps(4);
    harness.get_by_label("Graph: characters in this scene");
    harness.get_by_label("Suggested: Mara appears here (add?)");
    harness.get_by_label("Confirm Mara");
}

#[test]
#[ignore = "renders with wgpu; run on demand"]
fn render_network() {
    let Some(dir) = std::env::var_os("PROSECRAFT_RENDER_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let (mut harness, _) = app();
    prosecraft_ui_egui::setup(&harness.ctx);
    let save = |harness: &mut Harness<'_, ProseApp>, name: &str| {
        harness.run();
        let image = harness.render().unwrap_or_else(|e| panic!("render: {e}"));
        image
            .save(dir.join(format!("{name}.png")))
            .unwrap_or_else(|e| panic!("save: {e}"));
    };
    harness.get_by_role_and_label(Role::Tab, "Network").click();
    save(&mut harness, "network");
    harness
        .get_all_by_label("+ Add relation")
        .next()
        .expect("button")
        .click();
    save(&mut harness, "network-add");
    harness.get_by_label("Suggestions").click();
    save(&mut harness, "network-suggestions");
    harness.get_by_label("Cards").click();
    save(&mut harness, "network-cards");
    harness
        .get_all_by_label("Jonah")
        .find(|n| n.accesskit_node().role() != Role::TreeItem)
        .expect("card")
        .click();
    save(&mut harness, "network-card");
    harness.get_by_label("Matrix").click();
    save(&mut harness, "network-matrix");
    harness
        .get_by_role_and_label(Role::TreeItem, "Together 1")
        .click();
    harness.run();
    harness.get_by_label("Story data").click();
    save(&mut harness, "story-data");
}

/// `updatePinch`: two fingers moving together zoom the graph out around them.
#[test]
fn two_fingers_pinch_the_graph() {
    use egui::{Event, TouchDeviceId, TouchId, TouchPhase};
    let (mut harness, _) = app();
    harness.get_by_role_and_label(Role::Tab, "Network").click();
    harness.run();
    assert!(
        harness.query_by_label("300%").is_some(),
        "two pages fit at the largest zoom"
    );
    let centre = harness
        .get_all_by_label_contains("Relationship network of")
        .next()
        .expect("canvas")
        .rect()
        .center();
    let touch = |id: u64, phase: TouchPhase, pos: egui::Pos2| Event::Touch {
        device_id: TouchDeviceId(1),
        id: TouchId(id),
        phase,
        pos,
        force: None,
    };
    let (a, b) = (
        centre - egui::vec2(120.0, 0.0),
        centre + egui::vec2(120.0, 0.0),
    );
    harness.event(touch(1, TouchPhase::Start, a));
    harness.event(touch(2, TouchPhase::Start, b));
    harness.step();
    for k in 1..=6 {
        let d = egui::vec2(15.0 * k as f32, 0.0);
        harness.event(touch(1, TouchPhase::Move, a + d));
        harness.event(touch(2, TouchPhase::Move, b - d));
        harness.step();
    }
    harness.event(touch(1, TouchPhase::End, a + egui::vec2(90.0, 0.0)));
    harness.event(touch(2, TouchPhase::End, b - egui::vec2(90.0, 0.0)));
    harness.run();
    assert!(
        harness.query_by_label("300%").is_none(),
        "pinching in zooms out"
    );
}

#[test]
fn re_layout_fits_the_new_arrangement() {
    let (mut harness, _memory) = app_with(bundle_with(3));
    harness.get_by_role_and_label(Role::Tab, "Network").click();
    harness.run();
    // Each arrangement is framed again: every page, and the name below it, inside the canvas.
    for round in 0..6 {
        harness.get_by_label("↺ Re-layout").click();
        harness.run_steps(4);
        let canvas = harness
            .get_by_label_contains("Relationship network of")
            .rect();
        for page in harness.get_all_by_label_contains(", Character, ") {
            let name = page.accesskit_node().label().unwrap_or_default();
            let page = page.rect();
            // The name sits below the circle: 15 + 11 points at 100%, scaled with the circle.
            let scale = page.height() / 52.0;
            let bottom = page.bottom() + 26.0 * scale;
            assert!(
                canvas.contains_rect(page) && bottom <= canvas.bottom(),
                "round {round}: {name} at {page:?} (name to {bottom}) outside {canvas:?}"
            );
        }
    }
}

#[test]
fn the_character_card_stays_inside_a_narrow_window() {
    let (mut harness, _memory) = app();
    harness.get_by_role_and_label(Role::Tab, "Network").click();
    harness.run();
    harness.get_by_label("Cards").click();
    harness.run();
    harness
        .get_all_by_label("Jonah")
        .find(|n| n.accesskit_node().role() != Role::TreeItem)
        .expect("Jonah's card")
        .click();
    harness.run();
    // Narrower than the card's own width plus its margins.
    harness.set_size(egui::vec2(220.0, 600.0));
    harness.run_steps(4);
    let panel = harness.get_by_label("Jonah character card").rect();
    let window = harness.ctx.content_rect();
    assert!(
        panel.left() >= window.left() && panel.right() <= window.right(),
        "{panel:?} in {window:?}"
    );
}
