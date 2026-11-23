//! Sync folders checked against what ProseCraft's own providers write and decide.

use prosecraft_core::js::{self, JsString, JsValue};
use prosecraft_core::storage::crypto::{OsRandom, VaultOptions};
use prosecraft_core::sync::layout::safe_name;
use prosecraft_core::sync::memory::MemoryFs;
use prosecraft_core::sync::{
    EncryptedFileProvider, FolderProvider, SyncProvider, merge_projects, sync_via,
};

fn fixture() -> JsValue {
    let path = format!("{}/tests/fixtures/sync.json", env!("CARGO_MANIFEST_DIR"));
    js::parse(&std::fs::read_to_string(&path).expect("fixture")).expect("json")
}

fn text(value: &JsValue) -> String {
    value
        .as_js_string()
        .and_then(JsString::as_string)
        .expect("string")
}

fn files(value: &JsValue) -> MemoryFs {
    MemoryFs::with_files(
        value
            .as_object()
            .expect("files")
            .iter()
            .map(|(k, v)| (k.to_string_lossy(), text(v))),
    )
}

fn passphrase(f: &JsValue) -> JsString {
    f.get("passphrase")
        .and_then(JsValue::as_js_string)
        .cloned()
        .expect("passphrase")
}

fn projects(f: &JsValue) -> Vec<JsValue> {
    f.get("projects")
        .and_then(JsValue::as_array)
        .expect("projects")
        .to_vec()
}

/// The same projects by content (`stableStringify`): reading a folder puts the record lists
/// after the project's other fields, as ProseCraft's `readProjectFiles` does.
fn same_content(a: &[JsValue], b: &[JsValue]) {
    let s = |v: &[JsValue]| v.iter().map(js::stable_stringify).collect::<Vec<_>>();
    assert_eq!(s(a), s(b));
}

fn same(a: &[JsValue], b: &[JsValue]) {
    let s = |v: &[JsValue]| v.iter().map(js::stringify).collect::<Vec<_>>();
    assert_eq!(s(a), s(b));
}

fn quick() -> VaultOptions {
    VaultOptions {
        memory_kib: Some(64),
        iterations: Some(1),
        parallelism: Some(1),
        ..VaultOptions::default()
    }
}

#[test]
fn reads_the_encrypted_folder_prosecraft_writes() {
    let f = fixture();
    let mut fs = files(f.get("folder").expect("folder"));
    let connection = FolderProvider::new(&mut fs)
        .connect(&passphrase(&f), &mut OsRandom)
        .expect("connect");
    assert!(!connection.created);
    // pull sorts by id: "proj/../x é" before "project-1"? Compare as ProseCraft sorts.
    let mut want = projects(&f);
    want.sort_by_key(|p| {
        p.get("id")
            .and_then(JsValue::as_js_string)
            .map(|s| s.units().to_vec())
    });
    same_content(&connection.projects, &want);
}

#[test]
fn reads_the_single_file_prosecraft_writes() {
    let f = fixture();
    let mut fs = files(f.get("singleFile").expect("file"));
    let connection = EncryptedFileProvider::new(&mut fs, 0.0)
        .connect(&passphrase(&f), &mut OsRandom)
        .expect("connect");
    same(&connection.projects, &projects(&f));
}

#[test]
fn writes_the_folder_layout_prosecraft_reads() {
    let f = fixture();
    let want_paths: Vec<String> = f
        .get("folder")
        .and_then(JsValue::as_object)
        .expect("folder")
        .iter()
        .map(|(k, _)| k.to_string_lossy())
        .collect();
    let mut fs = MemoryFs::new();
    {
        let mut provider = FolderProvider::new(&mut fs).with_vault_options(quick());
        let connection = provider
            .connect(&passphrase(&f), &mut OsRandom)
            .expect("create");
        assert!(connection.created);
        provider
            .push(&projects(&f), connection.key.as_ref(), &mut OsRandom)
            .expect("push");
    }
    let mut got_paths: Vec<String> = fs.files().keys().cloned().collect();
    let mut want_sorted = want_paths.clone();
    got_paths.sort();
    want_sorted.sort();
    assert_eq!(got_paths, want_sorted);
    let connection = FolderProvider::new(&mut fs)
        .connect(&passphrase(&f), &mut OsRandom)
        .expect("reopen");
    let mut want = projects(&f);
    want.sort_by_key(|p| {
        p.get("id")
            .and_then(JsValue::as_js_string)
            .map(|s| s.units().to_vec())
    });
    same_content(&connection.projects, &want);
}

