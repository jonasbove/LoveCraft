//! `mcp/src/searchTools.test.ts`: the search, its parity with the app search, and the list
//! filters.

mod common;

use common::{ids, make_root, num, stored, str_of};
use prosecraft_core::js::{self, JsString, JsValue};
use prosecraft_core::search::{self, SearchOptions as AppOptions};
use prosecraft_core::sync::open_layout::normalize_document;
use prosecraft_mcp::search_tools::{SearchOptions, Space, search_documents};
use prosecraft_mcp::store::{ListFilter, list_chapters, list_scenes};

const LONG_BODY: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.";

fn project_fixture() -> String {
    format!(
        r##"{{
  "id": "project-1", "name": "Search Novel", "author": "Ada Writer", "activeDocumentId": "doc-1",
  "projectGoal": 50000, "chapterGoals": {{}}, "activity": {{}},
  "labels": [{{"id": "label-1", "name": "Plot", "color": "red"}}, {{"id": "label-2", "name": "Character", "color": "blue"}}],
  "researchFolders": [{{"id": "folder-1", "title": "Characters", "parentId": null}}],
  "documents": [
    {{"id": "doc-1", "title": "Opening Scene", "content": "<h1>Docks</h1><p>It was dark in the <strong>harbor</strong> and the Käpten waited.</p>",
      "markdownSource": "# Docks\n\nIt was dark in the **harbor** and the Käpten waited.", "chapterId": "chapter-1", "labelId": "label-1"}},
    {{"id": "doc-2", "title": "Käpten's Log", "content": "<p>The captain logs the tide.</p>", "chapterId": "chapter-2", "labelId": "label-2"}},
    {{"id": "doc-3", "title": "Mara", "content": "<p>Mara is a cartographer and a harbor pilot.</p>", "space": "Wiki", "folderId": "folder-1", "labelId": "label-2"}},
    {{"id": "doc-4", "title": "Cut Scene", "content": "<p>Harbor words in the trash.</p>", "chapterId": "chapter-1", "labelId": "label-1", "trashed": true}},
    {{"id": "doc-5", "title": "Old Chapter", "content": "<p>Harbor in a trashed chapter.</p>", "chapterId": "chapter-3", "labelId": "label-1"}},
    {{"id": "doc-6", "title": "Loose Page", "content": "<p>Harbor in a loose page and harbors beyond it.</p>", "space": "Wiki"}},
    {{"id": "doc-7", "title": "Hidden Draft", "content": "<p>Harbor without a chapter or space.</p>"}},
    {{"id": "doc-8", "title": "Long Read", "content": "<p>{LONG_BODY}</p>", "chapterId": "chapter-2", "labelId": "label-1"}},
    {{"id": "doc-9", "title": "Harbor Notes", "content": "<p>The harbor at night.</p>", "chapterId": "chapter-2", "labelId": "label-1"}}
  ],
  "chapters": [
    {{"id": "chapter-1", "title": "Chapter One", "expanded": true, "labelId": "label-2"}},
    {{"id": "chapter-2", "title": "Chapter Two", "expanded": false, "labelId": "label-1"}},
    {{"id": "chapter-3", "title": "Old Chapter", "expanded": false, "labelId": "label-2", "trashed": true}}
  ]
}}"##
    )
}

fn root() -> common::Root {
    make_root(&stored(&project_fixture()))
}

fn search(root: &common::Root, query: &str, options: SearchOptions) -> JsValue {
    search_documents(&root.path, "project-1", &JsString::from(query), &options).expect("search")
}

fn matches(hits: &JsValue) -> Vec<f64> {
    hits.as_array()
        .expect("list")
        .iter()
        .map(|h| num(h, "matches"))
        .collect()
}

#[test]
fn folds_diacritics_when_the_search_ignores_case() {
    let root = root();
    let hits = search(&root, "kapten", SearchOptions::default());
    assert_eq!(ids(&hits), ["doc-2", "doc-1"]);
    assert_eq!(matches(&hits), [1.0, 1.0]);
}

#[test]
fn matches_case_and_diacritics_exactly_when_case_sensitive() {
    let root = root();
    let exact = SearchOptions {
        case_sensitive: true,
        ..SearchOptions::default()
    };
    assert_eq!(js::stringify(&search(&root, "kapten", exact.clone())), "[]");
    assert_eq!(ids(&search(&root, "Käpten", exact)), ["doc-2", "doc-1"]);
}

