//! Wiki files and web links (ProseCraft's `src/lib/research.ts` and the research handlers in
//! `+page.svelte`): their names, types and sizes, and adding, moving and removing them on the
//! stored project. The files themselves live in a folder on the device; see the host.

use unicode_normalization::UnicodeNormalization;

use crate::edit::EditError;
use crate::js::{JsObject, JsValue};

/// `MAX_RESEARCH_FILE_BYTES`.
pub const MAX_FILE_BYTES: u64 = 200 * 1024 * 1024;
/// `MAX_RESEARCH_MARKDOWN_PREVIEW_BYTES`.
pub const MAX_MARKDOWN_PREVIEW_BYTES: u64 = 2 * 1024 * 1024;

/// `MIME_BY_EXTENSION`.
const MIME_BY_EXTENSION: [(&str, &str); 31] = [
    ("7z", "application/x-7z-compressed"),
    ("avif", "image/avif"),
    ("bmp", "image/bmp"),
    ("csv", "text/csv"),
    ("doc", "application/msword"),
    (
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    ("epub", "application/epub+zip"),
    ("gif", "image/gif"),
    ("heic", "image/heic"),
    ("html", "text/html"),
    ("jpeg", "image/jpeg"),
    ("jpg", "image/jpeg"),
    ("json", "application/json"),
    ("md", "text/markdown"),
    ("mp3", "audio/mpeg"),
    ("mp4", "video/mp4"),
    ("odt", "application/vnd.oasis.opendocument.text"),
    ("pdf", "application/pdf"),
    ("png", "image/png"),
    ("ppt", "application/vnd.ms-powerpoint"),
    (
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    ("rtf", "application/rtf"),
    ("svg", "image/svg+xml"),
    ("tif", "image/tiff"),
    ("tiff", "image/tiff"),
    ("txt", "text/plain"),
    ("webm", "video/webm"),
    ("webp", "image/webp"),
    ("xls", "application/vnd.ms-excel"),
    (
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    ("zip", "application/zip"),
];

fn utf16_prefix(text: &str, units: usize) -> String {
    let mut used = 0;
    text.chars()
        .take_while(|c| {
            used += c.len_utf16();
            used <= units
        })
        .collect()
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(crate::html::is_js_whitespace)
}

/// `normalizeResearchUrl`: an http or https address, `https://` added when there is no
/// scheme; `None` for anything else.
pub fn normalize_url(raw: &str) -> Option<String> {
    let value = js_trim(raw);
    if value.is_empty() {
        return None;
    }
    let has_scheme = {
        let mut chars = value.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && value.split_once(':').is_some_and(|(scheme, _)| {
                scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
            })
    };
    let value = if has_scheme {
        value.to_owned()
    } else {
        format!("https://{value}")
    };
    let url = url::Url::parse(&value).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}

/// `researchTitleFromUrl`: the host without `www.`, at most 200 UTF-16 units.
pub fn title_from_url(raw: &str) -> String {
    let Some(normalized) = normalize_url(raw) else {
        return utf16_prefix(js_trim(raw), 200);
    };
    match url::Url::parse(&normalized) {
        Ok(url) => {
            let host = url.host_str().unwrap_or_default();
            let host = if host.len() >= 4 && host[..4].eq_ignore_ascii_case("www.") {
                &host[4..]
            } else {
                host
            };
            let title = if host.is_empty() { url.as_str() } else { host };
            utf16_prefix(title, 200)
        }
        Err(_) => utf16_prefix(js_trim(raw), 200),
    }
}

fn is_unsafe_name_char(c: char) -> bool {
    matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || (c as u32) < 0x20
}

fn is_reserved(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || (stem.len() == 4
            && (stem.starts_with("com") || stem.starts_with("lpt"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

/// Runs of whitespace as one space (`replace(/\s+/g, " ")`).
fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if crate::html::is_js_whitespace(c) {
            space = true;
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.push(c);
        }
    }
    if space {
        out.push(' ');
    }
    out
}

/// `safeResearchFileName`: a name safe on every system, at most 160 UTF-16 units with its
/// extension kept. `now_ms` names a file that has no usable name.
pub fn safe_file_name(name: &str, now_ms: f64) -> String {
    let normalized: String = name.nfkc().collect();
    let replaced: String = normalized
        .chars()
        .map(|c| if is_unsafe_name_char(c) { '-' } else { c })
        .collect();
    let cleaned = js_trim(&collapse_spaces(&replaced))
        .trim_end_matches(['.', ' '])
        .to_owned();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return format!("research-file-{}", crate::js::number_to_string(now_ms));
    }
    let safe = if is_reserved(&cleaned) {
        format!("file-{cleaned}")
    } else {
        cleaned
    };
    let units: Vec<u16> = safe.encode_utf16().collect();
    if units.len() <= 160 {
        return safe;
    }
    let dot = units.iter().rposition(|u| *u == u16::from(b'.'));
    let extension: Vec<u16> = match dot {
        Some(d) if d > 0 && units.len() - d <= 16 => units[d..].to_vec(),
        _ => Vec::new(),
    };
    let mut kept = units[..160 - extension.len()].to_vec();
    kept.extend(extension);
    String::from_utf16_lossy(&kept)
}

/// `researchFileNameFromPath`.
pub fn file_name_from_path(path: &str, now_ms: f64) -> String {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or("file");
    safe_file_name(name, now_ms)
}

/// `researchMimeType`.
pub fn mime_type(file_name: &str) -> &'static str {
    let extension = file_name
        .rfind('.')
        .map(|d| file_name[d + 1..].to_lowercase())
        .unwrap_or_default();
    MIME_BY_EXTENSION
        .iter()
        .find(|(e, _)| *e == extension)
        .map_or("application/octet-stream", |(_, m)| m)
}

/// `formatResearchFileSize`.
pub fn format_size(bytes: f64) -> String {
    if !bytes.is_finite() || bytes < 0.0 {
        return String::new();
    }
    if bytes < 1024.0 {
        return format!("{} B", crate::js::number_to_string(bytes));
    }
    if bytes < 1024.0 * 1024.0 {
        return format!("{} KB", ((bytes / 1024.0) + 0.5).floor());
    }
    // `toFixed`: halves round up.
    let mb = bytes / (1024.0 * 1024.0);
    if bytes < 10.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", (mb * 10.0).round() / 10.0)
    } else {
        format!("{} MB", mb.round())
    }
}

/// The project folder name's cleaning (`createResearchFolderName`, `normalizeResearchFolderName`).
fn clean_folder(value: &str, limit: usize, trim_end_first: bool) -> String {
    let replaced: String = js_trim(value)
        .chars()
        .map(|c| if is_unsafe_name_char(c) { '-' } else { c })
        .collect();
    let collapsed = collapse_spaces(&replaced);
    let collapsed = collapsed.trim_start_matches('.');
    if trim_end_first {
        utf16_prefix(collapsed.trim_end_matches(['.', ' ']), limit)
    } else {
        utf16_prefix(collapsed, limit)
            .trim_end_matches(['.', ' '])
            .to_owned()
    }
}

/// `createResearchFolderName`: "Name (abcd1234)", the name at most 48 units.
pub fn create_folder_name(project_name: &str, project_id: &str) -> String {
    // `.replace(/^\.+/, "").replace(/[. ]+$/, "").slice(0, 48)`.
    let base = clean_folder(project_name, 48, true);
    let base = if base.is_empty() {
        "Untitled Project".to_owned()
    } else {
        base
    };
    let suffix: String = project_id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(8)
        .collect();
    let suffix = if suffix.is_empty() {
        "project".to_owned()
    } else {
        suffix
    };
    format!("{base} ({suffix})")
}

/// `normalizeResearchFolderName`.
pub fn normalize_folder_name(value: Option<&JsValue>) -> String {
    let Some(value) = value
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
    else {
        return String::new();
    };
    let cleaned = clean_folder(&value, 80, false);
    if cleaned == "." || cleaned == ".." {
        return String::new();
    }
    if is_reserved(&cleaned) && !cleaned.contains('.') {
        return format!("folder-{cleaned}");
    }
    cleaned
}

/// `isSafeStorageKey`: `research/<id>/<name>`.
pub fn is_safe_storage_key(value: &str) -> bool {
    let mut parts = value.split('/');
    let (Some("research"), Some(id), Some(name), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    !id.is_empty()
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !name.is_empty()
        && name != "."
        && name != ".."
        && !name.chars().any(|c| c == '\\' || (c as u32) < 0x20)
}

/// A file as the project stores it (`ResearchFile`).
#[derive(Clone, Debug, PartialEq)]
pub struct NewFile {
    pub id: String,
    pub folder_id: Option<String>,
    pub file_name: String,
    pub size: u64,
    pub added_at: f64,
}

impl NewFile {
    /// `research/<id>/<file name>`.
    pub fn storage_key(&self) -> String {
        format!("research/{}/{}", self.id, self.file_name)
    }

    fn to_value(&self) -> JsValue {
        let mut o = JsObject::new();
        o.insert("id", JsValue::str(&self.id));
        o.insert("kind", JsValue::str("file"));
        o.insert(
            "folderId",
            self.folder_id
                .as_deref()
                .map_or(JsValue::Null, JsValue::str),
        );
        o.insert("title", JsValue::str(&self.file_name));
        o.insert("fileName", JsValue::str(&self.file_name));
        o.insert("mimeType", JsValue::str(mime_type(&self.file_name)));
        o.insert("size", JsValue::Number(self.size as f64));
        o.insert("storageKey", JsValue::str(&self.storage_key()));
        o.insert("addedAt", JsValue::Number(self.added_at));
        JsValue::Object(o)
    }
}

/// A link to add (`saveResearchLink`); `None` with ProseCraft's message when the address
/// isn't http or https.
pub fn new_link(
    id: &str,
    folder_id: Option<&str>,
    url: &str,
    title: &str,
    description: &str,
    added_at: f64,
) -> Result<JsValue, &'static str> {
    let url = normalize_url(url).ok_or("Enter a valid http or https address.")?;
    let title = utf16_prefix(js_trim(title), 200);
    let title = if title.is_empty() {
        title_from_url(&url)
    } else {
        title
    };
    let description = utf16_prefix(js_trim(description), 2000);
    let mut o = JsObject::new();
    o.insert("id", JsValue::str(id));
    o.insert("kind", JsValue::str("link"));
    o.insert("folderId", folder_id.map_or(JsValue::Null, JsValue::str));
    o.insert("title", JsValue::str(&title));
    o.insert("url", JsValue::str(&url));
    if !description.is_empty() {
        o.insert("description", JsValue::str(&description));
    }
    o.insert("addedAt", JsValue::Number(added_at));
    Ok(JsValue::Object(o))
}

fn items_mut(project: &mut JsValue) -> Result<&mut Vec<JsValue>, EditError> {
    let object = project.as_object_mut().ok_or(EditError::Shape)?;
    if !matches!(object.get("researchItems"), Some(JsValue::Array(_))) {
        object.insert("researchItems", JsValue::Array(Vec::new()));
    }
    match object.get_mut("researchItems") {
        Some(JsValue::Array(list)) => Ok(list),
        _ => Err(EditError::Shape),
    }
}

/// Adds files the host has copied into the project folder.
pub fn add_files(project: &mut JsValue, files: &[NewFile]) -> Result<(), EditError> {
    items_mut(project)?.extend(files.iter().map(NewFile::to_value));
    Ok(())
}

/// Adds a link made by [`new_link`].
pub fn add_link(project: &mut JsValue, link: JsValue) -> Result<(), EditError> {
    items_mut(project)?.push(link);
    Ok(())
}

fn id_of(item: &JsValue) -> String {
    item.get("id")
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default()
}

/// `moveResearchItem`: into a folder that exists, or to the top level.
pub fn move_item(project: &mut JsValue, id: &str, folder: Option<&str>) -> Result<(), EditError> {
    if let Some(folder) = folder {
        let exists = project
            .get("researchFolders")
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .any(|f| id_of(f) == folder);
        if !exists {
            return Ok(());
        }
    }
    for item in items_mut(project)?.iter_mut().filter(|i| id_of(i) == id) {
        if let Some(o) = item.as_object_mut() {
            o.insert("folderId", folder.map_or(JsValue::Null, JsValue::str));
        }
    }
    Ok(())
}

/// `removeResearchItem` on the project (the host deletes the file).
pub fn remove_item(project: &mut JsValue, id: &str) -> Result<(), EditError> {
    items_mut(project)?.retain(|i| id_of(i) != id);
    Ok(())
}

/// `ensureResearchFolderName`: the project's wiki folder name, made and stored the first
/// time files are added.
pub fn ensure_folder_name(
    project: &mut JsValue,
    project_name: &str,
    project_id: &str,
) -> Result<String, EditError> {
    let current = normalize_folder_name(project.get("researchFolderName"));
    if !current.is_empty() {
        return Ok(current);
    }
    let name = create_folder_name(project_name, project_id);
    project
        .as_object_mut()
        .ok_or(EditError::Shape)?
        .insert("researchFolderName", JsValue::str(&name));
    Ok(name)
}

/// A stored file or link, read for the binder and the host.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    File {
        id: String,
        title: String,
        file_name: String,
        mime_type: String,
        size: f64,
        storage_key: String,
    },
    Link {
        id: String,
        title: String,
        url: String,
        description: String,
    },
}

