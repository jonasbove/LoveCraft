//! The open folder as the TypeScript server's `openStore.ts` reads and writes it: the folder
//! marker, project headers, document files with YAML front matter, chapter files, the hashes the
//! app checks, and the chapter and trash tools.

use prosecraft_core::js::{self, JsObject, JsString, JsValue, number_to_string, string_to_number};
use prosecraft_core::sync::layout::safe_name;
use sha2::{Digest, Sha256};

use crate::env::Env;
use crate::fsx::{self, join};
use crate::jsx::{self, object, prop};

const PROJECTS_DIR: &str = "projects";
const MARKER_FILE: &str = "prosecraft-folder.json";

/// A project folder and its parsed `project.json`.
pub struct Found {
    pub dir: String,
    pub header: JsValue,
}

/// SHA-256 of the value's `stableStringify` (`hashJson`).
pub fn hash_json(value: &JsValue) -> String {
    Sha256::digest(js::stable_stringify(value).as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `normalizeBody`: `\n` line endings, no trailing newlines.
pub fn normalize_body(body: &str) -> String {
    let body = body.replace("\r\n", "\n").replace('\r', "\n");
    body.trim_end_matches('\n').to_owned()
}

/// `formatYamlValue`.
fn format_yaml_value(value: &JsValue) -> String {
    match value {
        JsValue::Null => "null".into(),
        JsValue::Number(n) => number_to_string(*n),
        JsValue::Bool(b) => b.to_string(),
        other => js::stringify(other),
    }
}

/// `/^-?\d+(?:\.\d+)?$/`.
fn is_plain_number(raw: &str) -> bool {
    let digits = raw.strip_prefix('-').unwrap_or(raw);
    let (whole, fraction) = match digits.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (digits, None),
    };
    let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    all_digits(whole) && fraction.is_none_or(all_digits)
}

/// `JSON.parse(raw)`, failing with V8's message.
pub fn json_parse(raw: &str) -> Result<JsValue, String> {
    js::parse(raw)
        .map_err(|error| js::parse_error_message(raw).unwrap_or_else(|| error.to_string()))
}

/// `parseYamlValue`.
fn parse_yaml_value(raw: &str) -> Result<JsValue, String> {
    Ok(match raw {
        "" => JsValue::str(""),
        "null" => JsValue::Null,
        "true" => JsValue::Bool(true),
        "false" => JsValue::Bool(false),
        _ if is_plain_number(raw) => JsValue::Number(string_to_number(raw)),
        _ if raw.starts_with(['"', '{', '[']) => json_parse(raw)?,
        _ => JsValue::str(raw),
    })
}

/// `/^[A-Za-z0-9_-]+$/`.
fn is_field_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `parseMarkdown`: a document file's front matter fields and body.
pub fn parse_markdown(text: &str) -> Result<(JsObject, String), String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let Some(rest) = normalized.strip_prefix("---\n") else {
        return Err("missing frontmatter".into());
    };
    let Some(end) = rest.find("\n---\n") else {
        return Err("unterminated frontmatter".into());
    };
    let header = &rest[..end];
    let body = &rest[end + 5..];
    let body = normalize_body(body.strip_prefix('\n').unwrap_or(body));
    let mut fields = JsObject::new();
    for line in header.split('\n') {
        if jsx::trim(line).is_empty()
            || line
                .trim_start_matches(prosecraft_core::html::is_js_whitespace)
                .starts_with('#')
        {
            continue;
        }
        let colon = line
            .find(':')
            .filter(|&c| c > 0)
            .ok_or("invalid frontmatter line")?;
        let key = jsx::trim(&line[..colon]);
        if !is_field_key(key) {
            return Err("invalid frontmatter key".into());
        }
        let value = parse_yaml_value(jsx::trim(&line[colon + 1..]))?;
        // `fields.__proto__ = value` sets the prototype, not a property.
        if key != "__proto__" {
            fields.insert(key, value);
        }
    }
    Ok((fields, body))
}

