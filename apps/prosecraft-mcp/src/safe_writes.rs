//! The tools that write documents, labels and comments (`safeWrites.ts`): each under the write
//! lock, through temp files, with `expectedRevision` checks, a snapshot before an overwrite, the
//! revision bumped by "mcp" and the hashes the app checks.

use prosecraft_core::js::{JsObject, JsString, JsValue};

use crate::env::Env;
use crate::fsx::{self, join};
use crate::jsx::{self, object, prop};
use crate::store::{
    self, Found, assert_expected_revision, bump_meta, bump_project_header, chapter_file,
    document_path, entity, field_mut, find_project_dir, is_missing, new_meta, normalize_body,
    parse_markdown, serialize_markdown, set_entity, write_project_header,
};

/// Oldest snapshots of a document are dropped once it has more than this many.
pub const MAX_SNAPSHOTS_PER_DOCUMENT: usize = 10;
const SNAPSHOT_NOTE: &str = prosecraft_core::snapshots::MCP_SNAPSHOT_NOTE;
const CONTEXT_LENGTH: usize = 24;
const MAX_QUOTE_LENGTH: usize = 2000;
const MAX_COMMENT_BODY_LENGTH: usize = 4000;
const MAX_COMMENT_AUTHOR_LENGTH: usize = 60;

/// What `write_scene` is asked to do.
#[derive(Clone, Debug, Default)]
pub struct WriteSceneInput {
    pub scene_id: Option<JsString>,
    pub chapter_id: Option<JsString>,
    pub title: Option<JsString>,
    pub markdown: JsString,
    pub create: Option<bool>,
    pub snapshot: Option<bool>,
    pub expected_revision: Option<f64>,
}

/// What `create_wiki_page` is asked to do.
#[derive(Clone, Debug, Default)]
pub struct WikiPageInput {
    pub title: JsString,
    pub folder_id: Option<JsString>,
    pub content: Option<JsString>,
    pub label_id: Option<JsString>,
}

/// What `update_wiki_page` is asked to do.
#[derive(Clone, Debug, Default)]
pub struct WikiPageUpdate {
    pub page_id: JsString,
    pub title: Option<JsString>,
    pub content: Option<JsString>,
    pub folder_id: Option<JsString>,
    pub label_id: Option<JsString>,
    pub expected_revision: Option<f64>,
}

/// Which entity `set_label` labels.
#[derive(Clone, Debug)]
pub enum LabelTarget {
    Document(JsString),
    Chapter(JsString),
}

/// What `add_comment` is asked to do.
#[derive(Clone, Debug, Default)]
pub struct CommentInput {
    pub document_id: JsString,
    pub quote: JsString,
    pub text: JsString,
    pub author: Option<JsString>,
}

fn lossy(text: &JsString) -> String {
    text.to_string_lossy()
}

fn text(value: &JsString) -> JsValue {
    JsValue::String(value.clone())
}

/// `header.project[key]`, throwing like V8 when `project` is missing.
fn project_data<'a>(header: &'a JsValue, key: &str) -> Result<Option<&'a JsValue>, String> {
    prop(jsx::get(header, "project"), key)
}

fn has_str(value: &JsValue, key: &str) -> bool {
    jsx::as_str(jsx::get(value, key)).is_some()
}

/// The guard `readSnapshots` keeps entries by.
fn is_snapshot(value: &JsValue) -> bool {
    jsx::is_object_like(Some(value))
        && ["id", "documentId", "title", "content"]
            .iter()
            .all(|k| has_str(value, k))
        && matches!(jsx::get(value, "createdAt"), Some(JsValue::Number(_)))
        && (jsx::is_str(jsx::get(value, "reason"), "manual")
            || jsx::is_str(jsx::get(value, "reason"), "before-restore"))
}

/// The guard `readComments` keeps entries by.
fn is_comment(value: &JsValue) -> bool {
    jsx::is_object_like(Some(value))
        && ["id", "documentId", "quote", "body"]
            .iter()
            .all(|k| has_str(value, k))
        && matches!(jsx::get(value, "createdAt"), Some(JsValue::Number(_)))
        && matches!(jsx::get(value, "resolved"), Some(JsValue::Bool(_)))
}

/// `readSnapshots` / `readComments`: the entries that pass `guard`. Entries that fail it are not
/// written back, as in the TypeScript server.
fn read_list(
    header: &JsValue,
    key: &str,
    guard: fn(&JsValue) -> bool,
) -> Result<Vec<JsValue>, String> {
    Ok(project_data(header, key)?
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .filter(|v| guard(v))
        .cloned()
        .collect())
}

