//! A folder of Markdown files as a project (ProseCraft `src/lib/import/markdown-folder.ts`,
//! upstream `7dae544`): a notes vault or a manuscript kept as text files. The import dialog
//! offers each folder as manuscript, wiki or skipped; manuscript folders become chapters
//! (or every file one), the rest wiki pages in a folder tree, and `[[wikilinks]]` plain text.
//!
//! Names sort as `localeCompare` with numbers by value does, through ICU collation in the
//! Unicode root order. ProseCraft compares at base strength, so names that differ only in case
//! or accents tie and keep the order the folder was read in; here they still have an order
//! (lower case first), which makes the result the same however the folder was read.

use crate::html::{is_js_whitespace, sanitize_html};
use crate::js::regexp::Re;
use crate::js::{JsObject, JsString, JsValue, string_to_number};
use crate::markdown::markdown_to_html;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::OnceLock;

/// The largest Markdown file read, in bytes.
pub const MAX_MARKDOWN_FILE_BYTES: usize = 2 * 1024 * 1024;
/// The most Markdown files read from one folder.
pub const MAX_MARKDOWN_FILES: usize = 5000;
/// Files directly inside the picked folder are grouped under this name.
pub const ROOT_FOLDER: &str = "(loose files)";
/// The estimated stored size above which ProseCraft's browser storage fallback warns.
pub const STORAGE_WARN_CHARS: u64 = 3_000_000;

/// A Markdown file read from the folder: its path relative to the picked folder, with `/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownFile {
    pub path: String,
    pub text: String,
}

/// What a folder's files become.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FolderRole {
    Manuscript,
    Wiki,
    Skip,
}

impl FolderRole {
    pub fn as_str(self) -> &'static str {
        match self {
            FolderRole::Manuscript => "manuscript",
            FolderRole::Wiki => "wiki",
            FolderRole::Skip => "skip",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "manuscript" => Some(FolderRole::Manuscript),
            "wiki" => Some(FolderRole::Wiki),
            "skip" => Some(FolderRole::Skip),
            _ => None,
        }
    }
}

/// How manuscript files become chapters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChapterMode {
    /// Every folder is a chapter and every file a scene in it.
    Folders,
    /// Every file is a chapter of its own.
    Files,
}

/// The choices made in the import dialog: a role per folder path (`12_MS` or `12_MS/draft`;
/// the most specific folder with one wins, a folder without one is skipped).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownImportPlan {
    pub roles: HashMap<String, FolderRole>,
    pub chapter_mode: ChapterMode,
}

/// A folder offered in the import dialog: a top-level folder, or one of its subfolders when it
/// has several.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportFolder {
    pub path: String,
    pub name: String,
    pub depth: usize,
    pub files: usize,
    pub suggested: FolderRole,
}

/// What an import made.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MarkdownImportReport {
    pub chapters: usize,
    pub scenes: usize,
    pub wiki_pages: usize,
    pub wiki_folders: usize,
    pub links_resolved: usize,
    pub links_unresolved: usize,
}

macro_rules! rule {
    ($name:ident, $source:expr, $flags:expr) => {
        fn $name() -> &'static Re {
            static RULE: OnceLock<Re> = OnceLock::new();
            RULE.get_or_init(|| Re::new($source, $flags))
        }
    };
}

rule!(
    manuscript_name,
    r"(manuscript|manuskript|draft|entwurf|chapters?|kapitel|text|book|buch|roman)",
    "i"
);
rule!(markdown_extension, r"\.(md|markdown)$", "i");
rule!(list_item, r"^\s*-\s+(.*)$", "");
rule!(pair, r"^([A-Za-z0-9_-]+):\s*(.*)$", "");
rule!(leading_number, r"^\d+[_\-.\s]+", "");
rule!(underscores, r"[_]+", "");
rule!(first_heading_rule, r"^\s*#\s+(.+?)\s*#*\s*(?:\n|$)", "");
rule!(wikilink, r"(!?)\[\[([^\]\n]+)\]\]", "");

fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

