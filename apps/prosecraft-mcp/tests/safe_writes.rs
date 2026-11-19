//! `mcp/src/safeWrites.test.ts`.

mod common;

use common::{NOW, document, env, ids, make_root, meta, num, s, stored, str_of};
use prosecraft_core::js::JsValue;
use prosecraft_mcp::read_tools::read_snapshot;
use prosecraft_mcp::safe_writes::{
    CommentInput, LabelTarget, WikiPageInput, WikiPageUpdate, WriteSceneInput, add_comment,
    create_wiki_page, resolve_comment, set_label, update_wiki_page, write_scene,
};

const PROFILE: &str = r#""role":"Protagonist","age":"32","gender":"","pronouns":"she/her","ethnicity":"","occupation":"Cartographer","appearance":"","voice":"","personality":"","background":"","motivations":"","flaws":"","hobbies":"","relationships":"""#;

fn project_fixture(snapshots: Option<&str>) -> String {
    let snapshots = snapshots
        .map(|s| format!(r#""snapshots": {s},"#))
        .unwrap_or_default();
    format!(
        r#"{{
  "id": "project-1", "name": "Test Novel", "author": "Ada Writer", "activeDocumentId": "doc-1",
  "projectGoal": 50000, "chapterGoals": {{"chapter-1": 2000}}, "activity": {{}},
  "labels": [{{"id": "label-1", "name": "POV", "color": "blue"}}, {{"id": "label-2", "name": "Setting", "color": "green"}}],
  "researchFolders": [{{"id": "folder-1", "title": "Characters", "parentId": null}}, {{"id": "folder-2", "title": "Places", "parentId": null}}],
  {snapshots}
  "documents": [
    {{"id": "doc-1", "title": "Opening Scene", "content": "<p>It was dark.</p>", "chapterId": "chapter-1"}},
    {{"id": "doc-2", "title": "Mara", "content": "<p>Notes about Mara.</p>", "space": "Wiki", "folderId": "folder-1", "labelId": "label-1", "character": {{{PROFILE}}}}},
    {{"id": "doc-3", "title": "Duplicate", "content": "<p>The harbor was grey. The harbor was cold.</p>", "chapterId": "chapter-1"}}
  ],
  "chapters": [{{"id": "chapter-1", "title": "Chapter One", "expanded": true, "summary": "It begins."}}]
}}"#
    )
}

fn fixture() -> JsValue {
    stored(&project_fixture(None))
}

fn snapshots(root: &common::Root) -> Vec<JsValue> {
    root.project_section()
        .get("snapshots")
        .and_then(JsValue::as_array)
        .map(<[JsValue]>::to_vec)
        .unwrap_or_default()
}

fn overwrite(scene: &str, markdown: &str) -> WriteSceneInput {
    WriteSceneInput {
        scene_id: Some(s(scene)),
        markdown: s(markdown),
        ..WriteSceneInput::default()
    }
}

#[test]
fn write_scene_stores_the_previous_text_before_overwriting() {
    let root = make_root(&fixture());
    let before = num(&root.project_json(), "revision");

    let scene = write_scene(
        &root.path,
        &mut env(),
        "project-1",
        &overwrite("doc-1", "It was bright."),
    )
    .expect("write");

    let snapshot_id = str_of(&scene, "snapshotId");
    assert!(!snapshot_id.is_empty());
    let snapshots = snapshots(&root);
    assert_eq!(snapshots.len(), 1);
    let snapshot = &snapshots[0];
    assert_eq!(str_of(snapshot, "id"), snapshot_id);
    assert_eq!(str_of(snapshot, "documentId"), "doc-1");
    assert_eq!(str_of(snapshot, "content"), "It was dark.");
    assert_eq!(str_of(snapshot, "reason"), "manual");
    assert_eq!(str_of(snapshot, "note"), "Before MCP write");
    assert_eq!(str_of(snapshot, "contentFormat"), "markdown");
    let read = read_snapshot(&root.path, "project-1", &snapshot_id).expect("read");
    assert_eq!(str_of(&read, "contentFormat"), "markdown");
    assert_eq!(num(&root.project_json(), "revision"), before + 1.0);
}

