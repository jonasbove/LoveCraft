//! Scrivener import (ProseCraft `src/lib/import/scrivener.ts`, upstream `7dae544`): a `.scriv`
//! project (a folder or a zip) read into a project. Draft folders become chapters and their
//! texts scenes; the research folder and the other top-level items become Wiki pages and
//! folders. The importer is forgiving: one unreadable document never loses the rest.

use super::dom::Dom;
use super::rtf::rtf_to_html;
use crate::html::{PlainTextOptions, html_to_plain_text, is_js_whitespace, sanitize_html};
use crate::js::{JsObject, JsValue};

const MAX_UNPACKED_BYTES: u64 = 200 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;
const MAX_SUMMARY_LENGTH: usize = 4_000;
const MAX_FOLDER_TITLE_LENGTH: usize = 60;
const EMPTY_CONTENT: &str = "<p><br></p>";

/// Why a project can't be imported, in ProseCraft's words ("Scrivener import: …").
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ScrivenerError(pub String);

fn error(message: &str) -> ScrivenerError {
    ScrivenerError(format!("Scrivener import: {message}"))
}

/// The files of one project, by the relative paths they were picked with, in the order they
/// came (a later file of the same path replaces the earlier one in its place, as a `Map` does).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScrivenerFiles(Vec<(String, Vec<u8>)>);

impl ScrivenerFiles {
    fn insert(&mut self, path: String, bytes: Vec<u8>) {
        match self.0.iter_mut().find(|(p, _)| *p == path) {
            Some(slot) => slot.1 = bytes,
            None => self.0.push((path, bytes)),
        }
    }

    fn get(&self, path: &str) -> Option<&[u8]> {
        self.0
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, b)| b.as_slice())
    }

    fn has(&self, path: &str) -> bool {
        self.get(path).is_some()
    }
}

/// What the import found (`ScrivenerImportReport`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScrivenerReport {
    pub chapters: usize,
    pub scenes: usize,
    pub wiki_pages: usize,
    pub wiki_folders: usize,
    pub flattened_folders: usize,
    pub skipped_files: usize,
    pub items_with_notes: usize,
    pub missing_files: usize,
    /// "docs" (Scrivener 1 and 2) or "data" (Scrivener 3).
    pub layout: &'static str,
}

