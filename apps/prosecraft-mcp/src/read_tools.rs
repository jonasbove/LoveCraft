//! The read-only Wiki, label, comment and snapshot tools (`readTools.ts`).

use std::collections::{HashMap, HashSet};

use prosecraft_core::js::{JsObject, JsString, JsValue};

use crate::fsx;
use crate::jsx::{self, object};
use crate::store::{
    self, Found, document_path, entities, find_project_dir, is_deleted, parse_markdown,
    str_or_empty, str_or_null,
};

/// A live document as stored: its front matter, body and header meta.
pub struct StoredDocument {
    pub id: String,
    pub fields: JsObject,
    pub body: String,
    pub meta: JsValue,
}

/// A live chapter as stored.
pub struct StoredChapter {
    pub id: String,
    pub title: JsValue,
    pub label_id: Option<JsString>,
    pub trashed: bool,
}

/// `readStoredDocuments`: every live document in `order`, skipping malformed files.
pub fn read_stored_documents(dir: &str, header: &JsValue) -> Result<Vec<StoredDocument>, String> {
    let mut out = Vec::new();
    for (id, meta) in jsx::entries(jsx::nullish(entities(header, "documents")?)) {
        if is_deleted(&meta)? {
            continue;
        }
        // A malformed file must not hide the other documents.
        if let Ok((fields, body)) =
            fsx::read_text(&document_path(dir, &id)).and_then(|text| parse_markdown(&text))
        {
            out.push(StoredDocument {
                id,
                fields,
                body,
                meta,
            });
        }
    }
    let order = store::order(header, "documents")?;
    store::sort_by_order(&mut out, &order, |d| &d.id);
    Ok(out)
}

/// `readStoredChapters`: every live chapter, in the header's map order.
pub fn read_stored_chapters(dir: &str, header: &JsValue) -> Result<Vec<StoredChapter>, String> {
    let mut out = Vec::new();
    for (id, meta) in jsx::entries(jsx::nullish(entities(header, "chapters")?)) {
        if is_deleted(&meta)? {
            continue;
        }
        let Ok(raw) = fsx::read_text(&store::chapter_path(dir, &id))
            .and_then(|text| store::json_parse(&text))
        else {
            continue;
        };
        let record = jsx::get(&raw, "record").filter(|r| jsx::is_plain_object(Some(r)));
        let field = |key: &str| record.and_then(|r| jsx::get(r, key));
        out.push(StoredChapter {
            title: str_or_empty(field("title")),
            label_id: jsx::as_str(field("labelId")).cloned(),
            trashed: jsx::is_true(field("trashed")),
            id,
        });
    }
    Ok(out)
}

/// The `project` of `project.json`, or nothing when it is not an object.
fn project_field<'a>(header: &'a JsValue, key: &str) -> Option<&'a JsValue> {
    jsx::get(header, "project")
        .filter(|p| jsx::is_plain_object(Some(p)))
        .and_then(|p| jsx::get(p, key))
}

/// `readArray`: the entries of a list that pass `guard`.
fn read_array(value: Option<&JsValue>, guard: impl Fn(&JsValue) -> bool) -> Vec<&JsValue> {
    value
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .filter(|v| guard(v))
        .collect()
}

fn has_str(value: &JsValue, key: &str) -> bool {
    jsx::as_str(jsx::get(value, key)).is_some()
}

fn is_folder(value: &JsValue) -> bool {
    jsx::is_plain_object(Some(value))
        && has_str(value, "id")
        && has_str(value, "title")
        && (jsx::is_null(jsx::get(value, "parentId")) || has_str(value, "parentId"))
}

fn is_label(value: &JsValue) -> bool {
    jsx::is_plain_object(Some(value))
        && has_str(value, "id")
        && has_str(value, "name")
        && has_str(value, "color")
}

fn is_comment(value: &JsValue) -> bool {
    jsx::is_plain_object(Some(value))
        && ["id", "documentId", "quote", "body", "author"]
            .iter()
            .all(|k| has_str(value, k))
        && matches!(jsx::get(value, "createdAt"), Some(JsValue::Number(_)))
        && matches!(jsx::get(value, "resolved"), Some(JsValue::Bool(_)))
}

