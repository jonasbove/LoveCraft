//! The `craftcloud` program's answers, as its documentation records them (CraftCloud's
//! `docs/CRAFT-INTEGRATION.md`, section 3, and `docs/CLI.md`, "JSON output" and "Exit codes"),
//! given by a fake runner.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use super::{
    Failure, Output, Runner, SyncState, backup_file_name, device_sync_folder, is_backup_name,
    list_backups, save_backup, sync_folder,
};
use crate::sync::SyncMode;

/// Answers in order, and remembers what was asked.
#[derive(Default)]
struct Fake {
    answers: Mutex<VecDeque<Result<Output, String>>>,
    asked: Mutex<Vec<String>>,
    staged: Mutex<Vec<(String, Vec<u8>)>>,
    discarded: Mutex<Vec<String>>,
    timeouts: Mutex<Vec<Duration>>,
}

impl Fake {
    fn new(answers: impl IntoIterator<Item = Output>) -> Self {
        Self {
            answers: Mutex::new(answers.into_iter().map(Ok).collect()),
            ..Self::default()
        }
    }
    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl Runner for Fake {
    fn run(&self, args: &[&str], timeout: Duration) -> Result<Output, String> {
        self.asked.lock().unwrap().push(args.join(" "));
        self.timeouts.lock().unwrap().push(timeout);
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err("no answer left".into()))
    }
    fn stage(&self, name: &str, bytes: &[u8]) -> Result<String, String> {
        self.staged
            .lock()
            .unwrap()
            .push((name.to_owned(), bytes.to_vec()));
        Ok(format!("/tmp/lovecraft-x/{name}"))
    }
    fn discard(&self, path: &str) {
        self.discarded.lock().unwrap().push(path.to_owned());
    }
}

/// `files --app … --json` from a running client (its answer has no `id`), and from the
/// server (with `id`; a file not on this computer has `local_path: null`).
const FILES: &str = r#"[
 {"path": "Backups/old.prosecraft", "name": "old.prosecraft", "type": "file", "size": 812,
  "modified": "2026-10-01T08:02:11Z", "starred": false,
  "local_path": "/home/john/CraftCloud/Backups/old.prosecraft", "sync": "synced"},
 {"path": "LoveCraft/The Glass Orchard.prosecraft", "name": "The Glass Orchard.prosecraft",
  "type": "file", "size": 18234, "modified": "2026-11-13T21:54:00Z", "starred": false,
  "local_path": "/home/john/CraftCloud/LoveCraft/The Glass Orchard.prosecraft", "sync": "pending"},
 {"path": "Shared/Draft.PROSECRAFT", "name": "Draft.PROSECRAFT", "type": "file", "size": 9,
  "modified": "2026-11-14T08:02:11+01:00", "starred": true, "id": "f7fc37c9-0000",
  "local_path": null, "sync": "excluded"}
]"#;

/// `files --json` with no app: everything, folders too.
const EVERYTHING: &str = r#"[
 {"path": "Documents", "name": "Documents", "type": "folder", "size": null,
  "modified": "2026-11-13T21:53:39Z", "starred": false, "id": "d1"},
 {"path": "Documents/notes.md", "name": "notes.md", "type": "file", "size": 11,
  "modified": "2026-11-13T21:53:39Z", "starred": false,
  "local_path": "/home/john/CraftCloud/Documents/notes.md", "sync": "synced"},
 {"path": "LoveCraft/Book.prosecraft", "name": "Book.prosecraft", "type": "file", "size": 99,
  "modified": "2026-11-15T10:00:00Z", "starred": false,
  "local_path": "/home/john/CraftCloud/LoveCraft/Book.prosecraft", "sync": "synced"},
 {"path": "LoveCraft/.prosecraft", "name": ".prosecraft", "type": "file", "size": 1,
  "modified": "2026-11-15T10:00:00Z", "starred": false},
 {"path": "LoveCraft/Encrypted sync file/prosecraft-sync.prosecraft", "name": "prosecraft-sync.prosecraft",
  "type": "file", "size": 1, "modified": "2026-11-16T10:00:00Z", "starred": false,
  "local_path": "/home/john/CraftCloud/LoveCraft/Encrypted sync file/prosecraft-sync.prosecraft", "sync": "synced"}
]"#;