#[test]
fn writes_the_single_file_and_reads_it_back() {
    let f = fixture();
    let mut fs = MemoryFs::new();
    {
        let mut provider =
            EncryptedFileProvider::new(&mut fs, 1_800_000_000_000.0).with_vault_options(quick());
        let connection = provider
            .connect(&passphrase(&f), &mut OsRandom)
            .expect("create");
        provider
            .push(&projects(&f), connection.key.as_ref(), &mut OsRandom)
            .expect("push");
    }
    assert_eq!(
        fs.files().keys().collect::<Vec<_>>(),
        ["prosecraft-sync.prosecraft"],
        "no temporary file is left"
    );
    let connection = EncryptedFileProvider::new(&mut fs, 0.0)
        .connect(&passphrase(&f), &mut OsRandom)
        .expect("reopen");
    same(&connection.projects, &projects(&f));
}

#[test]
fn a_folder_of_one_layout_is_refused_by_the_other() {
    let f = fixture();
    let mut fs = files(f.get("folder").expect("folder"));
    let error = EncryptedFileProvider::new(&mut fs, 0.0)
        .connect(&passphrase(&f), &mut OsRandom)
        .err()
        .expect("refused");
    assert_eq!(
        error.to_string(),
        "This folder is set up for encrypted-folder sync, not single-file sync."
    );
    let mut fs = files(f.get("singleFile").expect("file"));
    let error = FolderProvider::new(&mut fs)
        .connect(&passphrase(&f), &mut OsRandom)
        .err()
        .expect("refused");
    assert_eq!(
        error.to_string(),
        "This folder is set up for single-file sync, not encrypted-folder sync."
    );
    // A folder with projects but no vault file isn't adopted.
    let mut folder = files(f.get("folder").expect("folder"));
    prosecraft_core::sync::SyncFileSystem::remove(&mut folder, "vault.json").expect("remove");
    let error = FolderProvider::new(&mut folder)
        .connect(&passphrase(&f), &mut OsRandom)
        .err()
        .expect("refused");
    assert_eq!(
        error.to_string(),
        "This folder contains project files but no vault file."
    );
}

#[test]
fn safe_names_match_prosecraft() {
    for case in fixture()
        .get("safeNames")
        .and_then(JsValue::as_array)
        .expect("cases")
    {
        let id = text(case.get("id").expect("id"));
        assert_eq!(
            safe_name(&id),
            text(case.get("name").expect("name")),
            "{id:?}"
        );
    }
}

#[test]
fn merges_match_prosecraft() {
    for (name, case) in fixture()
        .get("merges")
        .and_then(JsValue::as_object)
        .expect("merges")
        .iter()
    {
        let name = name.to_string_lossy();
        let list = |k: &str| case.get(k).and_then(JsValue::as_array).expect(k).to_vec();
        // ProseCraft's Math.random was pinned to 0.5: (0.5).toString(36).slice(2, 7) is "i".
        let outcome = merge_projects(
            &list("local"),
            &list("remote"),
            case.get("lastSynced").expect("synced"),
            5_000_000.0,
            "device-z",
            &mut || "i".to_owned(),
        );
        let want = case.get("outcome").expect("outcome");
        same(
            &outcome.projects,
            want.get("projects")
                .and_then(JsValue::as_array)
                .expect("projects"),
        );
        assert_eq!(
            js::stringify(&outcome.last_synced),
            js::stringify(want.get("lastSynced").expect("synced")),
            "{name}"
        );
        let count = |k: &str| want.get(k).and_then(JsValue::as_number).expect(k) as usize;
        assert_eq!(
            (outcome.imported, outcome.exported, outcome.conflicts),
            (count("imported"), count("exported"), count("conflicts")),
            "{name}"
        );
        let removed: Vec<String> = want
            .get("removedIds")
            .and_then(JsValue::as_array)
            .expect("removed")
            .iter()
            .map(text)
            .collect();
        assert_eq!(outcome.removed_ids, removed, "{name}");
    }
}