fn collapse(value: &str) -> String {
    value
        .split(is_js_whitespace)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `value.slice(0, max).trimEnd()` when it is longer (UTF-16 units).
fn truncate(value: &str, max: usize) -> String {
    let units: Vec<u16> = value.encode_utf16().collect();
    if units.len() <= max {
        return value.to_owned();
    }
    String::from_utf16_lossy(&units[..max])
        .trim_end_matches(is_js_whitespace)
        .to_owned()
}

/// `normalizePath`: forward slashes, no `.` or empty segments; absolute or escaping paths refused.
fn normalize_path(raw: &str) -> Result<String, ScrivenerError> {
    let path = raw.replace('\\', "/");
    if path.is_empty() {
        return Ok(String::new());
    }
    let mut chars = path.chars();
    let drive =
        matches!((chars.next(), chars.next()), (Some(c), Some(':')) if c.is_ascii_alphabetic());
    if path.starts_with('/') || drive {
        return Err(error("the project contains an absolute path."));
    }
    let segments: Vec<&str> = path
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .collect();
    if segments.contains(&"..") {
        return Err(error("the project contains an unsafe path."));
    }
    Ok(segments.join("/"))
}

/// `replace(/\.scriv$/i, "")`.
fn strip_scriv(name: &str) -> &str {
    let cut = name.len().saturating_sub(".scriv".len());
    match name.get(cut..) {
        Some(tail) if tail.eq_ignore_ascii_case(".scriv") => &name[..cut],
        _ => name,
    }
}

fn is_ignored_path(path: &str) -> bool {
    path.is_empty()
        || path.starts_with("__MACOSX/")
        || path == ".DS_Store"
        || path.ends_with("/.DS_Store")
}

/// `scrivenerFilesFromEntries`: a folder picker's files (relative paths).
pub fn files_from_entries(
    entries: Vec<(String, Vec<u8>)>,
) -> Result<ScrivenerFiles, ScrivenerError> {
    if entries.len() > MAX_ENTRIES {
        return Err(error("the project contains too many files."));
    }
    let mut total: u64 = 0;
    let mut files = ScrivenerFiles::default();
    for (path, bytes) in entries {
        let path = normalize_path(&path)?;
        if is_ignored_path(&path) {
            continue;
        }
        total += bytes.len() as u64;
        if total > MAX_UNPACKED_BYTES {
            return Err(error("the project is larger than 200 MB."));
        }
        files.insert(path, bytes);
    }
    Ok(files)
}

/// `scrivenerFilesFromZip`: a zipped `.scriv` folder (or a Scrivener backup zip).
pub fn files_from_zip(bytes: &[u8]) -> Result<ScrivenerFiles, ScrivenerError> {
    let not_zip = || error("this file is not a readable zip archive.");
    let limits = craft_zip::Limits {
        max_entries: 1_000_000,
        max_total_bytes: MAX_UNPACKED_BYTES + 1,
    };
    let mut zip = craft_zip::Archive::open(bytes, limits).map_err(|_| not_zip())?;
    let names: Vec<String> = zip.names().map(|n| n.into_owned()).collect();
    let mut count = 0;
    let mut total: u64 = 0;
    let mut entries = Vec::new();
    for name in names {
        if name.ends_with('/') {
            continue;
        }
        let path = normalize_path(&name)?;
        if is_ignored_path(&path) {
            continue;
        }
        count += 1;
        if count > MAX_ENTRIES {
            return Err(error("the project contains too many files."));
        }
        total += zip.declared_size(&name).unwrap_or(0);
        if total > MAX_UNPACKED_BYTES {
            return Err(error("the project is larger than 200 MB."));
        }
        let data = zip
            .read(&name, MAX_UNPACKED_BYTES)
            .map_err(|_| not_zip())?
            .unwrap_or_default();
        entries.push((name, data));
    }
    files_from_entries(entries)
}

/// `assembleProjectFiles`: the files below the shortest path holding `project.scrivx`, and
/// the `.scriv` folder's name.
fn assemble(files: &ScrivenerFiles) -> (ScrivenerFiles, Option<String>) {
    let mut candidates: Vec<&String> = files
        .0
        .iter()
        .map(|(p, _)| p)
        .filter(|p| *p == "project.scrivx" || p.ends_with("/project.scrivx"))
        .collect();
    candidates.sort_by_key(|p| p.encode_utf16().count());
    let root = candidates.first().map_or(String::new(), |p| {
        p[..p.len() - "project.scrivx".len()].to_owned()
    });
    let mut out = ScrivenerFiles::default();
    for (path, bytes) in &files.0 {
        let path = if root.is_empty() {
            path.as_str()
        } else {
            match path.strip_prefix(root.as_str()) {
                Some(rest) => rest,
                None => continue,
            }
        };
        if path.is_empty() {
            continue;
        }
        out.insert(path.to_owned(), bytes.clone());
    }
    let name = (!root.is_empty())
        .then(|| {
            let last = root.split('/').rfind(|s| !s.is_empty()).unwrap_or_default();
            collapse(strip_scriv(last))
        })
        .filter(|n| !n.is_empty());
    (out, name)
}

/// `new TextDecoder("utf-8").decode(bytes)`.
fn decode_utf8(bytes: &[u8]) -> String {
    let (text, _) = encoding_rs::UTF_8.decode_with_bom_removal(bytes);
    text.into_owned()
}

struct Builder<'a> {
    dom: Dom,
    files: ScrivenerFiles,
    report: ScrivenerReport,
    create_id: &'a mut dyn FnMut() -> String,
}

/// A chapter being filled: its record and its scenes.
struct ChapterDraft {
    chapter: JsObject,
    scenes: Vec<JsValue>,
}