fn is_snapshot(value: &JsValue) -> bool {
    jsx::is_plain_object(Some(value))
        && ["id", "documentId", "title", "content"]
            .iter()
            .all(|k| has_str(value, k))
        && matches!(jsx::get(value, "createdAt"), Some(JsValue::Number(_)))
        && (jsx::is_str(jsx::get(value, "reason"), "manual")
            || jsx::is_str(jsx::get(value, "reason"), "before-restore"))
        && (jsx::get(value, "note").is_none() || has_str(value, "note"))
}

fn str_of(value: &JsValue, key: &str) -> String {
    jsx::as_str(jsx::get(value, key))
        .map(JsString::to_string_lossy)
        .unwrap_or_default()
}

fn number_of(value: &JsValue, key: &str) -> f64 {
    jsx::get(value, key)
        .and_then(JsValue::as_number)
        .unwrap_or(0.0)
}

/// `a - b` as a sort comparator: equal (and kept in order) when neither is smaller.
pub fn compare_numbers(a: f64, b: f64) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
}

fn copy(value: &JsValue, keys: &[&str]) -> JsObject {
    let mut out = JsObject::new();
    for key in keys {
        if let Some(item) = jsx::get(value, key) {
            out.insert(*key, item.clone());
        }
    }
    out
}

/// `pageKind`.
fn page_kind(fields: &JsObject) -> &'static str {
    if jsx::is_plain_object(fields.get("character")) {
        "character"
    } else if jsx::is_plain_object(fields.get("place")) {
        "place"
    } else {
        "page"
    }
}

struct Folder {
    title: JsValue,
    parent_id: Option<String>,
}

/// `folderMap`: the Wiki folders by id; a repeated id keeps its last entry.
fn folder_map(header: &JsValue) -> HashMap<String, Folder> {
    read_array(project_field(header, "researchFolders"), is_folder)
        .into_iter()
        .map(|f| {
            (
                str_of(f, "id"),
                Folder {
                    title: jsx::get(f, "title").cloned().unwrap_or(JsValue::Null),
                    parent_id: jsx::as_str(jsx::get(f, "parentId")).map(JsString::to_string_lossy),
                },
            )
        })
        .collect()
}

/// `folderPath`: folder titles from the root down; stops at a missing folder or a cycle.
fn folder_path(folder_id: Option<&str>, folders: &HashMap<String, Folder>) -> JsValue {
    let mut titles = Vec::new();
    let mut seen = HashSet::new();
    let mut current = folder_id.map(str::to_owned);
    while let Some(id) = current {
        if seen.contains(&id) {
            break;
        }
        let Some(folder) = folders.get(&id) else {
            break;
        };
        titles.push(folder.title.clone());
        current = folder.parent_id.clone();
        seen.insert(id);
    }
    titles.reverse();
    JsValue::Array(titles)
}

/// `listWikiPages`.
pub fn list_wiki_pages(
    root: &str,
    project_id: &str,
    folder_id: Option<&str>,
) -> Result<JsValue, String> {
    let Found { dir, header } = find_project_dir(root, project_id)?;
    let folders = folder_map(&header);
    let mut out = Vec::new();
    for document in read_stored_documents(&dir, &header)? {
        let fields = &document.fields;
        if !jsx::is_str(fields.get("space"), "Wiki") || jsx::is_true(fields.get("trashed")) {
            continue;
        }
        let page_folder = jsx::as_str(fields.get("folderId")).map(JsString::to_string_lossy);
        if folder_id.is_some_and(|wanted| page_folder.as_deref() != Some(wanted)) {
            continue;
        }
        let mut page = JsObject::new();
        page.insert("id", JsValue::str(&document.id));
        page.insert("title", str_or_empty(fields.get("title")));
        page.insert("folderId", str_or_null(fields.get("folderId")));
        page.insert("folderPath", folder_path(page_folder.as_deref(), &folders));
        page.insert("kind", JsValue::str(page_kind(fields)));
        insert_meta_numbers(&mut page, &document.meta);
        out.push(JsValue::Object(page));
    }
    Ok(JsValue::Array(out))
}

