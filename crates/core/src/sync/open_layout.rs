//! The open sync folder (ProseCraft `src/lib/storage/sync/open-layout.ts` and
//! `open-provider.ts`, upstream `7dae544`): plain Markdown and JSON, one folder per project, for
//! writers who read, edit, diff or version their projects with other tools. Nothing in it is
//! encrypted.
//!
//! Each project folder holds `project.json` (the project without its records, the records'
//! sync metadata and their order), a Markdown file per document with YAML front matter, a JSON
//! file per chapter, and the storyboard's rows and cards. Every file carries a SHA-256 hash of
//! what ProseCraft wrote; a file whose hash no longer matches was edited outside, and its record
//! counts as changed by "external" now, so the next sync takes it.

use super::layout::{project_dir_name, safe_name};
use super::providers::{Connection, SyncProvider};
use super::{BUNDLE_FILE, OPEN_MARKER_FILE, PROJECTS_DIR, SyncError, SyncFileSystem, VAULT_FILE};
use crate::html::sanitize::is_valid_footnote_id;
use crate::html::{is_js_whitespace, sanitize_html, sanitize_inline_html};
use crate::js::regexp::Re;
use crate::js::{self, JsObject, JsString, JsValue, number_to_string, string_to_number};
use crate::markdown::{FootnoteDef, html_to_markdown, markdown_to_document};
use crate::storage::crypto::{RandomSource, VaultKey};
use crate::storage::schema::{SCHEMA_VERSION, is_entity_meta, is_entity_meta_map, is_project_data};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

pub const OPEN_FORMAT: &str = "prosecraft.open";
pub const OPEN_VERSION: f64 = 1.0;
const PROJECT_FORMAT: &str = "prosecraft.project";
const PROJECT_VERSION: f64 = 1.0;
const PROJECT_FILE: &str = "project.json";
const CHAPTERS_DIR: &str = "chapters";
const DOCUMENTS_DIR: &str = "documents";
const STORYBOARD_DIR: &str = "storyboard";
const ROWS_FILE: &str = "rows.json";
const CARDS_FILE: &str = "cards.json";
const RECORD_KINDS: [&str; 4] = ["documents", "chapters", "storyboardRows", "storyboardCards"];
/// The front matter fields the hash covers (`documentPayload`).
const DOCUMENT_FIELDS: [&str; 16] = [
    "id",
    "title",
    "chapterId",
    "space",
    "format",
    "folderId",
    "trashed",
    "trashedWithChapterId",
    "labelId",
    "footnotes",
    "updatedAt",
    "revision",
    "updatedBy",
    "deletedAt",
    "character",
    "place",
];

fn fail(message: impl Into<String>) -> SyncError {
    SyncError::Folder(message.into())
}

fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

/// `compareStrings`: UTF-16 code unit order, as JavaScript compares strings.
fn compare_strings(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

fn text(value: Option<&JsValue>) -> Option<String> {
    value
        .and_then(JsValue::as_js_string)
        .map(JsString::to_string_lossy)
}

fn number(value: Option<&JsValue>) -> Option<f64> {
    value.and_then(JsValue::as_number)
}

/// `isObject`: a plain object, not an array.
fn is_object(value: Option<&JsValue>) -> bool {
    matches!(value, Some(JsValue::Object(_)))
}

fn sha256_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `hashJson`: SHA-256 of the value's `stableStringify`.
fn hash_json(value: &JsValue) -> String {
    sha256_hex(&js::stable_stringify(value))
}

fn object(entries: Vec<(&str, JsValue)>) -> JsValue {
    let mut o = JsObject::new();
    for (k, v) in entries {
        o.insert(k, v);
    }
    JsValue::Object(o)
}

/// `normalizeBody`: `\n` line endings, no trailing newlines.
fn normalize_body(body: &str) -> String {
    let body = body.replace("\r\n", "\n").replace('\r', "\n");
    body.trim_end_matches('\n').to_owned()
}

/// `normalizeFootnotes`: valid, unique footnote definitions with their HTML sanitized; none
/// when the value is not a list.
fn normalize_footnotes(value: Option<&JsValue>) -> Option<JsValue> {
    let list = value?.as_array()?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for entry in list {
        let Some(item) = entry.as_object() else {
            continue;
        };
        let (Some(id), Some(html)) = (text(item.get("id")), text(item.get("html"))) else {
            continue;
        };
        if !is_valid_footnote_id(&id) || !seen.insert(id.clone()) {
            continue;
        }
        out.push(object(vec![
            ("id", JsValue::str(&id)),
            ("html", JsValue::str(&sanitize_inline_html(&html))),
        ]));
    }
    Some(JsValue::Array(out))
}

fn footnote_defs(value: Option<&JsValue>) -> Vec<FootnoteDef> {
    value
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|f| {
            Some(FootnoteDef {
                id: text(f.get("id"))?,
                html: text(f.get("html")).unwrap_or_default(),
            })
        })
        .collect()
}

/// `document.markdownSource ?? htmlToMarkdown(document.content, document.footnotes)`.
fn document_markdown(document: &JsObject) -> String {
    match document.get("markdownSource") {
        Some(JsValue::String(source)) => source.to_string_lossy(),
        _ => html_to_markdown(
            &text(document.get("content")).unwrap_or_default(),
            &footnote_defs(document.get("footnotes")),
        ),
    }
}

