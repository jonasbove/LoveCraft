//! Open from CraftCloud… and Save to CraftCloud…, against a fake `craftcloud` program that
//! answers what CraftCloud's documentation records (its `docs/CRAFT-INTEGRATION.md`, section
//! 3, and `docs/CLI.md`, "Exit codes"). No test needs a real CraftCloud.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use egui::Key;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use prosecraft_core::craftcloud::{Output, Runner};
use prosecraft_ui_egui::{HostRequest, LibraryStore, ProjectLibrary, ProseApp};

/// A `craftcloud` that answers in order and remembers what it was asked.
#[derive(Default)]
struct Fake {
    answers: Mutex<VecDeque<Output>>,
    asked: Mutex<Vec<String>>,
    staged: Mutex<Vec<String>>,
    /// How long a `save` takes to answer.
    slow_save: std::time::Duration,
}

impl Fake {
    fn new(answers: impl IntoIterator<Item = Output>) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into_iter().collect()),
            ..Self::default()
        })
    }
    fn slow(answers: impl IntoIterator<Item = Output>) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into_iter().collect()),
            slow_save: std::time::Duration::from_millis(400),
            ..Self::default()
        })
    }
    fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("asked").clone()
    }
}

impl Runner for Fake {
    fn run(&self, args: &[&str], _timeout: std::time::Duration) -> Result<Output, String> {
        self.asked.lock().expect("asked").push(args.join(" "));
        if args.first() == Some(&"save") {
            std::thread::sleep(self.slow_save);
        }
        self.answers
            .lock()
            .expect("answers")
            .pop_front()
            .ok_or_else(|| "no answer left".to_owned())
    }
    fn stage(&self, name: &str, _bytes: &[u8]) -> Result<String, String> {
        self.staged.lock().expect("staged").push(name.to_owned());
        Ok(format!("/tmp/prosecraft-rs-craftcloud-x/{name}"))
    }
    fn discard(&self, _path: &str) {}
}

struct Store;

impl LibraryStore for Store {
    fn save(&mut self, _text: &str) -> Result<(), String> {
        Ok(())
    }
    fn now_ms(&self) -> f64 {
        1_800_000_000_000.0
    }
    fn new_id(&mut self) -> String {
        "new".into()
    }
}

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/../core/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The app with a library of two projects, and CraftCloud when `cloud` is given.
fn app(cloud: Option<Arc<Fake>>) -> Harness<'static, ProseApp> {
    let mut app = ProseApp::with_library(ProjectLibrary::new("device".into()), Box::new(Store));
    app.open_backup(
        "bundle-plain.prosecraft",
        &fixture("bundle-plain.prosecraft"),
    );
    if let Some(cloud) = cloud {
        app.set_craftcloud(cloud);
    }
    let mut harness = Harness::builder()
        .with_size([1280.0, 850.0])
        .build_ui_state(|ui, app: &mut ProseApp| app.show(ui), app);
    prosecraft_ui_egui::setup_for_tests(&harness.ctx);
    harness.run_steps(4);
    harness
}

/// Draws frames until `done` (the fake answers on a background thread).
fn settle(harness: &mut Harness<'static, ProseApp>, done: impl Fn(&Harness<ProseApp>) -> bool) {
    for _ in 0..300 {
        harness.run_steps(2);
        if done(harness) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the dialog never got there");
}

fn open_file_menu(harness: &mut Harness<'static, ProseApp>) {
    harness
        .get_by_role_and_label(Role::MenuItem, "File")
        .click();
    harness.run_steps(4);
}

/// The answers recorded in CraftCloud's documentation: `files --json` from a running client,
/// and a file only in the cloud (`local_path: null`).
const FILES: &str = r#"[
 {"path": "Backups/old.prosecraft", "name": "old.prosecraft", "type": "file", "size": 812,
  "modified": "2026-10-01T08:02:11Z", "starred": false,
  "local_path": "/home/john/CraftCloud/Backups/old.prosecraft", "sync": "synced"},
 {"path": "ProseCraft-rs/The Glass Orchard.prosecraft", "name": "The Glass Orchard.prosecraft",
  "type": "file", "size": 18234, "modified": "2026-11-13T21:54:00Z", "starred": false,
  "local_path": "/home/john/CraftCloud/ProseCraft-rs/The Glass Orchard.prosecraft", "sync": "pending"},
 {"path": "Shared/Draft.prosecraft", "name": "Draft.prosecraft", "type": "file", "size": 9,
  "modified": "2026-11-14T08:02:11Z", "starred": false, "id": "f7fc37c9",
  "local_path": null, "sync": "excluded"}
]"#;

