//! `mcp/src/openStore.test.ts`, against the `write_scene` the server registers (the one in
//! `safeWrites.ts`).

mod common;

use std::fs;

use common::{NOW, document, env, make_root, meta, num, s, stored, str_of};
use prosecraft_core::js::{self, JsValue};
use prosecraft_mcp::fsx::LOCK_FILE;
use prosecraft_mcp::safe_writes::{WriteSceneInput, write_scene};
use prosecraft_mcp::store::{
    ChapterPatch, ListFilter, list_chapters, list_scenes, read_scene, trash_scene, update_chapter,
};

const PROFILE: &str = r#""role":"Protagonist","age":"32","gender":"","pronouns":"she/her","ethnicity":"","occupation":"Cartographer","appearance":"","voice":"","personality":"","background":"","motivations":"","flaws":"","hobbies":"","relationships":"""#;

fn project_fixture() -> String {
    format!(
        r#"{{
  "id": "project-1", "name": "Test Novel", "author": "Ada Writer", "activeDocumentId": "doc-1",
  "projectGoal": 50000, "chapterGoals": {{"chapter-1": 2000}}, "activity": {{}},
  "documents": [
    {{"id": "doc-1", "title": "Opening Scene", "content": "<p>It was dark.</p>", "chapterId": "chapter-1"}},
    {{"id": "doc-2", "title": "Mara", "content": "<p>Notes about Mara.</p>", "space": "Wiki", "folderId": "folder-1", "labelId": "label-1", "character": {{{PROFILE}}}}},
    {{"id": "doc-3", "title": "Harbor", "content": "<p>A grey harbor.</p>", "space": "Wiki", "folderId": "folder-2", "labelId": "label-2",
      "place": {{"type": "City", "region": "North", "climate": "", "population": "", "government": "", "economy": "", "appearance": "", "atmosphere": "", "landmarks": "", "history": "", "culture": "", "inhabitants": "", "significance": "", "dangers": ""}}}},
    {{"id": "doc-4", "title": "Cut Scene", "content": "<p>Cut.</p>", "chapterId": "chapter-1", "trashed": true, "trashedWithChapterId": "chapter-1"}}
  ],
  "chapters": [{{"id": "chapter-1", "title": "Chapter One", "expanded": true, "summary": "It begins."}}]
}}"#
    )
}

fn fixture() -> JsValue {
    stored(&project_fixture())
}

fn input(scene: &str, markdown: &str) -> WriteSceneInput {
    WriteSceneInput {
        scene_id: Some(s(scene)),
        markdown: s(markdown),
        ..WriteSceneInput::default()
    }
}

#[test]
fn write_scene_keeps_wiki_page_fields_and_produces_the_apps_hash() {
    let root = make_root(&fixture());
    let scene = write_scene(
        &root.path,
        &mut env(),
        "project-1",
        &input("doc-2", "Mara is a *cartographer*."),
    )
    .expect("write");

    let text = root.read(&root.doc_path("doc-2"));
    assert!(text.contains("space: \"Wiki\""));
    assert!(text.contains("folderId: \"folder-1\""));
    assert!(text.contains("labelId: \"label-1\""));

    // Reading with the app must not flag the file as an external edit.
    let reread = root.reread();
    let meta = meta(&reread, "documents", "doc-2");
    assert_eq!(str_of(meta, "updatedBy"), "mcp");
    assert_eq!(
        num(meta, "revision"),
        num(scene.get("meta").expect("meta"), "revision")
    );
    let document = document(&reread, "doc-2").expect("doc-2");
    assert_eq!(str_of(document, "space"), "Wiki");
    assert_eq!(str_of(document, "folderId"), "folder-1");
    assert_eq!(str_of(document, "labelId"), "label-1");
    assert_eq!(
        str_of(document.get("character").expect("profile"), "occupation"),
        "Cartographer"
    );
    assert!(str_of(document, "markdownSource").contains("cartographer"));
}