fn parsed_footnotes(notes: &[FootnoteDef]) -> JsValue {
    JsValue::Array(
        notes
            .iter()
            .map(|n| {
                object(vec![
                    ("id", JsValue::str(&n.id)),
                    ("html", JsValue::str(&n.html)),
                ])
            })
            .collect(),
    )
}

/// `normalizeDocument`: a document as it comes back through the Markdown the open folder keeps,
/// so a first sync doesn't count every document as changed.
pub fn normalize_document(document: &JsObject) -> JsObject {
    let body = normalize_body(&document_markdown(document));
    let parsed = markdown_to_document(&body);
    let footnotes = if parsed.footnotes.is_empty() {
        normalize_footnotes(document.get("footnotes"))
    } else {
        Some(parsed_footnotes(&parsed.footnotes))
    };
    let mut next = document.clone();
    next.insert("content", JsValue::str(&sanitize_html(&parsed.html)));
    next.insert("markdownSource", JsValue::str(&body));
    if !matches!(
        text(next.get("format")).as_deref(),
        Some("prose" | "screenplay")
    ) {
        next.remove("format");
    }
    match footnotes {
        Some(notes) => next.insert("footnotes", notes),
        None => {
            next.remove("footnotes");
        }
    }
    next
}

/// `formatYamlValue`: a front matter value, JSON where YAML would need quoting.
fn format_yaml_value(value: &JsValue) -> String {
    match value {
        JsValue::Null => "null".into(),
        JsValue::Number(n) => number_to_string(*n),
        JsValue::Bool(b) => b.to_string(),
        other => js::stringify(other),
    }
}

/// `documentPayload`: the front matter fields the hash covers.
fn document_payload(fields: &JsObject) -> JsValue {
    let mut payload = JsObject::new();
    for key in DOCUMENT_FIELDS {
        if let Some(value) = fields.get(key) {
            payload.insert(key, value.clone());
        }
    }
    JsValue::Object(payload)
}

fn markdown_hash(fields: &JsObject, body: &str) -> String {
    hash_json(&object(vec![
        ("fields", document_payload(fields)),
        ("body", JsValue::str(body)),
    ]))
}

/// `serializeMarkdown`: a document file, front matter then the body.
fn serialize_markdown(fields: &JsObject, body: &str) -> String {
    let hash = markdown_hash(fields, body);
    let mut entries: Vec<String> = fields
        .iter()
        .map(|(key, value)| format!("{}: {}", key.to_string_lossy(), format_yaml_value(value)))
        .collect();
    entries.push(format!("hash: {}", js::stringify(&JsValue::str(&hash))));
    format!(
        "---\n{}\n---\n\n{}\n",
        entries.join("\n"),
        normalize_body(body)
    )
}

macro_rules! rule {
    ($name:ident, $source:expr) => {
        fn $name() -> &'static Re {
            static RULE: OnceLock<Re> = OnceLock::new();
            RULE.get_or_init(|| Re::new($source, ""))
        }
    };
}

rule!(plain_number, r"^-?\d+(?:\.\d+)?$");
rule!(field_key, r"^[A-Za-z0-9_-]+$");

/// `parseYamlValue`.
fn parse_yaml_value(raw: &str) -> Result<JsValue, ()> {
    Ok(match raw {
        "" => JsValue::str(""),
        "null" => JsValue::Null,
        "true" => JsValue::Bool(true),
        "false" => JsValue::Bool(false),
        _ if plain_number().test(raw) => JsValue::Number(string_to_number(raw)),
        _ if raw.starts_with(['"', '{', '[']) => js::parse(raw).map_err(|_| ())?,
        _ => JsValue::str(raw),
    })
}

/// `parseMarkdown`: a document file's front matter fields and body.
fn parse_markdown(text: &str) -> Result<(JsObject, String), ()> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if !normalized.starts_with("---\n") {
        return Err(());
    }
    let end = normalized[4..].find("\n---\n").ok_or(())? + 4;
    let header = &normalized[4..end];
    let body = &normalized[end + 5..];
    let body = normalize_body(body.strip_prefix('\n').unwrap_or(body));
    let mut fields = JsObject::new();
    for line in header.split('\n') {
        if js_trim(line).is_empty() || line.trim_start_matches(is_js_whitespace).starts_with('#') {
            continue;
        }
        let colon = line.find(':').filter(|&c| c > 0).ok_or(())?;
        let key = js_trim(&line[..colon]);
        if !field_key().test(key) {
            return Err(());
        }
        fields.insert(key, parse_yaml_value(js_trim(&line[colon + 1..]))?);
    }
    Ok((fields, body))
}

/// `documentFields`: what a document's front matter holds, in ProseCraft's order.
fn document_fields(document: &JsObject, meta: &JsValue) -> JsObject {
    let mut fields = JsObject::new();
    for key in [
        "id",
        "title",
        "chapterId",
        "space",
        "format",
        "folderId",
        "trashed",
        "trashedWithChapterId",
        "labelId",
    ] {
        if let Some(value) = document.get(key) {
            fields.insert(key, value.clone());
        }
    }
    if let Some(notes) = normalize_footnotes(document.get("footnotes")) {
        fields.insert("footnotes", notes);
    }
    for key in ["updatedAt", "revision", "updatedBy", "deletedAt"] {
        if let Some(value) = meta.get(key) {
            fields.insert(key, value.clone());
        }
    }
    for key in ["character", "place"] {
        if let Some(value) = document.get(key) {
            fields.insert(key, value.clone());
        }
    }
    fields
}