/// A string cut to `units` UTF-16 units, as `slice(0, units)` cuts it.
fn js_prefix(s: &str, units: usize) -> JsValue {
    JsValue::String(JsString::from_units(s.encode_utf16().take(units).collect()))
}

/// `isImportableMarkdownPath`: a `.md` or `.markdown` file outside hidden and archive folders
/// (`.obsidian`, `_archiv`).
pub fn is_importable_markdown_path(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    let name = parts.last().copied().unwrap_or_default();
    markdown_extension().test(name) && !parts.iter().any(|p| p.starts_with(['.', '_']))
}

fn top_folder_of(path: &str) -> &str {
    match path.split_once('/') {
        Some((top, _)) => top,
        None => ROOT_FOLDER,
    }
}

/// `suggestRole`: a folder named like a manuscript (in English or German) is one; any other
/// is wiki.
pub fn suggest_role(folder_name: &str) -> FolderRole {
    if manuscript_name().test(folder_name) {
        FolderRole::Manuscript
    } else {
        FolderRole::Wiki
    }
}

/// `roleOf`: the role of a file's most specific folder that has one. Loose files use
/// [`ROOT_FOLDER`].
pub fn role_of(path: &str, roles: &HashMap<String, FolderRole>) -> FolderRole {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() == 1 {
        return roles.get(ROOT_FOLDER).copied().unwrap_or(FolderRole::Skip);
    }
    (1..parts.len())
        .rev()
        .find_map(|length| roles.get(&parts[..length].join("/")).copied())
        .unwrap_or(FolderRole::Skip)
}

/// `naturalCompare`: `localeCompare` with numbers compared by value (see the module notes).
pub fn natural_compare(a: &str, b: &str) -> Ordering {
    static COLLATOR: OnceLock<craft_i18n::Collator> = OnceLock::new();
    COLLATOR
        .get_or_init(|| craft_i18n::Collator::new("und"))
        .compare(a, b)
}

/// `listImportFolders`: every top-level folder, and its subfolders when it has several (drafts
/// of a manuscript, say). Of several drafts only the last is suggested as the manuscript, and
/// only the first manuscript-like folder is suggested as one.
pub fn list_import_folders(files: &[MarkdownFile]) -> Vec<ImportFolder> {
    let mut top: Vec<(String, Vec<(String, usize)>)> = Vec::new();
    for file in files
        .iter()
        .filter(|f| is_importable_markdown_path(&f.path))
    {
        let parts: Vec<&str> = file.path.split('/').collect();
        let name = top_folder_of(&file.path).to_owned();
        let sub = if parts.len() > 2 { parts[1] } else { "" }.to_owned();
        let index = match top.iter().position(|(n, _)| *n == name) {
            Some(i) => i,
            None => {
                top.push((name, Vec::new()));
                top.len() - 1
            }
        };
        let subs = &mut top[index].1;
        match subs.iter_mut().find(|(s, _)| *s == sub) {
            Some((_, count)) => *count += 1,
            None => subs.push((sub, 1)),
        }
    }
    top.sort_by(|a, b| natural_compare(&a.0, &b.0));
    let mut folders = Vec::new();
    let mut manuscript_seen = false;
    for (name, subs) in top {
        let total = subs.iter().map(|(_, c)| c).sum();
        let mut suggested = suggest_role(&name);
        if suggested == FolderRole::Manuscript {
            if manuscript_seen {
                suggested = FolderRole::Wiki;
            }
            manuscript_seen = true;
        }
        let mut named: Vec<&(String, usize)> = subs.iter().filter(|(s, _)| !s.is_empty()).collect();
        named.sort_by(|a, b| natural_compare(&a.0, &b.0));
        let has_drafts = name != ROOT_FOLDER && named.len() >= 2;
        folders.push(ImportFolder {
            path: name.clone(),
            name: name.clone(),
            depth: 0,
            files: total,
            // With several drafts, loose files next to them are usually notes, not manuscript.
            suggested: if has_drafts && suggested == FolderRole::Manuscript {
                FolderRole::Wiki
            } else {
                suggested
            },
        });
        if !has_drafts {
            continue;
        }
        let last = named.len() - 1;
        for (index, (sub, count)) in named.into_iter().enumerate() {
            folders.push(ImportFolder {
                path: format!("{name}/{sub}"),
                name: sub.clone(),
                depth: 1,
                files: *count,
                suggested: if suggested == FolderRole::Manuscript && index != last {
                    FolderRole::Skip
                } else {
                    suggested
                },
            });
        }
    }
    folders
}