impl Item {
    /// `researchItemTitle`.
    pub fn title(&self) -> &str {
        match self {
            Self::File {
                title, file_name, ..
            } => {
                if title.is_empty() {
                    file_name
                } else {
                    title
                }
            }
            Self::Link { title, url, .. } => {
                if title.is_empty() {
                    url
                } else {
                    title
                }
            }
        }
    }

    /// `researchItemMeta`.
    pub fn meta(&self) -> String {
        match self {
            Self::Link {
                url, description, ..
            } => {
                let description = js_trim(description);
                if description.is_empty() {
                    url.clone()
                } else {
                    format!("{url}\n{description}")
                }
            }
            Self::File {
                size, mime_type, ..
            } => [format_size(*size), mime_type.clone()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · "),
        }
    }
}

/// `normalizeResearchItems`: the project's files and links as ProseCraft keeps them on load.
pub fn normalize_items(
    value: Option<&JsValue>,
    folder_ids: &[String],
) -> Vec<(Item, Option<String>, f64)> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let text = |v: &JsValue, k: &str| {
        v.get(k)
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
    };
    for entry in value.and_then(JsValue::as_array).unwrap_or_default() {
        if entry.as_object().is_none() {
            continue;
        }
        let Some(id) = text(entry, "id").filter(|i| !i.is_empty() && !seen.contains(i)) else {
            continue;
        };
        let kind = text(entry, "kind").unwrap_or_default();
        if kind != "file" && kind != "link" {
            continue;
        }
        let folder = text(entry, "folderId").filter(|f| folder_ids.contains(f));
        let title = text(entry, "title")
            .map(|t| utf16_prefix(js_trim(&t), 200))
            .unwrap_or_default();
        let added = entry
            .get("addedAt")
            .and_then(JsValue::as_number)
            .filter(|n| n.is_finite())
            .unwrap_or(0.0);
        if kind == "file" {
            let (Some(file_name), Some(mime), Some(key)) = (
                text(entry, "fileName").filter(|n| !n.is_empty()),
                text(entry, "mimeType"),
                text(entry, "storageKey").filter(|k| is_safe_storage_key(k)),
            ) else {
                continue;
            };
            let Some(size) = entry
                .get("size")
                .and_then(JsValue::as_number)
                .filter(|n| n.is_finite() && *n >= 0.0)
            else {
                continue;
            };
            seen.insert(id.clone());
            let mime = utf16_prefix(&mime, 200);
            out.push((
                Item::File {
                    id,
                    title: if title.is_empty() {
                        file_name.clone()
                    } else {
                        title
                    },
                    file_name: utf16_prefix(&file_name, 255),
                    mime_type: if mime.is_empty() {
                        "application/octet-stream".to_owned()
                    } else {
                        mime
                    },
                    size,
                    storage_key: key,
                },
                folder,
                added,
            ));
            continue;
        }
        let Some(url) = text(entry, "url").and_then(|u| normalize_url(&u)) else {
            continue;
        };
        seen.insert(id.clone());
        out.push((
            Item::Link {
                id,
                title: if title.is_empty() {
                    title_from_url(&url)
                } else {
                    title
                },
                url,
                description: text(entry, "description")
                    .map(|d| utf16_prefix(&d, 2000))
                    .unwrap_or_default(),
            },
            folder,
            added,
        ));
    }
    out
}