#[test]
fn two_devices_sync_through_one_folder() {
    let f = fixture();
    let pass = passphrase(&f);
    let mut fs = MemoryFs::new();
    let none = JsValue::Object(Default::default());
    let mut suffix = || "x".to_owned();
    // Device A sets up the folder with its projects.
    let a = sync_via(
        &mut FolderProvider::new(&mut fs).with_vault_options(quick()),
        &pass,
        &projects(&f),
        &none,
        1.0,
        "device-a",
        &mut OsRandom,
        &mut suffix,
    )
    .expect("a");
    assert!(a.created && a.exported == 2);
    // Device B, empty, receives them.
    let b = sync_via(
        &mut FolderProvider::new(&mut fs),
        &pass,
        &[],
        &none,
        2.0,
        "device-b",
        &mut OsRandom,
        &mut suffix,
    )
    .expect("b");
    assert_eq!((b.imported, b.projects.len()), (2, 2));
    // B deletes one project; A's next sync removes it from the folder and from A.
    let kept: Vec<JsValue> = b
        .projects
        .iter()
        .filter(|p| {
            p.get("id")
                .and_then(JsValue::as_js_string)
                .is_some_and(|i| i.eq_str("project-1"))
        })
        .cloned()
        .collect();
    let b2 = sync_via(
        &mut FolderProvider::new(&mut fs),
        &pass,
        &kept,
        &b.last_synced,
        3.0,
        "device-b",
        &mut OsRandom,
        &mut suffix,
    )
    .expect("b2");
    assert_eq!(b2.removed, 1);
    let a2 = sync_via(
        &mut FolderProvider::new(&mut fs),
        &pass,
        &a.projects,
        &a.last_synced,
        4.0,
        "device-a",
        &mut OsRandom,
        &mut suffix,
    )
    .expect("a2");
    assert_eq!(a2.projects.len(), 1);
    assert!(
        !fs.files().keys().any(|p| p.starts_with("projects/~")),
        "the deleted project's folder is gone"
    );
}

/// Writes both layouts to `PROSECRAFT_INTEROP_DIR` for ProseCraft's own providers to read
/// (the interop check in the fixture generator). Run with `--ignored`.
#[test]
#[ignore = "writes files for the ProseCraft interop check"]
fn write_layouts_for_prosecraft() {
    let Some(dir) = std::env::var_os("PROSECRAFT_INTEROP_DIR") else {
        return;
    };
    let f = fixture();
    for (name, folder) in [("folder", true), ("single-file", false)] {
        let mut fs = MemoryFs::new();
        if folder {
            let mut provider = FolderProvider::new(&mut fs).with_vault_options(quick());
            let c = provider
                .connect(&passphrase(&f), &mut OsRandom)
                .expect("create");
            provider
                .push(&projects(&f), c.key.as_ref(), &mut OsRandom)
                .expect("push");
        } else {
            let mut provider = EncryptedFileProvider::new(&mut fs, 1_800_000_000_000.0)
                .with_vault_options(quick());
            let c = provider
                .connect(&passphrase(&f), &mut OsRandom)
                .expect("create");
            provider
                .push(&projects(&f), c.key.as_ref(), &mut OsRandom)
                .expect("push");
        }
        let mut out = prosecraft_core::js::JsObject::new();
        for (path, content) in fs.files() {
            out.insert(path.as_str(), JsValue::str(content));
        }
        std::fs::write(
            std::path::Path::new(&dir).join(format!("{name}.json")),
            js::stringify(&JsValue::Object(out)),
        )
        .expect("write");
    }
}

