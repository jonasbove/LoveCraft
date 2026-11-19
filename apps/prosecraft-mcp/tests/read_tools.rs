//! `mcp/src/readTools.test.ts`, plus the folder tree's cycles and repeated ids.

mod common;

use std::fs;

use common::{ids, make_root, stored, str_of};
use prosecraft_core::js::{self, JsValue};
use prosecraft_mcp::read_tools::{
    list_comments, list_labels, list_snapshots, list_wiki_folders, list_wiki_pages, read_snapshot,
    read_wiki_page,
};
use prosecraft_mcp::store::read_scene;

const PROFILE: &str = r#""role":"Protagonist","age":"32","gender":"","pronouns":"she/her","ethnicity":"","occupation":"Cartographer","appearance":"","voice":"","personality":"","background":"","motivations":"","flaws":"","hobbies":"","relationships":"""#;

const NOW: i64 = 1_700_000_000_000;

fn project_fixture() -> String {
    format!(
        r#"{{
  "id": "project-1", "name": "Wiki Novel", "author": "Ada Writer", "activeDocumentId": "doc-1",
  "projectGoal": 50000, "chapterGoals": {{}}, "activity": {{}},
  "labels": [{{"id": "label-1", "name": "Plot", "color": "red"}}, {{"id": "label-2", "name": "Character", "color": "blue"}}, {{"id": "label-3", "name": "Unused", "color": "grey"}}],
  "researchFolders": [{{"id": "folder-1", "title": "Characters", "parentId": null}}, {{"id": "folder-2", "title": "Places", "parentId": null}}, {{"id": "folder-3", "title": "Harbor", "parentId": "folder-2"}}],
  "documents": [
    {{"id": "doc-1", "title": "Opening Scene", "content": "<p>It was dark.</p>", "chapterId": "chapter-1", "labelId": "label-1"}},
    {{"id": "doc-2", "title": "Mara", "content": "<p>Notes about Mara.</p>", "space": "Wiki", "folderId": "folder-1", "labelId": "label-2", "character": {{{PROFILE}}}}},
    {{"id": "doc-3", "title": "Harbor", "content": "<p>A grey harbor.</p>", "space": "Wiki", "folderId": "folder-3", "labelId": "label-1",
      "place": {{"type": "City", "region": "North", "climate": "", "population": "", "government": "", "economy": "", "appearance": "", "atmosphere": "", "landmarks": "", "history": "", "culture": "", "inhabitants": "", "significance": "", "dangers": ""}}}},
    {{"id": "doc-4", "title": "Cut Scene", "content": "<p>Cut.</p>", "chapterId": "chapter-1", "labelId": "label-1", "trashed": true, "trashedWithChapterId": "chapter-1"}},
    {{"id": "doc-5", "title": "Loose Page", "content": "<p>Loose notes.</p>", "space": "Wiki"}}
  ],
  "chapters": [{{"id": "chapter-1", "title": "Chapter One", "expanded": true, "labelId": "label-2"}}, {{"id": "chapter-2", "title": "Chapter Two", "expanded": false, "labelId": "label-1"}}],
  "comments": [
    {{"id": "comment-1", "documentId": "doc-1", "quote": "dark", "prefix": "It was ", "suffix": ".", "body": "Too vague?", "author": "Patty", "createdAt": {NOW}, "resolved": false}},
    {{"id": "comment-2", "documentId": "doc-1", "quote": "It", "prefix": "", "suffix": " was", "body": "Weak opening.", "author": "Patty", "createdAt": {}, "resolved": true}},
    {{"id": "comment-3", "documentId": "doc-2", "quote": "Mara", "prefix": "", "suffix": " notes", "body": "Add her age.", "author": "Henry", "createdAt": {}, "resolved": false}}
  ],
  "snapshots": [
    {{"id": "snap-1", "documentId": "doc-1", "createdAt": {NOW}, "title": "Opening Scene", "content": "<p>It was dark.</p>", "note": "first", "reason": "manual"}},
    {{"id": "snap-2", "documentId": "doc-1", "createdAt": {}, "title": "Opening Scene", "content": "<p>It was darker.</p>", "reason": "before-restore"}},
    {{"id": "snap-3", "documentId": "doc-2", "createdAt": {}, "title": "Mara", "content": "<p>Old Mara.</p>", "reason": "manual"}}
  ]
}}"#,
        NOW + 1_000,
        NOW + 2_000,
        NOW + 5_000,
        NOW + 3_000
    )
}