/// `updatedAt` and `revision` from a meta, left out when the meta has none (as `undefined`).
fn insert_meta_numbers(out: &mut JsObject, meta: &JsValue) {
    for key in ["updatedAt", "revision"] {
        if let Some(value) = jsx::get(meta, key) {
            out.insert(key, value.clone());
        }
    }
}

/// `readWikiPage`.
pub fn read_wiki_page(root: &str, project_id: &str, page_id: &str) -> Result<JsValue, String> {
    let Found { dir, header } = find_project_dir(root, project_id)?;
    let meta = store::entity(&header, "documents", page_id)?;
    if store::is_missing(meta) {
        return Err(format!("Wiki page not found: {page_id}"));
    }
    let meta = meta.cloned().unwrap_or(JsValue::Null);
    let file = document_path(&dir, page_id);
    let (fields, body) = fsx::read_text(&file)
        .and_then(|text| parse_markdown(&text))
        .map_err(|reason| format!("Wiki page {page_id} could not be read: {reason} ({file})"))?;
    if !jsx::is_str(fields.get("space"), "Wiki") {
        return Err(format!("Document is not a Wiki page: {page_id}"));
    }
    if jsx::is_true(fields.get("trashed")) {
        return Err(format!("Wiki page is in the trash: {page_id}"));
    }
    let folder = jsx::as_str(fields.get("folderId")).map(JsString::to_string_lossy);
    let mut page = JsObject::new();
    page.insert("id", JsValue::str(page_id));
    page.insert("title", str_or_empty(fields.get("title")));
    page.insert("folderId", str_or_null(fields.get("folderId")));
    page.insert(
        "folderPath",
        folder_path(folder.as_deref(), &folder_map(&header)),
    );
    page.insert("kind", JsValue::str(page_kind(&fields)));
    insert_meta_numbers(&mut page, &meta);
    page.insert("markdown", JsValue::str(&body));
    page.insert("fields", JsValue::Object(fields));
    page.insert("meta", meta);
    Ok(JsValue::Object(page))
}

/// Deepest folder nesting written out; deeper trees fail as `JSON.stringify` runs out of stack.
const MAX_FOLDER_DEPTH: usize = 1000;
/// Most folder nodes written out; a list of repeated ids can share one node many times over.
const MAX_FOLDER_NODES: usize = 100_000;

struct FolderNode {
    id: JsValue,
    title: JsValue,
    parent_id: JsValue,
    children: Vec<usize>,
}

/// `listWikiFolders`: the folder tree. A folder whose parent is gone is a root.
pub fn list_wiki_folders(root: &str, project_id: &str) -> Result<JsValue, String> {
    let Found { header, .. } = find_project_dir(root, project_id)?;
    let folders = read_array(project_field(&header, "researchFolders"), is_folder);
    // One node per id, holding the last entry with that id, as `nodes.set` leaves it.
    let mut nodes: Vec<FolderNode> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for folder in &folders {
        let node = FolderNode {
            id: jsx::get(folder, "id").cloned().unwrap_or(JsValue::Null),
            title: jsx::get(folder, "title").cloned().unwrap_or(JsValue::Null),
            parent_id: jsx::get(folder, "parentId")
                .cloned()
                .unwrap_or(JsValue::Null),
            children: Vec::new(),
        };
        match index.get(&str_of(folder, "id")) {
            Some(&at) => {
                if let Some(slot) = nodes.get_mut(at) {
                    *slot = node;
                }
            }
            None => {
                index.insert(str_of(folder, "id"), nodes.len());
                nodes.push(node);
            }
        }
    }
    let mut roots = Vec::new();
    for folder in &folders {
        let Some(&node) = index.get(&str_of(folder, "id")) else {
            continue;
        };
        let parent = jsx::as_str(jsx::get(folder, "parentId"))
            .and_then(|p| index.get(&p.to_string_lossy()).copied());
        match parent.and_then(|p| nodes.get_mut(p)) {
            Some(parent) => parent.children.push(node),
            None => roots.push(node),
        }
    }
    let mut writer = TreeWriter {
        nodes: &nodes,
        path: Vec::new(),
        taken: Vec::new(),
        written: 0,
    };
    let mut out = Vec::new();
    for root in roots {
        out.push(writer.node(root)?);
    }
    Ok(JsValue::Array(out))
}