#[test]
fn without_craftcloud_none_of_it_shows() {
    let mut plain = app(None);
    assert!(plain.query_by_label("Open from CraftCloud…").is_none());
    open_file_menu(&mut plain);
    plain.get_by_role_and_label(Role::MenuItem, "Import EPUB…");
    assert!(plain.query_by_label("Open from CraftCloud…").is_none());
    assert!(plain.query_by_label("Save to CraftCloud…").is_none());

    let mut cloud = app(Some(Fake::new([])));
    cloud.get_by_role_and_label(Role::Button, "Open from CraftCloud…");
    open_file_menu(&mut cloud);
    cloud.get_by_role_and_label(Role::MenuItem, "Open from CraftCloud…");
    cloud.get_by_role_and_label(Role::MenuItem, "Save to CraftCloud…");
}

#[test]
fn open_lists_the_backups_newest_first_and_opens_one_by_keyboard() {
    let fake = Fake::new([Output::ok(FILES)]);
    let mut harness = app(Some(fake.clone()));
    harness
        .get_by_role_and_label(Role::Button, "Open from CraftCloud…")
        .focus();
    harness.run_steps(2);
    harness.key_press(Key::Enter);
    settle(&mut harness, |h| {
        h.query_by_label("old.prosecraft").is_some()
    });
    assert_eq!(fake.asked(), ["files --app prosecraft-rs --json"]);
    let top = |label: &str| harness.get_by_label(label).rect().top();
    assert!(
        top("Draft.prosecraft") < top("The Glass Orchard.prosecraft")
            && top("The Glass Orchard.prosecraft") < top("old.prosecraft"),
        "newest first"
    );
    // Where each stands with the sync, and the one not on this computer greyed out.
    harness.get_by_label("Waiting to sync");
    harness.get_by_label("Synced");
    harness.get_by_label("Not on this computer");
    assert!(
        harness
            .get_by_label("Draft.prosecraft")
            .accesskit_node()
            .is_disabled()
    );
    harness.get_by_label_contains("ProseCraft-rs · 13 Nov 2026, 21:54 UTC");
    harness
        .get_by_role_and_label(Role::Button, "The Glass Orchard.prosecraft")
        .focus();
    harness.run_steps(2);
    harness.key_press(Key::Enter);
    harness.run_steps(4);
    settle(&mut harness, |h| {
        h.query_by_label("old.prosecraft").is_none()
    });
    let requests = harness.state_mut().take_requests();
    assert!(
        requests
            .iter()
            .any(|r| matches!(r, HostRequest::OpenBackupPath(p)
            if p == "/home/john/CraftCloud/ProseCraft-rs/The Glass Orchard.prosecraft")),
        "opened as Open backup opens a file"
    );
}