#[test]
fn pruning_keeps_files_the_folder_did_not_write() {
    use prosecraft_core::sync::SyncFileSystem;
    let f = fixture();
    let mut fs = files(f.get("folder").expect("folder"));
    let original = fs
        .files()
        .get("projects/project-1/documents/doc-1.enc")
        .cloned()
        .expect("doc-1");
    // Another sync tool's conflict copies repeat doc-1's envelope under other names; a stray
    // file isn't an envelope at all.
    for name in [
        "doc-1 (conflict 2026-11-17 10.00 Laptop).enc",
        "doc-1.sync-conflict-20261117-100000-ABC.enc",
    ] {
        fs.write_text(&format!("projects/project-1/documents/{name}"), &original)
            .expect("copy");
    }
    fs.write_text("projects/project-1/documents/notes.enc", "not an envelope")
        .expect("stray");
    // A record deleted since: its canonical file is ours to remove.
    fs.write_text(
        "projects/project-1/documents/doc-9.enc",
        &original.replace("\"id\": \"doc-1\"", "\"id\": \"doc-9\""),
    )
    .expect("old");
    let mut provider = FolderProvider::new(&mut fs);
    let connection = provider
        .connect(&passphrase(&f), &mut OsRandom)
        .expect("connect");
    provider
        .push(&connection.projects, connection.key.as_ref(), &mut OsRandom)
        .expect("push");
    let names: Vec<&str> = fs
        .files()
        .keys()
        .filter_map(|k| k.strip_prefix("projects/project-1/documents/"))
        .collect();
    assert_eq!(
        names,
        [
            "doc-1 (conflict 2026-11-17 10.00 Laptop).enc",
            "doc-1.enc",
            "doc-1.sync-conflict-20261117-100000-ABC.enc",
            "doc-2.enc",
            "doc-3.enc",
            "notes.enc"
        ],
        "conflict copies and stray files stay; doc-9.enc, written by this layout for a record no longer live, goes"
    );
}

#[test]
fn conflict_copies_are_reported_not_removed() {
    use prosecraft_core::sync::SyncFileSystem;
    let f = fixture();
    let none = JsValue::Object(Default::default());
    // The single file: a conflict copy beside it.
    let mut fs = files(f.get("singleFile").expect("file"));
    let bundle = fs
        .files()
        .get("prosecraft-sync.prosecraft")
        .cloned()
        .expect("bundle");
    fs.write_text(
        "prosecraft-sync (conflict 2026-11-17 10.00 Laptop).prosecraft",
        &bundle,
    )
    .expect("copy");
    let result = sync_via(
        &mut EncryptedFileProvider::new(&mut fs, 1.0),
        &passphrase(&f),
        &[],
        &none,
        1.0,
        "d",
        &mut OsRandom,
        &mut || "x".to_owned(),
    )
    .expect("sync");
    assert_eq!(
        result.conflict_files,
        ["prosecraft-sync (conflict 2026-11-17 10.00 Laptop).prosecraft"]
    );
    assert!(
        fs.files()
            .contains_key("prosecraft-sync (conflict 2026-11-17 10.00 Laptop).prosecraft")
    );
    // The folder: a copy of a scene file.
    let mut fs = files(f.get("folder").expect("folder"));
    let original = fs
        .files()
        .get("projects/project-1/documents/doc-1.enc")
        .cloned()
        .expect("doc-1");
    fs.write_text(
        "projects/project-1/documents/doc-1 (conflict 2026-11-17 10.00 Laptop).enc",
        &original,
    )
    .expect("copy");
    let result = sync_via(
        &mut FolderProvider::new(&mut fs),
        &passphrase(&f),
        &[],
        &none,
        1.0,
        "d",
        &mut OsRandom,
        &mut || "x".to_owned(),
    )
    .expect("sync");
    assert_eq!(
        result.conflict_files,
        ["projects/project-1/documents/doc-1 (conflict 2026-11-17 10.00 Laptop).enc"]
    );
}

mod open_folder {
    use super::{files, text};
    use prosecraft_core::js::{self, JsString, JsValue};
    use prosecraft_core::storage::crypto::OsRandom;
    use prosecraft_core::sync::memory::MemoryFs;
    use prosecraft_core::sync::open_layout::{normalize_document, read_open_projects};
    use prosecraft_core::sync::{OpenFolderProvider, SyncProvider, sync_via};