/// The front matter fields the app hashes (`documentPayload`).
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

fn document_payload(fields: &JsObject) -> JsValue {
    let mut payload = JsObject::new();
    for key in DOCUMENT_FIELDS {
        if let Some(value) = fields.get(key) {
            payload.insert(key, value.clone());
        }
    }
    JsValue::Object(payload)
}

/// `/^\[\^[^\]\s]+\]:/m`: a line that starts a Markdown footnote definition.
fn has_footnote_definition(body: &str) -> bool {
    let is_line_start = |prev: Option<char>| {
        prev.is_none_or(|c| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
    };
    let mut prev = None;
    for (index, c) in body.char_indices() {
        if is_line_start(prev) && c == '[' && footnote_label_at(&body[index..]) {
            return true;
        }
        prev = Some(c);
    }
    false
}

fn footnote_label_at(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("[^") else {
        return false;
    };
    let label_len = rest
        .char_indices()
        .find(|(_, c)| *c == ']' || prosecraft_core::html::is_js_whitespace(*c))
        .map_or(rest.len(), |(i, _)| i);
    label_len > 0 && rest[label_len..].starts_with("]:")
}

/// `serializeMarkdown`: the document file, front matter then the body. When the body carries
/// footnote definitions they are the footnotes, so the front matter copy is left out.
pub fn serialize_markdown(all_fields: &JsObject, body: &str) -> String {
    let mut fields = all_fields.clone();
    if has_footnote_definition(body) {
        fields.remove("footnotes");
    }
    let body = normalize_body(body);
    let hash = hash_json(&object(vec![
        ("fields", document_payload(&fields)),
        ("body", JsValue::str(&body)),
    ]));
    let mut entries: Vec<String> = fields
        .iter()
        .map(|(key, value)| format!("{}: {}", key.to_string_lossy(), format_yaml_value(value)))
        .collect();
    entries.push(format!("hash: {}", js::stringify(&JsValue::str(&hash))));
    format!("---\n{}\n---\n\n{body}\n", entries.join("\n"))
}

/// `assertOpenRoot`.
pub fn assert_open_root(root: &str) -> Result<(), String> {
    let marker = join(&[root, MARKER_FILE]);
    let ok = fsx::read_text(&marker)
        .ok()
        .and_then(|text| js::parse(&text).ok())
        .is_some_and(|raw| jsx::is_str(jsx::opt(Some(&raw), "format"), "prosecraft.open"));
    if ok {
        Ok(())
    } else {
        Err(format!("Not a ProseCraft open folder: {root}"))
    }
}

/// `readProjectHeader`.
fn read_project_header(root: &str, dir_name: &str) -> Result<Found, String> {
    let dir = join(&[root, PROJECTS_DIR, dir_name]);
    let raw = json_parse(&fsx::read_text(&join(&[&dir, "project.json"]))?)?;
    let consistent = jsx::as_str(jsx::opt(Some(&raw), "id"))
        .is_some_and(|id| safe_name(&id.to_string_lossy()) == dir_name);
    if !consistent {
        return Err(format!(
            "Project {dir_name} has inconsistent identity metadata."
        ));
    }
    Ok(Found { dir, header: raw })
}

/// A project of `list_projects`.
pub struct ProjectEntry {
    pub id: JsString,
    pub name: JsValue,
    pub dir_name: String,
}

impl ProjectEntry {
    pub fn to_js(&self) -> JsValue {
        object(vec![
            ("id", JsValue::String(self.id.clone())),
            ("name", self.name.clone()),
            ("dirName", JsValue::str(&self.dir_name)),
        ])
    }
}

/// `listProjects`: live projects, by id.
pub fn list_projects(root: &str) -> Result<Vec<ProjectEntry>, String> {
    assert_open_root(root)?;
    let base = join(&[root, PROJECTS_DIR]);
    let entries = fsx::read_dir(&base).unwrap_or_default();
    let mut out = Vec::new();
    for (name, is_dir) in entries {
        if !is_dir {
            continue;
        }
        // Incomplete or corrupt project folders are skipped; a direct access reports them.
        let Ok(found) = read_project_header(root, &name) else {
            continue;
        };
        let header = Some(&found.header);
        if !jsx::is_null(jsx::opt(header, "deletedAt")) {
            continue;
        }
        let Some(id) = jsx::as_str(jsx::opt(header, "id")).cloned() else {
            continue;
        };
        let name_value = jsx::nullish(jsx::opt(jsx::opt(header, "project"), "name"))
            .cloned()
            .unwrap_or_else(|| JsValue::String(id.clone()));
        out.push(ProjectEntry {
            id,
            name: name_value,
            dir_name: name,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn assert_live_project(project_id: &str, found: Found) -> Result<Found, String> {
    if !jsx::is_null(jsx::opt(Some(&found.header), "deletedAt")) {
        return Err(format!("Project is deleted: {project_id}"));
    }
    Ok(found)
}

/// `findProjectDir`: the folder named after the id, else a scan for an encoded name.
pub fn find_project_dir(root: &str, project_id: &str) -> Result<Found, String> {
    if let Ok(found) = read_project_header(root, &safe_name(project_id)) {
        return assert_live_project(project_id, found);
    }
    for project in list_projects(root)? {
        if project.id.eq_str(project_id) {
            return assert_live_project(project_id, read_project_header(root, &project.dir_name)?);
        }
    }
    Err(format!("Project not found: {project_id}"))
}

/// `header.entities[kind]`, throwing like V8 when `entities` is missing.
pub fn entities<'a>(header: &'a JsValue, kind: &str) -> Result<Option<&'a JsValue>, String> {
    prop(prop(Some(header), "entities")?, kind)
}

/// `header.order[kind] ?? []`, for sorting by it.
pub fn order(header: &JsValue, kind: &str) -> Result<Vec<JsValue>, String> {
    let list = jsx::nullish(prop(prop(Some(header), "order")?, kind)?);
    Ok(match list {
        Some(JsValue::Array(items)) => items.clone(),
        _ => Vec::new(),
    })
}

/// `meta.deletedAt !== null`, throwing like V8 when `meta` is `null`.
pub fn is_deleted(meta: &JsValue) -> Result<bool, String> {
    Ok(!jsx::is_null(prop(Some(meta), "deletedAt")?))
}

/// `!meta || meta.deletedAt !== null`: no live entity under this id.
pub fn is_missing(meta: Option<&JsValue>) -> bool {
    jsx::falsy(meta) || !jsx::is_null(meta.and_then(|m| jsx::get(m, "deletedAt")))
}

/// `header.entities[kind]?.[id]`.
pub fn entity<'a>(
    header: &'a JsValue,
    kind: &str,
    id: &str,
) -> Result<Option<&'a JsValue>, String> {
    Ok(jsx::opt(entities(header, kind)?, id))
}

/// Sorts by each id's place in `order` (`order.indexOf(a.id) - order.indexOf(b.id)`); ids
/// missing from it come first. The sort is stable, like `Array.prototype.sort`.
pub fn sort_by_order<T>(items: &mut [T], order: &[JsValue], id: impl Fn(&T) -> &str) {
    items.sort_by_key(|item| jsx::index_of(order, id(item)));
}

pub fn document_path(dir: &str, id: &str) -> String {
    join(&[dir, "documents", &format!("{}.md", safe_name(id))])
}

pub fn chapter_path(dir: &str, id: &str) -> String {
    join(&[dir, "chapters", &format!("{}.json", safe_name(id))])
}

/// `typeof value === "string" ? value : ""`.
pub fn str_or_empty(value: Option<&JsValue>) -> JsValue {
    match value {
        Some(JsValue::String(s)) => JsValue::String(s.clone()),
        _ => JsValue::str(""),
    }
}

/// `typeof value === "string" ? value : null`.
pub fn str_or_null(value: Option<&JsValue>) -> JsValue {
    match value {
        Some(JsValue::String(s)) => JsValue::String(s.clone()),
        _ => JsValue::Null,
    }
}

/// A list filter: `labelId`, and `trashed` (`true` lists only trashed items).
#[derive(Clone, Debug, Default)]
pub struct ListFilter {
    pub label_id: Option<String>,
    pub trashed: Option<bool>,
    pub chapter_id: Option<String>,
}

/// `listChapters`.
pub fn list_chapters(
    root: &str,
    project_id: &str,
    filter: &ListFilter,
) -> Result<Vec<JsValue>, String> {
    let Found { dir, header } = find_project_dir(root, project_id)?;
    let mut out: Vec<(String, JsValue)> = Vec::new();
    for (id, meta) in jsx::entries(jsx::nullish(entities(&header, "chapters")?)) {
        if is_deleted(&meta)? {
            continue;
        }
        let raw = json_parse(&fsx::read_text(&chapter_path(&dir, &id))?)?;
        let empty = JsValue::Object(JsObject::new());
        let record = jsx::nullish(jsx::opt(Some(&raw), "record")).unwrap_or(&empty);
        let label_id = str_or_null(jsx::get(record, "labelId"));
        let trashed = jsx::is_true(jsx::get(record, "trashed"));
        if filter
            .label_id
            .as_deref()
            .is_some_and(|wanted| !jsx::is_str(Some(&label_id), wanted))
        {
            continue;
        }
        if trashed != (filter.trashed == Some(true)) {
            continue;
        }
        let mut entry = JsObject::new();
        entry.insert("id", JsValue::str(&id));
        entry.insert("title", str_or_empty(jsx::get(record, "title")));
        entry.insert(
            "expanded",
            JsValue::Bool(jsx::is_true(jsx::get(record, "expanded"))),
        );
        if let Some(summary) = jsx::as_str(jsx::get(record, "summary")) {
            entry.insert("summary", JsValue::String(summary.clone()));
        }
        entry.insert("labelId", label_id);
        entry.insert("trashed", JsValue::Bool(trashed));
        entry.insert("meta", meta);
        out.push((id, JsValue::Object(entry)));
    }
    let order = order(&header, "chapters")?;
    sort_by_order(&mut out, &order, |(id, _)| id);
    Ok(out.into_iter().map(|(_, entry)| entry).collect())
}

/// A document of `list_scenes`, before the server slims it.
pub struct SceneEntry {
    pub id: String,
    pub title: JsValue,
    pub chapter_id: Option<JsString>,
    pub wiki: bool,
    pub folder_id: JsValue,
    pub label_id: JsValue,
    pub trashed: bool,
    pub markdown: String,
    pub meta: JsValue,
}

/// `listScenes`: live documents; a malformed file is skipped (`read_scene` reports it).
pub fn list_scenes(
    root: &str,
    project_id: &str,
    filter: &ListFilter,
) -> Result<Vec<SceneEntry>, String> {
    let Found { dir, header } = find_project_dir(root, project_id)?;
    let mut out = Vec::new();
    for (id, meta) in jsx::entries(jsx::nullish(entities(&header, "documents")?)) {
        if is_deleted(&meta)? {
            continue;
        }
        let Ok((fields, body)) =
            fsx::read_text(&document_path(&dir, &id)).and_then(|text| parse_markdown(&text))
        else {
            continue;
        };
        let label_id = str_or_null(fields.get("labelId"));
        let trashed = jsx::is_true(fields.get("trashed"));
        if filter
            .chapter_id
            .as_deref()
            .is_some_and(|wanted| !jsx::is_str(fields.get("chapterId"), wanted))
        {
            continue;
        }
        if filter
            .label_id
            .as_deref()
            .is_some_and(|wanted| !jsx::is_str(Some(&label_id), wanted))
        {
            continue;
        }
        if trashed != (filter.trashed == Some(true)) {
            continue;
        }
        out.push(SceneEntry {
            title: str_or_empty(fields.get("title")),
            chapter_id: jsx::as_str(fields.get("chapterId")).cloned(),
            wiki: jsx::is_str(fields.get("space"), "Wiki"),
            folder_id: str_or_null(fields.get("folderId")),
            label_id,
            trashed,
            markdown: body,
            meta,
            id,
        });
    }
    let order = order(&header, "documents")?;
    sort_by_order(&mut out, &order, |entry| &entry.id);
    Ok(out)
}

/// `{ id, title, chapterId?, markdown, meta, fields }`, the shape `read_scene` and
/// `write_scene` return.
pub fn scene_record(
    id: &str,
    title: JsValue,
    chapter_id: Option<JsValue>,
    markdown: &str,
    meta: JsValue,
    fields: JsObject,
) -> JsObject {
    let mut out = JsObject::new();
    out.insert("id", JsValue::str(id));
    out.insert("title", title);
    if let Some(chapter_id) = chapter_id {
        out.insert("chapterId", chapter_id);
    }
    out.insert("markdown", JsValue::str(markdown));
    out.insert("meta", meta);
    out.insert("fields", JsValue::Object(fields));
    out
}

/// `readScene`.
pub fn read_scene(
    root: &str,
    project_id: &str,
    scene_id: &str,
    include_trashed: bool,
) -> Result<JsValue, String> {
    let Found { dir, header } = find_project_dir(root, project_id)?;
    let meta = entity(&header, "documents", scene_id)?;
    if is_missing(meta) {
        return Err(format!("Scene not found: {scene_id}"));
    }
    let meta = meta.cloned().unwrap_or(JsValue::Null);
    let file = document_path(&dir, scene_id);
    let (fields, body) = fsx::read_text(&file)
        .and_then(|text| parse_markdown(&text))
        .map_err(|reason| format!("Scene {scene_id} could not be read: {reason} ({file})"))?;
    if jsx::is_true(fields.get("trashed")) && !include_trashed {
        return Err(format!(
            "Scene is in the trash: {scene_id} (pass includeTrashed to read it)"
        ));
    }
    let title = str_or_empty(fields.get("title"));
    let chapter_id = jsx::as_str(fields.get("chapterId")).map(|c| JsValue::String(c.clone()));
    Ok(JsValue::Object(scene_record(
        scene_id, title, chapter_id, &body, meta, fields,
    )))
}

/// `bumpMeta`: the next revision, by "mcp", never earlier than the last.
pub fn bump_meta(meta: &JsValue, now: f64) -> JsValue {
    let mut next = jsx::spread(Some(meta));
    let updated_at = jsx::add_one(jsx::get(meta, "updatedAt"));
    next.insert("updatedAt", jsx::math_max(now, &updated_at));
    next.insert("revision", jsx::add_one(jsx::get(meta, "revision")));
    next.insert("updatedBy", JsValue::str("mcp"));
    JsValue::Object(next)
}

/// The meta of an entity that did not exist yet.
pub fn new_meta() -> JsValue {
    object(vec![
        ("updatedAt", JsValue::Number(0.0)),
        ("revision", JsValue::Number(0.0)),
        ("updatedBy", JsValue::str("mcp")),
        ("deletedAt", JsValue::Null),
    ])
}

/// The header after a write: `updatedAt` moved on, `revision` up by one, `updatedBy: "mcp"`.
pub fn bump_project_header(header: &mut JsValue, now: f64) -> Result<(), String> {
    let updated_at = jsx::math_max(now, &jsx::add_one(jsx::get(header, "updatedAt")));
    jsx::set_prop(Some(header), "updatedAt", updated_at)?;
    let revision = jsx::add_one(jsx::get(header, "revision"));
    jsx::set_prop(Some(header), "revision", revision)?;
    jsx::set_prop(Some(header), "updatedBy", JsValue::str("mcp"))
}

/// `writeProjectHeader`: the header with its hash recomputed over everything else.
pub fn write_project_header(dir: &str, header: &JsValue) -> Result<(), String> {
    let mut payload = jsx::spread(Some(header));
    payload.remove("hash");
    let hash = hash_json(&JsValue::Object(payload.clone()));
    payload.insert("hash", JsValue::str(&hash));
    let content = format!("{}\n", js::stringify_pretty(&JsValue::Object(payload)));
    fsx::write_atomic(&join(&[dir, "project.json"]), &content)
}

/// `header.entities[kind] = { ...header.entities[kind], [id]: meta }`.
pub fn set_entity(header: &mut JsValue, kind: &str, id: &str, meta: JsValue) -> Result<(), String> {
    let mut map = jsx::spread(jsx::nullish(entities(header, kind)?));
    map.insert(id, meta);
    jsx::set_prop(field_mut(header, "entities"), kind, JsValue::Object(map))
}

/// `header[key]`, mutable, whatever its type (an assignment into a primitive then fails as in
/// JavaScript).
pub fn field_mut<'a>(header: &'a mut JsValue, key: &str) -> Option<&'a mut JsValue> {
    header.as_object_mut()?.get_mut(key)
}