fn root() -> common::Root {
    make_root(&stored(&project_fixture()))
}

#[test]
fn list_wiki_pages_lists_pages_with_folder_path_and_kind() {
    let root = root();
    let pages = list_wiki_pages(&root.path, "project-1", None).expect("pages");
    assert_eq!(ids(&pages), ["doc-2", "doc-3", "doc-5"]);
    let list = pages.as_array().expect("list");
    assert_eq!(str_of(&list[0], "title"), "Mara");
    assert_eq!(str_of(&list[0], "folderId"), "folder-1");
    assert_eq!(
        js::stringify(list[0].get("folderPath").expect("path")),
        r#"["Characters"]"#
    );
    assert_eq!(str_of(&list[0], "kind"), "character");
    assert_eq!(str_of(&list[1], "title"), "Harbor");
    assert_eq!(
        js::stringify(list[1].get("folderPath").expect("path")),
        r#"["Places","Harbor"]"#
    );
    assert_eq!(str_of(&list[1], "kind"), "place");
    assert_eq!(list[2].get("folderId"), Some(&JsValue::Null));
    assert_eq!(
        js::stringify(list[2].get("folderPath").expect("path")),
        "[]"
    );
    assert_eq!(str_of(&list[2], "kind"), "page");
    assert!(list.iter().all(|page| page.get("markdown").is_none()));
}

#[test]
fn list_wiki_pages_filters_by_folder() {
    let root = root();
    let pages = list_wiki_pages(&root.path, "project-1", Some("folder-3")).expect("pages");
    assert_eq!(ids(&pages), ["doc-3"]);
}

#[test]
fn read_wiki_page_returns_the_body_and_every_field() {
    let root = root();
    let page = read_wiki_page(&root.path, "project-1", "doc-2").expect("page");
    assert!(str_of(&page, "markdown").contains("Notes about Mara."));
    let fields = page.get("fields").expect("fields");
    assert_eq!(str_of(fields, "space"), "Wiki");
    assert_eq!(str_of(fields, "folderId"), "folder-1");
    assert_eq!(str_of(fields, "labelId"), "label-2");
    assert_eq!(
        str_of(fields.get("character").expect("profile"), "role"),
        "Protagonist"
    );
    assert_eq!(
        js::stringify(page.get("folderPath").expect("path")),
        r#"["Characters"]"#
    );
    assert_eq!(str_of(&page, "kind"), "character");
}

#[test]
fn read_wiki_page_rejects_other_documents() {
    let root = root();
    let error = read_wiki_page(&root.path, "project-1", "doc-1").expect_err("not wiki");
    assert!(error.contains("not a Wiki page"));
}