    fn fixture() -> JsValue {
        let path = format!(
            "{}/tests/fixtures/sync-open.json",
            env!("CARGO_MANIFEST_DIR")
        );
        js::parse(&std::fs::read_to_string(&path).expect("fixture")).expect("json")
    }

    fn list(f: &JsValue, key: &str) -> Vec<JsValue> {
        f.get(key)
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .to_vec()
    }

    fn json(values: &[JsValue]) -> String {
        js::stringify(&JsValue::Array(values.to_vec()))
    }

    fn same_files(fs: &MemoryFs, want: &JsValue, label: &str) {
        let have: Vec<(String, String)> = fs
            .files()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut want: Vec<(String, String)> = want
            .as_object()
            .expect("files")
            .iter()
            .map(|(k, v)| (k.to_string_lossy(), text(v)))
            .collect();
        want.sort();
        assert_eq!(
            have.len(),
            want.len(),
            "{label}: {:?}",
            have.iter().map(|f| &f.0).collect::<Vec<_>>()
        );
        for ((hp, hc), (wp, wc)) in have.iter().zip(&want) {
            assert_eq!(hp, wp, "{label}");
            assert_eq!(hc, wc, "{label}: {hp}");
        }
    }

    fn now(f: &JsValue) -> f64 {
        f.get("now").and_then(JsValue::as_number).expect("now")
    }

    /// The open folder ProseCraft writes, file for file: the marker, the projects normalized
    /// through Markdown, each document's front matter and hash, chapters, storyboard and header.
    #[test]
    fn writes_the_open_folder_prosecraft_writes() {
        let f = fixture();
        let mut fs = MemoryFs::new();
        let mut provider = OpenFolderProvider::new(&mut fs, now(&f));
        let connection = provider
            .connect(&JsString::from(""), &mut OsRandom)
            .expect("connect");
        assert!(connection.created && connection.projects.is_empty());
        let normalized = provider.normalize(list(&f, "projects"));
        assert_eq!(json(&normalized), json(&list(&f, "normalized")));
        provider
            .push(&normalized, None, &mut OsRandom)
            .expect("push");
        same_files(&fs, f.get("files").expect("files"), "written");
        assert_eq!(fs.files().len(), 14);
        assert_eq!(list(&f, "documents").len(), 7);
        for case in list(&f, "documents") {
            let document = case
                .get("document")
                .and_then(JsValue::as_object)
                .expect("document");
            assert_eq!(
                js::stringify(&JsValue::Object(normalize_document(document))),
                case.get("normalized")
                    .map(js::stringify)
                    .unwrap_or_default()
            );
        }
    }

    /// Reading it back, and what an edit made outside ProseCraft does: a changed body, title or
    /// chapter, a removed hash, a new file, an edited storyboard or header, and the folders it
    /// refuses with ProseCraft's messages.
    #[test]
    fn reads_the_open_folder_and_its_outside_edits_as_prosecraft_does() {
        let f = fixture();
        let mut fs = files(f.get("files").expect("files"));
        let read = read_open_projects(&mut fs, now(&f)).expect("read");
        assert_eq!(read.len(), 2);
        assert_eq!(json(&read), json(&list(&f, "read")));
        assert_eq!(list(&f, "outside").len(), 14);
        assert_eq!(list(&f, "refusals").len(), 6);
        for case in list(&f, "outside") {
            let name = case.get("name").map(text).unwrap_or_default();
            let mut fs = files(case.get("files").expect("files"));
            match (read_open_projects(&mut fs, now(&f)), case.get("error")) {
                (Ok(projects), None) => assert_eq!(
                    json(&projects),
                    case.get("projects").map(js::stringify).unwrap_or_default(),
                    "{name}"
                ),
                (Err(error), Some(want)) => assert_eq!(error.to_string(), text(want), "{name}"),
                (got, want) => panic!("{name}: {got:?} where ProseCraft gave {want:?}"),
            }
        }
        for case in list(&f, "refusals") {
            let name = case.get("name").map(text).unwrap_or_default();
            let mut fs = files(case.get("files").expect("files"));
            let result = OpenFolderProvider::new(&mut fs, now(&f))
                .connect(&JsString::from(""), &mut OsRandom);
            match (result, case.get("error")) {
                (Err(error), Some(want)) if !want.is_null() => {
                    assert_eq!(error.to_string(), text(want), "{name}")
                }
                (Ok(_), Some(want)) if want.is_null() => {}
                (got, want) => panic!(
                    "{name}: {:?} where ProseCraft gave {want:?}",
                    got.map(|c| c.created)
                ),
            }
        }
    }