/// `header.entities[kind][id] = meta`, writing into the map in place.
pub fn replace_entity(
    header: &mut JsValue,
    kind: &str,
    id: &str,
    meta: JsValue,
) -> Result<(), String> {
    let map = field_mut(header, "entities").and_then(|e| jsx::get_mut(e, kind));
    jsx::set_prop(map, id, meta)
}

/// `assertExpectedRevision`.
pub fn assert_expected_revision(
    label: &str,
    expected: Option<f64>,
    current: Option<&JsValue>,
) -> Result<(), String> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let expected_text = number_to_string(expected);
    let Some(current) = current else {
        return Err(format!(
            "{label} does not exist, so expectedRevision {expected_text} cannot match; nothing was written."
        ));
    };
    if current.as_number() != Some(expected) {
        return Err(format!(
            "{label} revision mismatch: expectedRevision {expected_text}, current revision is {}; nothing was written.",
            jsx::to_js_string(Some(current))
        ));
    }
    Ok(())
}

/// The chapter file: `{ record, meta, hash }` with the hash over record and meta.
pub fn chapter_file(record: &JsValue, meta: &JsValue) -> String {
    let hash = hash_json(&object(vec![
        ("record", record.clone()),
        ("meta", meta.clone()),
    ]));
    let file = object(vec![
        ("record", record.clone()),
        ("meta", meta.clone()),
        ("hash", JsValue::str(&hash)),
    ]);
    format!("{}\n", js::stringify_pretty(&file))
}