fn set_project_list(header: &mut JsValue, key: &str, list: Vec<JsValue>) -> Result<(), String> {
    jsx::set_prop(field_mut(header, "project"), key, JsValue::Array(list))
}

fn created_at(entry: &JsValue) -> f64 {
    jsx::get(entry, "createdAt")
        .and_then(JsValue::as_number)
        .unwrap_or(0.0)
}

fn id_of(entry: &JsValue) -> Option<&JsString> {
    jsx::as_str(jsx::get(entry, "id"))
}

/// `addSnapshot`: adds `snapshot` and drops its document's oldest beyond the cap.
fn add_snapshot(snapshots: Vec<JsValue>, snapshot: JsValue) -> Vec<JsValue> {
    let document = jsx::get(&snapshot, "documentId").cloned();
    let same_document = |entry: &JsValue| jsx::get(entry, "documentId") == document.as_ref();
    let mut own: Vec<&JsValue> = snapshots
        .iter()
        .filter(|e| same_document(e))
        .chain(std::iter::once(&snapshot))
        .collect();
    own.sort_by(|a, b| crate::read_tools::compare_numbers(created_at(a), created_at(b)));
    let keep: Vec<JsString> = own
        .iter()
        .skip(own.len().saturating_sub(MAX_SNAPSHOTS_PER_DOCUMENT))
        .filter_map(|e| id_of(e).cloned())
        .collect();
    let mut all = snapshots.clone();
    all.push(snapshot.clone());
    all.into_iter()
        .filter(|e| !same_document(e) || id_of(e).is_some_and(|id| keep.contains(id)))
        .collect()
}

/// A "Before MCP write" snapshot of a document's previous title and Markdown.
fn before_write_snapshot(
    id: &str,
    document_id: &str,
    now: f64,
    title: JsValue,
    body: &str,
) -> JsValue {
    object(vec![
        ("id", JsValue::str(id)),
        ("documentId", JsValue::str(document_id)),
        ("createdAt", JsValue::Number(now)),
        ("title", title),
        ("content", JsValue::str(body)),
        ("contentFormat", JsValue::str("markdown")),
        ("reason", JsValue::str("manual")),
        ("note", JsValue::str(SNAPSHOT_NOTE)),
    ])
}

fn take_snapshot(header: &mut JsValue, snapshot: JsValue) -> Result<(), String> {
    let snapshots = read_list(header, "snapshots", is_snapshot)?;
    set_project_list(header, "snapshots", add_snapshot(snapshots, snapshot))
}

/// `assertLabel` and `assertFolder`: the id must be in the project's list.
fn assert_known(header: &JsValue, key: &str, id: &str, message: &str) -> Result<(), String> {
    let known = project_data(header, key)?
        .and_then(JsValue::as_array)
        .is_some_and(|list| {
            list.iter().any(|item| {
                jsx::is_object_like(Some(item)) && jsx::is_str(jsx::get(item, "id"), id)
            })
        });
    if known {
        Ok(())
    } else {
        Err(message.to_owned())
    }
}

fn assert_label(header: &JsValue, label_id: &str) -> Result<(), String> {
    assert_known(
        header,
        "labels",
        label_id,
        &format!("Label not found: {label_id}. Use list_labels to see the project labels."),
    )
}

fn assert_folder(header: &JsValue, folder_id: &str) -> Result<(), String> {
    assert_known(
        header,
        "researchFolders",
        folder_id,
        &format!(
            "Wiki folder not found: {folder_id}. Use list_wiki_folders to see the folder tree."
        ),
    )
}

fn read_document(dir: &str, id: &str) -> Result<(JsObject, String), String> {
    parse_markdown(&fsx::read_text(&document_path(dir, id))?)
}

/// `applyMeta`: the bumped meta into the front matter, the stale hash out.
fn apply_meta(fields: &mut JsObject, meta: &JsValue) {
    fields.remove("hash");
    for key in ["updatedAt", "revision", "updatedBy", "deletedAt"] {
        match jsx::get(meta, key) {
            Some(value) => fields.insert(key, value.clone()),
            None => {
                fields.remove(key);
            }
        }
    }
}

