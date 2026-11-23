//! The hosted session driven as the host page drives it: an `open` with a backup's bytes, edits
//! made through the app by pointer and keyboard, `saved` answers (or none), all without a browser.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use prosecraft_core::js::{self, JsValue};
use prosecraft_ui_egui::{LibraryStore, ProjectLibrary, ProseApp};

use super::{Bridge, Effect, SAVE_EVERY, SAVE_TIMEOUT};

const NAME: &str = "Lighthouse.prosecraft";

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/../../crates/core/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The browser's store as the session sees it: time, ids and settings. Its library is never
/// written.
#[derive(Default)]
struct Device {
    ids: usize,
}

impl LibraryStore for Device {
    fn save(&mut self, _text: &str) -> Result<(), String> {
        panic!("the browser's library is never written for a hosted file")
    }
    fn now_ms(&self) -> f64 {
        1_800_000_000_000.0
    }
    fn new_id(&mut self) -> String {
        self.ids += 1;
        format!("new-{}", self.ids)
    }
}

/// The page: the app and its session, a clock the test moves, what was posted.
struct Page {
    app: ProseApp,
    bridge: Bridge,
    now: f64,
    posted: Vec<Effect>,
}

impl Page {
    fn open(name: &str, text: &str) -> Self {
        let mut bridge = Bridge::new();
        let app = bridge
            .open(
                name,
                text.as_bytes(),
                "this-browser".into(),
                Box::new(Device::default()),
            )
            .unwrap_or_else(|e| panic!("{name} opens: {e}"));
        let posted = bridge.take_effects();
        Self {
            app,
            bridge,
            now: 0.0,
            posted,
        }
    }

    fn frame(&mut self, ui: &mut egui::Ui) {
        self.bridge.keys(ui.ctx(), &mut self.app);
        self.app.show(ui);
        self.bridge.tick(self.now, &mut self.app);
        self.posted.extend(self.bridge.take_effects());
    }

    /// Moves the clock and lets the session act on it.
    fn at(&mut self, now: f64) {
        self.now = now;
        self.bridge.tick(now, &mut self.app);
        self.posted.extend(self.bridge.take_effects());
    }

    fn saves(&self) -> Vec<(String, String, String)> {
        self.posted
            .iter()
            .filter_map(|e| match e {
                Effect::Save { name, bytes, id } => Some((
                    name.clone(),
                    String::from_utf8_lossy(bytes).into_owned(),
                    id.clone(),
                )),
                _ => None,
            })
            .collect()
    }

    fn dirty(&self) -> Vec<bool> {
        self.posted
            .iter()
            .filter_map(|e| match e {
                Effect::Dirty(d) => Some(*d),
                _ => None,
            })
            .collect()
    }
}

fn harness(page: Page) -> Harness<'static, Page> {
    let harness = Harness::builder()
        .with_max_steps(8)
        .with_size([1280.0, 850.0])
        .build_ui_state(|ui, page: &mut Page| page.frame(ui), page);
    prosecraft_ui_egui::setup_for_tests(&harness.ctx);
    harness
}

/// Adds a chapter through the binder and waits out the app's pause before it saves.
fn add_chapter(harness: &mut Harness<'static, Page>) {
    harness
        .get_all_by_role_and_label(Role::Button, "Add chapter")
        .next()
        .expect("an add button")
        .click();
    harness.run();
    harness.run_steps(12);
}

#[test]
fn an_opened_file_is_shown_and_opening_writes_nothing() {
    let mut harness = harness(Page::open(NAME, &fixture("bundle-plain.prosecraft")));
    harness.run();
    harness.get_by_role_and_label(Role::TreeItem, "Opening Scene");
    harness.state_mut().at(SAVE_TIMEOUT * 3.0);
    harness.run();
    assert_eq!(
        harness.state().posted,
        [],
        "nothing is sent until something changes"
    );
}

#[test]
fn an_edit_is_sent_under_the_files_name_and_saved_clears_dirty() {
    let mut harness = harness(Page::open(NAME, &fixture("bundle-plain.prosecraft")));
    harness.run();
    add_chapter(&mut harness);

    let page = harness.state();
    let saves = page.saves();
    assert_eq!(saves.len(), 1, "{:?}", page.posted);
    let (name, text, id) = &saves[0];
    assert_eq!(name, NAME);
    assert_eq!(page.dirty(), [true]);
    let library = ProjectLibrary::from_text(text).expect("the save is a backup");
    assert_eq!(library.device_id, "this-browser");
    assert_eq!(library.projects().count(), 2, "both projects go back");
    assert!(text.contains("Chapter 1"), "the new chapter is in the save");

    let id = id.clone();
    harness.state_mut().bridge.saved("an-old-id");
    harness.run();
    assert_eq!(
        harness.state().dirty(),
        [true],
        "another id changes nothing"
    );
    harness.state_mut().bridge.saved(&id);
    harness.run();
    assert_eq!(harness.state().dirty(), [true, false]);
    assert!(harness.state().bridge.wake_in(1.0).is_none());
}