/// `bumpMeta`: changed outside, now.
fn bump_meta(meta: &JsValue, now: f64) -> JsValue {
    let mut next = meta.as_object().cloned().unwrap_or_default();
    let updated = number(meta.get("updatedAt")).unwrap_or(0.0);
    let revision = number(meta.get("revision")).unwrap_or(0.0);
    next.insert("updatedAt", JsValue::Number(now.max(updated + 1.0)));
    next.insert("revision", JsValue::Number(revision + 1.0));
    next.insert("updatedBy", JsValue::str("external"));
    JsValue::Object(next)
}

/// `readMeta`, with `fallbackMeta` for what is missing.
fn read_meta(fields: &JsObject) -> JsValue {
    let updated_at = number(fields.get("updatedAt")).unwrap_or(0.0);
    let revision = number(fields.get("revision")).unwrap_or(0.0);
    let updated_by = text(fields.get("updatedBy"))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "external".into());
    let deleted_at = match fields.get("deletedAt") {
        Some(JsValue::Number(n)) => JsValue::Number(*n),
        _ => JsValue::Null,
    };
    object(vec![
        ("updatedAt", JsValue::Number(updated_at)),
        ("revision", JsValue::Number(revision)),
        ("updatedBy", JsValue::str(&updated_by)),
        ("deletedAt", deleted_at),
    ])
}

/// A record read from its file.
struct Decoded {
    id: String,
    record: JsValue,
    meta: JsValue,
    external: bool,
    changed_at: f64,
}

/// `meta.deletedAt !== null`.
fn is_deleted(meta: &JsValue) -> bool {
    !matches!(meta.get("deletedAt"), Some(JsValue::Null))
}

/// `decodeDocument`.
fn decode_document(path: &str, file: &str, now: f64) -> Result<Decoded, SyncError> {
    let (fields, body) = parse_markdown(file)
        .map_err(|()| fail(format!("The document file {path} has invalid frontmatter.")))?;
    let id = text(fields.get("id"))
        .filter(|id| !id.is_empty())
        .ok_or_else(|| fail(format!("The document file {path} is missing an id.")))?;
    let meta = read_meta(&fields);
    let stored_hash = text(fields.get("hash")).unwrap_or_default();
    let external = stored_hash != markdown_hash(&fields, &body);
    let meta = if external {
        bump_meta(&meta, now)
    } else {
        meta
    };
    let parsed = markdown_to_document(&body);
    let from_body = (!parsed.footnotes.is_empty()).then(|| parsed_footnotes(&parsed.footnotes));
    let from_fields = normalize_footnotes(fields.get("footnotes"));
    let footnotes = if external {
        from_body.or(from_fields)
    } else {
        from_fields.or(from_body)
    };
    let mut record = JsObject::new();
    record.insert("id", JsValue::str(&id));
    record.insert(
        "title",
        JsValue::str(&text(fields.get("title")).unwrap_or_default()),
    );
    record.insert("content", JsValue::str(&sanitize_html(&parsed.html)));
    record.insert("markdownSource", JsValue::str(&body));
    for key in ["chapterId", "space"] {
        if let Some(value @ JsValue::String(_)) = fields.get(key) {
            record.insert(key, value.clone());
        }
    }
    if matches!(
        text(fields.get("format")).as_deref(),
        Some("prose" | "screenplay")
    ) {
        record.insert(
            "format",
            fields.get("format").cloned().unwrap_or(JsValue::Null),
        );
    }
    if let Some(value @ JsValue::String(_)) = fields.get("folderId") {
        record.insert("folderId", value.clone());
    }
    if fields.get("trashed") == Some(&JsValue::Bool(true)) {
        record.insert("trashed", JsValue::Bool(true));
    }
    for key in ["trashedWithChapterId", "labelId"] {
        if let Some(value @ JsValue::String(_)) = fields.get(key) {
            record.insert(key, value.clone());
        }
    }
    if let Some(notes) = footnotes {
        record.insert("footnotes", notes);
    }
    for key in ["character", "place"] {
        if is_object(fields.get(key)) {
            record.insert(key, fields.get(key).cloned().unwrap_or(JsValue::Null));
        }
    }
    let changed_at = number(meta.get("updatedAt")).unwrap_or(0.0);
    Ok(Decoded {
        id,
        record: JsValue::Object(record),
        meta,
        external,
        changed_at,
    })
}

/// `decodeChapter`.
fn decode_chapter(path: &str, file: &str, now: f64) -> Result<Decoded, SyncError> {
    let raw =
        js::parse(file).map_err(|_| fail(format!("The chapter file {path} could not be read.")))?;
    let invalid = || {
        fail(format!(
            "The chapter file {path} is not a valid ProseCraft record."
        ))
    };
    let (Some(record), Some(meta)) = (raw.get("record"), raw.get("meta")) else {
        return Err(invalid());
    };
    if !is_object(Some(&raw)) || !is_object(Some(record)) || !is_entity_meta(meta) {
        return Err(invalid());
    }
    let id = text(record.get("id"))
        .filter(|id| !id.is_empty())
        .ok_or_else(invalid)?;
    let title = text(record.get("title")).ok_or_else(invalid)?;
    let stored_hash = text(raw.get("hash")).unwrap_or_default();
    let expected = hash_json(&object(vec![
        ("record", record.clone()),
        ("meta", meta.clone()),
    ]));
    let external = stored_hash != expected;
    let meta = if external {
        bump_meta(meta, now)
    } else {
        meta.clone()
    };
    let mut out = JsObject::new();
    out.insert("id", JsValue::str(&id));
    out.insert("title", JsValue::str(&title));
    out.insert(
        "expanded",
        JsValue::Bool(record.get("expanded") == Some(&JsValue::Bool(true))),
    );
    if let Some(summary @ JsValue::String(_)) = record.get("summary") {
        out.insert("summary", summary.clone());
    }
    if record.get("trashed") == Some(&JsValue::Bool(true)) {
        out.insert("trashed", JsValue::Bool(true));
    }
    if let Some(label @ JsValue::String(_)) = record.get("labelId") {
        out.insert("labelId", label.clone());
    }
    let changed_at = number(meta.get("updatedAt")).unwrap_or(0.0);
    Ok(Decoded {
        id,
        record: JsValue::Object(out),
        meta,
        external,
        changed_at,
    })
}