#[test]
fn write_scene_skips_the_snapshot_when_asked_and_for_new_scenes() {
    let root = make_root(&fixture());
    let mut input = overwrite("doc-1", "It was bright.");
    input.snapshot = Some(false);
    let overwritten = write_scene(&root.path, &mut env(), "project-1", &input).expect("write");
    assert_eq!(overwritten.get("snapshotId"), Some(&JsValue::Null));

    let created = write_scene(
        &root.path,
        &mut env(),
        "project-1",
        &WriteSceneInput {
            title: Some(s("New")),
            markdown: s("Fresh."),
            ..WriteSceneInput::default()
        },
    )
    .expect("create");
    assert_eq!(created.get("snapshotId"), Some(&JsValue::Null));
    assert!(snapshots(&root).is_empty());
}

#[test]
fn write_scene_caps_one_documents_snapshots_at_ten() {
    let seeded: Vec<String> = (0..10)
        .map(|i| {
            format!(
                r#"{{"id":"snap-{i}","documentId":"doc-1","createdAt":{},"title":"Opening Scene","content":"old {i}","reason":"manual"}}"#,
                NOW - 10_000.0 + f64::from(i)
            )
        })
        .collect();
    let root = make_root(&stored(&project_fixture(Some(&format!(
        "[{}]",
        seeded.join(",")
    )))));

    let scene = write_scene(
        &root.path,
        &mut env(),
        "project-1",
        &overwrite("doc-1", "It was bright."),
    )
    .expect("write");

    let snapshots = snapshots(&root);
    assert_eq!(snapshots.len(), 10);
    let kept: Vec<String> = snapshots.iter().map(|s| str_of(s, "id")).collect();
    assert!(!kept.contains(&"snap-0".to_owned()));
    let id = str_of(&scene, "snapshotId");
    let new = snapshots
        .iter()
        .find(|s| str_of(s, "id") == id)
        .expect("new snapshot");
    assert_eq!(str_of(new, "content"), "It was dark.");
}

#[test]
fn create_wiki_page_puts_a_page_in_the_wiki_with_folder_and_label() {
    let root = make_root(&fixture());
    let page = create_wiki_page(
        &root.path,
        &mut env(),
        "project-1",
        &WikiPageInput {
            title: s("Old Town"),
            folder_id: Some(s("folder-2")),
            content: Some(s("Cobblestones.")),
            label_id: Some(s("label-2")),
        },
    )
    .expect("create");

    let id = str_of(&page, "id");
    let text = root.read(&root.doc_path(&id));
    assert!(text.contains("space: \"Wiki\""));
    assert!(text.contains("folderId: \"folder-2\""));
    assert!(text.contains("labelId: \"label-2\""));
    let reread = root.reread();
    let document = document(&reread, &id).expect("page");
    assert_eq!(str_of(document, "space"), "Wiki");
    assert!(str_of(document, "markdownSource").contains("Cobblestones"));
}

#[test]
fn create_wiki_page_rejects_unknown_folders_and_labels_without_writing() {
    let root = make_root(&fixture());
    let before = root.read(&root.project_path());
    let lost = |folder: Option<&str>, label: Option<&str>| WikiPageInput {
        title: s("Lost"),
        folder_id: folder.map(s),
        label_id: label.map(s),
        content: None,
    };
    let error = create_wiki_page(
        &root.path,
        &mut env(),
        "project-1",
        &lost(Some("folder-nope"), None),
    )
    .expect_err("folder");
    assert!(error.starts_with("Wiki folder not found"));
    let error = create_wiki_page(
        &root.path,
        &mut env(),
        "project-1",
        &lost(None, Some("label-nope")),
    )
    .expect_err("label");
    assert!(error.starts_with("Label not found"));
    assert_eq!(root.read(&root.project_path()), before);
}