/// `header.order.documents` with `id` added; `only_new` adds it only when it is not there yet.
fn push_document_order(header: &mut JsValue, id: &str, only_new: bool) -> Result<(), String> {
    let current = jsx::nullish(prop(prop(Some(&*header), "order")?, "documents")?).cloned();
    let mut list = match current {
        None => Vec::new(),
        Some(JsValue::Array(items)) => items,
        Some(_) => {
            let method = if only_new { "includes" } else { "push" };
            return Err(format!("order.{method} is not a function"));
        }
    };
    if !only_new || jsx::index_of(&list, id) < 0 {
        list.push(JsValue::str(id));
    }
    jsx::set_prop(
        field_mut(header, "order"),
        "documents",
        JsValue::Array(list),
    )
}

/// `typeof value === "string" ? value : ""` for a title.
fn title_of(fields: &JsObject) -> JsValue {
    store::str_or_empty(fields.get("title"))
}

/// `writeScene`: creates or overwrites a scene, keeping every front matter field it does not own.
pub fn write_scene(
    root: &str,
    env: &mut dyn Env,
    project_id: &str,
    input: &WriteSceneInput,
) -> Result<JsValue, String> {
    fsx::with_lock(root, env, |env| {
        let Found { dir, mut header } = find_project_dir(root, project_id)?;
        let now = env.now();
        let id = match &input.scene_id {
            Some(id) => lossy(id),
            None => env.uuid()?,
        };
        let existing = entity(&header, "documents", &id)?.cloned();
        if input.scene_id.as_ref().is_some_and(|s| !s.is_empty())
            && jsx::falsy(existing.as_ref())
            && input.create != Some(true)
        {
            return Err(format!(
                "Scene not found: {id}. Pass create: true to create a new scene with that id."
            ));
        }
        assert_expected_revision(
            &format!("Scene {id}"),
            input.expected_revision,
            existing.as_ref().and_then(|m| jsx::get(m, "revision")),
        )?;
        let (prev_fields, prev_body) = if jsx::falsy(existing.as_ref()) {
            (JsObject::new(), String::new())
        } else {
            read_document(&dir, &id)?
        };
        let chapter_id: Option<JsString> = input
            .chapter_id
            .clone()
            .or_else(|| jsx::as_str(prev_fields.get("chapterId")).cloned());
        if let Some(chapter) = chapter_id.as_ref().filter(|c| !c.is_empty()) {
            let chapters = jsx::nullish(store::entities(&header, "chapters")?);
            let meta = chapters.and_then(|c| jsx::get(c, &lossy(chapter)));
            if is_missing(meta) {
                return Err(format!("Chapter not found: {}", lossy(chapter)));
            }
        }
        let base = jsx::nullish(existing.as_ref())
            .cloned()
            .unwrap_or_else(new_meta);
        let meta = bump_meta(&base, now);

        // Start from the stored front matter so the fields this tool does not own survive.
        let mut fields = prev_fields.clone();
        fields.insert("id", JsValue::str(&id));
        let title = match &input.title {
            Some(title) => text(title),
            None => title_of(&prev_fields),
        };
        fields.insert("title", title.clone());
        if let Some(chapter) = chapter_id.as_ref().filter(|c| !c.is_empty()) {
            fields.insert("chapterId", text(chapter));
        }
        apply_meta(&mut fields, &meta);
        fields.insert("deletedAt", JsValue::Null);

        let mut snapshot_id = JsValue::Null;
        if !jsx::falsy(existing.as_ref()) && input.snapshot != Some(false) {
            let id_of_snapshot = env.uuid()?;
            let snapshot = before_write_snapshot(
                &id_of_snapshot,
                &id,
                now,
                title_of(&prev_fields),
                &prev_body,
            );
            snapshot_id = JsValue::str(&id_of_snapshot);
            take_snapshot(&mut header, snapshot)?;
        }

        let markdown = lossy(&input.markdown);
        fsx::mkdir_all(&join(&[&dir, "documents"]))?;
        fsx::write_atomic(
            &document_path(&dir, &id),
            &serialize_markdown(&fields, &markdown),
        )?;

        set_entity(&mut header, "documents", &id, meta.clone())?;
        push_document_order(&mut header, &id, true)?;
        bump_project_header(&mut header, now)?;
        write_project_header(&dir, &header)?;

        let chapter_out = chapter_id.filter(|c| !c.is_empty()).map(|c| text(&c));
        let title_out = JsValue::str(&jsx::to_js_string(Some(&title)));
        let mut out = store::scene_record(
            &id,
            title_out,
            chapter_out,
            &normalize_body(&markdown),
            meta,
            fields,
        );
        out.insert("snapshotId", snapshot_id);
        Ok(JsValue::Object(out))
    })
}