/// `decodeStoryboard`: a storyboard file's records, all changed outside when its hash is off.
fn decode_storyboard(
    path: &str,
    file: &str,
    now: f64,
) -> Result<(Vec<Decoded>, bool, f64), SyncError> {
    let raw = js::parse(file)
        .map_err(|_| fail(format!("The storyboard file {path} could not be read.")))?;
    let invalid = || {
        fail(format!(
            "The storyboard file {path} is not a valid ProseCraft record."
        ))
    };
    let records = match (is_object(Some(&raw)), raw.get("records")) {
        (true, Some(JsValue::Array(records))) => records.clone(),
        _ => return Err(invalid()),
    };
    let mut entries = Vec::new();
    for item in &records {
        let (Some(record), Some(meta)) = (item.get("record"), item.get("meta")) else {
            return Err(invalid());
        };
        if !is_object(Some(item)) || !is_object(Some(record)) || !is_entity_meta(meta) {
            return Err(invalid());
        }
        let id = text(record.get("id"))
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                fail(format!(
                    "The storyboard file {path} contains a record without an id."
                ))
            })?;
        entries.push(Decoded {
            id,
            record: record.clone(),
            meta: meta.clone(),
            external: false,
            changed_at: number(meta.get("updatedAt")).unwrap_or(0.0),
        });
    }
    let stored_hash = text(raw.get("hash")).unwrap_or_default();
    let external = stored_hash != hash_json(&JsValue::Array(records));
    let mut changed_at: f64 = 0.0;
    if external {
        for entry in &mut entries {
            entry.meta = bump_meta(&entry.meta, now);
            entry.external = true;
            entry.changed_at = number(entry.meta.get("updatedAt")).unwrap_or(0.0);
            changed_at = changed_at.max(entry.changed_at);
        }
    }
    Ok((entries, external, changed_at))
}

/// `readOrder`.
fn read_order(order: Option<&JsValue>, kind: &str) -> Vec<String> {
    order
        .and_then(|o| o.as_object())
        .and_then(|o| o.get(kind))
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|id| text(Some(id)))
        .collect()
}

/// What was read of one kind of record.
struct ReadEntities {
    records: Vec<JsValue>,
    metas: JsObject,
    external: bool,
    changed_at: f64,
}

/// One kind of record kept a file each: its folder, file extension, name in messages, and
/// how a file is read.
struct EntityFiles<'a> {
    base: &'a str,
    extension: &'a str,
    noun: &'a str,
    decode: fn(&str, &str, f64) -> Result<Decoded, SyncError>,
}

/// `readDocuments` and `readChapters`: one file per record in `base`. Every live record the
/// project knows must have its file; a file for a record it doesn't know is a new record.
fn read_entity_files(
    fs: &mut dyn SyncFileSystem,
    files: &EntityFiles<'_>,
    known: &JsObject,
    order: &[String],
    now: f64,
) -> Result<ReadEntities, SyncError> {
    let EntityFiles {
        base,
        extension,
        noun,
        decode,
    } = *files;
    let mut decoded: HashMap<String, Decoded> = HashMap::new();
    for entry in fs.list_dir(base)? {
        if entry.is_directory || !entry.name.ends_with(extension) {
            continue;
        }
        let path = format!("{base}/{}", entry.name);
        let file = fs.read_text(&path)?;
        let record = decode(&path, &file, now)?;
        if decoded.contains_key(&record.id) {
            return Err(fail(format!(
                "Two {noun} files claim the id {}.",
                record.id
            )));
        }
        decoded.insert(record.id.clone(), record);
    }
    let mut metas = known.clone();
    let mut records: HashMap<String, JsValue> = HashMap::new();
    let mut live: Vec<String> = Vec::new();
    let mut external = false;
    let mut changed_at: f64 = 0.0;
    let mut take = |id: &str, record: &Decoded, metas: &mut JsObject| {
        metas.insert(id, record.meta.clone());
        external |= record.external;
        changed_at = changed_at.max(number(record.meta.get("updatedAt")).unwrap_or(0.0));
        if !is_deleted(&record.meta) {
            records.insert(id.to_owned(), record.record.clone());
            live.push(id.to_owned());
        }
    };
    for (id, meta) in known.iter() {
        if is_deleted(meta) {
            continue;
        }
        let id = id.to_string_lossy();
        let record = decoded.get(&id).ok_or_else(|| {
            fail(format!(
                "The {noun} file for {id} is missing from the open sync folder."
            ))
        })?;
        take(&id, record, &mut metas);
    }
    let mut extras: Vec<&String> = decoded
        .keys()
        .filter(|id| known.get(id.as_str()).is_none())
        .collect();
    extras.sort_by(|a, b| compare_strings(a, b));
    for id in extras {
        if let Some(record) = decoded.get(id) {
            take(id, record, &mut metas);
        }
    }
    let live_set: HashSet<&String> = live.iter().collect();
    let mut ordered: Vec<String> = order
        .iter()
        .filter(|id| live_set.contains(id))
        .cloned()
        .collect();
    for id in &live {
        if !ordered.contains(id) {
            ordered.push(id.clone());
        }
    }
    Ok(ReadEntities {
        records: ordered
            .iter()
            .filter_map(|id| records.get(id).cloned())
            .collect(),
        metas,
        external,
        changed_at,
    })
}