    /// A whole sync after an edit outside ProseCraft and one inside: what it reports, the
    /// projects it hands back, and the folder after it.
    #[test]
    fn syncs_an_open_folder_as_prosecraft_does() {
        let f = fixture();
        let sync = f.get("sync").expect("sync");
        let mut fs = files(sync.get("files").expect("files"));
        let mut provider = OpenFolderProvider::new(&mut fs, now(&f));
        let mut suffix = || "x".to_owned();
        let result = sync_via(
            &mut provider,
            &JsString::from(""),
            &list(sync, "local"),
            sync.get("lastSynced").expect("lastSynced"),
            now(&f),
            "device-a",
            &mut OsRandom,
            &mut suffix,
        )
        .expect("sync");
        let want = sync.get("result").expect("result");
        let n = |k: &str| want.get(k).and_then(JsValue::as_number).unwrap_or(-1.0) as usize;
        assert_eq!(
            (
                result.imported,
                result.exported,
                result.removed,
                result.conflicts
            ),
            (n("imported"), n("exported"), n("removed"), n("conflicts"))
        );
        assert!(!result.created);
        assert_eq!(result.projects.len(), 2);
        assert_eq!(json(&result.projects), json(&list(want, "projects")));
        assert_eq!(
            js::stringify(&result.last_synced),
            want.get("lastSynced")
                .map(js::stringify)
                .unwrap_or_default()
        );
        same_files(&fs, sync.get("after").expect("after"), "after sync");
    }