#[test]
fn whole_word_skips_words_that_only_contain_the_query() {
    let root = root();
    let wiki = SearchOptions {
        space: Some(Space::Wiki),
        ..SearchOptions::default()
    };
    let loose = search(&root, "harbor", wiki.clone());
    assert_eq!(ids(&loose), ["doc-3", "doc-6"]);
    assert_eq!(matches(&loose)[1], 2.0);
    let whole = search(
        &root,
        "harbor",
        SearchOptions {
            whole_word: true,
            ..wiki
        },
    );
    assert_eq!(ids(&whole), ["doc-3", "doc-6"]);
    assert_eq!(matches(&whole)[1], 1.0);
}

#[test]
fn filters_by_space_chapter_folder_and_label() {
    let root = root();
    let with = |options: SearchOptions| ids(&search(&root, "harbor", options));
    assert_eq!(
        with(SearchOptions {
            space: Some(Space::Wiki),
            ..SearchOptions::default()
        }),
        ["doc-3", "doc-6"]
    );
    assert_eq!(
        with(SearchOptions {
            chapter_id: Some("chapter-2".into()),
            ..SearchOptions::default()
        }),
        ["doc-9"]
    );
    assert_eq!(
        with(SearchOptions {
            folder_id: Some("folder-1".into()),
            ..SearchOptions::default()
        }),
        ["doc-3"]
    );
    assert_eq!(
        with(SearchOptions {
            label_id: Some("label-1".into()),
            ..SearchOptions::default()
        }),
        ["doc-9", "doc-1"]
    );
}

#[test]
fn hides_the_trash_unless_asked() {
    let root = root();
    assert_eq!(
        ids(&search(&root, "harbor", SearchOptions::default())),
        ["doc-9", "doc-1", "doc-3", "doc-6"]
    );
    let all = SearchOptions {
        include_trashed: true,
        ..SearchOptions::default()
    };
    assert_eq!(
        ids(&search(&root, "harbor", all)),
        ["doc-9", "doc-1", "doc-4", "doc-5", "doc-3", "doc-6"]
    );
}

#[test]
fn returns_a_plain_text_snippet_with_chapter_and_label() {
    let root = root();
    let hits = search(
        &root,
        "harbor",
        SearchOptions {
            chapter_id: Some("chapter-1".into()),
            ..SearchOptions::default()
        },
    );
    assert_eq!(
        js::stringify(&hits.as_array().expect("list")[0]),
        r#"{"id":"doc-1","title":"Opening Scene","space":"scene","chapterTitle":"Chapter One","labelId":"label-1","matches":1,"snippet":"Docks It was dark in the harbor and the Käpten waited."}"#
    );
    let wiki = search(&root, "cartographer", SearchOptions::default());
    let hit = &wiki.as_array().expect("list")[0];
    assert_eq!(str_of(hit, "id"), "doc-3");
    assert_eq!(str_of(hit, "space"), "wiki");
    assert_eq!(hit.get("chapterTitle"), Some(&JsValue::Null));
    assert_eq!(str_of(hit, "labelId"), "label-2");
}

#[test]
fn cuts_long_bodies_to_a_short_snippet() {
    let root = root();
    let hits = search(&root, "veniam", SearchOptions::default());
    let hit = &hits.as_array().expect("list")[0];
    assert_eq!(str_of(hit, "id"), "doc-8");
    let snippet = str_of(hit, "snippet");
    assert!(snippet.contains("veniam"));
    assert!(snippet.starts_with('…') && snippet.ends_with('…'));
    assert!(snippet.trim_matches('…').encode_utf16().count() <= 80);
}

#[test]
fn caps_the_results_and_ignores_blank_queries() {
    let root = root();
    let limit = |n: f64| SearchOptions {
        limit: Some(n),
        ..SearchOptions::default()
    };
    assert_eq!(
        ids(&search(&root, "harbor", limit(2.0))),
        ["doc-9", "doc-1"]
    );
    assert_eq!(
        search(&root, "harbor", limit(0.0))
            .as_array()
            .map(<[JsValue]>::len),
        Some(1)
    );
    assert_eq!(
        js::stringify(&search(&root, "   ", SearchOptions::default())),
        "[]"
    );
}