/// `readStoryboard`: the order in the file is the storyboard's order.
fn read_storyboard(
    fs: &mut dyn SyncFileSystem,
    path: &str,
    known: &JsObject,
    now: f64,
) -> Result<ReadEntities, SyncError> {
    let (entries, mut external, mut changed_at) = if fs.exists(path)? {
        let file = fs.read_text(path)?;
        decode_storyboard(path, &file, now)?
    } else {
        (Vec::new(), false, 0.0)
    };
    let mut seen = HashSet::new();
    for entry in &entries {
        if !seen.insert(entry.id.clone()) {
            return Err(fail(format!(
                "Two storyboard records in {path} claim the id {}.",
                entry.id
            )));
        }
    }
    for (id, meta) in known.iter() {
        let id = id.to_string_lossy();
        if !is_deleted(meta) && !seen.contains(&id) {
            return Err(fail(format!(
                "The storyboard record {id} is missing from the open sync folder."
            )));
        }
    }
    let mut metas = known.clone();
    let mut records = Vec::new();
    for entry in entries {
        if known.get(&entry.id).is_some_and(is_deleted) {
            continue;
        }
        metas.insert(entry.id.as_str(), entry.meta.clone());
        external |= entry.external;
        changed_at = changed_at.max(number(entry.meta.get("updatedAt")).unwrap_or(0.0));
        if !is_deleted(&entry.meta) {
            records.push(entry.record);
        }
    }
    Ok(ReadEntities {
        records,
        metas,
        external,
        changed_at,
    })
}

/// `readOpenMarker`: the layout marker, or `None` when the folder is not an open layout.
pub fn read_open_marker(fs: &mut dyn SyncFileSystem) -> Result<Option<JsValue>, SyncError> {
    if !fs.exists(OPEN_MARKER_FILE)? {
        return Ok(None);
    }
    let file = fs.read_text(OPEN_MARKER_FILE)?;
    let raw =
        js::parse(&file).map_err(|_| fail("The open sync folder marker could not be read."))?;
    let valid = is_object(Some(&raw))
        && text(raw.get("format")).as_deref() == Some(OPEN_FORMAT)
        && number(raw.get("version")) == Some(OPEN_VERSION)
        && number(raw.get("createdAt")).is_some();
    if !valid {
        return Err(fail("The open sync folder marker is not valid."));
    }
    Ok(Some(raw))
}

/// `openMarkerText`.
pub fn open_marker_text(created_at: f64) -> String {
    let marker = object(vec![
        ("format", JsValue::str(OPEN_FORMAT)),
        ("version", JsValue::Number(OPEN_VERSION)),
        ("createdAt", JsValue::Number(created_at)),
    ]);
    format!("{}\n", js::stringify_pretty(&marker))
}

fn normalize_project_kind(value: Option<&JsValue>) -> Option<JsValue> {
    matches!(text(value).as_deref(), Some("novel" | "screenplay"))
        .then(|| value.cloned())
        .flatten()
}