/// Writes the folder tree as `JSON.stringify` walks it, failing as it does on a cycle.
struct TreeWriter<'a> {
    nodes: &'a [FolderNode],
    /// The nodes being written, from a root down to the current one.
    path: Vec<usize>,
    /// For each node on `path` but the last, the index of the child it went down into.
    taken: Vec<usize>,
    written: usize,
}

impl TreeWriter<'_> {
    fn node(&mut self, at: usize) -> Result<JsValue, String> {
        self.written += 1;
        if self.written > MAX_FOLDER_NODES {
            return Err("Invalid string length".into());
        }
        if self.path.len() >= MAX_FOLDER_DEPTH {
            return Err("Maximum call stack size exceeded".into());
        }
        let Some(node) = self.nodes.get(at) else {
            return Ok(JsValue::Null);
        };
        self.path.push(at);
        let mut children = Vec::new();
        let mut failure = None;
        for (position, &child) in node.children.iter().enumerate() {
            if let Some(start) = self.path.iter().position(|n| *n == child) {
                failure = Some(self.circular(start, position));
                break;
            }
            self.taken.push(position);
            let written = self.node(child);
            self.taken.pop();
            match written {
                Ok(value) => children.push(value),
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        self.path.pop();
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(object(vec![
            ("id", node.id.clone()),
            ("title", node.title.clone()),
            ("parentId", node.parent_id.clone()),
            ("children", JsValue::Array(children)),
        ]))
    }

    /// V8's "Converting circular structure to JSON" message for a cycle from the `start`th node
    /// on the path down to the current one, closed by its child at `position`.
    fn circular(&self, start: usize, position: usize) -> String {
        const ARRAY: &str = "property 'children' -> object with constructor 'Array'";
        let last = self.path.len().saturating_sub(1);
        let mut lines = Vec::new();
        for step in start..=last {
            lines.push(ARRAY.to_owned());
            if step < last {
                let index = self.taken.get(step).copied().unwrap_or(0);
                lines.push(format!("index {index} -> object with constructor 'Object'"));
            }
        }
        let shown: Vec<String> = if lines.len() > 3 {
            let mut shown: Vec<String> = lines.iter().take(2).cloned().collect();
            shown.push("...".into());
            shown.extend(lines.last().cloned());
            shown
        } else {
            lines
        };
        let mut message = String::from(
            "Converting circular structure to JSON\n    --> starting at object with constructor 'Object'",
        );
        for line in shown {
            message.push_str("\n    |     ");
            message.push_str(&line);
        }
        message.push_str(&format!("\n    --- index {position} closes the circle"));
        message
    }
}

/// `listLabels`: each label with the live documents and chapters that carry it.
pub fn list_labels(root: &str, project_id: &str) -> Result<JsValue, String> {
    let Found { dir, header } = find_project_dir(root, project_id)?;
    let labels = read_array(project_field(&header, "labels"), is_label);
    let documents: Vec<StoredDocument> = read_stored_documents(&dir, &header)?
        .into_iter()
        .filter(|d| !jsx::is_true(d.fields.get("trashed")))
        .collect();
    let chapters = read_stored_chapters(&dir, &header)?;
    let out = labels
        .into_iter()
        .map(|label| {
            let id = str_of(label, "id");
            let scenes = documents
                .iter()
                .filter(|d| jsx::is_str(d.fields.get("labelId"), &id))
                .count();
            let chapters = chapters
                .iter()
                .filter(|c| !c.trashed && c.label_id.as_ref().is_some_and(|l| l.eq_str(&id)))
                .count();
            let mut entry = copy(label, &["id", "name", "color"]);
            entry.insert("scenes", JsValue::Number(scenes as f64));
            entry.insert("chapters", JsValue::Number(chapters as f64));
            JsValue::Object(entry)
        })
        .collect();
    Ok(JsValue::Array(out))
}

/// `listComments`: oldest first; resolved ones only when asked for.
pub fn list_comments(
    root: &str,
    project_id: &str,
    document_id: Option<&str>,
    include_resolved: bool,
) -> Result<JsValue, String> {
    let Found { header, .. } = find_project_dir(root, project_id)?;
    let mut comments: Vec<&JsValue> = read_array(project_field(&header, "comments"), is_comment)
        .into_iter()
        .filter(|c| document_id.is_none_or(|d| jsx::is_str(jsx::get(c, "documentId"), d)))
        .filter(|c| include_resolved || !jsx::is_true(jsx::get(c, "resolved")))
        .collect();
    comments.sort_by(|a, b| compare_numbers(number_of(a, "createdAt"), number_of(b, "createdAt")));
    let keys = [
        "id",
        "documentId",
        "quote",
        "body",
        "author",
        "createdAt",
        "resolved",
    ];
    Ok(JsValue::Array(
        comments
            .into_iter()
            .map(|c| JsValue::Object(copy(c, &keys)))
            .collect(),
    ))
}

fn snapshot_summary(snapshot: &JsValue) -> JsObject {
    copy(
        snapshot,
        &["id", "documentId", "reason", "createdAt", "note"],
    )
}

/// `listSnapshots`: metadata only, newest first.
pub fn list_snapshots(
    root: &str,
    project_id: &str,
    document_id: Option<&str>,
) -> Result<JsValue, String> {
    let Found { header, .. } = find_project_dir(root, project_id)?;
    let mut snapshots: Vec<&JsValue> = read_array(project_field(&header, "snapshots"), is_snapshot)
        .into_iter()
        .filter(|s| document_id.is_none_or(|d| jsx::is_str(jsx::get(s, "documentId"), d)))
        .collect();
    snapshots.sort_by(|a, b| compare_numbers(number_of(b, "createdAt"), number_of(a, "createdAt")));
    Ok(JsValue::Array(
        snapshots
            .into_iter()
            .map(|s| JsValue::Object(snapshot_summary(s)))
            .collect(),
    ))
}

/// `snapshotIsMarkdown`: this server's snapshots say so; older ones carry its fixed note.
fn snapshot_is_markdown(snapshot: &JsValue) -> bool {
    match jsx::get(snapshot, "contentFormat") {
        Some(format) => jsx::is_str(Some(format), "markdown"),
        None => {
            jsx::is_str(jsx::get(snapshot, "reason"), "manual")
                && jsx::is_str(
                    jsx::get(snapshot, "note"),
                    prosecraft_core::snapshots::MCP_SNAPSHOT_NOTE,
                )
        }
    }
}

/// `readSnapshot`: one snapshot with its text.
pub fn read_snapshot(root: &str, project_id: &str, snapshot_id: &str) -> Result<JsValue, String> {
    let Found { header, .. } = find_project_dir(root, project_id)?;
    let snapshot = read_array(project_field(&header, "snapshots"), is_snapshot)
        .into_iter()
        .find(|s| jsx::is_str(jsx::get(s, "id"), snapshot_id))
        .ok_or_else(|| format!("Snapshot not found: {snapshot_id}"))?;
    let mut out = snapshot_summary(snapshot);
    for key in ["title", "content"] {
        if let Some(value) = jsx::get(snapshot, key) {
            out.insert(key, value.clone());
        }
    }
    let format = if snapshot_is_markdown(snapshot) {
        "markdown"
    } else {
        "html"
    };
    out.insert("contentFormat", JsValue::str(format));
    Ok(JsValue::Object(out))
}