#[test]
fn update_wiki_page_changes_only_the_given_fields_and_snapshots_the_old_text() {
    let root = make_root(&fixture());
    let page = update_wiki_page(
        &root.path,
        &mut env(),
        "project-1",
        &WikiPageUpdate {
            page_id: s("doc-2"),
            content: Some(s("Mara maps the coast.")),
            ..WikiPageUpdate::default()
        },
    )
    .expect("update");

    let snapshot_id = str_of(&page, "snapshotId");
    assert!(!snapshot_id.is_empty());
    let text = root.read(&root.doc_path("doc-2"));
    assert!(text.contains("title: \"Mara\""));
    assert!(text.contains("folderId: \"folder-1\""));
    assert!(text.contains("labelId: \"label-1\""));
    assert!(text.contains("\"occupation\":\"Cartographer\""));

    let reread = root.reread();
    let document = document(&reread, "doc-2").expect("page");
    assert!(str_of(document, "markdownSource").contains("maps the coast"));
    assert_eq!(
        str_of(document.get("character").expect("profile"), "occupation"),
        "Cartographer"
    );
    assert_eq!(
        str_of(meta(&reread, "documents", "doc-2"), "updatedBy"),
        "mcp"
    );
    let snapshots = snapshots(&root);
    let snapshot = snapshots
        .iter()
        .find(|s| str_of(s, "id") == snapshot_id)
        .expect("snapshot");
    assert_eq!(str_of(snapshot, "content"), "Notes about Mara.");
}

#[test]
fn update_wiki_page_fails_on_a_stale_revision_and_writes_nothing() {
    let root = make_root(&fixture());
    let page_before = root.read(&root.doc_path("doc-2"));
    let project_before = root.read(&root.project_path());
    let error = update_wiki_page(
        &root.path,
        &mut env(),
        "project-1",
        &WikiPageUpdate {
            page_id: s("doc-2"),
            title: Some(s("Nope")),
            expected_revision: Some(99.0),
            ..WikiPageUpdate::default()
        },
    )
    .expect_err("stale");
    assert!(error.contains("expectedRevision"), "{error}");
    assert_eq!(root.read(&root.doc_path("doc-2")), page_before);
    assert_eq!(root.read(&root.project_path()), project_before);
}

#[test]
fn update_wiki_page_refuses_pages_outside_the_wiki() {
    let root = make_root(&fixture());
    let error = update_wiki_page(
        &root.path,
        &mut env(),
        "project-1",
        &WikiPageUpdate {
            page_id: s("doc-1"),
            title: Some(s("No")),
            ..WikiPageUpdate::default()
        },
    )
    .expect_err("not wiki");
    assert!(error.contains("not a Wiki page"), "{error}");
}

#[test]
fn set_label_sets_and_clears_a_document_label() {
    let root = make_root(&fixture());
    let doc = LabelTarget::Document(s("doc-1"));
    set_label(
        &root.path,
        &mut env(),
        "project-1",
        &doc,
        Some(&s("label-1")),
    )
    .expect("set");
    assert!(
        root.read(&root.doc_path("doc-1"))
            .contains("labelId: \"label-1\"")
    );
    set_label(&root.path, &mut env(), "project-1", &doc, None).expect("clear");
    assert!(!root.read(&root.doc_path("doc-1")).contains("labelId:"));
    // The app reads the cleared document without counting it as an outside edit.
    let reread = root.reread();
    assert_eq!(
        str_of(meta(&reread, "documents", "doc-1"), "updatedBy"),
        "mcp"
    );
}

#[test]
fn set_label_rejects_unknown_labels_without_writing() {
    let root = make_root(&fixture());
    let before = root.read(&root.doc_path("doc-1"));
    let error = set_label(
        &root.path,
        &mut env(),
        "project-1",
        &LabelTarget::Document(s("doc-1")),
        Some(&s("label-nope")),
    )
    .expect_err("unknown");
    assert!(error.starts_with("Label not found"));
    assert_eq!(root.read(&root.doc_path("doc-1")), before);
}

#[test]
fn set_label_labels_a_chapter() {
    let root = make_root(&fixture());
    let result = set_label(
        &root.path,
        &mut env(),
        "project-1",
        &LabelTarget::Chapter(s("chapter-1")),
        Some(&s("label-2")),
    )
    .expect("label");
    assert_eq!(str_of(&result, "chapterId"), "chapter-1");
    let chapter =
        prosecraft_core::js::parse(&root.read(&root.chapter_path("chapter-1"))).expect("json");
    assert_eq!(
        str_of(chapter.get("record").expect("record"), "labelId"),
        "label-2"
    );
    let reread = root.reread();
    assert_eq!(
        str_of(meta(&reread, "chapters", "chapter-1"), "updatedBy"),
        "mcp"
    );
}

fn comment(quote: &str, text: &str, author: Option<&str>) -> CommentInput {
    CommentInput {
        document_id: s("doc-3"),
        quote: s(quote),
        text: s(text),
        author: author.map(s),
    }
}