#[test]
fn backups_are_listed_newest_first_with_where_they_stand() {
    let fake = Fake::new([Output::ok(FILES)]);
    let files = list_backups(&fake).expect("files");
    assert_eq!(fake.asked(), ["files --app prosecraft-rs --json"]);
    let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Draft.PROSECRAFT",
            "The Glass Orchard.prosecraft",
            "old.prosecraft"
        ],
        "newest first: 08:02+01:00 on the 14th is after 21:54Z on the 13th"
    );
    assert_eq!(files[1].sync, SyncState::Pending);
    assert_eq!(files[1].sync.badge(), "Waiting to sync");
    assert_eq!(files[1].folder(), "LoveCraft");
    assert!(files[1].is_here());
    assert_eq!(files[0].local_path, None);
    assert!(!files[0].is_here());
    assert_eq!(files[0].sync, SyncState::Excluded);
    assert_eq!(files[2].size, Some(812));
}

#[test]
fn a_craftcloud_without_the_app_lists_everything_and_the_backups_are_kept_here() {
    let fake = Fake::new([
        Output::failed(
            2,
            "usage",
            "--app prosecraft-rs: use one of wordcraft, gridcraft",
        ),
        Output::ok(EVERYTHING),
    ]);
    let files = list_backups(&fake).expect("files");
    assert_eq!(
        fake.asked(),
        ["files --app prosecraft-rs --json", "files --json"]
    );
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "LoveCraft/Book.prosecraft");
    assert_eq!(files[0].sync, SyncState::Synced);
}

#[test]
fn each_exit_code_says_what_to_do() {
    let cases = [
        (
            Output::failed(3, "signed_out", "not signed in: run `craftcloud login`"),
            "CraftCloud isn't signed in on this computer. Sign in to CraftCloud, then try again.",
        ),
        (
            Output::failed(8, "unreachable", "cloud.example.org: connection refused"),
            "The CraftCloud server can't be reached. Check the connection, then try again.",
        ),
        (
            Output::failed(7, "forbidden", "not allowed"),
            "The CraftCloud server refused. Not allowed.",
        ),
        (
            Output {
                code: Some(1),
                stderr: "craftcloud: something broke\n".into(),
                ..Output::default()
            },
            "CraftCloud didn't manage it. Something broke.",
        ),
        (
            Output {
                code: None,
                ..Output::default()
            },
            "CraftCloud didn't manage it. CraftCloud was stopped before it answered.",
        ),
        (
            Output {
                timed_out: true,
                ..Output::default()
            },
            "CraftCloud didn't answer in time. Check that the CraftCloud desktop client is running, then try again.",
        ),
    ];
    for (output, want) in cases {
        let fake = Fake::new([output]);
        assert_eq!(list_backups(&fake).unwrap_err().message(), want);
    }
    let fake = Fake::default();
    assert!(matches!(
        list_backups(&fake),
        Err(Failure::NotStarted(why)) if why == "no answer left"
    ));
    let fake = Fake::new([Output::ok("not json")]);
    assert_eq!(
        list_backups(&fake).unwrap_err().message(),
        "CraftCloud didn't manage it. CraftCloud's answer couldn't be read."
    );
}

/// `save --json` through the desktop client, with `--wait`.
const SAVED: &str = r#"{"path": "LoveCraft/The Glass Orchard.prosecraft",
 "local_path": "/home/john/CraftCloud/LoveCraft/The Glass Orchard.prosecraft",
 "via": "client", "state": "synced", "replaced": true, "size": 812}"#;