#[test]
fn list_wiki_folders_returns_a_tree() {
    let root = root();
    let tree = list_wiki_folders(&root.path, "project-1").expect("tree");
    let summary: Vec<(String, Vec<String>)> = tree
        .as_array()
        .expect("list")
        .iter()
        .map(|node| {
            let children = node
                .get("children")
                .and_then(JsValue::as_array)
                .expect("children");
            (
                str_of(node, "title"),
                children.iter().map(|c| str_of(c, "title")).collect(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("Characters".to_owned(), vec![]),
            ("Places".to_owned(), vec!["Harbor".to_owned()])
        ]
    );
}

/// Rewrites `researchFolders` in `project.json` (the server does not check the header hash).
fn with_folders(root: &common::Root, folders: &str) {
    let mut header = root.project_json();
    let project = header
        .as_object_mut()
        .and_then(|h| h.get_mut("project"))
        .and_then(JsValue::as_object_mut)
        .expect("project");
    project.insert("researchFolders", js::parse(folders).expect("folders"));
    fs::write(root.project_path(), js::stringify_pretty(&header)).expect("write");
}

#[test]
fn list_wiki_folders_fails_like_json_stringify_on_a_cycle() {
    let root = root();
    // A repeated id makes a folder its own child, which `JSON.stringify` refuses.
    with_folders(
        &root,
        r#"[{"id":"a","title":"A","parentId":null},{"id":"a","title":"A2","parentId":"a"}]"#,
    );
    assert_eq!(
        list_wiki_folders(&root.path, "project-1").err().as_deref(),
        Some(
            "Converting circular structure to JSON\n    --> starting at object with constructor 'Object'\n    |     property 'children' -> object with constructor 'Array'\n    --- index 0 closes the circle"
        )
    );
    // Folders that only point at each other are no root's descendants: an empty tree.
    with_folders(
        &root,
        r#"[{"id":"a","title":"A","parentId":"b"},{"id":"b","title":"B","parentId":"a"}]"#,
    );
    assert_eq!(
        js::stringify(&list_wiki_folders(&root.path, "project-1").expect("tree")),
        "[]"
    );
}

#[test]
fn list_labels_counts_live_documents_and_chapters() {
    let root = root();
    let labels = list_labels(&root.path, "project-1").expect("labels");
    assert_eq!(
        js::stringify(&labels),
        r#"[{"id":"label-1","name":"Plot","color":"red","scenes":2,"chapters":1},{"id":"label-2","name":"Character","color":"blue","scenes":1,"chapters":1},{"id":"label-3","name":"Unused","color":"grey","scenes":0,"chapters":0}]"#
    );
}

#[test]
fn list_comments_hides_resolved_ones_and_filters_by_document() {
    let root = root();
    let open = list_comments(&root.path, "project-1", None, false).expect("open");
    let all = list_comments(&root.path, "project-1", None, true).expect("all");
    let for_page = list_comments(&root.path, "project-1", Some("doc-2"), false).expect("page");
    assert_eq!(ids(&open), ["comment-1", "comment-3"]);
    assert_eq!(ids(&all), ["comment-1", "comment-2", "comment-3"]);
    assert_eq!(ids(&for_page), ["comment-3"]);
    assert!(all.as_array().expect("list")[0].get("prefix").is_none());
}

#[test]
fn list_snapshots_lists_metadata_newest_first() {
    let root = root();
    let snapshots = list_snapshots(&root.path, "project-1", None).expect("snapshots");
    let for_scene = list_snapshots(&root.path, "project-1", Some("doc-1")).expect("scene");
    assert_eq!(ids(&snapshots), ["snap-2", "snap-3", "snap-1"]);
    let list = snapshots.as_array().expect("list");
    assert_eq!(str_of(&list[0], "documentId"), "doc-1");
    assert_eq!(str_of(&list[0], "reason"), "before-restore");
    assert!(list[0].get("content").is_none());
    assert_eq!(str_of(&list[2], "note"), "first");
    assert_eq!(ids(&for_scene), ["snap-2", "snap-1"]);
}

#[test]
fn read_snapshot_returns_the_stored_text() {
    let root = root();
    let snapshot = read_snapshot(&root.path, "project-1", "snap-1").expect("snapshot");
    assert_eq!(str_of(&snapshot, "title"), "Opening Scene");
    assert!(str_of(&snapshot, "content").contains("It was dark."));
    assert_eq!(str_of(&snapshot, "note"), "first");
    assert_eq!(str_of(&snapshot, "contentFormat"), "html");
}

#[test]
fn read_snapshot_fails_for_an_unknown_id() {
    let root = root();
    let error = read_snapshot(&root.path, "project-1", "missing").expect_err("unknown");
    assert!(error.starts_with("Snapshot not found"));
}

#[test]
fn read_scene_passes_through_fields_it_does_not_interpret() {
    let root = root();
    let file = root.doc_path("doc-2");
    let text = root
        .read(&file)
        .replacen("\n---\n", "\nmeta: {\"pov\":\"Mara\"}\n---\n", 1);
    fs::write(&file, text).expect("write");
    let scene = read_scene(&root.path, "project-1", "doc-2", false).expect("read");
    let fields = scene.get("fields").expect("fields");
    assert_eq!(str_of(fields, "space"), "Wiki");
    assert_eq!(str_of(fields, "labelId"), "label-2");
    assert_eq!(str_of(fields.get("meta").expect("meta"), "pov"), "Mara");
    assert!(str_of(&scene, "markdown").contains("Notes about Mara."));
}