/// `readOpenProject`: one project folder. A folder without its `project.json` is damaged, not
/// deleted (a deletion removes the folder), so it is an error rather than a removal.
pub fn read_open_project(
    fs: &mut dyn SyncFileSystem,
    dir_name: &str,
    now: f64,
) -> Result<JsValue, SyncError> {
    let dir = format!("{PROJECTS_DIR}/{dir_name}");
    let project_path = format!("{dir}/{PROJECT_FILE}");
    if !fs.exists(&project_path)? {
        return Err(fail(format!(
            "The project folder {dir_name} is missing its project.json file."
        )));
    }
    let file = fs.read_text(&project_path)?;
    let raw = js::parse(&file)
        .map_err(|_| fail(format!("The project file in {dir_name} could not be read.")))?;
    let invalid = || {
        fail(format!(
            "The project file in {dir_name} is not a valid ProseCraft record."
        ))
    };
    let Some(raw_object) = raw.as_object() else {
        return Err(invalid());
    };
    if text(raw.get("format")).as_deref() != Some(PROJECT_FORMAT)
        || number(raw.get("version")) != Some(PROJECT_VERSION)
    {
        return Err(fail(format!(
            "The project file in {dir_name} has an unsupported format."
        )));
    }
    let mut payload = raw_object.clone();
    let stored_hash = payload.remove("hash");
    let id = text(payload.get("id")).filter(|id| !id.is_empty());
    let deleted_ok = matches!(
        payload.get("deletedAt"),
        Some(JsValue::Null | JsValue::Number(_))
    );
    let project = payload.get("project");
    let valid = id.is_some()
        && number(payload.get("createdAt")).is_some()
        && number(payload.get("updatedAt")).is_some()
        && number(payload.get("revision")).is_some()
        && text(payload.get("updatedBy")).is_some()
        && deleted_ok
        && is_project_data(project)
        && text(project.and_then(|p| p.get("id"))) == id
        && is_object(payload.get("entities"))
        && is_object(payload.get("order"));
    let Some(id) = id.filter(|_| valid) else {
        return Err(invalid());
    };
    if safe_name(&id) != dir_name {
        return Err(fail(format!(
            "The project file in {dir_name} has inconsistent identity metadata."
        )));
    }
    let entities = payload.get("entities").cloned().unwrap_or(JsValue::Null);
    let mut known = Vec::new();
    for kind in RECORD_KINDS {
        let map = entities.get(kind);
        if !is_entity_meta_map(map) {
            return Err(fail(format!(
                "The project file {project_path} has invalid entity metadata."
            )));
        }
        known.push(
            map.and_then(JsValue::as_object)
                .cloned()
                .unwrap_or_default(),
        );
    }
    let project_external = text(stored_hash.as_ref()).unwrap_or_default()
        != hash_json(&JsValue::Object(payload.clone()));
    let order = payload.get("order");
    let documents_dir = format!("{dir}/{DOCUMENTS_DIR}");
    let documents = read_entity_files(
        fs,
        &EntityFiles {
            base: &documents_dir,
            extension: ".md",
            noun: "document",
            decode: decode_document,
        },
        &known[0],
        &read_order(order, "documents"),
        now,
    )?;
    let chapters_dir = format!("{dir}/{CHAPTERS_DIR}");
    let chapters = read_entity_files(
        fs,
        &EntityFiles {
            base: &chapters_dir,
            extension: ".json",
            noun: "chapter",
            decode: decode_chapter,
        },
        &known[1],
        &read_order(order, "chapters"),
        now,
    )?;
    let rows = read_storyboard(
        fs,
        &format!("{dir}/{STORYBOARD_DIR}/{ROWS_FILE}"),
        &known[2],
        now,
    )?;
    let cards = read_storyboard(
        fs,
        &format!("{dir}/{STORYBOARD_DIR}/{CARDS_FILE}"),
        &known[3],
        now,
    )?;
    let parts = [&documents, &chapters, &rows, &cards];
    let external = project_external || parts.iter().any(|p| p.external);
    let updated_at = number(payload.get("updatedAt")).unwrap_or(0.0);
    let changed_at = parts
        .iter()
        .map(|p| p.changed_at)
        .fold(now.max(updated_at + 1.0), f64::max);

    let mut record = payload
        .get("project")
        .and_then(JsValue::as_object)
        .cloned()
        .unwrap_or_default();
    record.insert("id", JsValue::str(&id));
    for (key, part) in RECORD_KINDS.iter().zip(&parts) {
        record.insert(*key, JsValue::Array(part.records.clone()));
    }
    match normalize_project_kind(record.get("kind")) {
        Some(kind) => record.insert("kind", kind),
        None => {
            record.remove("kind");
        }
    }
    if let Some(citations) = record.get("citations").cloned() {
        record.insert(
            "citations",
            JsValue::Array(crate::csl::normalize_csl_items(&citations)),
        );
    }
    let revision = number(payload.get("revision")).unwrap_or(0.0);
    let mut stored = JsObject::new();
    stored.insert(
        "schemaVersion",
        JsValue::Number(number(payload.get("schemaVersion")).unwrap_or(SCHEMA_VERSION)),
    );
    stored.insert("id", JsValue::str(&id));
    stored.insert(
        "createdAt",
        payload.get("createdAt").cloned().unwrap_or(JsValue::Null),
    );
    stored.insert(
        "updatedAt",
        JsValue::Number(if external { changed_at } else { updated_at }),
    );
    stored.insert(
        "revision",
        JsValue::Number(if external { revision + 1.0 } else { revision }),
    );
    stored.insert(
        "updatedBy",
        if external {
            JsValue::str("external")
        } else {
            payload.get("updatedBy").cloned().unwrap_or(JsValue::Null)
        },
    );
    stored.insert(
        "deletedAt",
        payload.get("deletedAt").cloned().unwrap_or(JsValue::Null),
    );
    stored.insert("project", JsValue::Object(record));
    let mut metas = JsObject::new();
    for (key, part) in RECORD_KINDS.iter().zip(parts) {
        metas.insert(*key, JsValue::Object(part.metas.clone()));
    }
    stored.insert("entities", JsValue::Object(metas));
    Ok(JsValue::Object(stored))
}

/// `readOpenProjects`: every project folder, by id.
pub fn read_open_projects(
    fs: &mut dyn SyncFileSystem,
    now: f64,
) -> Result<Vec<JsValue>, SyncError> {
    let mut projects = Vec::new();
    for entry in fs.list_dir(PROJECTS_DIR)? {
        if entry.is_directory {
            projects.push(read_open_project(fs, &entry.name, now)?);
        }
    }
    let id = |p: &JsValue| text(p.get("id")).unwrap_or_default();
    projects.sort_by(|a, b| compare_strings(&id(a), &id(b)));
    Ok(projects)
}

fn write_if_changed(
    fs: &mut dyn SyncFileSystem,
    path: &str,
    content: &str,
) -> Result<(), SyncError> {
    if fs.exists(path)? && fs.read_text(path).is_ok_and(|existing| existing == content) {
        return Ok(());
    }
    fs.write_text(path, content)
}