#[test]
fn a_listing_that_fails_says_why_and_can_be_tried_again() {
    let fake = Fake::new([
        Output::failed(8, "unreachable", "cloud.example.org: connection refused"),
        Output::ok("[]"),
    ]);
    let mut harness = app(Some(fake.clone()));
    harness
        .get_by_role_and_label(Role::Button, "Open from CraftCloud…")
        .click();
    settle(&mut harness, |h| h.query_by_role(Role::Alert).is_some());
    assert_eq!(
        harness
            .get_by_role(Role::Alert)
            .accesskit_node()
            .label()
            .as_deref(),
        Some("The CraftCloud server can't be reached. Check the connection, then try again.")
    );
    harness
        .get_by_role_and_label(Role::Button, "Try again")
        .click();
    settle(&mut harness, |h| {
        h.query_by_label_contains("no ProseCraft backups in CraftCloud yet")
            .is_some()
    });
    harness.key_press(Key::Escape);
    settle(&mut harness, |h| {
        h.query_by_label("Open from CraftCloud").is_none()
    });
}

/// `save --json` through the desktop client, after it replaced a file.
const SAVED: &str = r#"{"path": "ProseCraft-rs/The Lighthouse 🌊 Keeper.prosecraft",
 "local_path": "/home/john/CraftCloud/ProseCraft-rs/The Lighthouse 🌊 Keeper.prosecraft",
 "via": "client", "state": "synced", "replaced": true, "size": 812}"#;

/// `files --in ProseCraft-rs --json`: the file already there.
const EXISTING: &str = r#"[{"path": "ProseCraft-rs/The Lighthouse 🌊 Keeper.prosecraft",
 "name": "The Lighthouse 🌊 Keeper.prosecraft", "type": "file", "size": 900,
 "modified": "2026-11-13T21:54:00Z", "starred": false,
 "local_path": "/home/john/CraftCloud/ProseCraft-rs/The Lighthouse 🌊 Keeper.prosecraft", "sync": "synced"}]"#;

#[test]
fn save_asks_before_replacing_and_then_forces() {
    let fake = Fake::new([
        Output::failed(
            5,
            "conflict",
            "ProseCraft-rs/The Lighthouse 🌊 Keeper.prosecraft is already in CraftCloud (--force replaces it; the old version is kept)",
        ),
        Output::ok(EXISTING),
        Output::ok(SAVED),
    ]);
    let mut harness = app(Some(fake.clone()));
    open_file_menu(&mut harness);
    harness
        .get_by_role_and_label(Role::MenuItem, "Save to CraftCloud…")
        .click();
    settle(&mut harness, |h| h.query_by_label("Replace?").is_some());
    harness.get_by_label_contains("There is already a The Lighthouse 🌊 Keeper.prosecraft (last changed 13 Nov 2026, 21:54 UTC) in CraftCloud's ProseCraft-rs folder");
    harness.get_by_label_contains("is already in CraftCloud (--force replaces it");
    harness
        .get_by_role_and_label(Role::Button, "Replace")
        .focus();
    harness.run_steps(2);
    harness.key_press(Key::Enter);
    settle(&mut harness, |h| {
        h.state()
            .state()
            .status()
            .is_some_and(|s| s.starts_with("Saved to CraftCloud"))
    });
    assert_eq!(
        harness.state().state().status(),
        Some(
            "Saved to CraftCloud as ProseCraft-rs/The Lighthouse 🌊 Keeper.prosecraft. The one it replaced is kept among CraftCloud's earlier versions."
        )
    );
    assert_eq!(
        fake.asked(),
        [
            "save /tmp/prosecraft-rs-craftcloud-x/The Lighthouse 🌊 Keeper.prosecraft --to ProseCraft-rs --wait --json",
            "files --in ProseCraft-rs --json",
            "save /tmp/prosecraft-rs-craftcloud-x/The Lighthouse 🌊 Keeper.prosecraft --to ProseCraft-rs --wait --json --force",
        ]
    );
    settle(&mut harness, |h| {
        h.query_by_label("Save to CraftCloud").is_none()
    });
}