    /// What CraftCloud leaves in a synced open folder: conflict copies of a scene (two on the
    /// same minute), a chapter, a project's header, the storyboard and the marker, a project
    /// folder it had to rename, its own `.craftcloud` folder and a download in progress. The
    /// sync reads none of them as a record (a scene's copy repeats its id), removes none of
    /// them, and reports the conflict copies.
    #[test]
    fn craftcloud_files_in_the_folder_are_reported_and_left_alone() {
        use prosecraft_core::sync::SyncFileSystem;
        let f = fixture();
        let sync = f.get("sync").expect("sync");
        let mut fs = files(sync.get("files").expect("files"));
        let copy = |fs: &MemoryFs, from: &str| fs.files().get(from).cloned().expect(from);
        let doc = "projects/project-1/documents/doc-1.md";
        let edited = copy(&fs, doc).replace("revision:", "revision: 7 #");
        let extra = [
            (
                "projects/project-1/documents/doc-1 (conflict 2026-11-13 14.32 Studio).md",
                edited.clone(),
            ),
            (
                "projects/project-1/documents/doc-1 (conflict 2026-11-13 14.32 Studio 2).md",
                edited,
            ),
            (
                "projects/project-1/chapters/chapter-1 (conflict 2026-11-13 14.32 Jo's Laptop).json",
                copy(&fs, "projects/project-1/chapters/chapter-1.json"),
            ),
            (
                "projects/project-1/project (conflict 2026-11-13 14.32 Studio).json",
                copy(&fs, "projects/project-1/project.json"),
            ),
            (
                "projects/project-1/storyboard/rows (conflict 2026-11-13 14.32 Studio).json",
                copy(&fs, "projects/project-1/storyboard/rows.json"),
            ),
            (
                "prosecraft-folder (conflict 2026-11-13 14.32 Studio).json",
                copy(&fs, "prosecraft-folder.json"),
            ),
            (
                "projects/project-1 (conflict 2026-11-13 14.32 Studio)/project.json",
                copy(&fs, "projects/project-1/project.json"),
            ),
            ("projects/.craftcloud/engine.lock", "1".to_owned()),
            (
                "projects/project-1/documents/.doc-9.md.d7fc8c633e524686-1794615066.craftcloud-part",
                "half a download".to_owned(),
            ),
            (
                "projects/project-1/documents/.ccpart-0123456789abcdef01234567.craftcloud-part",
                "half a save".to_owned(),
            ),
        ];
        for (path, content) in &extra {
            fs.write_text(path, content).expect("write");
        }
        let mut suffix = || "x".to_owned();
        let result = sync_via(
            &mut OpenFolderProvider::new(&mut fs, now(&f)),
            &JsString::from(""),
            &list(sync, "local"),
            sync.get("lastSynced").expect("lastSynced"),
            now(&f),
            "device-a",
            &mut OsRandom,
            &mut suffix,
        )
        .expect("CraftCloud's files don't stop the sync");
        let want = sync.get("result").expect("result");
        assert_eq!(
            json(&result.projects),
            json(&list(want, "projects")),
            "the same projects as without them"
        );
        for (path, content) in &extra {
            assert_eq!(
                fs.files().get(*path),
                Some(content),
                "{path} is left as it was"
            );
        }
        assert_eq!(
            result.conflict_files,
            [
                "prosecraft-folder (conflict 2026-11-13 14.32 Studio).json",
                "projects/project-1 (conflict 2026-11-13 14.32 Studio)",
                "projects/project-1/project (conflict 2026-11-13 14.32 Studio).json",
                "projects/project-1/documents/doc-1 (conflict 2026-11-13 14.32 Studio 2).md",
                "projects/project-1/documents/doc-1 (conflict 2026-11-13 14.32 Studio).md",
                "projects/project-1/chapters/chapter-1 (conflict 2026-11-13 14.32 Jo's Laptop).json",
                "projects/project-1/storyboard/rows (conflict 2026-11-13 14.32 Studio).json",
            ]
        );
        // Everything else is the folder a sync without them leaves.
        let mut without = fs.clone();
        for (path, _) in &extra {
            without.remove(path).expect("remove");
        }
        without
            .remove("projects/project-1 (conflict 2026-11-13 14.32 Studio)")
            .expect("remove");
        without.remove("projects/.craftcloud").expect("remove");
        same_files(&without, sync.get("after").expect("after"), "after sync");
    }

    fn projects_with(f: &JsValue, from: &str, to: &str) -> Vec<JsValue> {
        let text = js::stringify(f.get("projects").expect("projects"));
        let text = text.replace(&format!("\"{from}\""), &format!("\"{to}\""));
        js::parse(&text)
            .expect("json")
            .as_array()
            .expect("array")
            .to_vec()
    }