/// `trashScene`: tombstones the header first, then removes the file, the order the app's reader
/// tolerates.
pub fn trash_scene(
    root: &str,
    env: &mut dyn Env,
    project_id: &str,
    scene_id: &str,
    expected_revision: Option<f64>,
) -> Result<JsValue, String> {
    fsx::with_lock(root, env, |env| {
        let Found { dir, mut header } = find_project_dir(root, project_id)?;
        let meta = entity(&header, "documents", scene_id)?;
        if is_missing(meta) {
            return Err(format!("Scene not found: {scene_id}"));
        }
        let meta = meta.cloned().unwrap_or(JsValue::Null);
        assert_expected_revision(
            &format!("Scene {scene_id}"),
            expected_revision,
            jsx::get(&meta, "revision"),
        )?;
        let now = env.now();
        let mut next = jsx::spread(Some(&meta));
        next.insert("deletedAt", JsValue::Number(now));
        next.insert("updatedAt", JsValue::Number(now));
        next.insert("revision", jsx::add_one(jsx::get(&meta, "revision")));
        next.insert("updatedBy", JsValue::str("mcp"));
        let revision = next.get("revision").cloned().unwrap_or(JsValue::Null);
        replace_entity(&mut header, "documents", scene_id, JsValue::Object(next))?;
        bump_project_header(&mut header, now)?;
        write_project_header(&dir, &header)?;
        fsx::remove_file(&document_path(&dir, scene_id))?;
        Ok(object(vec![
            ("id", JsValue::str(scene_id)),
            ("revision", revision),
        ]))
    })
}