impl Builder<'_> {
    fn children(&self, parent: usize, name: &str) -> Vec<usize> {
        self.dom
            .child_elements(parent)
            .into_iter()
            .filter(|c| self.dom.local(*c) == name)
            .collect()
    }

    fn first(&self, parent: usize, name: &str) -> Option<usize> {
        self.children(parent, name).first().copied()
    }

    fn text_of(&self, parent: usize, name: &str) -> String {
        self.first(parent, name)
            .map(|e| collapse(&self.dom.text_content(e)))
            .unwrap_or_default()
    }

    fn child_items(&self, item: usize) -> Vec<usize> {
        self.first(item, "Children")
            .map(|c| self.children(c, "BinderItem"))
            .unwrap_or_default()
    }

    fn kind(&self, item: usize) -> String {
        self.dom.attr(item, "Type").unwrap_or_default().to_owned()
    }

    fn title(&self, item: usize, fallback: &str) -> String {
        let t = self.text_of(item, "Title");
        if t.is_empty() { fallback.to_owned() } else { t }
    }

    /// Scrivener only writes a content file for documents that were ever edited.
    fn has_file_extension(&self, item: usize) -> bool {
        self.first(item, "MetaData")
            .is_some_and(|m| self.first(m, "FileExtension").is_some())
    }

    fn find_first(&self, items: &[usize], kind: &str) -> Option<usize> {
        for item in items {
            if self.kind(*item) == kind {
                return Some(*item);
            }
            if let Some(nested) = self.find_first(&self.child_items(*item), kind) {
                return Some(nested);
            }
        }
        None
    }

    fn ids(&self, item: usize) -> (Option<String>, Option<String>) {
        let get = |k: &str| {
            self.dom
                .attr(item, k)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        (get("UUID"), get("ID"))
    }

    /// `readContent`: the item's text as sanitized HTML, or `None` (a missing file counted
    /// when the item says it has one and `required`).
    fn read_content(&mut self, item: usize, required: bool) -> Option<String> {
        let (uuid, id) = self.ids(item);
        let paths: Vec<String> = uuid
            .map(|u| format!("Files/Data/{u}/content.rtf"))
            .into_iter()
            .chain(id.map(|i| format!("Files/Docs/{i}.rtf")))
            .collect();
        let Some(path) = paths.into_iter().find(|p| self.files.has(p)) else {
            if required && self.has_file_extension(item) {
                self.report.missing_files += 1;
            }
            return None;
        };
        let bytes = self.files.get(&path).unwrap_or_default();
        Some(sanitize_html(&rtf_to_html(bytes)))
    }

    fn has_content(html: Option<&String>) -> bool {
        html.is_some_and(|h| {
            !html_to_plain_text(h, &PlainTextOptions::default())
                .trim_matches(is_js_whitespace)
                .is_empty()
        })
    }

    fn count_notes(&mut self, item: usize) {
        let (uuid, id) = self.ids(item);
        if uuid.is_some_and(|u| self.files.has(&format!("Files/Data/{u}/notes.rtf")))
            || id.is_some_and(|i| self.files.has(&format!("Files/Docs/{i}_notes.rtf")))
        {
            self.report.items_with_notes += 1;
        }
    }

    fn synopsis(&self, item: usize) -> String {
        let (uuid, id) = self.ids(item);
        let paths = uuid
            .map(|u| format!("Files/Data/{u}/synopsis.txt"))
            .into_iter()
            .chain(id.map(|i| format!("Files/Docs/{i}_synopsis.txt")));
        for path in paths {
            if let Some(bytes) = self.files.get(&path) {
                return truncate(&collapse(&decode_utf8(bytes)), MAX_SUMMARY_LENGTH);
            }
        }
        String::new()
    }

    fn new_chapter(&mut self, drafts: &mut Vec<ChapterDraft>, title: &str, summary: &str) -> usize {
        let mut c = JsObject::new();
        c.insert("id", JsValue::str(&(self.create_id)()));
        let title = collapse(title);
        c.insert(
            "title",
            JsValue::str(if title.is_empty() {
                "Untitled chapter"
            } else {
                &title
            }),
        );
        c.insert("expanded", JsValue::Bool(true));
        c.insert(
            "summary",
            JsValue::str(&truncate(&collapse(summary), MAX_SUMMARY_LENGTH)),
        );
        drafts.push(ChapterDraft {
            chapter: c,
            scenes: Vec::new(),
        });
        drafts.len() - 1
    }

    fn add_scene(
        &mut self,
        drafts: &mut [ChapterDraft],
        chapter: usize,
        title: &str,
        content: &str,
    ) {
        let chapter_id = drafts[chapter]
            .chapter
            .get("id")
            .cloned()
            .unwrap_or(JsValue::Null);
        let mut d = JsObject::new();
        d.insert("id", JsValue::str(&(self.create_id)()));
        let title = collapse(title);
        d.insert(
            "title",
            JsValue::str(if title.is_empty() {
                "Untitled scene"
            } else {
                &title
            }),
        );
        d.insert("content", JsValue::str(content));
        d.insert("chapterId", chapter_id);
        drafts[chapter].scenes.push(JsValue::Object(d));
    }

    fn walk_draft(
        &mut self,
        drafts: &mut Vec<ChapterDraft>,
        items: &[usize],
        owner: usize,
        direct: bool,
    ) {
        for item in items.iter().copied() {
            let kind = self.kind(item);
            if kind == "TrashFolder" {
                continue;
            }
            self.count_notes(item);
            match kind.as_str() {
                "Folder" => {
                    if !direct {
                        self.report.flattened_folders += 1;
                    }
                    let title = self.title(item, "Untitled chapter");
                    let summary = self.synopsis(item);
                    let chapter = self.new_chapter(drafts, &title, &summary);
                    let own = self.read_content(item, false);
                    if Self::has_content(own.as_ref()) {
                        self.add_scene(drafts, chapter, &title, own.as_deref().unwrap_or_default());
                    }
                    let children = self.child_items(item);
                    self.walk_draft(drafts, &children, chapter, false);
                }
                "Text" => {
                    let title = self.title(item, "Untitled scene");
                    let content = self
                        .read_content(item, true)
                        .unwrap_or_else(|| EMPTY_CONTENT.to_owned());
                    self.add_scene(drafts, owner, &title, &content);
                    let children = self.child_items(item);
                    self.walk_draft(drafts, &children, owner, false);
                }
                _ => self.report.skipped_files += 1,
            }
        }
    }

    fn walk_wiki(
        &mut self,
        folders: &mut Vec<JsValue>,
        pages: &mut Vec<JsValue>,
        items: &[usize],
        parent: Option<&str>,
    ) {
        for item in items.iter().copied() {
            let kind = self.kind(item);
            if kind == "TrashFolder" {
                continue;
            }
            self.count_notes(item);
            match kind.as_str() {
                "Folder" | "ResearchFolder" => {
                    let id = (self.create_id)();
                    let mut f = JsObject::new();
                    f.insert("id", JsValue::str(&id));
                    f.insert(
                        "title",
                        JsValue::str(&truncate(
                            &self.title(item, "Untitled folder"),
                            MAX_FOLDER_TITLE_LENGTH,
                        )),
                    );
                    f.insert("parentId", parent.map_or(JsValue::Null, JsValue::str));
                    folders.push(JsValue::Object(f));
                    let children = self.child_items(item);
                    self.walk_wiki(folders, pages, &children, Some(&id));
                }
                "Text" => {
                    let mut page = JsObject::new();
                    page.insert("id", JsValue::str(&(self.create_id)()));
                    page.insert("title", JsValue::str(&self.title(item, "Untitled page")));
                    let content = self
                        .read_content(item, true)
                        .unwrap_or_else(|| EMPTY_CONTENT.to_owned());
                    page.insert("content", JsValue::str(&content));
                    page.insert("space", JsValue::str("Wiki"));
                    if let Some(parent) = parent {
                        page.insert("folderId", JsValue::str(parent));
                    }
                    pages.push(JsValue::Object(page));
                    let children = self.child_items(item);
                    self.walk_wiki(folders, pages, &children, parent);
                }
                _ => self.report.skipped_files += 1,
            }
        }
    }
}