/// `{ id, title, markdown, meta, fields, snapshotId }`, the shape the Wiki tools return.
fn wiki_record(
    id: &str,
    title: JsValue,
    markdown: &str,
    meta: JsValue,
    fields: JsObject,
    snapshot_id: JsValue,
) -> JsValue {
    object(vec![
        ("id", JsValue::str(id)),
        ("title", title),
        ("markdown", JsValue::str(markdown)),
        ("meta", meta),
        ("fields", JsValue::Object(fields)),
        ("snapshotId", snapshot_id),
    ])
}

/// `createWikiPage`: a new document in the Wiki space.
pub fn create_wiki_page(
    root: &str,
    env: &mut dyn Env,
    project_id: &str,
    input: &WikiPageInput,
) -> Result<JsValue, String> {
    fsx::with_lock(root, env, |env| {
        let Found { dir, mut header } = find_project_dir(root, project_id)?;
        if let Some(folder) = &input.folder_id {
            assert_folder(&header, &lossy(folder))?;
        }
        if let Some(label) = &input.label_id {
            assert_label(&header, &lossy(label))?;
        }
        let now = env.now();
        let id = env.uuid()?;
        let meta = bump_meta(&new_meta(), now);
        let mut fields = JsObject::new();
        fields.insert("id", JsValue::str(&id));
        fields.insert("title", text(&input.title));
        fields.insert("space", JsValue::str("Wiki"));
        if let Some(folder) = &input.folder_id {
            fields.insert("folderId", text(folder));
        }
        if let Some(label) = &input.label_id {
            fields.insert("labelId", text(label));
        }
        for key in ["updatedAt", "revision", "updatedBy", "deletedAt"] {
            if let Some(value) = jsx::get(&meta, key) {
                fields.insert(key, value.clone());
            }
        }
        let content = input.content.as_ref().map(lossy).unwrap_or_default();
        fsx::mkdir_all(&join(&[&dir, "documents"]))?;
        fsx::write_atomic(
            &document_path(&dir, &id),
            &serialize_markdown(&fields, &content),
        )?;

        set_entity(&mut header, "documents", &id, meta.clone())?;
        push_document_order(&mut header, &id, false)?;
        bump_project_header(&mut header, now)?;
        write_project_header(&dir, &header)?;
        Ok(wiki_record(
            &id,
            text(&input.title),
            &normalize_body(&content),
            meta,
            fields,
            JsValue::Null,
        ))
    })
}

/// `updateWikiPage`: changes the given fields, keeps the rest, snapshots the previous text.
pub fn update_wiki_page(
    root: &str,
    env: &mut dyn Env,
    project_id: &str,
    input: &WikiPageUpdate,
) -> Result<JsValue, String> {
    fsx::with_lock(root, env, |env| {
        let page_id = lossy(&input.page_id);
        let Found { dir, mut header } = find_project_dir(root, project_id)?;
        let existing = entity(&header, "documents", &page_id)?;
        if is_missing(existing) {
            return Err(format!("Wiki page not found: {page_id}"));
        }
        let existing = existing.cloned().unwrap_or(JsValue::Null);
        assert_expected_revision(
            &format!("Wiki page {page_id}"),
            input.expected_revision,
            jsx::get(&existing, "revision"),
        )?;
        if let Some(folder) = &input.folder_id {
            assert_folder(&header, &lossy(folder))?;
        }
        if let Some(label) = &input.label_id {
            assert_label(&header, &lossy(label))?;
        }
        let (prev_fields, prev_body) = read_document(&dir, &page_id)?;
        if !jsx::is_str(prev_fields.get("space"), "Wiki") {
            return Err(format!("Document is not a Wiki page: {page_id}"));
        }
        if jsx::is_true(prev_fields.get("trashed")) {
            return Err(format!("Wiki page is in the trash: {page_id}"));
        }

        let now = env.now();
        let meta = bump_meta(&existing, now);
        let mut fields = prev_fields.clone();
        apply_meta(&mut fields, &meta);
        if let Some(title) = &input.title {
            fields.insert("title", text(title));
        }
        if let Some(folder) = &input.folder_id {
            fields.insert("folderId", text(folder));
        }
        if let Some(label) = &input.label_id {
            fields.insert("labelId", text(label));
        }
        let markdown = input.content.as_ref().map_or(prev_body.clone(), lossy);

        let snapshot_id = env.uuid()?;
        let snapshot = before_write_snapshot(
            &snapshot_id,
            &page_id,
            now,
            title_of(&prev_fields),
            &prev_body,
        );
        take_snapshot(&mut header, snapshot)?;

        fsx::write_atomic(
            &document_path(&dir, &page_id),
            &serialize_markdown(&fields, &markdown),
        )?;
        set_entity(&mut header, "documents", &page_id, meta.clone())?;
        bump_project_header(&mut header, now)?;
        write_project_header(&dir, &header)?;
        let title = JsValue::str(&jsx::to_js_string(
            jsx::nullish(fields.get("title")).or(Some(&JsValue::str(""))),
        ));
        Ok(wiki_record(
            &page_id,
            title,
            &normalize_body(&markdown),
            meta,
            fields,
            JsValue::str(&snapshot_id),
        ))
    })
}