fn records_of<'a>(project: &'a JsValue, kind: &str) -> &'a [JsValue] {
    project
        .get("project")
        .and_then(|p| p.get(kind))
        .and_then(JsValue::as_array)
        .unwrap_or_default()
}

fn meta_of<'a>(project: &'a JsValue, kind: &str, id: Option<&JsValue>) -> Option<&'a JsValue> {
    let id = text(id)?;
    project.get("entities")?.get(kind)?.as_object()?.get(&id)
}

/// `projectPayload`: `project.json` without its hash.
fn project_payload(project: &JsValue) -> JsValue {
    let data = project
        .get("project")
        .and_then(JsValue::as_object)
        .cloned()
        .unwrap_or_default();
    let mut rest = JsObject::new();
    for (key, value) in data.iter() {
        if ![
            "documents",
            "chapters",
            "storyboardRows",
            "storyboardCards",
            "kind",
            "citations",
        ]
        .iter()
        .any(|k| key.eq_str(k))
        {
            rest.insert(key.clone(), value.clone());
        }
    }
    if let Some(kind) = normalize_project_kind(data.get("kind")) {
        rest.insert("kind", kind);
    }
    if let Some(citations) = data.get("citations") {
        rest.insert(
            "citations",
            JsValue::Array(crate::csl::normalize_csl_items(citations)),
        );
    }
    for kind in RECORD_KINDS {
        rest.insert(kind, JsValue::Array(Vec::new()));
    }
    let mut order = JsObject::new();
    for kind in RECORD_KINDS {
        let ids = records_of(project, kind)
            .iter()
            .map(|r| r.get("id").cloned().unwrap_or(JsValue::Null))
            .collect();
        order.insert(kind, JsValue::Array(ids));
    }
    let field = |k: &str| project.get(k).cloned().unwrap_or(JsValue::Null);
    object(vec![
        ("format", JsValue::str(PROJECT_FORMAT)),
        ("version", JsValue::Number(PROJECT_VERSION)),
        ("schemaVersion", field("schemaVersion")),
        ("id", field("id")),
        ("createdAt", field("createdAt")),
        ("updatedAt", field("updatedAt")),
        ("revision", field("revision")),
        ("updatedBy", field("updatedBy")),
        ("deletedAt", field("deletedAt")),
        ("project", JsValue::Object(rest)),
        ("entities", field("entities")),
        ("order", JsValue::Object(order)),
    ])
}

fn with_hash(value: JsValue, hash: String) -> JsValue {
    let mut o = value.as_object().cloned().unwrap_or_default();
    o.insert("hash", JsValue::str(&hash));
    JsValue::Object(o)
}

/// `writeOpenProject`: one project folder, each file written only when it changed, the header
/// last, then files of records no longer live removed.
pub fn write_open_project(fs: &mut dyn SyncFileSystem, project: &JsValue) -> Result<(), SyncError> {
    let id = text(project.get("id")).unwrap_or_default();
    let dir = project_dir_name(&id);
    for sub in [DOCUMENTS_DIR, CHAPTERS_DIR, STORYBOARD_DIR] {
        fs.create_dir(&format!("{dir}/{sub}"))?;
    }
    let live = |meta: Option<&JsValue>| meta.filter(|m| !is_deleted(m)).cloned();

    let mut document_paths = HashSet::new();
    for document in records_of(project, "documents") {
        let Some(meta) = live(meta_of(project, "documents", document.get("id"))) else {
            continue;
        };
        let Some(record) = document.as_object() else {
            continue;
        };
        let body = normalize_body(&document_markdown(record));
        let doc_id = text(document.get("id")).unwrap_or_default();
        let path = format!("{dir}/{DOCUMENTS_DIR}/{}.md", safe_name(&doc_id));
        write_if_changed(
            fs,
            &path,
            &serialize_markdown(&document_fields(record, &meta), &body),
        )?;
        document_paths.insert(path);
    }

    let mut chapter_paths = HashSet::new();
    for chapter in records_of(project, "chapters") {
        let Some(meta) = live(meta_of(project, "chapters", chapter.get("id"))) else {
            continue;
        };
        let chapter_id = text(chapter.get("id")).unwrap_or_default();
        let path = format!("{dir}/{CHAPTERS_DIR}/{}.json", safe_name(&chapter_id));
        let mut record = JsObject::new();
        for key in ["id", "title", "expanded"] {
            if let Some(value) = chapter.get(key) {
                record.insert(key, value.clone());
            }
        }
        if let Some(summary @ JsValue::String(_)) = chapter.get("summary") {
            record.insert("summary", summary.clone());
        }
        let truthy = |v: Option<&JsValue>| match v {
            None | Some(JsValue::Null) | Some(JsValue::Bool(false)) => false,
            Some(JsValue::Number(n)) => *n != 0.0 && !n.is_nan(),
            Some(JsValue::String(s)) => !s.is_empty(),
            Some(_) => true,
        };
        if truthy(chapter.get("trashed")) {
            record.insert("trashed", JsValue::Bool(true));
        }
        if truthy(chapter.get("labelId")) {
            record.insert(
                "labelId",
                chapter.get("labelId").cloned().unwrap_or(JsValue::Null),
            );
        }
        let pair = object(vec![("record", JsValue::Object(record)), ("meta", meta)]);
        let content = format!(
            "{}\n",
            js::stringify_pretty(&with_hash(pair.clone(), hash_json(&pair)))
        );
        write_if_changed(fs, &path, &content)?;
        chapter_paths.insert(path);
    }

    for (kind, file) in [
        ("storyboardRows", ROWS_FILE),
        ("storyboardCards", CARDS_FILE),
    ] {
        let records: Vec<JsValue> = records_of(project, kind)
            .iter()
            .filter_map(|record| {
                let meta = live(meta_of(project, kind, record.get("id")))?;
                Some(object(vec![("record", record.clone()), ("meta", meta)]))
            })
            .collect();
        let list = JsValue::Array(records);
        let content = format!(
            "{}\n",
            js::stringify_pretty(&object(vec![
                ("records", list.clone()),
                ("hash", JsValue::str(&hash_json(&list)))
            ]))
        );
        write_if_changed(fs, &format!("{dir}/{STORYBOARD_DIR}/{file}"), &content)?;
    }

    let payload = project_payload(project);
    let content = format!(
        "{}\n",
        js::stringify_pretty(&with_hash(payload.clone(), hash_json(&payload)))
    );
    // The header last, so an interrupted upload doesn't list files not yet written.
    write_if_changed(fs, &format!("{dir}/{PROJECT_FILE}"), &content)?;

    prune(
        fs,
        &format!("{dir}/{DOCUMENTS_DIR}"),
        &document_paths,
        ".md",
    )?;
    prune(
        fs,
        &format!("{dir}/{CHAPTERS_DIR}"),
        &chapter_paths,
        ".json",
    )
}