#[test]
fn a_save_that_fails_says_what_to_do() {
    for (output, want) in [
        (
            Output::failed(3, "signed_out", "not signed in: run `craftcloud login`"),
            "CraftCloud isn't signed in on this computer. Sign in to CraftCloud, then try again.",
        ),
        (
            Output::failed(8, "unreachable", "cloud.example.org: connection refused"),
            "The CraftCloud server can't be reached. Check the connection, then try again.",
        ),
    ] {
        let fake = Fake::new([output]);
        let mut harness = app(Some(fake));
        open_file_menu(&mut harness);
        harness
            .get_by_role_and_label(Role::MenuItem, "Save to CraftCloud…")
            .click();
        settle(&mut harness, |h| h.query_by_role(Role::Alert).is_some());
        assert_eq!(
            harness
                .get_by_role(Role::Alert)
                .accesskit_node()
                .label()
                .as_deref(),
            Some(want)
        );
        harness
            .get_by_role_and_label(Role::Button, "Close Save to CraftCloud")
            .click();
        // A closed window can take a frame or two to leave the tree on a loaded machine.
        settle(&mut harness, |h| h.query_by_role(Role::Alert).is_none());
    }
}

/// `--wait` cut short when the desktop client stops (exit 6): the backup is in the sync
/// folder already, so it isn't a failure.
#[test]
fn a_client_that_stops_mid_upload_is_not_a_failure() {
    let fake = Fake::new([Output::failed(
        6,
        "no_client",
        "the desktop client stopped before the upload",
    )]);
    let mut harness = app(Some(fake));
    open_file_menu(&mut harness);
    harness
        .get_by_role_and_label(Role::MenuItem, "Save to CraftCloud…")
        .click();
    settle(&mut harness, |h| {
        h.state()
            .state()
            .status()
            .is_some_and(|s| s.contains("is in your CraftCloud folder"))
    });
    assert!(harness.query_by_role(Role::Alert).is_none());
}

/// A save goes on in the background: Escape puts its window aside, the menus stay usable, and
/// the status line says how it ended, a failure too.
#[test]
fn a_save_put_aside_says_how_it_ended() {
    for (answer, says) in [
        (
            Output::ok(SAVED),
            "Saved to CraftCloud as ProseCraft-rs/The Lighthouse",
        ),
        (
            Output::failed(8, "unreachable", "cloud.example.org: connection refused"),
            "The CraftCloud server can't be reached.",
        ),
    ] {
        let mut harness = app(Some(Fake::slow([answer])));
        open_file_menu(&mut harness);
        harness
            .get_by_role_and_label(Role::MenuItem, "Save to CraftCloud…")
            .click();
        harness.run_steps(2);
        harness.get_by_role_and_label(Role::Button, "Continue in background");
        harness.key_press(Key::Escape);
        settle(&mut harness, |h| {
            h.query_by_label("Continue in background").is_none()
        });
        // The menus aren't held while it saves.
        open_file_menu(&mut harness);
        assert!(
            !harness
                .get_by_role_and_label(Role::MenuItem, "Open from CraftCloud…")
                .accesskit_node()
                .is_disabled()
        );
        harness.key_press(Key::Escape);
        settle(&mut harness, |h| {
            h.state().state().status().is_some_and(|s| s.contains(says))
        });
        assert!(harness.query_by_role(Role::Alert).is_none());
    }
}

/// `save --wait` that found the file changed in the cloud too: CraftCloud kept both, so it is
/// saved; "Replace?" (and `--force`) is never offered.
#[test]
fn a_save_kept_beside_a_cloud_change_is_reported_as_saved() {
    let fake = Fake::new([Output::failed(
        5,
        "conflict",
        "ProseCraft-rs/The Lighthouse 🌊 Keeper.prosecraft changed in the cloud too: both versions were kept",
    )]);
    let mut harness = app(Some(fake.clone()));
    open_file_menu(&mut harness);
    harness
        .get_by_role_and_label(Role::MenuItem, "Save to CraftCloud…")
        .click();
    settle(&mut harness, |h| {
        h.state()
            .state()
            .status()
            .is_some_and(|s| s.starts_with("Saved, but the file had changed in the cloud too"))
    });
    assert!(harness.query_by_label("Replace?").is_none());
    assert_eq!(fake.asked().len(), 1);
}