#[test]
fn a_save_goes_through_a_temporary_file_named_for_the_backup() {
    let fake = Fake::new([
        Output::failed(
            5,
            "conflict",
            "LoveCraft/The Glass Orchard.prosecraft is already in CraftCloud (--force replaces it; the old version is kept)",
        ),
        Output::ok(FILES),
        Output::ok(SAVED),
    ]);
    let refused = save_backup(
        &fake,
        "The Glass Orchard.prosecraft",
        b"{}",
        "LoveCraft",
        false,
    )
    .unwrap_err();
    assert!(
        matches!(&refused, Failure::Exists { message, modified }
            if message.contains("is already in CraftCloud")
                && modified.as_deref() == Some("2026-11-13T21:54:00Z")),
        "{refused:?}: with when the file there last changed"
    );
    let saved = save_backup(
        &fake,
        "The Glass Orchard.prosecraft",
        b"{}",
        "LoveCraft",
        true,
    )
    .expect("saved");
    assert_eq!(
        fake.asked(),
        [
            "save /tmp/lovecraft-x/The Glass Orchard.prosecraft --to LoveCraft --wait --json",
            "files --in LoveCraft --json",
            "save /tmp/lovecraft-x/The Glass Orchard.prosecraft --to LoveCraft --wait --json --force",
        ]
    );
    let timeouts = fake.timeouts.lock().unwrap().clone();
    assert_eq!(timeouts, [super::UPLOAD, super::QUICK, super::UPLOAD]);
    assert!(saved.replaced);
    assert_eq!(saved.state, "synced");
    assert_eq!(saved.path, "LoveCraft/The Glass Orchard.prosecraft");
    assert_eq!(
        fake.discarded.lock().unwrap().len(),
        2,
        "the temporary file goes after every try"
    );
    assert_eq!(fake.staged.lock().unwrap()[0].1, b"{}");
}

/// `save --wait` that placed the file and then found it changed in the cloud too (CraftCloud's
/// `wait_uploaded`): saved, as a conflict copy. Never a "Replace?" with `--force`.
#[test]
fn a_save_kept_beside_a_cloud_change_is_saved_not_refused() {
    let fake = Fake::new([Output::failed(
        5,
        "conflict",
        "LoveCraft/The Glass Orchard.prosecraft changed in the cloud too: both versions were kept",
    )]);
    let result = save_backup(
        &fake,
        "The Glass Orchard.prosecraft",
        b"{}",
        "LoveCraft",
        false,
    );
    assert!(matches!(&result, Err(Failure::BothKept(_))), "{result:?}");
    assert_eq!(fake.asked().len(), 1, "nothing more is asked");
    let fake = Fake::new([Output::failed(
        5,
        "conflict",
        "LoveCraft/x.prosecraft is a folder",
    )]);
    let result = save_backup(&fake, "x.prosecraft", b"{}", "LoveCraft", false);
    assert!(matches!(&result, Err(Failure::Conflict(_))), "{result:?}");
    assert_eq!(fake.discarded.lock().unwrap().len(), 1);
}

#[test]
fn the_sync_folder_is_the_running_clients() {
    let status = r#"{"syncing": true, "server": "https://cloud.example.org", "local_folder": "/home/john/CraftCloud",
 "state": "up_to_date", "files_done": 0, "files_total": 0, "cycles": 12, "last_synced": 1794606853,
 "conflicts": [], "activity": []}"#;
    let fake = Fake::new([Output::ok(status)]);
    assert_eq!(sync_folder(&fake).unwrap(), "/home/john/CraftCloud");
    assert_eq!(fake.asked(), ["status --json"]);
    // Exit 9 for places that can't be read: the folder is there and usable.
    let held = Output {
        code: Some(9),
        stdout: status.to_owned(),
        ..Output::default()
    };
    assert_eq!(
        sync_folder(&Fake::new([held])).unwrap(),
        "/home/john/CraftCloud"
    );
    // Held for the person: nothing is to be written there now.
    for (state, says) in [
        ("mass_delete", "large delete"),
        ("folder_missing", "missing"),
    ] {
        let held = Output {
            code: Some(9),
            stdout: status.replace("up_to_date", state),
            ..Output::default()
        };
        let refused = sync_folder(&Fake::new([held])).unwrap_err();
        assert!(matches!(refused, Failure::Held(_)), "{state}");
        assert!(refused.message().contains(says), "{}", refused.message());
    }
    let signed_out = r#"{"syncing": false, "local_folder": "", "state": "signed_out"}"#;
    assert_eq!(
        sync_folder(&Fake::new([Output::ok(signed_out)])),
        Err(Failure::SignedOut)
    );
    let none = Output::failed(6, "no_client", "the desktop client isn't running");
    assert_eq!(
        sync_folder(&Fake::new([none])).unwrap_err().message(),
        "The CraftCloud desktop client isn't running. Start CraftCloud, then try again."
    );
}