#[test]
fn saves_are_gathered_one_at_a_time_and_ctrl_s_sends_at_once() {
    let mut harness = harness(Page::open(NAME, &fixture("bundle-plain.prosecraft")));
    harness.run();
    add_chapter(&mut harness);
    let first = harness.state().saves()[0].2.clone();

    // Another change while the first save is unconfirmed: it waits.
    harness.state_mut().at(1.0);
    add_chapter(&mut harness);
    assert_eq!(harness.state().saves().len(), 1);
    harness.state_mut().bridge.saved(&first);
    harness.state_mut().at(2.0);
    assert_eq!(harness.state().saves().len(), 1, "sooner than SAVE_EVERY");
    assert_eq!(harness.state().dirty(), [true], "still unsaved");
    assert!(
        harness
            .state()
            .bridge
            .wake_in(2.0)
            .is_some_and(|s| s > 0.0 && s <= SAVE_EVERY)
    );

    // Ctrl+S sends it now.
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    harness.run();
    let saves = harness.state().saves();
    assert_eq!(saves.len(), 2);
    assert!(saves[1].1.contains("Chapter 2"), "the newest file is sent");
    assert_ne!(saves[1].2, first, "each save has its own id");

    // Without Ctrl+S, the next change goes once SAVE_EVERY has passed.
    let second = saves[1].2.clone();
    harness.state_mut().bridge.saved(&second);
    add_chapter(&mut harness);
    harness.state_mut().at(2.0 + SAVE_EVERY - 1.0);
    assert_eq!(harness.state().saves().len(), 2);
    harness.state_mut().at(2.0 + SAVE_EVERY);
    assert_eq!(harness.state().saves().len(), 3);
}

#[test]
fn an_unanswered_save_is_reported_and_sent_again() {
    let mut harness = harness(Page::open(NAME, &fixture("bundle-plain.prosecraft")));
    harness.run();
    add_chapter(&mut harness);
    let (_, text, id) = harness.state().saves()[0].clone();

    harness.state_mut().at(SAVE_TIMEOUT);
    let page = harness.state();
    assert!(page.posted.contains(&Effect::Failed {
        error: format!(
            "{NAME} wasn't saved: the host didn't confirm the save. It will be sent again."
        ),
        id: Some(id.clone()),
    }));
    assert_eq!(page.dirty(), [true], "still unsaved");
    assert!(
        page.app
            .state()
            .status()
            .is_some_and(|s| s.contains("wasn't saved")),
        "the person is told"
    );

    harness.state_mut().at(SAVE_TIMEOUT + SAVE_EVERY);
    let saves = harness.state().saves();
    assert_eq!(saves.len(), 2, "sent again");
    assert_eq!(saves[1].1, text);
    let retry = saves[1].2.clone();
    assert_ne!(retry, id);

    // The first answer comes late, then the retry's: both count, nothing more is sent.
    harness.state_mut().bridge.saved(&id);
    harness.state_mut().bridge.saved(&retry);
    harness.state_mut().at(SAVE_TIMEOUT * 4.0);
    assert_eq!(harness.state().saves().len(), 2);
    assert_eq!(harness.state().dirty(), [true, false]);
}

#[test]
fn a_late_answer_for_a_timed_out_save_clears_what_waits() {
    let mut harness = harness(Page::open(NAME, &fixture("bundle-plain.prosecraft")));
    harness.run();
    add_chapter(&mut harness);
    let id = harness.state().saves()[0].2.clone();
    harness.state_mut().at(SAVE_TIMEOUT);
    harness.state_mut().bridge.saved(&id);
    harness.state_mut().at(SAVE_TIMEOUT + 1.0);
    assert_eq!(harness.state().saves().len(), 1);
    assert_eq!(harness.state().dirty(), [true, false]);
}

#[test]
fn a_save_that_could_not_be_posted_waits_for_the_next_turn() {
    let mut harness = harness(Page::open(NAME, &fixture("bundle-plain.prosecraft")));
    harness.run();
    add_chapter(&mut harness);
    let (_, text, id) = harness.state().saves()[0].clone();
    harness.state_mut().bridge.not_sent(&id);
    harness.state_mut().at(SAVE_EVERY);
    let saves = harness.state().saves();
    assert_eq!(saves.len(), 2);
    assert_eq!(saves[1].1, text);
}

fn projects(bundle: &mut JsValue) -> &mut Vec<JsValue> {
    match bundle.as_object_mut().and_then(|b| b.get_mut("projects")) {
        Some(JsValue::Array(projects)) => projects,
        _ => panic!("a bundle has projects"),
    }
}