/// `header.order[kind]` with `id` appended, as `[...(header.order[kind] ?? []), id]`.
fn order_with(header: &JsValue, kind: &str, id: &str) -> Result<JsValue, String> {
    let current = jsx::nullish(prop(prop(Some(header), "order")?, kind)?);
    let mut list = match current {
        None => Vec::new(),
        Some(JsValue::Array(items)) => items.clone(),
        Some(JsValue::String(text)) => {
            // A string spreads into its characters.
            jsx::entries(Some(&JsValue::String(text.clone())))
                .into_iter()
                .map(|(_, c)| c)
                .collect()
        }
        Some(_) => return Err(format!("(header.order.{kind} ?? []) is not iterable")),
    };
    list.push(JsValue::str(id));
    Ok(JsValue::Array(list))
}

/// `createChapter`.
pub fn create_chapter(
    root: &str,
    env: &mut dyn Env,
    project_id: &str,
    title: &JsString,
    summary: Option<&JsString>,
) -> Result<JsValue, String> {
    fsx::with_lock(root, env, |env| {
        let Found { dir, mut header } = find_project_dir(root, project_id)?;
        let now = env.now();
        let id = env.uuid()?;
        let meta = object(vec![
            ("updatedAt", JsValue::Number(now)),
            ("revision", JsValue::Number(1.0)),
            ("updatedBy", JsValue::str("mcp")),
            ("deletedAt", JsValue::Null),
        ]);
        let mut record = JsObject::new();
        record.insert("id", JsValue::str(&id));
        record.insert("title", JsValue::String(title.clone()));
        record.insert("expanded", JsValue::Bool(true));
        if let Some(summary) = summary.filter(|s| !s.is_empty()) {
            record.insert("summary", JsValue::String(summary.clone()));
        }
        let record = JsValue::Object(record);
        fsx::mkdir_all(&join(&[&dir, "chapters"]))?;
        fsx::write_atomic(&chapter_path(&dir, &id), &chapter_file(&record, &meta))?;
        set_entity(&mut header, "chapters", &id, meta.clone())?;
        let order = order_with(&header, "chapters", &id)?;
        jsx::set_prop(field_mut(&mut header, "order"), "chapters", order)?;
        bump_project_header(&mut header, now)?;
        write_project_header(&dir, &header)?;
        let mut out = jsx::spread(Some(&record));
        out.insert("meta", meta);
        Ok(JsValue::Object(out))
    })
}