#[test]
fn matches_the_app_search_on_the_same_inputs() {
    let root = root();
    let project = js::parse(&project_fixture()).expect("fixture");
    let documents: Vec<JsValue> = project
        .get("documents")
        .and_then(JsValue::as_array)
        .expect("documents")
        .iter()
        .map(|d| JsValue::Object(normalize_document(d.as_object().expect("document"))))
        .collect();
    let chapters = project
        .get("chapters")
        .and_then(JsValue::as_array)
        .expect("chapters")
        .to_vec();
    let variants = [
        AppOptions::default(),
        AppOptions {
            case_sensitive: true,
            ..AppOptions::default()
        },
        AppOptions {
            whole_word: true,
            ..AppOptions::default()
        },
        AppOptions {
            include_trashed: true,
            ..AppOptions::default()
        },
    ];
    for query in ["harbor", "Käpten", "kapten", "harbors", "veniam"] {
        for options in variants {
            let expected: Vec<String> =
                search::search_documents(&documents, &chapters, query, options)
                    .into_iter()
                    .map(|hit| {
                        format!(
                            "{} {} {} {:?} {} {}",
                            hit.document_id,
                            hit.title,
                            if hit.wiki { "wiki" } else { "scene" },
                            hit.chapter_title,
                            hit.matches,
                            String::from_utf16_lossy(&hit.snippet)
                        )
                    })
                    .collect();
            let mcp = SearchOptions {
                case_sensitive: options.case_sensitive,
                whole_word: options.whole_word,
                include_trashed: options.include_trashed,
                limit: Some(100.0),
                ..SearchOptions::default()
            };
            let actual: Vec<String> = search(&root, query, mcp)
                .as_array()
                .expect("list")
                .iter()
                .map(|hit| {
                    let chapter = hit
                        .get("chapterTitle")
                        .and_then(JsValue::as_js_string)
                        .map(|c| c.to_string_lossy());
                    format!(
                        "{} {} {} {:?} {} {}",
                        str_of(hit, "id"),
                        str_of(hit, "title"),
                        str_of(hit, "space"),
                        chapter,
                        num(hit, "matches"),
                        str_of(hit, "snippet")
                    )
                })
                .collect();
            assert_eq!(actual, expected, "{query} {options:?}");
        }
    }
}

#[test]
fn scenes_carry_space_folder_and_label() {
    let root = root();
    let scenes = list_scenes(&root.path, "project-1", &ListFilter::default()).expect("scenes");
    let find = |id: &str| scenes.iter().find(|s| s.id == id).expect("scene");
    let doc1 = find("doc-1");
    assert!(!doc1.wiki);
    assert_eq!(
        doc1.chapter_id
            .as_ref()
            .map(JsString::to_string_lossy)
            .as_deref(),
        Some("chapter-1")
    );
    assert_eq!(doc1.folder_id, JsValue::Null);
    assert_eq!(doc1.label_id, JsValue::str("label-1"));
    let doc3 = find("doc-3");
    assert!(doc3.wiki);
    assert_eq!(doc3.folder_id, JsValue::str("folder-1"));
    assert_eq!(doc3.label_id, JsValue::str("label-2"));
    assert!(doc3.chapter_id.is_none());
    let doc7 = find("doc-7");
    assert!(!doc7.wiki);
    assert_eq!(doc7.label_id, JsValue::Null);
    assert!(doc7.chapter_id.is_none());
}

#[test]
fn lists_trashed_scenes_only_when_asked() {
    let root = root();
    let list = |filter: ListFilter| -> Vec<String> {
        list_scenes(&root.path, "project-1", &filter)
            .expect("scenes")
            .into_iter()
            .map(|s| s.id)
            .collect()
    };
    assert!(!list(ListFilter::default()).contains(&"doc-4".to_owned()));
    assert_eq!(
        list(ListFilter {
            trashed: Some(true),
            ..ListFilter::default()
        }),
        ["doc-4"]
    );
    assert_eq!(
        list(ListFilter {
            label_id: Some("label-2".into()),
            ..ListFilter::default()
        }),
        ["doc-2", "doc-3"]
    );
}

#[test]
fn filters_chapters_by_label_and_trash() {
    let root = root();
    let list = |filter: ListFilter| {
        ids(&JsValue::Array(
            list_chapters(&root.path, "project-1", &filter).expect("chapters"),
        ))
    };
    assert_eq!(list(ListFilter::default()), ["chapter-1", "chapter-2"]);
    assert_eq!(
        list(ListFilter {
            trashed: Some(true),
            ..ListFilter::default()
        }),
        ["chapter-3"]
    );
    assert_eq!(
        list(ListFilter {
            label_id: Some("label-1".into()),
            ..ListFilter::default()
        }),
        ["chapter-2"]
    );
    let first = list_chapters(&root.path, "project-1", &ListFilter::default())
        .expect("chapters")
        .remove(0);
    assert_eq!(str_of(&first, "labelId"), "label-2");
    assert_eq!(first.get("trashed"), Some(&JsValue::Bool(false)));
}
