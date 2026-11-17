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
            .push(&projects(&f), &connection.key, &mut OsRandom)
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
            .push(&projects(&f), &connection.key, &mut OsRandom)
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
                .push(&projects(&f), &c.key, &mut OsRandom)
                .expect("push");
        } else {
            let mut provider = EncryptedFileProvider::new(&mut fs, 1_800_000_000_000.0)
                .with_vault_options(quick());
            let c = provider
                .connect(&passphrase(&f), &mut OsRandom)
                .expect("create");
            provider
                .push(&projects(&f), &c.key, &mut OsRandom)
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