#[test]
fn the_0_1_sync_file_is_looked_for_in_the_cloud() {
    let listing = r#"[{"path": "ProseCraft/prosecraft-sync.prosecraft", "name": "prosecraft-sync.prosecraft",
 "type": "file", "size": 9, "modified": "2026-11-01T08:00:00Z", "starred": false, "id": "f1",
 "local_path": null, "sync": "excluded"}]"#;
    let fake = Fake::new([Output::ok(listing)]);
    assert!(super::legacy_sync_file_in_cloud(&fake));
    assert_eq!(fake.asked(), ["ls ProseCraft --json"]);
    let missing = Output::failed(4, "not_found", "ProseCraft: no such file or folder");
    assert!(!super::legacy_sync_file_in_cloud(&Fake::new([missing])));
    let root = |p: &str| super::craftcloud_root_of(p).map(std::path::PathBuf::from);
    let path = |p: &str| Some(std::path::PathBuf::from(p));
    assert_eq!(root("/c/LoveCraft/Open sync folder"), path("/c"));
    assert_eq!(root("/c/ProseCraft"), path("/c"));
    assert_eq!(root("/c/Other/Open sync folder"), None);
    assert_eq!(root("/home/me/sync"), None);
}

#[test]
fn backup_names() {
    assert!(is_backup_name("a.prosecraft") && is_backup_name("A.ProseCraft"));
    assert!(!is_backup_name(".prosecraft") && !is_backup_name("a.prosecraft.md"));
    assert_eq!(
        backup_file_name("The Glass Orchard"),
        "The Glass Orchard.prosecraft"
    );
    assert_eq!(backup_file_name("A/B: c?"), "A-B- c-.prosecraft");
    assert_eq!(backup_file_name(" .. "), "Untitled project.prosecraft");
    assert_eq!(backup_file_name(""), "Untitled project.prosecraft");
    // Long names fit a file system's 255 bytes, cut between characters.
    let long = backup_file_name(&"灯台守の物語".repeat(30));
    assert!(long.len() <= 211, "{} bytes", long.len());
    assert!(std::str::from_utf8(long.as_bytes()).is_ok());
    // Names Windows or Office keep for themselves.
    for (name, want) in [
        ("con", "con (project).prosecraft"),
        ("NUL", "NUL (project).prosecraft"),
        ("Com7", "Com7 (project).prosecraft"),
        ("lpt1.draft", "lpt1.draft (project).prosecraft"),
        ("~$draft", "-$draft.prosecraft"),
        ("COM0", "COM0.prosecraft"),
        ("Console", "Console.prosecraft"),
    ] {
        assert_eq!(backup_file_name(name), want, "{name}");
    }
}

#[test]
fn device_sync_has_a_folder_for_each_method() {
    let at = |mode, legacy| device_sync_folder("/home/john/CraftCloud", mode, legacy);
    // Compared as paths: on Windows the parts are joined with `\`.
    let path = |p: &str| std::path::PathBuf::from(p);
    assert_eq!(
        path(&at(SyncMode::OpenFolder, false)),
        path("/home/john/CraftCloud/LoveCraft/Open sync folder")
    );
    assert_eq!(
        path(&at(SyncMode::EncryptedFile, false)),
        path("/home/john/CraftCloud/LoveCraft/Encrypted sync file")
    );
    assert_eq!(
        path(&at(SyncMode::EncryptedFolder, true)),
        path("/home/john/CraftCloud/LoveCraft/Encrypted sync folder")
    );
    assert_eq!(
        path(&at(SyncMode::EncryptedFile, true)),
        path("/home/john/CraftCloud/ProseCraft"),
        "0.1's folder, once it has the sync file"
    );
}