/// `setLabel`: sets or clears (`None`) the label of one document or chapter.
pub fn set_label(
    root: &str,
    env: &mut dyn Env,
    project_id: &str,
    target: &LabelTarget,
    label_id: Option<&JsString>,
) -> Result<JsValue, String> {
    fsx::with_lock(root, env, |env| {
        let Found { dir, mut header } = find_project_dir(root, project_id)?;
        if let Some(label) = label_id {
            assert_label(&header, &lossy(label))?;
        }
        let now = env.now();
        let label_value = label_id.map_or(JsValue::Null, text);
        match target {
            LabelTarget::Document(document_id) => {
                let document_id = lossy(document_id);
                let meta = entity(&header, "documents", &document_id)?;
                if is_missing(meta) {
                    return Err(format!("Document not found: {document_id}"));
                }
                let meta = meta.cloned().unwrap_or(JsValue::Null);
                let (mut fields, body) = read_document(&dir, &document_id)?;
                match label_id {
                    Some(label) => fields.insert("labelId", text(label)),
                    None => {
                        fields.remove("labelId");
                    }
                }
                let next = bump_meta(&meta, now);
                apply_meta(&mut fields, &next);
                fsx::write_atomic(
                    &document_path(&dir, &document_id),
                    &serialize_markdown(&fields, &body),
                )?;
                let revision = jsx::get(&next, "revision")
                    .cloned()
                    .unwrap_or(JsValue::Null);
                set_entity(&mut header, "documents", &document_id, next)?;
                bump_project_header(&mut header, now)?;
                write_project_header(&dir, &header)?;
                Ok(object(vec![
                    ("documentId", JsValue::str(&document_id)),
                    ("labelId", label_value),
                    ("revision", revision),
                ]))
            }
            LabelTarget::Chapter(chapter_id) => {
                let chapter_id = lossy(chapter_id);
                let meta = entity(&header, "chapters", &chapter_id)?;
                if is_missing(meta) {
                    return Err(format!("Chapter not found: {chapter_id}"));
                }
                let meta = meta.cloned().unwrap_or(JsValue::Null);
                let file = store::chapter_path(&dir, &chapter_id);
                // Chapter files are `{ record, meta, hash }`; only the record carries labelId.
                let raw = store::json_parse(&fsx::read_text(&file)?)?;
                let Some(mut record) = jsx::get(&raw, "record")
                    .filter(|r| jsx::is_object_like(Some(r)) && jsx::is_object_like(Some(&raw)))
                    .cloned()
                else {
                    return Err(format!(
                        "Chapter {chapter_id} has an unexpected layout: {file}"
                    ));
                };
                if let JsValue::Object(fields) = &mut record {
                    match label_id {
                        Some(label) => fields.insert("labelId", text(label)),
                        None => {
                            fields.remove("labelId");
                        }
                    }
                }
                let next = bump_meta(&meta, now);
                fsx::write_atomic(&file, &chapter_file(&record, &next))?;
                let revision = jsx::get(&next, "revision")
                    .cloned()
                    .unwrap_or(JsValue::Null);
                set_entity(&mut header, "chapters", &chapter_id, next)?;
                bump_project_header(&mut header, now)?;
                write_project_header(&dir, &header)?;
                Ok(object(vec![
                    ("chapterId", JsValue::str(&chapter_id)),
                    ("labelId", label_value),
                    ("revision", revision),
                ]))
            }
        }
    })
}