/// `buildScrivenerProject`: the project and what was found. `create_id` makes each id, in
/// ProseCraft's order; `project_name` (the `.scriv` folder's name) names it.
pub fn build_scrivener_project(
    input: &ScrivenerFiles,
    create_id: &mut dyn FnMut() -> String,
    project_name: Option<&str>,
) -> Result<(JsValue, ScrivenerReport), ScrivenerError> {
    let (files, root_name) = assemble(input);
    let Some(scrivx) = files.get("project.scrivx") else {
        return Err(error("project.scrivx is missing."));
    };
    let dom = Dom::from_xml(decode_utf8(scrivx).as_bytes())
        .ok_or_else(|| error("project.scrivx is not valid XML."))?;
    let root = dom
        .document_element()
        .ok_or_else(|| error("project.scrivx is not valid XML."))?;
    let layout = if files.0.iter().any(|(p, _)| p.starts_with("Files/Data/")) {
        "data"
    } else {
        "docs"
    };
    let mut b = Builder {
        dom,
        files,
        report: ScrivenerReport {
            layout,
            ..ScrivenerReport::default()
        },
        create_id,
    };
    let binder = b
        .first(root, "Binder")
        .ok_or_else(|| error("project.scrivx has no Binder."))?;
    let top = b.children(binder, "BinderItem");
    let draft = b.find_first(&top, "DraftFolder");
    let research = b.find_first(&top, "ResearchFolder");

    let mut drafts: Vec<ChapterDraft> = Vec::new();
    let draft_title = draft.map_or("Draft".to_owned(), |d| b.title(d, "Draft"));
    let mut default_chapter: Option<usize> = None;
    if let Some(draft) = draft {
        b.count_notes(draft);
        let own = b.read_content(draft, false);
        let synopsis = b.synopsis(draft);
        if Builder::has_content(own.as_ref()) {
            let chapter = *default_chapter
                .get_or_insert_with(|| b.new_chapter(&mut drafts, &draft_title, &synopsis));
            b.add_scene(
                &mut drafts,
                chapter,
                &draft_title,
                own.as_deref().unwrap_or_default(),
            );
        }
        let chapter = *default_chapter
            .get_or_insert_with(|| b.new_chapter(&mut drafts, &draft_title, &synopsis));
        let children = b.child_items(draft);
        b.walk_draft(&mut drafts, &children, chapter, true);
    }
    let chapters: Vec<&ChapterDraft> = drafts.iter().filter(|d| !d.scenes.is_empty()).collect();
    let manuscript: Vec<JsValue> = chapters.iter().flat_map(|d| d.scenes.clone()).collect();

    let mut folders = Vec::new();
    let mut pages = Vec::new();
    // Every top-level item outside the draft and the trash is Wiki material; the first research
    // folder keeps its children at the root.
    for item in top.iter().copied() {
        let kind = b.kind(item);
        if Some(item) == draft || kind == "DraftFolder" || kind == "TrashFolder" {
            continue;
        }
        if Some(item) == research {
            b.count_notes(item);
            let children = b.child_items(item);
            b.walk_wiki(&mut folders, &mut pages, &children, None);
            continue;
        }
        b.walk_wiki(&mut folders, &mut pages, &[item], None);
    }

    let fallback_chapter_id = (b.create_id)();
    let fallback_scene_id = (b.create_id)();
    let final_chapters: Vec<JsValue> = if chapters.is_empty() {
        let mut c = JsObject::new();
        c.insert("id", JsValue::str(&fallback_chapter_id));
        c.insert("title", JsValue::str("Chapter 1"));
        c.insert("expanded", JsValue::Bool(true));
        c.insert("summary", JsValue::str(""));
        vec![JsValue::Object(c)]
    } else {
        chapters
            .iter()
            .map(|d| JsValue::Object(d.chapter.clone()))
            .collect()
    };
    let final_manuscript: Vec<JsValue> = if manuscript.is_empty() {
        let mut d = JsObject::new();
        d.insert("id", JsValue::str(&fallback_scene_id));
        d.insert("title", JsValue::str("Scene 1"));
        d.insert("content", JsValue::str(EMPTY_CONTENT));
        d.insert("chapterId", JsValue::str(&fallback_chapter_id));
        vec![JsValue::Object(d)]
    } else {
        manuscript.clone()
    };
    b.report.chapters = chapters.len();
    b.report.scenes = manuscript.len();
    b.report.wiki_pages = pages.len();
    b.report.wiki_folders = folders.len();

    let active = final_manuscript
        .first()
        .and_then(|d| d.get("id"))
        .cloned()
        .unwrap_or_else(|| JsValue::str(""));
    let mut p = JsObject::new();
    p.insert("id", JsValue::str(&(b.create_id)()));
    let given = collapse(project_name.unwrap_or_default());
    let given = strip_scriv(&given).to_owned();
    let name = if !given.is_empty() {
        given
    } else {
        root_name.unwrap_or_else(|| "Imported Scrivener project".into())
    };
    p.insert("name", JsValue::str(&name));
    p.insert("author", JsValue::str(""));
    p.insert(
        "documents",
        JsValue::Array(final_manuscript.into_iter().chain(pages).collect()),
    );
    p.insert("chapters", JsValue::Array(final_chapters));
    p.insert("activeDocumentId", active);
    p.insert("projectGoal", JsValue::Number(50_000.0));
    p.insert("chapterGoals", JsValue::Object(JsObject::new()));
    p.insert("activity", JsValue::Object(JsObject::new()));
    p.insert("storyboardCards", JsValue::Array(Vec::new()));
    let mut row = JsObject::new();
    row.insert("id", JsValue::str(&(b.create_id)()));
    row.insert("title", JsValue::str("Timeline"));
    p.insert("storyboardRows", JsValue::Array(vec![JsValue::Object(row)]));
    p.insert("timelineNodes", JsValue::Array(Vec::new()));
    p.insert("noteFolders", JsValue::Array(Vec::new()));
    p.insert("researchFolderName", JsValue::str(""));
    p.insert("researchFolders", JsValue::Array(folders));
    p.insert("researchItems", JsValue::Array(Vec::new()));
    p.insert("publishOptions", crate::starters::default_publish_options());
    Ok((JsValue::Object(p), b.report))
}
