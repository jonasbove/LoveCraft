//! The `craftcloud` program's answers, as its documentation records them (CraftCloud's
//! `docs/CRAFT-INTEGRATION.md`, section 3, and `docs/CLI.md`, "JSON output" and "Exit codes"),
//! given by a fake runner.

use std::collections::VecDeque;
use std::sync::Mutex;

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
    fn run(&self, args: &[&str]) -> Result<Output, String> {
        self.asked.lock().unwrap().push(args.join(" "));
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
        Ok(format!("/tmp/prosecraft-rs-x/{name}"))
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
 {"path": "ProseCraft-rs/The Glass Orchard.prosecraft", "name": "The Glass Orchard.prosecraft",
  "type": "file", "size": 18234, "modified": "2026-11-13T21:54:00Z", "starred": false,
  "local_path": "/home/john/CraftCloud/ProseCraft-rs/The Glass Orchard.prosecraft", "sync": "pending"},
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
 {"path": "ProseCraft-rs/Book.prosecraft", "name": "Book.prosecraft", "type": "file", "size": 99,
  "modified": "2026-11-15T10:00:00Z", "starred": false,
  "local_path": "/home/john/CraftCloud/ProseCraft-rs/Book.prosecraft", "sync": "synced"},
 {"path": "ProseCraft-rs/.prosecraft", "name": ".prosecraft", "type": "file", "size": 1,
  "modified": "2026-11-15T10:00:00Z", "starred": false},
 {"path": "ProseCraft-rs/Encrypted sync file/prosecraft-sync.prosecraft", "name": "prosecraft-sync.prosecraft",
  "type": "file", "size": 1, "modified": "2026-11-16T10:00:00Z", "starred": false,
  "local_path": "/home/john/CraftCloud/ProseCraft-rs/Encrypted sync file/prosecraft-sync.prosecraft", "sync": "synced"}
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
    assert_eq!(files[1].folder(), "ProseCraft-rs");
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
    assert_eq!(files[0].path, "ProseCraft-rs/Book.prosecraft");
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
                stdout: String::new(),
                stderr: "craftcloud: something broke\n".into(),
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
const SAVED: &str = r#"{"path": "ProseCraft-rs/The Glass Orchard.prosecraft",
 "local_path": "/home/john/CraftCloud/ProseCraft-rs/The Glass Orchard.prosecraft",
 "via": "client", "state": "synced", "replaced": true, "size": 812}"#;

#[test]
fn a_save_goes_through_a_temporary_file_named_for_the_backup() {
    let fake = Fake::new([
        Output::failed(
            5,
            "conflict",
            "ProseCraft-rs/The Glass Orchard.prosecraft is already in CraftCloud (--force replaces it; the old version is kept)",
        ),
        Output::ok(SAVED),
    ]);
    let refused = save_backup(
        &fake,
        "The Glass Orchard.prosecraft",
        b"{}",
        "ProseCraft-rs",
        false,
    )
    .unwrap_err();
    assert!(matches!(&refused, Failure::Exists(why) if why.contains("is already in CraftCloud")));
    let saved = save_backup(
        &fake,
        "The Glass Orchard.prosecraft",
        b"{}",
        "ProseCraft-rs",
        true,
    )
    .expect("saved");
    assert_eq!(
        fake.asked(),
        [
            "save /tmp/prosecraft-rs-x/The Glass Orchard.prosecraft --to ProseCraft-rs --wait --json",
            "save /tmp/prosecraft-rs-x/The Glass Orchard.prosecraft --to ProseCraft-rs --wait --json --force",
        ]
    );
    assert!(saved.replaced);
    assert_eq!(saved.state, "synced");
    assert_eq!(saved.path, "ProseCraft-rs/The Glass Orchard.prosecraft");
    assert_eq!(
        fake.discarded.lock().unwrap().len(),
        2,
        "the temporary file goes after every try"
    );
    assert_eq!(fake.staged.lock().unwrap()[0].1, b"{}");
}

#[test]
fn the_sync_folder_is_the_running_clients() {
    let status = r#"{"syncing": true, "server": "https://cloud.example.org", "local_folder": "/home/john/CraftCloud",
 "state": "up_to_date", "files_done": 0, "files_total": 0, "cycles": 12, "last_synced": 1794606853,
 "conflicts": [], "activity": []}"#;
    let fake = Fake::new([Output::ok(status)]);
    assert_eq!(sync_folder(&fake).unwrap(), "/home/john/CraftCloud");
    assert_eq!(fake.asked(), ["status --json"]);
    // Held (exit 9): it still says where the folder is.
    let held = Output {
        code: Some(9),
        stdout: status.replace("up_to_date", "mass_delete"),
        stderr: String::new(),
    };
    assert_eq!(
        sync_folder(&Fake::new([held])).unwrap(),
        "/home/john/CraftCloud"
    );
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
}

#[test]
fn device_sync_has_a_folder_for_each_method() {
    let at = |mode, legacy| device_sync_folder("/home/john/CraftCloud", mode, legacy);
    // Compared as paths: on Windows the parts are joined with `\`.
    let path = |p: &str| std::path::PathBuf::from(p);
    assert_eq!(
        path(&at(SyncMode::OpenFolder, false)),
        path("/home/john/CraftCloud/ProseCraft-rs/Open sync folder")
    );
    assert_eq!(
        path(&at(SyncMode::EncryptedFile, false)),
        path("/home/john/CraftCloud/ProseCraft-rs/Encrypted sync file")
    );
    assert_eq!(
        path(&at(SyncMode::EncryptedFolder, true)),
        path("/home/john/CraftCloud/ProseCraft-rs/Encrypted sync folder")
    );
    assert_eq!(
        path(&at(SyncMode::EncryptedFile, true)),
        path("/home/john/CraftCloud/ProseCraft"),
        "0.1's folder, once it has the sync file"
    );
}