/// A front matter value: a string, or a list of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrontValue {
    Text(String),
    List(Vec<String>),
}

/// A file's front matter and the text after it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Frontmatter {
    pub data: Vec<(String, FrontValue)>,
    pub body: String,
}

impl Frontmatter {
    pub fn get(&self, key: &str) -> Option<&FrontValue> {
        self.data.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    fn set(&mut self, key: &str, value: FrontValue) {
        match self.data.iter_mut().find(|(k, _)| k == key) {
            Some((_, existing)) => *existing = value,
            None => self.data.push((key.to_owned(), value)),
        }
    }
}

fn unquote(value: &str) -> String {
    let trimmed = js_trim(value);
    let quoted = |q: char| trimmed.starts_with(q) && trimmed.ends_with(q);
    if quoted('"') || quoted('\'') {
        let mut chars = trimmed.chars();
        chars.next();
        chars.next_back();
        return chars.as_str().to_owned();
    }
    trimmed.to_owned()
}

/// `parseFrontmatter`: the YAML header of a file, as far as notes apps write it (`key: value`,
/// block lists of `- item` and inline lists of `[a, b]`), and the text after it.
pub fn parse_frontmatter(text: &str) -> Frontmatter {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let unparsed = || Frontmatter {
        data: Vec::new(),
        body: normalized.clone(),
    };
    if !normalized.starts_with("---\n") {
        return unparsed();
    }
    let Some(end) = normalized[4..].find("\n---").map(|i| i + 4) else {
        return unparsed();
    };
    let header = &normalized[4..end];
    let rest = &normalized[end + 4..];
    let mut out = Frontmatter {
        data: Vec::new(),
        body: rest.strip_prefix('\n').unwrap_or(rest).to_owned(),
    };
    let mut list_key: Option<String> = None;
    for line in header.split('\n') {
        if let Some(key) = &list_key
            && let Some(item) = list_item().exec(line)
        {
            let mut list = match out.get(key) {
                Some(FrontValue::List(items)) => items.clone(),
                _ => Vec::new(),
            };
            list.push(unquote(item.at(1)));
            let key = key.clone();
            out.set(&key, FrontValue::List(list));
            continue;
        }
        let Some(found) = pair().exec(line) else {
            continue;
        };
        let (key, value) = (found.at(1), found.at(2));
        if value.is_empty() {
            list_key = Some(key.to_owned());
            out.set(key, FrontValue::List(Vec::new()));
        } else if value.starts_with('[') && value.ends_with(']') && value.len() >= 2 {
            list_key = None;
            let items = value[1..value.len() - 1]
                .split(',')
                .map(unquote)
                .filter(|s| !s.is_empty())
                .collect();
            out.set(key, FrontValue::List(items));
        } else {
            list_key = None;
            out.set(key, FrontValue::Text(unquote(value)));
        }
    }
    out
}

/// `humanizeName`: `03_CHARACTERS` → `Characters`, `entwurf_v4_master` → `Entwurf v4 master`.
pub fn humanize_name(name: &str) -> String {
    let cleaned = markdown_extension().replace(name, false, "");
    let cleaned = leading_number().replace(&cleaned, false, "");
    let cleaned = underscores().replace(&cleaned, true, " ");
    let cleaned = js_trim(&cleaned);
    if cleaned.is_empty() {
        return name.to_owned();
    }
    let mut chars = cleaned.chars();
    let Some(first) = chars.next() else {
        return name.to_owned();
    };
    let rest = chars.as_str();
    // `charAt(0)` is one UTF-16 unit: an astral first character keeps its case.
    let astral = first.len_utf16() == 2;
    if cleaned == cleaned.to_uppercase() {
        format!("{first}{}", rest.to_lowercase())
    } else if astral {
        cleaned.to_owned()
    } else {
        format!("{}{rest}", first.to_uppercase())
    }
}

fn strip_extension(name: &str) -> String {
    markdown_extension().replace(name, false, "")
}

struct Parsed<'f> {
    file: &'f MarkdownFile,
    title: String,
    body: String,
    position: Option<f64>,
    stem: String,
}