#[test]
fn write_scene_produces_the_apps_hash_for_screenplays_and_footnotes() {
    let mut data = js::parse(&project_fixture()).expect("fixture");
    let doc = data
        .as_object_mut()
        .and_then(|p| p.get_mut("documents"))
        .and_then(|d| match d {
            JsValue::Array(items) => items.get_mut(0),
            _ => None,
        })
        .and_then(JsValue::as_object_mut)
        .expect("doc-1");
    doc.insert("format", JsValue::str("screenplay"));
    doc.insert(
        "footnotes",
        js::parse(r#"[{"id":"fn-1","html":"A note."}]"#).expect("notes"),
    );
    let root = make_root(&prosecraft_core::storage::stamp::create_stored_project(
        data, "device-a", NOW,
    ));

    let scene = write_scene(
        &root.path,
        &mut env(),
        "project-1",
        &input("doc-1", "INT. LIGHTHOUSE - NIGHT"),
    )
    .expect("write");

    let reread = root.reread();
    let meta = meta(&reread, "documents", "doc-1");
    assert_eq!(str_of(meta, "updatedBy"), "mcp");
    assert_eq!(
        num(meta, "revision"),
        num(scene.get("meta").expect("meta"), "revision")
    );
    let document = document(&reread, "doc-1").expect("doc-1");
    assert_eq!(str_of(document, "format"), "screenplay");
    assert_eq!(
        document.get("footnotes"),
        Some(&js::parse(r#"[{"id":"fn-1","html":"A note."}]"#).expect("notes"))
    );
}

#[test]
fn write_scene_takes_footnotes_from_the_new_body() {
    let mut data = js::parse(&project_fixture()).expect("fixture");
    let doc = data
        .as_object_mut()
        .and_then(|p| p.get_mut("documents"))
        .and_then(|d| match d {
            JsValue::Array(items) => items.get_mut(0),
            _ => None,
        })
        .and_then(JsValue::as_object_mut)
        .expect("doc-1");
    doc.insert(
        "content",
        JsValue::str(r#"<p>It was dark.<sup class="footnote-ref" data-footnote-id="fn-1" contenteditable="false"></sup></p>"#),
    );
    doc.insert(
        "footnotes",
        js::parse(r#"[{"id":"fn-1","html":"A note."}]"#).expect("notes"),
    );
    let root = make_root(&prosecraft_core::storage::stamp::create_stored_project(
        data, "device-a", NOW,
    ));

    write_scene(
        &root.path,
        &mut env(),
        "project-1",
        &input("doc-1", "It was dark.[^fn-1] And cold.[^fn-2]\n\n[^fn-1]: An edited note.\n[^fn-2]: A second note."),
    )
    .expect("write");

    let reread = root.reread();
    assert_eq!(
        str_of(meta(&reread, "documents", "doc-1"), "updatedBy"),
        "mcp"
    );
    let document = document(&reread, "doc-1").expect("doc-1");
    assert_eq!(
        document.get("footnotes"),
        Some(
            &js::parse(
                r#"[{"id":"fn-1","html":"An edited note."},{"id":"fn-2","html":"A second note."}]"#
            )
            .expect("notes")
        )
    );
}

#[test]
fn write_scene_keeps_place_profiles_and_trash_markers() {
    let root = make_root(&fixture());
    write_scene(
        &root.path,
        &mut env(),
        "project-1",
        &input("doc-3", "The harbor at dawn."),
    )
    .expect("write");
    let place = root.read(&root.doc_path("doc-3"));
    assert!(place.contains("space: \"Wiki\""));
    assert!(place.contains("folderId: \"folder-2\""));
    assert!(place.contains("labelId: \"label-2\""));
    assert!(place.contains("\"type\":\"City\""));

    let scene = read_scene(&root.path, "project-1", "doc-4", true).expect("read trashed");
    assert_eq!(str_of(&scene, "markdown"), "Cut.");
    let refused = read_scene(&root.path, "project-1", "doc-4", false).expect_err("trashed");
    assert!(refused.contains("in the trash"));

    write_scene(
        &root.path,
        &mut env(),
        "project-1",
        &input("doc-4", "Cut after all."),
    )
    .expect("write");
    let trashed = root.read(&root.doc_path("doc-4"));
    assert!(trashed.contains("trashed: true"));
    assert!(trashed.contains("trashedWithChapterId: \"chapter-1\""));
}

#[test]
fn write_scene_rejects_an_unknown_scene_id_unless_create_is_set() {
    let root = make_root(&fixture());
    let error = write_scene(
        &root.path,
        &mut env(),
        "project-1",
        &input("doc-9", "New scene."),
    )
    .expect_err("unknown");
    assert!(
        error.starts_with("Scene not found: doc-9") && error.contains("create: true"),
        "{error}"
    );

    let created = write_scene(
        &root.path,
        &mut env(),
        "project-1",
        &WriteSceneInput {
            scene_id: Some(s("doc-9")),
            chapter_id: Some(s("chapter-1")),
            title: Some(s("New Scene")),
            markdown: s("New scene."),
            create: Some(true),
            ..WriteSceneInput::default()
        },
    )
    .expect("create");
    assert_eq!(str_of(&created, "id"), "doc-9");

    // A new document gets only the fields this tool owns, not a copy of a sibling.
    let text = root.read(&root.doc_path("doc-9"));
    let head = "---\nid: \"doc-9\"\ntitle: \"New Scene\"\nchapterId: \"chapter-1\"\nupdatedAt: ";
    assert!(text.starts_with(head), "{text}");
    let rest = &text[head.len()..];
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    assert!(digits > 0);
    let rest = &rest[digits..];
    let middle = "\nrevision: 1\nupdatedBy: \"mcp\"\ndeletedAt: null\nhash: \"";
    assert!(rest.starts_with(middle), "{text}");
    let hash = &rest[middle.len()..middle.len() + 64];
    assert!(
        hash.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
    assert!(rest[middle.len() + 64..].starts_with("\"\n---\n"));
}

#[test]
fn lists_skip_a_malformed_document_but_a_direct_read_reports_it() {
    let root = make_root(&fixture());
    fs::write(root.doc_path("doc-2"), "no frontmatter here\n").expect("write");

    let scenes = list_scenes(&root.path, "project-1", &ListFilter::default()).expect("list");
    assert_eq!(
        scenes.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        ["doc-1", "doc-3"]
    );

    let error = read_scene(&root.path, "project-1", "doc-2", false).expect_err("malformed");
    assert!(
        error.starts_with("Scene doc-2 could not be read: missing frontmatter"),
        "{error}"
    );
}

#[test]
fn projects_in_the_trash_are_refused() {
    let mut project = fixture();
    if let Some(p) = project.as_object_mut() {
        p.insert("deletedAt", JsValue::Number(NOW + 5_000.0));
    }
    let root = make_root(&project);

    let error = list_scenes(&root.path, "project-1", &ListFilter::default()).err();
    assert_eq!(error.as_deref(), Some("Project is deleted: project-1"));
    let error = write_scene(
        &root.path,
        &mut env(),
        "project-1",
        &input("doc-1", "Nope."),
    )
    .err();
    assert_eq!(error.as_deref(), Some("Project is deleted: project-1"));
}

#[test]
fn a_failed_rename_leaves_no_temp_file_and_the_old_state() {
    let root = make_root(&fixture());
    let target = root.doc_path("doc-1");
    let header_before = num(&root.project_json(), "revision");

    // A directory at the target path makes the rename fail, like a crash mid-write.
    fs::remove_file(&target).expect("rm");
    fs::create_dir(&target).expect("mkdir");
    fs::write(format!("{target}/keep"), "x").expect("write");

    write_scene(
        &root.path,
        &mut env(),
        "project-1",
        &input("doc-1", "Broken."),
    )
    .expect_err("rename fails");

    let dir = format!("{}/projects/project-1/documents", root.path);
    let leftovers: Vec<String> = fs::read_dir(&dir)
        .expect("dir")
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .filter(|name| name.starts_with('.'))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    assert_eq!(root.read(&format!("{target}/keep")), "x");
    assert_eq!(num(&root.project_json(), "revision"), header_before);
}

#[test]
fn trash_scene_tombstones_the_header_first_and_reads_back_with_the_app() {
    let root = make_root(&fixture());
    let before = num(
        read_scene(&root.path, "project-1", "doc-1", false)
            .expect("read")
            .get("meta")
            .expect("meta"),
        "revision",
    );

    let result = trash_scene(&root.path, &mut env(), "project-1", "doc-1", None).expect("trash");

    assert_eq!(
        js::stringify(&result),
        format!("{{\"id\":\"doc-1\",\"revision\":{}}}", before + 1.0)
    );
    assert!(!std::path::Path::new(&root.doc_path("doc-1")).exists());
    let reread = root.reread();
    let meta = meta(&reread, "documents", "doc-1");
    assert!(num(meta, "deletedAt") > 0.0);
    assert_eq!(num(meta, "revision"), before + 1.0);
    assert!(document(&reread, "doc-1").is_none());
}

#[test]
fn a_missing_file_is_harmless_only_once_the_id_is_tombstoned() {
    let root = make_root(&fixture());
    fs::remove_file(root.doc_path("doc-1")).expect("rm");
    let mut memory = common::load(std::path::Path::new(&root.path));
    let error = prosecraft_core::sync::open_layout::read_open_project(
        &mut memory,
        "project-1",
        NOW + 1_000.0,
    )
    .expect_err("live id without its file");
    assert!(
        error
            .to_string()
            .contains("missing from the open sync folder"),
        "{error}"
    );

    trash_scene(&root.path, &mut env(), "project-1", "doc-1", None).expect("trash");
    let reread = root.reread();
    assert!(num(meta(&reread, "documents", "doc-1"), "deletedAt") > 0.0);
}

#[test]
fn a_matching_expected_revision_writes_and_returns_the_new_one() {
    let root = make_root(&fixture());
    let before = num(
        read_scene(&root.path, "project-1", "doc-1", false)
            .expect("read")
            .get("meta")
            .expect("meta"),
        "revision",
    );
    let mut write = input("doc-1", "Revised.");
    write.expected_revision = Some(before);
    let scene = write_scene(&root.path, &mut env(), "project-1", &write).expect("write");
    assert_eq!(
        num(scene.get("meta").expect("meta"), "revision"),
        before + 1.0
    );
}

#[test]
fn a_stale_revision_is_refused_for_write_trash_and_chapter_update_without_writing() {
    let root = make_root(&fixture());
    let file_before = root.read(&root.doc_path("doc-1"));
    let revision = num(
        read_scene(&root.path, "project-1", "doc-1", false)
            .expect("read")
            .get("meta")
            .expect("meta"),
        "revision",
    );

    let mut stale = input("doc-1", "Stale.");
    stale.expected_revision = Some(revision - 1.0);
    let error = write_scene(&root.path, &mut env(), "project-1", &stale).expect_err("stale");
    assert!(
        error.contains(&format!("current revision is {revision}")),
        "{error}"
    );
    assert_eq!(root.read(&root.doc_path("doc-1")), file_before);

    let error = trash_scene(
        &root.path,
        &mut env(),
        "project-1",
        "doc-1",
        Some(revision + 1.0),
    )
    .expect_err("stale");
    assert!(
        error.contains(&format!("current revision is {revision}")),
        "{error}"
    );
    let after = read_scene(&root.path, "project-1", "doc-1", false).expect("read");
    assert_eq!(num(after.get("meta").expect("meta"), "revision"), revision);

    let chapters =
        list_chapters(&root.path, "project-1", &ListFilter::default()).expect("chapters");
    let chapter = chapters.first().expect("chapter");
    let chapter_revision = num(chapter.get("meta").expect("meta"), "revision");
    let patch = ChapterPatch {
        title: Some(s("Nope")),
        expected_revision: Some(chapter_revision + 1.0),
        ..ChapterPatch::default()
    };
    let error = update_chapter(
        &root.path,
        &mut env(),
        "project-1",
        &str_of(chapter, "id"),
        &patch,
    )
    .expect_err("stale");
    assert!(
        error.contains(&format!("current revision is {chapter_revision}")),
        "{error}"
    );
    let chapters =
        list_chapters(&root.path, "project-1", &ListFilter::default()).expect("chapters");
    assert_eq!(
        str_of(chapters.first().expect("chapter"), "title"),
        str_of(chapter, "title")
    );

    let patch = ChapterPatch {
        title: Some(s("Renamed")),
        expected_revision: Some(chapter_revision),
        ..ChapterPatch::default()
    };
    let updated = update_chapter(
        &root.path,
        &mut env(),
        "project-1",
        &str_of(chapter, "id"),
        &patch,
    )
    .expect("update");
    assert_eq!(
        num(updated.get("meta").expect("meta"), "revision"),
        chapter_revision + 1.0
    );
}

#[test]
fn a_fresh_lock_fails_the_write_clearly() {
    let root = make_root(&fixture());
    let lock = format!("{}/{LOCK_FILE}", root.path);
    fs::write(&lock, "other").expect("lock");

    let error = write_scene(
        &root.path,
        &mut env(),
        "project-1",
        &input("doc-1", "Blocked."),
    )
    .expect_err("locked");
    assert_eq!(
        error,
        format!(
            "Another ProseCraft MCP write is in progress ({lock}). Retry in a moment; a lock older than 30s is treated as stale."
        )
    );
    let scene = read_scene(&root.path, "project-1", "doc-1", false).expect("read");
    assert!(!str_of(&scene, "markdown").contains("Blocked"));
    assert!(std::path::Path::new(&lock).exists());
}

#[test]
fn a_lock_older_than_30_seconds_is_taken_over_and_removed_afterwards() {
    let root = make_root(&fixture());
    let lock = format!("{}/{LOCK_FILE}", root.path);
    fs::write(&lock, "stale").expect("lock");
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    fs::File::options()
        .write(true)
        .open(&lock)
        .expect("open")
        .set_modified(old)
        .expect("utimes");

    let scene = write_scene(
        &root.path,
        &mut env(),
        "project-1",
        &input("doc-1", "After stale."),
    )
    .expect("write");
    assert_eq!(str_of(&scene, "markdown"), "After stale.");
    assert!(!std::path::Path::new(&lock).exists());
}