/// `addComment`: a comment anchored to the one place the quote occurs in the document's
/// Markdown, with 24 units of context each side, as the app's `buildAnchor` keeps them.
pub fn add_comment(
    root: &str,
    env: &mut dyn Env,
    project_id: &str,
    input: &CommentInput,
) -> Result<JsValue, String> {
    fsx::with_lock(root, env, |env| {
        if jsx::trim_units(input.quote.units()).is_empty() {
            return Err("Quote must not be empty.".into());
        }
        let document_id = lossy(&input.document_id);
        let Found { dir, mut header } = find_project_dir(root, project_id)?;
        let meta = entity(&header, "documents", &document_id)?;
        if is_missing(meta) {
            return Err(format!("Document not found: {document_id}"));
        }
        let (fields, body) = read_document(&dir, &document_id)?;
        if jsx::is_true(fields.get("trashed")) {
            return Err(format!("Document is in the trash: {document_id}"));
        }
        let body = jsx::utf16(&body);
        let quote = input.quote.units();
        let Some(start) = jsx::index_of_units(&body, quote, 0) else {
            return Err(format!("Quote not found in document {document_id}."));
        };
        if jsx::index_of_units(&body, quote, start + 1).is_some() {
            return Err(format!(
                "Quote occurs more than once in document {document_id}; pick a longer, unique passage."
            ));
        }
        let end = start + quote.len();
        let quoted = jsx::slice_units(&body, start, end);
        let quoted = jsx::slice_units(quoted, 0, MAX_QUOTE_LENGTH);
        let prefix = jsx::slice_units(&body, start.saturating_sub(CONTEXT_LENGTH), start);
        let suffix = jsx::slice_units(&body, end, end + CONTEXT_LENGTH);
        let comment_body = jsx::slice_units(
            jsx::trim_units(input.text.units()),
            0,
            MAX_COMMENT_BODY_LENGTH,
        );
        let author = input
            .author
            .as_ref()
            .map(|a| jsx::slice_units(jsx::trim_units(a.units()), 0, MAX_COMMENT_AUTHOR_LENGTH))
            .filter(|a| !a.is_empty())
            .map_or_else(|| JsValue::str("Me"), jsx::units);
        let id = env.uuid()?;
        let created_at = env.now();
        let comment = object(vec![
            ("id", JsValue::str(&id)),
            ("documentId", JsValue::str(&document_id)),
            ("quote", jsx::units(quoted)),
            ("prefix", jsx::units(prefix)),
            ("suffix", jsx::units(suffix)),
            ("body", jsx::units(comment_body)),
            ("author", author),
            ("createdAt", JsValue::Number(created_at)),
            ("resolved", JsValue::Bool(false)),
        ]);
        let mut comments = read_list(&header, "comments", is_comment)?;
        comments.push(comment.clone());
        set_project_list(&mut header, "comments", comments)?;
        bump_project_header(&mut header, created_at)?;
        write_project_header(&dir, &header)?;
        Ok(comment)
    })
}

/// `resolveComment`.
pub fn resolve_comment(
    root: &str,
    env: &mut dyn Env,
    project_id: &str,
    comment_id: &str,
) -> Result<JsValue, String> {
    fsx::with_lock(root, env, |env| {
        let Found { dir, mut header } = find_project_dir(root, project_id)?;
        let comments = read_list(&header, "comments", is_comment)?;
        let Some(found) = comments
            .iter()
            .find(|c| jsx::is_str(jsx::get(c, "id"), comment_id))
            .cloned()
        else {
            return Err(format!("Comment not found: {comment_id}"));
        };
        let now = env.now();
        let resolved = comments
            .into_iter()
            .map(|entry| {
                if jsx::is_str(jsx::get(&entry, "id"), comment_id) {
                    let mut next = jsx::spread(Some(&entry));
                    next.insert("resolved", JsValue::Bool(true));
                    JsValue::Object(next)
                } else {
                    entry
                }
            })
            .collect();
        set_project_list(&mut header, "comments", resolved)?;
        bump_project_header(&mut header, now)?;
        write_project_header(&dir, &header)?;
        Ok(object(vec![
            (
                "id",
                jsx::get(&found, "id").cloned().unwrap_or(JsValue::Null),
            ),
            (
                "documentId",
                jsx::get(&found, "documentId")
                    .cloned()
                    .unwrap_or(JsValue::Null),
            ),
            ("resolved", JsValue::Bool(true)),
        ]))
    })
}