/// `pruneEntityDir`.
fn prune(
    fs: &mut dyn SyncFileSystem,
    dir: &str,
    keep: &HashSet<String>,
    extension: &str,
) -> Result<(), SyncError> {
    for entry in fs.list_dir(dir)? {
        if entry.is_directory || !entry.name.ends_with(extension) {
            continue;
        }
        let path = format!("{dir}/{}", entry.name);
        if !keep.contains(&path) {
            fs.remove(&path)?;
        }
    }
    Ok(())
}

/// The open folder (`OpenFolderSyncProvider`).
pub struct OpenFolderProvider<'a> {
    fs: &'a mut dyn SyncFileSystem,
    now: f64,
}

impl<'a> OpenFolderProvider<'a> {
    pub fn new(fs: &'a mut dyn SyncFileSystem, now: f64) -> Self {
        Self { fs, now }
    }

    /// `pull`: every project in the folder.
    pub fn pull(&mut self) -> Result<Vec<JsValue>, SyncError> {
        read_open_projects(self.fs, self.now)
    }
}

impl SyncProvider for OpenFolderProvider<'_> {
    fn connect(
        &mut self,
        _passphrase: &JsString,
        _random: &mut dyn RandomSource,
    ) -> Result<Connection, SyncError> {
        if read_open_marker(self.fs)?.is_some() {
            return Ok(Connection {
                key: None,
                projects: self.pull()?,
                created: false,
            });
        }
        if self.fs.exists(VAULT_FILE)? {
            return Err(fail(
                "This folder is set up for encrypted-folder sync, not open-folder sync.",
            ));
        }
        if self.fs.exists(BUNDLE_FILE)? {
            return Err(fail(
                "This folder is set up for single-file sync, not open-folder sync.",
            ));
        }
        for entry in self.fs.list_dir(PROJECTS_DIR)? {
            if !entry.is_directory {
                continue;
            }
            if self
                .fs
                .exists(&format!("{PROJECTS_DIR}/{}/project.enc", entry.name))?
            {
                return Err(fail(
                    "This folder contains encrypted project files but no vault file.",
                ));
            }
            if self
                .fs
                .exists(&format!("{PROJECTS_DIR}/{}/project.json", entry.name))?
            {
                return Err(fail(
                    "This folder contains open-folder projects but no layout marker.",
                ));
            }
        }
        self.fs
            .write_text(OPEN_MARKER_FILE, &open_marker_text(self.now))?;
        Ok(Connection {
            key: None,
            projects: Vec::new(),
            created: true,
        })
    }

    fn push(
        &mut self,
        projects: &[JsValue],
        _key: Option<&VaultKey>,
        _random: &mut dyn RandomSource,
    ) -> Result<(), SyncError> {
        if !self.fs.exists(OPEN_MARKER_FILE)? {
            self.fs
                .write_text(OPEN_MARKER_FILE, &open_marker_text(self.now))?;
        }
        self.fs.create_dir(PROJECTS_DIR)?;
        for project in projects {
            let id = text(project.get("id")).unwrap_or_default();
            self.fs.create_dir(&project_dir_name(&id))?;
            write_open_project(self.fs, project)?;
        }
        Ok(())
    }

    fn remove(&mut self, project_ids: &[String]) -> Result<(), SyncError> {
        for id in project_ids {
            self.fs.remove(&project_dir_name(id))?;
        }
        Ok(())
    }

    /// Markdown is what this layout keeps of a document, so the records match what comes back
    /// through it; otherwise the first sync would look like a change that never settles.
    fn normalize(&mut self, projects: Vec<JsValue>) -> Vec<JsValue> {
        projects
            .into_iter()
            .map(|mut project| {
                if let Some(documents) = project
                    .as_object_mut()
                    .and_then(|p| p.get_mut("project"))
                    .and_then(JsValue::as_object_mut)
                    .and_then(|p| p.get_mut("documents"))
                    && let JsValue::Array(list) = documents
                {
                    for document in list.iter_mut() {
                        if let Some(object) = document.as_object() {
                            *document = JsValue::Object(normalize_document(object));
                        }
                    }
                }
                project
            })
            .collect()
    }
}