#[test]
fn add_comment_anchors_on_the_quote_with_its_context() {
    let root = make_root(&fixture());
    let added = add_comment(
        &root.path,
        &mut env(),
        "project-1",
        &comment("was grey", "Nice image.", Some("Reader")),
    )
    .expect("add");
    assert_eq!(str_of(&added, "documentId"), "doc-3");
    assert_eq!(str_of(&added, "quote"), "was grey");
    assert_eq!(str_of(&added, "prefix"), "The harbor ");
    assert_eq!(str_of(&added, "suffix"), ". The harbor was cold.");
    assert_eq!(str_of(&added, "body"), "Nice image.");
    assert_eq!(str_of(&added, "author"), "Reader");
    assert_eq!(added.get("resolved"), Some(&JsValue::Bool(false)));
    let stored = root
        .project_section()
        .get("comments")
        .cloned()
        .expect("comments");
    assert!(ids(&stored).contains(&str_of(&added, "id")));
}

#[test]
fn add_comment_fails_on_an_ambiguous_or_missing_quote_and_writes_nothing() {
    let root = make_root(&fixture());
    let before = root.read(&root.project_path());
    let error = add_comment(
        &root.path,
        &mut env(),
        "project-1",
        &comment("The harbor", "Which one?", None),
    )
    .expect_err("twice");
    assert!(error.contains("more than once"));
    let error = add_comment(
        &root.path,
        &mut env(),
        "project-1",
        &comment("not there", "Hm", None),
    )
    .expect_err("missing");
    assert!(error.contains("not found"));
    assert_eq!(root.read(&root.project_path()), before);
}

#[test]
fn resolve_comment_marks_it_resolved() {
    let root = make_root(&fixture());
    let added = add_comment(
        &root.path,
        &mut env(),
        "project-1",
        &comment("was cold", "Agreed.", None),
    )
    .expect("add");
    assert_eq!(str_of(&added, "author"), "Me");
    let id = str_of(&added, "id");
    let result = resolve_comment(&root.path, &mut env(), "project-1", &id).expect("resolve");
    assert_eq!(result.get("resolved"), Some(&JsValue::Bool(true)));
    let stored = root
        .project_section()
        .get("comments")
        .cloned()
        .expect("comments");
    let entry = stored
        .as_array()
        .expect("list")
        .iter()
        .find(|c| str_of(c, "id") == id)
        .expect("comment");
    assert_eq!(entry.get("resolved"), Some(&JsValue::Bool(true)));
}

/// Unlike the TypeScript server, comments and snapshots it doesn't recognise (a newer app's
/// fields, a hand-edited entry) stay in the project when a tool writes it.
#[test]
fn writes_keep_comments_and_snapshots_the_server_does_not_recognise() {
    let lists = r#"[{"id": "snap-odd", "documentId": "doc-1", "title": "Opening Scene", "content": "<p>Old</p>", "createdAt": 1, "reason": "future-reason"}],
  "comments": [{"id": "c-odd", "documentId": "doc-3", "quote": "grey", "body": "x", "createdAt": 1, "resolved": "maybe"}]"#;
    let root = make_root(&stored(&project_fixture(Some(lists))));
    let added = add_comment(
        &root.path,
        &mut env(),
        "project-1",
        &comment("was grey", "Nice image.", None),
    )
    .expect("add");
    write_scene(
        &root.path,
        &mut env(),
        "project-1",
        &overwrite("doc-1", "It was bright."),
    )
    .expect("write");
    resolve_comment(&root.path, &mut env(), "project-1", &str_of(&added, "id")).expect("resolve");
    let error =
        resolve_comment(&root.path, &mut env(), "project-1", "c-odd").expect_err("unrecognised");
    assert!(error.contains("Comment not found"));
    let project = root.project_section();
    let comments: Vec<String> = project.get("comments").map(ids).unwrap_or_default();
    assert_eq!(comments, vec!["c-odd".to_owned(), str_of(&added, "id")]);
    let snapshots = snapshots(&root);
    assert_eq!(snapshots.len(), 2);
    assert_eq!(str_of(&snapshots[0], "id"), "snap-odd");
    assert_eq!(str_of(&snapshots[0], "reason"), "future-reason");
}