/// What `update_chapter` changes.
#[derive(Clone, Debug, Default)]
pub struct ChapterPatch {
    pub title: Option<JsString>,
    pub summary: Option<JsString>,
    pub expected_revision: Option<f64>,
}

/// `updateChapter`.
pub fn update_chapter(
    root: &str,
    env: &mut dyn Env,
    project_id: &str,
    chapter_id: &str,
    patch: &ChapterPatch,
) -> Result<JsValue, String> {
    fsx::with_lock(root, env, |env| {
        let Found { dir, mut header } = find_project_dir(root, project_id)?;
        let meta = entity(&header, "chapters", chapter_id)?;
        if is_missing(meta) {
            return Err(format!("Chapter not found: {chapter_id}"));
        }
        let meta = meta.cloned().unwrap_or(JsValue::Null);
        assert_expected_revision(
            &format!("Chapter {chapter_id}"),
            patch.expected_revision,
            jsx::get(&meta, "revision"),
        )?;
        let file = chapter_path(&dir, chapter_id);
        let raw = json_parse(&fsx::read_text(&file)?)?;
        let mut record = jsx::nullish(jsx::opt(Some(&raw), "record"))
            .cloned()
            .unwrap_or_else(|| {
                object(vec![
                    ("id", JsValue::str(chapter_id)),
                    ("title", JsValue::str("")),
                    ("expanded", JsValue::Bool(true)),
                ])
            });
        if let Some(title) = &patch.title {
            jsx::set_prop(Some(&mut record), "title", JsValue::String(title.clone()))?;
        }
        if let Some(summary) = &patch.summary {
            jsx::set_prop(
                Some(&mut record),
                "summary",
                JsValue::String(summary.clone()),
            )?;
        }
        let now = env.now();
        let next = bump_meta(&meta, now);
        fsx::write_atomic(&file, &chapter_file(&record, &next))?;
        replace_entity(&mut header, "chapters", chapter_id, next.clone())?;
        bump_project_header(&mut header, now)?;
        write_project_header(&dir, &header)?;
        let mut out = JsObject::new();
        out.insert("id", JsValue::str(chapter_id));
        if let Some(title) = jsx::get(&record, "title") {
            out.insert("title", title.clone());
        }
        out.insert(
            "expanded",
            JsValue::Bool(jsx::is_true(jsx::get(&record, "expanded"))),
        );
        if let Some(summary) = jsx::as_str(jsx::get(&record, "summary")) {
            out.insert("summary", JsValue::String(summary.clone()));
        }
        out.insert("meta", next);
        Ok(JsValue::Object(out))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_round_trips_with_the_typescript_rules() {
        let text = "---\r\nid: \"a\"\n# note\nrevision: 007\nflag: true\nbare: plain words\nn: 1.50\nobj: {\"b\":1}\n---\n\nBody\n\n\n";
        let (fields, body) = parse_markdown(text).expect("parses");
        assert_eq!(body, "Body");
        assert_eq!(fields.get("revision"), Some(&JsValue::Number(7.0)));
        assert_eq!(fields.get("n"), Some(&JsValue::Number(1.5)));
        assert_eq!(fields.get("bare"), Some(&JsValue::str("plain words")));
        let out = serialize_markdown(&fields, &body);
        assert!(out.starts_with("---\nid: \"a\"\nrevision: 7\nflag: true\nbare: \"plain words\"\nn: 1.5\nobj: {\"b\":1}\nhash: \""));
        assert!(out.ends_with("\n---\n\nBody\n"));
    }

    #[test]
    fn front_matter_errors_name_the_problem() {
        assert_eq!(parse_markdown("no"), Err("missing frontmatter".into()));
        assert_eq!(
            parse_markdown("---\nid: 1\n"),
            Err("unterminated frontmatter".into())
        );
        assert_eq!(
            parse_markdown("---\n: x\n---\n"),
            Err("invalid frontmatter line".into())
        );
        assert_eq!(
            parse_markdown("---\nbad key: 1\n---\n"),
            Err("invalid frontmatter key".into())
        );
        assert_eq!(
            parse_markdown("---\nid: \"open\n---\n"),
            Err("Unterminated string in JSON at position 5 (line 1 column 6)".into())
        );
    }

    #[test]
    fn footnote_definitions_replace_the_front_matter_copy() {
        let mut fields = JsObject::new();
        fields.insert("id", JsValue::str("d"));
        fields.insert("footnotes", JsValue::Array(Vec::new()));
        assert!(serialize_markdown(&fields, "Text").contains("footnotes: []"));
        assert!(!serialize_markdown(&fields, "Text[^a]\n\n[^a]: Note").contains("footnotes"));
        assert!(serialize_markdown(&fields, "Text [^a]: inline").contains("footnotes"));
        assert!(serialize_markdown(&fields, "x\r[^a b]: no").contains("footnotes"));
    }

    #[test]
    fn plain_numbers_match_the_typescript_pattern() {
        assert!(is_plain_number("-12.5"));
        assert!(!is_plain_number("1."));
        assert!(!is_plain_number(".5"));
        assert!(!is_plain_number("1e3"));
        assert!(!is_plain_number("-"));
    }
}