/// `firstHeading`: a level-one heading at the start, and the text after it.
fn first_heading(body: &str) -> Option<(String, &str)> {
    let found = first_heading_rule().exec(body)?;
    Some((js_trim(found.at(1)).to_owned(), &body[found.end()..]))
}

fn parse_file(file: &MarkdownFile) -> Parsed<'_> {
    let front = parse_frontmatter(&file.text);
    let file_name = file.path.rsplit('/').next().unwrap_or(&file.path);
    let heading = first_heading(&front.body);
    let front_title = match front.get("title") {
        Some(FrontValue::Text(t)) => js_trim(t).to_owned(),
        _ => String::new(),
    };
    let title = if !front_title.is_empty() {
        front_title
    } else {
        match &heading {
            Some((t, _)) if !t.is_empty() => t.clone(),
            _ => humanize_name(file_name),
        }
    };
    // A leading heading that repeats the title would show it twice: scenes have a title field.
    let body = match &heading {
        Some((t, rest)) if *t == title => (*rest).to_owned(),
        _ => front.body.clone(),
    };
    let position = match front.get("position") {
        Some(FrontValue::Text(p)) if !js_trim(p).is_empty() => {
            Some(string_to_number(p)).filter(|n| n.is_finite())
        }
        _ => None,
    };
    Parsed {
        file,
        title,
        body,
        position,
        stem: strip_extension(file_name),
    }
}

/// `sortParsed`: by `position` when every file has one (ties by path), else by path.
fn sort_parsed<'a, 'f>(items: &[&'a Parsed<'f>]) -> Vec<&'a Parsed<'f>> {
    let mut sorted = items.to_vec();
    let all_positioned = !items.is_empty() && items.iter().all(|i| i.position.is_some());
    sorted.sort_by(|a, b| {
        let by_path = || natural_compare(&a.file.path, &b.file.path);
        if all_positioned {
            let difference = a.position.unwrap_or(0.0) - b.position.unwrap_or(0.0);
            if difference < 0.0 {
                Ordering::Less
            } else if difference > 0.0 {
                Ordering::Greater
            } else {
                by_path()
            }
        } else {
            by_path()
        }
    });
    sorted
}

/// `resolveWikilinks`: `[[stem]]` and `[[stem|text]]` as plain text, the target's title when
/// it was imported, so wiki mentions find it. `![[embeds]]` are kept.
fn resolve_wikilinks(
    markdown: &str,
    titles: &HashMap<String, String>,
    report: &mut MarkdownImportReport,
) -> String {
    wikilink().replace_with(markdown, true, |m| {
        if !m.at(1).is_empty() {
            return m.whole().to_owned();
        }
        let mut parts = m.at(2).split('|');
        let target_part = parts.next().unwrap_or_default();
        let alias = parts.next().map(js_trim).filter(|a| !a.is_empty());
        let target = js_trim(target_part.split('#').next().unwrap_or_default());
        let known = titles.get(&target.to_lowercase());
        if known.is_some() {
            report.links_resolved += 1;
        } else {
            report.links_unresolved += 1;
        }
        alias
            .map(str::to_owned)
            .or_else(|| known.cloned().filter(|k| !k.is_empty()))
            .unwrap_or_else(|| target.to_owned())
    })
}

fn to_html(markdown: &str) -> String {
    let html = sanitize_html(&markdown_to_html(markdown));
    if js_trim(&html).is_empty() {
        "<p><br></p>".into()
    } else {
        html
    }
}