/// `bundle-plain.prosecraft` with one project from a newer ProseCraft.
fn newer() -> String {
    let mut bundle = js::parse(&fixture("bundle-plain.prosecraft")).expect("the fixture");
    if let Some(project) = projects(&mut bundle)
        .first_mut()
        .and_then(JsValue::as_object_mut)
    {
        project.insert("schemaVersion", JsValue::Number(99.0));
    }
    js::stringify(&bundle)
}

#[test]
fn a_file_from_a_newer_prosecraft_is_shown_and_never_sent_back() {
    let mut harness = harness(Page::open(NAME, &newer()));
    harness.run();
    assert!(!harness.state().app.state().is_editable());
    harness.get_by_label_contains("newer version of ProseCraft");
    // Even a save the app made would stay here.
    harness.state_mut().app.flush();
    *harness.state().bridge.outbox.borrow_mut() = Some("anything".into());
    harness.state_mut().bridge.save_now = true;
    harness.state_mut().at(SAVE_EVERY * 2.0);
    harness.run();
    assert_eq!(harness.state().posted, []);
}

#[test]
fn an_encrypted_file_asks_for_its_passphrase_and_is_never_sent_back() {
    let mut harness = harness(Page::open(NAME, &fixture("bundle-encrypted.prosecraft")));
    harness.run();
    assert!(harness.state().app.is_locked());
    *harness.state().bridge.outbox.borrow_mut() = Some("plain text".into());
    harness.state_mut().at(SAVE_EVERY * 2.0);
    harness.run();
    assert_eq!(
        harness.state().posted,
        [],
        "projects never leave unencrypted"
    );
}

#[test]
fn records_this_build_cannot_read_go_back_unchanged() {
    let mut bundle = js::parse(&fixture("bundle-plain.prosecraft")).expect("the fixture");
    projects(&mut bundle)
        .push(js::parse(r#"{"kind":"from-the-future","keep":"me"}"#).expect("json"));
    let mut harness = harness(Page::open(NAME, &js::stringify(&bundle)));
    harness.run();
    add_chapter(&mut harness);
    let (_, text, _) = harness.state().saves()[0].clone();
    assert!(text.contains(r#""keep": "me""#), "{text}");
}

#[test]
fn what_is_not_a_backup_is_refused_and_the_host_told() {
    for (bytes, why) in [
        (b"{\"hello\":1}".to_vec(), "Couldn't open notes.prosecraft:"),
        (vec![0xff, 0xfe, 0x00], "it isn't a ProseCraft backup"),
        (Vec::new(), "Couldn't open notes.prosecraft:"),
    ] {
        let mut bridge = Bridge::new();
        let opened = bridge.open(
            "notes.prosecraft",
            &bytes,
            "b".into(),
            Box::new(Device::default()),
        );
        let Err(error) = opened else {
            panic!("{bytes:?} isn't a backup")
        };
        assert!(error.contains(why), "{error}");
        assert_eq!(bridge.take_effects(), [Effect::Failed { error, id: None }]);
        // Nothing open, so nothing is ever sent.
        *bridge.outbox.borrow_mut() = Some("x".into());
        bridge.tick(100.0, &mut ProseApp::new());
        assert_eq!(bridge.take_effects(), []);
    }
}

#[test]
fn a_second_file_is_refused_not_merged_into_the_first() {
    let mut page = Page::open(NAME, &fixture("bundle-plain.prosecraft"));
    let second = page.bridge.open(
        "Other.prosecraft",
        fixture("bundle-plain.prosecraft").as_bytes(),
        "b".into(),
        Box::new(Device::default()),
    );
    assert!(second.is_err());
    assert!(matches!(
        page.bridge.take_effects().as_slice(),
        [Effect::Failed { id: None, .. }]
    ));
}

#[test]
fn a_file_far_too_large_is_not_read() {
    let size = ProseApp::MAX_BACKUP_BYTES as f64 + 1.0;
    assert!(Bridge::too_large("huge.prosecraft", size).is_some());
    assert!(Bridge::too_large("fine.prosecraft", 1024.0).is_none());
}

#[test]
fn the_host_messages_are_this_apps() {
    use craft_web_host::{Incoming, message_type};
    assert_eq!(
        Incoming::parse(super::APP_ID, "prosecraft-rs:open"),
        Some(Incoming::Open)
    );
    assert_eq!(
        Incoming::parse(super::APP_ID, "prosecraft-rs:saved"),
        Some(Incoming::Saved)
    );
    assert_eq!(Incoming::parse(super::APP_ID, "wordcraft:open"), None);
    assert_eq!(message_type(super::APP_ID, "save"), "prosecraft-rs:save");
}