    fn doc_ids(projects: &[JsValue]) -> Vec<String> {
        projects
            .iter()
            .flat_map(|p| {
                p.get("project")
                    .and_then(|p| p.get("documents"))
                    .and_then(JsValue::as_array)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|d| d.get("id").and_then(JsValue::as_js_string))
                    .map(|s| s.to_string_lossy())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn sync_open(
        fs: &mut MemoryFs,
        local: &[JsValue],
        last: &JsValue,
        now: f64,
        device: &str,
    ) -> prosecraft_core::sync::SyncResult {
        sync_via(
            &mut OpenFolderProvider::new(fs, now),
            &JsString::from(""),
            local,
            last,
            now,
            device,
            &mut OsRandom,
            &mut || "x".to_owned(),
        )
        .expect("sync")
    }

    /// A scene whose id `safe_name` keeps as it is, but which looks like a Syncthing conflict
    /// stamp, is the project's own: it reaches the second computer. (It was written and never
    /// read back.)
    #[test]
    fn a_scene_whose_id_looks_like_a_conflict_stamp_reaches_the_other_computer() {
        let f = fixture();
        let id = "doc-2.sync-conflict-20261113-143200-A";
        let local = projects_with(&f, "doc-2", id);
        let none = JsValue::Object(Default::default());
        let mut fs = MemoryFs::new();
        sync_open(&mut fs, &local, &none, now(&f), "device-a");
        let file = format!("projects/project-1/documents/{id}.md");
        assert!(fs.files().contains_key(&file), "written");
        let b = sync_open(&mut fs, &[], &none, now(&f) + 1.0, "device-b");
        assert!(
            doc_ids(&b.projects).iter().any(|d| d == id),
            "{:?}",
            doc_ids(&b.projects)
        );
        assert!(b.conflict_files.is_empty(), "{:?}", b.conflict_files);
        assert!(fs.files().contains_key(&file), "still there after B's sync");
    }

    /// Deleting a project removes its files but not a conflict copy in its folder, which can be
    /// the only copy of an edit; the folder stays for it, later syncs don't mistake it for a
    /// damaged project, and the copy is reported.
    #[test]
    fn deleting_a_project_keeps_the_conflict_copies_in_its_folder() {
        use prosecraft_core::sync::SyncFileSystem;
        let f = fixture();
        let local = list(&f, "projects");
        let none = JsValue::Object(Default::default());
        let mut fs = MemoryFs::new();
        let a = sync_open(&mut fs, &local, &none, now(&f), "device-a");
        let copy = "projects/project-1/documents/doc-1 (conflict 2026-11-13 14.32 Studio).md";
        fs.write_text(copy, "the only copy of an edit made on Studio")
            .expect("write");
        let kept: Vec<JsValue> = a
            .projects
            .iter()
            .filter(|p| {
                !p.get("id")
                    .and_then(JsValue::as_js_string)
                    .is_some_and(|i| i.eq_str("project-1"))
            })
            .cloned()
            .collect();
        let a2 = sync_open(&mut fs, &kept, &a.last_synced, now(&f) + 1.0, "device-a");
        assert_eq!(a2.removed, 1, "project-1 was deleted");
        assert_eq!(
            fs.files().get(copy).map(String::as_str),
            Some("the only copy of an edit made on Studio")
        );
        let left: Vec<&String> = fs
            .files()
            .keys()
            .filter(|k| k.starts_with("projects/project-1/"))
            .collect();
        assert_eq!(left, [copy], "only the copy is left of the project");
        assert_eq!(a2.conflict_files, [copy]);
        let again = sync_open(
            &mut fs,
            &a2.projects,
            &a2.last_synced,
            now(&f) + 2.0,
            "device-a",
        );
        assert_eq!(
            again.projects.len(),
            kept.len(),
            "the folder isn't read as a project"
        );
        assert_eq!(again.conflict_files, [copy]);
    }

    /// Listing conflict copies is only a report: when it fails, the sync (already written)
    /// still succeeds.
    #[test]
    fn a_conflict_listing_that_fails_does_not_fail_the_sync() {
        use prosecraft_core::storage::crypto::{RandomSource, VaultKey};
        use prosecraft_core::sync::SyncError;
        use prosecraft_core::sync::providers::Connection;
        struct NoListing<'a>(OpenFolderProvider<'a>);
        impl SyncProvider for NoListing<'_> {
            fn connect(
                &mut self,
                passphrase: &JsString,
                random: &mut dyn RandomSource,
            ) -> Result<Connection, SyncError> {
                self.0.connect(passphrase, random)
            }
            fn push(
                &mut self,
                projects: &[JsValue],
                key: Option<&VaultKey>,
                random: &mut dyn RandomSource,
            ) -> Result<(), SyncError> {
                self.0.push(projects, key, random)
            }
            fn remove(&mut self, ids: &[String]) -> Result<(), SyncError> {
                self.0.remove(ids)
            }
            fn conflict_files(&mut self) -> Result<Vec<String>, SyncError> {
                Err(SyncError::Io("Permission denied".into()))
            }
        }
        let f = fixture();
        let mut fs = MemoryFs::new();
        let result = sync_via(
            &mut NoListing(OpenFolderProvider::new(&mut fs, now(&f))),
            &JsString::from(""),
            &list(&f, "projects"),
            &JsValue::Object(Default::default()),
            now(&f),
            "device-a",
            &mut OsRandom,
            &mut || "x".to_owned(),
        )
        .expect("the sync succeeds");
        assert!(result.conflict_files.is_empty());
        assert_eq!(result.projects.len(), 2);
    }
}