/// `estimateStoredChars`: about how much the import would store (three copies and some
/// overhead), in UTF-16 units.
pub fn estimate_stored_chars(files: &[MarkdownFile], roles: &HashMap<String, FolderRole>) -> u64 {
    let chars: usize = files
        .iter()
        .filter(|f| {
            is_importable_markdown_path(&f.path) && role_of(&f.path, roles) != FolderRole::Skip
        })
        .map(|f| f.text.encode_utf16().count())
        .sum();
    (chars as f64 * 3.0 * 1.2).round() as u64
}

fn document(id: String, title: &str, content: String) -> JsObject {
    let mut doc = JsObject::new();
    doc.insert("id", JsValue::str(&id));
    doc.insert("title", JsValue::str(title));
    doc.insert("content", JsValue::str(&content));
    doc
}

fn chapter(id: &str, title: &str) -> JsValue {
    let mut chapter = JsObject::new();
    chapter.insert("id", JsValue::str(id));
    chapter.insert("title", JsValue::str(title));
    chapter.insert("expanded", JsValue::Bool(true));
    chapter.insert("summary", JsValue::str(""));
    JsValue::Object(chapter)
}

/// The wiki folder for a folder path, made (with its parents) on first use. As in ProseCraft,
/// a folder's id is taken before its parent's.
fn folder_for(
    path: &str,
    ids: &mut HashMap<String, String>,
    folders: &mut Vec<JsValue>,
    create_id: &mut dyn FnMut() -> String,
) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    if let Some(id) = ids.get(path) {
        return Some(id.clone());
    }
    let id = create_id();
    let (parent_path, last) = path.rsplit_once('/').unwrap_or(("", path));
    let parent = folder_for(parent_path, ids, folders, create_id);
    let mut folder = JsObject::new();
    folder.insert("id", JsValue::str(&id));
    folder.insert("title", js_prefix(&humanize_name(last), 60));
    folder.insert(
        "parentId",
        parent.map_or(JsValue::Null, |p| JsValue::str(&p)),
    );
    folders.push(JsValue::Object(folder));
    ids.insert(path.to_owned(), id.clone());
    Some(id)
}

/// `buildMarkdownProject`: the project the import dialog's plan makes of the files.
pub fn build_markdown_project(
    files: &[MarkdownFile],
    plan: &MarkdownImportPlan,
    create_id: &mut dyn FnMut() -> String,
    name: Option<&str>,
) -> (JsValue, MarkdownImportReport) {
    let parsed: Vec<Parsed> = files
        .iter()
        .filter(|f| is_importable_markdown_path(&f.path))
        .filter(|f| role_of(&f.path, &plan.roles) != FolderRole::Skip)
        .map(parse_file)
        .collect();
    let mut titles = HashMap::new();
    for item in &parsed {
        titles.insert(item.stem.to_lowercase(), item.title.clone());
    }
    let mut report = MarkdownImportReport::default();
    let mut chapters: Vec<JsValue> = Vec::new();
    let mut documents: Vec<JsObject> = Vec::new();
    let mut folders: Vec<JsValue> = Vec::new();

    let manuscript: Vec<&Parsed> = parsed
        .iter()
        .filter(|i| role_of(&i.file.path, &plan.roles) == FolderRole::Manuscript)
        .collect();
    let scene = |item: &Parsed,
                 chapter_id: &str,
                 report: &mut MarkdownImportReport,
                 create_id: &mut dyn FnMut() -> String| {
        let id = create_id();
        let mut doc = document(
            id,
            &item.title,
            to_html(&resolve_wikilinks(&item.body, &titles, report)),
        );
        doc.insert("chapterId", JsValue::str(chapter_id));
        doc
    };
    match plan.chapter_mode {
        ChapterMode::Files => {
            for item in sort_parsed(&manuscript) {
                let chapter_id = create_id();
                chapters.push(chapter(&chapter_id, &item.title));
                documents.push(scene(item, &chapter_id, &mut report, create_id));
            }
        }
        ChapterMode::Folders => {
            let mut groups: Vec<(String, Vec<&Parsed>)> = Vec::new();
            for item in &manuscript {
                let parts: Vec<&str> = item.file.path.split('/').collect();
                let key = if parts.len() > 2 {
                    parts[..parts.len() - 1].join("/")
                } else {
                    parts[0].to_owned()
                };
                match groups.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, items)) => items.push(item),
                    None => groups.push((key, vec![item])),
                }
            }
            groups.sort_by(|a, b| natural_compare(&a.0, &b.0));
            for (key, items) in &groups {
                let chapter_id = create_id();
                let last = key.rsplit('/').next().unwrap_or(key);
                chapters.push(chapter(&chapter_id, &humanize_name(last)));
                for item in sort_parsed(items) {
                    documents.push(scene(item, &chapter_id, &mut report, create_id));
                }
            }
        }
    }

    let mut folder_ids = HashMap::new();
    let mut wiki: Vec<&Parsed> = parsed
        .iter()
        .filter(|i| role_of(&i.file.path, &plan.roles) == FolderRole::Wiki)
        .collect();
    wiki.sort_by(|a, b| natural_compare(&a.file.path, &b.file.path));
    for item in wiki {
        let folder_path = item.file.path.rsplit_once('/').map_or("", |(dir, _)| dir);
        let id = create_id();
        let mut doc = document(
            id,
            &item.title,
            to_html(&resolve_wikilinks(&item.body, &titles, &mut report)),
        );
        doc.insert("space", JsValue::str("Wiki"));
        if let Some(folder) = folder_for(folder_path, &mut folder_ids, &mut folders, create_id) {
            doc.insert("folderId", JsValue::str(&folder));
        }
        documents.push(doc);
    }

    if chapters.is_empty() {
        let chapter_id = create_id();
        chapters.push(chapter(&chapter_id, "Chapter 1"));
        let mut doc = document(create_id(), "Scene 1", "<p><br></p>".into());
        doc.insert("chapterId", JsValue::str(&chapter_id));
        documents.insert(0, doc);
    }
    let first_scene = documents
        .iter()
        .find(|d| d.get("chapterId").is_some())
        .or_else(|| documents.first())
        .and_then(|d| d.get("id").cloned())
        .unwrap_or_else(|| JsValue::str(""));

    report.chapters = chapters.len();
    report.scenes = documents
        .iter()
        .filter(|d| d.get("chapterId").is_some())
        .count();
    report.wiki_pages = documents
        .iter()
        .filter(|d| d.get("space").is_some())
        .count();
    report.wiki_folders = folders.len();

    let mut project = JsObject::new();
    project.insert("id", JsValue::str(&create_id()));
    let given = name.map(js_trim).unwrap_or_default();
    project.insert(
        "name",
        if given.is_empty() {
            JsValue::str("Imported Markdown")
        } else {
            js_prefix(given, 100)
        },
    );
    project.insert("author", JsValue::str(""));
    project.insert(
        "documents",
        JsValue::Array(documents.into_iter().map(JsValue::Object).collect()),
    );
    project.insert("chapters", JsValue::Array(chapters));
    project.insert("activeDocumentId", first_scene);
    project.insert("projectGoal", JsValue::Number(50_000.0));
    project.insert("chapterGoals", JsValue::Object(JsObject::new()));
    project.insert("activity", JsValue::Object(JsObject::new()));
    project.insert("storyboardCards", JsValue::Array(Vec::new()));
    let mut row = JsObject::new();
    row.insert("id", JsValue::str(&create_id()));
    row.insert("title", JsValue::str("Timeline"));
    project.insert("storyboardRows", JsValue::Array(vec![JsValue::Object(row)]));
    project.insert("timelineNodes", JsValue::Array(Vec::new()));
    project.insert("noteFolders", JsValue::Array(Vec::new()));
    project.insert("researchFolderName", JsValue::str(""));
    project.insert("researchFolders", JsValue::Array(folders));
    project.insert("researchItems", JsValue::Array(Vec::new()));
    project.insert("publishOptions", crate::starters::default_publish_options());
    (JsValue::Object(project), report)
}
