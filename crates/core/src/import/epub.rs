//! EPUB import (ProseCraft `src/lib/import/epub.ts`, upstream `7dae544`): an EPUB 2 or 3 book
//! read into chapters and scenes, and made a project.
//!
//! As ProseCraft's, it never trusts archive paths, never decompresses more than a fixed budget,
//! and runs every scene through the editor's sanitizer. Content documents are read as the
//! browser's `DOMParser` reads them (XHTML as strict, namespace-aware XML, falling back to HTML),
//! and each scene's markup is written as the browser writes it before it is sanitized, so the
//! scenes come out as ProseCraft's do.

use std::collections::{BTreeMap, HashMap};

use base64::Engine;

use super::dom::{Dom, Kind};
use crate::html::{PlainTextOptions, html_to_plain_text, is_js_whitespace, sanitize_html};
use crate::js::{JsObject, JsString, JsValue};

const MAX_ARCHIVE_BYTES: usize = 100 * 1024 * 1024;
const MAX_EXTRACTED_BYTES: u64 = 200 * 1024 * 1024;
const MAX_DOCUMENT_BYTES: usize = 10 * 1024 * 1024;
const MAX_SPINE_ITEMS: usize = 2000;
const MAX_CHAPTER_TITLE_LENGTH: usize = 200;
/// The sanitizer's limit for an embedded image (`MAX_EMBEDDED_IMAGE_BYTES`).
const MAX_EMBEDDED_IMAGE_BYTES: usize = 1_500_000;

const CONTAINER_FILE: &str = "META-INF/container.xml";
const ENCRYPTION_FILE: &str = "META-INF/encryption.xml";
const RIGHTS_FILE: &str = "META-INF/rights.xml";

/// Guide references that point at page furniture rather than book content.
const SKIPPED_GUIDE_TYPES: [&str; 9] = [
    "cover",
    "title-page",
    "titlepage",
    "copyright-page",
    "copyright",
    "toc",
    "contents",
    "license",
    "colophon",
];
const HTML_MEDIA_TYPES: [&str; 3] = ["application/xhtml+xml", "text/html", "application/html"];
const WRAPPERS: [&str; 4] = ["div", "section", "article", "main"];

/// A scene: its title and sanitized editor HTML.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpubScene {
    pub title: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpubChapter {
    pub title: String,
    pub scenes: Vec<EpubScene>,
}

/// What a book reads into (`EpubManuscript`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpubManuscript {
    pub title: String,
    pub author: String,
    pub chapters: Vec<EpubChapter>,
    /// An embedded data URL for the cover, or empty when none is usable.
    pub cover_image: String,
    /// Images that could not be embedded (unsupported format, too large, missing).
    pub skipped_images: usize,
}

/// Why a book can't be imported, in ProseCraft's words.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct EpubError(pub String);

fn fail<T>(message: impl Into<String>) -> Result<T, EpubError> {
    Err(EpubError(message.into()))
}

/// JavaScript's `\s`.
fn is_space(c: char) -> bool {
    is_js_whitespace(c)
}

/// `collapse`: whitespace runs as one space, trimmed.
fn collapse(value: &str) -> String {
    value
        .split(is_space)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn js_trim(value: &str) -> &str {
    value.trim_matches(is_space)
}

/// `decodeURIComponent`, or the value as it is when it can't be decoded.
fn decode_part(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = |b: Option<&u8>| b.and_then(|c| (*c as char).to_digit(16));
            match (hex(bytes.get(i + 1)), hex(bytes.get(i + 2))) {
                (Some(h), Some(l)) => {
                    out.push((h * 16 + l) as u8);
                    i += 3;
                }
                _ => return value.to_owned(),
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| value.to_owned())
}

/// `normalizeArchivePath`: `None` for a path that escapes the archive.
fn normalize_archive_path(path: &str) -> Option<String> {
    let mut result: Vec<&str> = Vec::new();
    let replaced = path.replace('\\', "/");
    for part in replaced.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                result.pop()?;
            }
            p => result.push(p),
        }
    }
    Some(result.join("/"))
}

fn dir_name(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

fn base_name(path: &str) -> &str {
    path.rfind('/').map_or(path, |i| &path[i + 1..])
}

/// The file name without its extension (`replace(/\.[^.]+$/, "")`).
fn stem(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i + 1 < name.len() => &name[..i],
        _ => name,
    }
}

/// `resolveHref`: an archive path and fragment, or `None` (a URL with a scheme, or a path that
/// escapes the archive).
fn resolve_href(base_dir: &str, href: &str) -> Option<(String, String)> {
    let trimmed = js_trim(href);
    let has_scheme = {
        let mut chars = trimmed.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && {
            let rest: String = chars
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
                .collect();
            trimmed[1 + rest.len()..].starts_with(':')
        }
    };
    if trimmed.is_empty() || has_scheme || trimmed.starts_with("//") {
        return None;
    }
    let (raw_path, fragment) = match trimmed.find('#') {
        Some(i) => (&trimmed[..i], decode_part(&trimmed[i + 1..])),
        None => (trimmed, String::new()),
    };
    let clean = js_trim(raw_path.find('?').map_or(raw_path, |i| &raw_path[..i]));
    if clean.is_empty() {
        return None;
    }
    let decoded = clean
        .split('/')
        .map(decode_part)
        .collect::<Vec<_>>()
        .join("/");
    let joined = if base_dir.is_empty() {
        decoded
    } else {
        format!("{base_dir}/{decoded}")
    };
    Some((normalize_archive_path(&joined)?, fragment))
}

fn is_image_extension(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".webp", ".gif", ".svg", ".avif"]
        .iter()
        .any(|e| lower.ends_with(e))
}

/// The archive's entries by their normalized paths.
struct Archive<'a> {
    zip: craft_zip::Archive<'a>,
    /// Normalized path → the entry's name.
    names: HashMap<String, String>,
    remaining: u64,
}

impl<'a> Archive<'a> {
    fn open(data: &'a [u8]) -> Result<Self, EpubError> {
        let limits = craft_zip::Limits {
            max_entries: 1_000_000,
            max_total_bytes: MAX_EXTRACTED_BYTES + 1,
        };
        let zip = craft_zip::Archive::open(data, limits)
            .map_err(|_| EpubError("That file is not a valid EPUB archive.".into()))?;
        let mut names = HashMap::new();
        for name in zip.names() {
            if name.ends_with('/') {
                continue;
            }
            // Unsafe entries are never needed: everything read is resolved through the package
            // document, and those paths are normalized too.
            if let Some(path) = normalize_archive_path(&name) {
                names.insert(path, name.into_owned());
            }
        }
        Ok(Self {
            zip,
            names,
            remaining: MAX_EXTRACTED_BYTES,
        })
    }

    /// `readArchive`: the entries `wanted` picks, within the extraction budget.
    fn read(
        &mut self,
        wanted: impl Fn(&str) -> bool,
    ) -> Result<HashMap<String, Vec<u8>>, EpubError> {
        let mut picked: Vec<(String, String)> = self
            .names
            .iter()
            .filter(|(p, _)| wanted(p))
            .map(|(p, n)| (p.clone(), n.clone()))
            .collect();
        picked.sort();
        let mut too_large = false;
        let mut out = HashMap::new();
        for (path, name) in picked {
            let size = self.zip.declared_size(&name).unwrap_or(0);
            if size > self.remaining {
                too_large = true;
                continue;
            }
            self.remaining -= size;
            match self.zip.read(&name, MAX_EXTRACTED_BYTES) {
                Ok(Some(bytes)) => {
                    out.insert(path, bytes);
                }
                Ok(None) => {}
                Err(_) => return fail("That file is not a valid EPUB archive."),
            }
        }
        if too_large {
            return fail("This EPUB is too large to import (200 MB extracted limit).");
        }
        Ok(out)
    }
}

/// `strFromU8`: UTF-8, a byte-order mark dropped.
fn text_of(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned()
}

fn parse_xml(bytes: &[u8], what: &str) -> Result<Dom, EpubError> {
    Dom::from_xml(text_of(bytes).as_bytes())
        .ok_or_else(|| EpubError(format!("The EPUB's {what} is not valid XML.")))
}

/// `parseContentDocument`: XHTML as XML, else as HTML.
fn parse_content(bytes: &[u8]) -> Dom {
    let text = text_of(bytes);
    match Dom::from_xml(text.as_bytes()) {
        Some(dom) if dom.body().is_some() => dom,
        _ => Dom::from_html(&text),
    }
}

#[derive(Clone, Debug)]
struct ManifestItem {
    id: String,
    path: String,
    media_type: String,
    properties: String,
}

impl ManifestItem {
    fn has_property(&self, property: &str) -> bool {
        self.properties.split(is_space).any(|p| p == property)
    }

    fn is_image(&self) -> bool {
        self.media_type.starts_with("image/") || is_image_extension(&self.path)
    }
}

struct Package {
    title: String,
    author: String,
    manifest: Vec<ManifestItem>,
    spine: Vec<(String, bool)>,
    nav_path: String,
    ncx_path: String,
    cover_path: String,
    guide_paths: Vec<String>,
}

fn metadata_value(dom: &Dom, name: &str) -> String {
    dom.elements(0, name)
        .into_iter()
        .map(|e| collapse(&dom.text_content(e)))
        .find(|t| !t.is_empty())
        .unwrap_or_default()
}

/// `/(?:^|[-_\s])cover(?:[-_\s]|$)/i` on a file stem.
fn names_cover(stem: &str) -> bool {
    let lower = stem.to_lowercase();
    let sep = |c: char| c == '-' || c == '_' || is_space(c);
    lower.match_indices("cover").any(|(i, _)| {
        let before_ok = lower[..i].chars().next_back().is_none_or(sep);
        let after_ok = lower[i + 5..].chars().next().is_none_or(sep);
        before_ok && after_ok
    })
}

/// `findCoverPath`.
fn find_cover_path(dom: &Dom, manifest: &[ManifestItem]) -> String {
    if let Some(item) = manifest
        .iter()
        .find(|i| i.has_property("cover-image") && i.is_image())
    {
        return item.path.clone();
    }
    let cover_id = dom
        .elements(0, "meta")
        .into_iter()
        .find(|m| {
            dom.attr(*m, "name")
                .is_some_and(|n| n.to_lowercase() == "cover")
        })
        .and_then(|m| dom.attr(m, "content"))
        .unwrap_or_default();
    if !cover_id.is_empty()
        && let Some(item) = manifest.iter().find(|i| i.id == cover_id && i.is_image())
    {
        return item.path.clone();
    }
    manifest
        .iter()
        .find(|i| i.is_image() && names_cover(stem(base_name(&i.path))))
        .map(|i| i.path.clone())
        .unwrap_or_default()
}

/// `readPackage`.
fn read_package(dom: &Dom, opf_path: &str) -> Package {
    let opf_dir = dir_name(opf_path);
    let mut manifest = Vec::new();
    for item in dom.elements(0, "item") {
        let href = dom.attr(item, "href").unwrap_or_default();
        let Some((path, _)) = resolve_href(opf_dir, href) else {
            continue;
        };
        manifest.push(ManifestItem {
            id: dom.attr(item, "id").unwrap_or_default().to_owned(),
            path,
            media_type: dom
                .attr(item, "media-type")
                .unwrap_or_default()
                .to_lowercase(),
            properties: dom
                .attr(item, "properties")
                .unwrap_or_default()
                .to_lowercase(),
        });
    }
    let spine_element = dom.elements(0, "spine").first().copied();
    let spine = spine_element
        .map(|s| {
            dom.child_elements(s)
                .into_iter()
                .filter(|c| dom.local(*c) == "itemref")
                .map(|r| {
                    (
                        dom.attr(r, "idref").unwrap_or_default().to_owned(),
                        dom.attr(r, "linear").unwrap_or("yes").to_lowercase() != "no",
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let nav_path = manifest
        .iter()
        .find(|i| i.has_property("nav"))
        .map(|i| i.path.clone())
        .unwrap_or_default();
    let toc_id = spine_element
        .and_then(|s| dom.attr(s, "toc"))
        .unwrap_or_default();
    let ncx_path = manifest
        .iter()
        .find(|i| i.id == toc_id)
        .or_else(|| {
            manifest
                .iter()
                .find(|i| i.media_type == "application/x-dtbncx+xml")
        })
        .map(|i| i.path.clone())
        .unwrap_or_default();
    let mut guide_paths = Vec::new();
    if let Some(guide) = dom.elements(0, "guide").first().copied() {
        for reference in dom
            .child_elements(guide)
            .into_iter()
            .filter(|c| dom.local(*c) == "reference")
        {
            let kind = dom
                .attr(reference, "type")
                .unwrap_or_default()
                .to_lowercase();
            if !SKIPPED_GUIDE_TYPES.contains(&kind.as_str()) {
                continue;
            }
            if let Some((path, _)) =
                resolve_href(opf_dir, dom.attr(reference, "href").unwrap_or_default())
            {
                guide_paths.push(path);
            }
        }
    }
    let author = dom
        .elements(0, "creator")
        .into_iter()
        .map(|c| collapse(&dom.text_content(c)))
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    Package {
        title: metadata_value(dom, "title"),
        author,
        cover_path: find_cover_path(dom, &manifest),
        manifest,
        spine,
        nav_path,
        ncx_path,
        guide_paths,
    }
}

/// `looksEncrypted`: DRM (a rights file, or content listed as encrypted); font obfuscation is not.
fn looks_encrypted(files: &HashMap<String, Vec<u8>>) -> bool {
    if files.contains_key(RIGHTS_FILE) {
        return true;
    }
    let Some(encryption) = files.get(ENCRYPTION_FILE) else {
        return false;
    };
    let Ok(dom) = parse_xml(encryption, "encryption file") else {
        return false;
    };
    dom.elements(0, "EncryptedData").into_iter().any(|e| {
        let uri = dom
            .elements(e, "CipherReference")
            .first()
            .and_then(|r| dom.attr(*r, "URI"))
            .unwrap_or_default()
            .to_lowercase();
        [".xhtml", ".xht", ".html", ".htm", ".opf", ".ncx"]
            .iter()
            .any(|x| uri.ends_with(x))
    })
}

/// `SKIPPED_FILE_STEMS`.
fn skipped_stem(stem: &str) -> bool {
    let s = stem.to_lowercase();
    let joined =
        |a: &str, b: &str, seps: &[&str]| seps.iter().any(|sep| s == format!("{a}{sep}{b}"));
    let seps = ["", "-", "_", " "];
    matches!(
        s.as_str(),
        "cover" | "copyright" | "toc" | "contents" | "nav" | "colophon" | "license" | "licence"
    ) || joined("title", "page", &seps)
        || joined("half", "title", &seps)
        || joined("pg", "header", &seps)
        || joined("pg", "footer", &seps)
}

/// `SKIPPED_TOC_LABELS`, on a collapsed label.
fn skipped_toc_label(label: &str) -> bool {
    let s = label.to_lowercase();
    let either =
        |a: &str, b: &str, seps: &[&str]| seps.iter().any(|sep| s == format!("{a}{sep}{b}"));
    if matches!(
        s.as_str(),
        "cover"
            | "copyright"
            | "table of contents"
            | "contents"
            | "toc"
            | "nav"
            | "navigation"
            | "colophon"
            | "about this ebook"
            | "gutenberg license"
            | "gutenberg licence"
    ) || either("title", "page", &["", "-", " "])
        || either("half", "title", &["", "-", " "])
        || either("copyright", "page", &["-", " "])
    {
        return true;
    }
    // (?:the )?(?:full )?project gutenberg(?:™|\(tm\))?(?: ebook)? licen[cs]e
    let rest = s.strip_prefix("the ").unwrap_or(&s);
    let rest = rest.strip_prefix("full ").unwrap_or(rest);
    let Some(rest) = rest.strip_prefix("project gutenberg") else {
        return false;
    };
    let rest = rest
        .strip_prefix('™')
        .or_else(|| rest.strip_prefix("(tm)"))
        .unwrap_or(rest);
    let rest = rest.strip_prefix(" ebook").unwrap_or(rest);
    rest == " license" || rest == " licence"
}

/// `shouldSkipDocument`.
fn should_skip(item: &ManifestItem, package: &Package) -> bool {
    let lower = item.path.to_ascii_lowercase();
    let html_extension = [".xhtml", ".xht", ".html", ".htm"]
        .iter()
        .any(|x| lower.ends_with(x));
    if !HTML_MEDIA_TYPES.contains(&item.media_type.as_str()) && !html_extension {
        return true;
    }
    if item.has_property("cover-image")
        || item.path == package.nav_path
        || item.path == package.ncx_path
        || package.guide_paths.contains(&item.path)
    {
        return true;
    }
    skipped_stem(stem(base_name(&item.path)))
}

/// `(?:^|\s)WORD[-_ ]?WORD(?:\s|$)` (case-insensitive) somewhere in `class`.
fn class_names(class: &str, first: &[&str], second: &[&str]) -> bool {
    let lower: Vec<char> = class.to_lowercase().chars().collect();
    let starts = |i: usize, word: &str| -> Option<usize> {
        let w: Vec<char> = word.chars().collect();
        (lower.get(i..i + w.len()) == Some(&w[..])).then_some(i + w.len())
    };
    let boundary_before = |i: usize| i == 0 || is_space(lower[i - 1]);
    let boundary_after = |i: usize| i == lower.len() || is_space(lower[i]);
    (0..lower.len()).filter(|i| boundary_before(*i)).any(|i| {
        first.iter().filter_map(|a| starts(i, a)).any(|j| {
            let mut options = vec![j];
            if lower.get(j).is_some_and(|c| matches!(c, '-' | '_' | ' ')) {
                options.push(j + 1);
            }
            if second.is_empty() {
                return boundary_after(j);
            }
            options.into_iter().any(|k| {
                second
                    .iter()
                    .filter_map(|b| starts(k, b))
                    .any(boundary_after)
            })
        })
    })
}

struct Book {
    docs: Vec<ContentDocument>,
}

struct ContentDocument {
    path: String,
    dom: Dom,
    blocks: Vec<usize>,
}

/// One block of the book: its document and its node there.
#[derive(Clone, Copy, Debug)]
struct Block {
    doc: usize,
    node: usize,
}

impl Book {
    fn dom(&self, b: Block) -> &Dom {
        &self.docs[b.doc].dom
    }
}

/// `isSceneBreak`.
fn is_scene_break(dom: &Dom, block: usize) -> bool {
    if dom.local(block) == "hr" {
        return true;
    }
    let class = dom.attr(block, "class").unwrap_or_default();
    if class_names(
        class,
        &["scene", "section", "chapter"],
        &["break", "separator", "divider", "ornament"],
    ) || class_names(
        class,
        &["separator", "divider", "ornament", "asterism"],
        &[],
    ) {
        return true;
    }
    if dom.has_descendant(block, &["img", "svg"]) {
        return false;
    }
    let text = collapse(&dom.text_content(block));
    if text.is_empty() {
        return false;
    }
    let only = |marks: &[char], extra_space: bool| {
        text.chars()
            .all(|c| marks.contains(&c) || (extra_space && is_space(c)))
    };
    let count = |marks: &[char]| text.chars().filter(|c| marks.contains(c)).count();
    (text.starts_with('*') && only(&['*'], true) && count(&['*']) >= 3)
        || (text.starts_with('#') && only(&['#'], true) && count(&['#']) >= 3)
        || {
            let marks = ['•', '·', '◆', '◇', '❖', '⁂', '—', '–'];
            only(&marks, true) && (1..=8).contains(&count(&marks))
        }
        || (text.starts_with(['-', '_']) && only(&['-', '_'], true) && count(&['-', '_']) >= 3)
}

fn is_heading_name(local: &str) -> bool {
    let l = local.to_ascii_lowercase();
    l.len() == 2 && l.starts_with('h') && matches!(l.as_bytes()[1], b'1'..=b'6')
}

/// `isNarrativeBlock`.
fn is_narrative(dom: &Dom, block: usize) -> bool {
    let local = dom.local(block);
    if is_heading_name(local) || local == "hr" {
        return false;
    }
    if local == "img" || local == "svg" || dom.has_descendant(block, &["img", "svg"]) {
        return true;
    }
    !collapse(&dom.text_content(block)).is_empty()
}

fn is_heading(dom: &Dom, block: usize) -> bool {
    is_heading_name(dom.local(block)) && !collapse(&dom.text_content(block)).is_empty()
}

/// `contentContainer`.
fn content_container(dom: &Dom) -> usize {
    let mut container = dom.body().or_else(|| dom.document_element()).unwrap_or(0);
    loop {
        let children = dom.child_elements(container);
        if children.len() != 1 || !WRAPPERS.contains(&dom.local(children[0])) {
            return container;
        }
        container = children[0];
    }
}

/// `collectBlocks`: the document's top-level content blocks, structural wrappers flattened, and
/// loose text made paragraphs.
fn collect_blocks(dom: &mut Dom) -> Vec<usize> {
    fn visit(dom: &mut Dom, parent: usize, blocks: &mut Vec<usize>) {
        for child in dom.nodes[parent].children.clone() {
            match dom.nodes[child].kind.clone() {
                Kind::Text(text) => {
                    if js_trim(&text).is_empty() {
                        continue;
                    }
                    let p = dom.create("p");
                    dom.add(Some(p), Kind::Text(text));
                    blocks.push(p);
                }
                Kind::Element { local, .. } => {
                    if WRAPPERS.contains(&local.as_str()) && !is_scene_break(dom, child) {
                        visit(dom, child, blocks);
                    } else {
                        blocks.push(child);
                    }
                }
                Kind::Document => {}
            }
        }
    }
    let container = content_container(dom);
    let mut blocks = Vec::new();
    visit(dom, container, &mut blocks);
    blocks
}

/// `blockIndexForTarget`.
fn block_index_for_target(dom: &Dom, target: usize, blocks: &[usize]) -> Option<usize> {
    if let Some(i) = blocks.iter().position(|b| dom.contains(*b, target)) {
        return Some(i);
    }
    if let Some(i) = blocks.iter().position(|b| dom.contains(target, *b)) {
        return Some(i);
    }
    let order = dom.order();
    blocks.iter().position(|b| {
        order[*b] != usize::MAX && order[target] != usize::MAX && order[*b] > order[target]
    })
}

struct TocEntry {
    label: String,
    path: String,
    fragment: String,
}

/// `readNavEntries`.
fn read_nav(dom: &Dom, nav_path: &str) -> Vec<TocEntry> {
    let navs = dom.elements(0, "nav");
    let is_toc = |n: usize| {
        dom.attr(n, "epub:type")
            .or_else(|| dom.attr(n, "type"))
            .unwrap_or_default()
            .split(is_space)
            .any(|t| t == "toc")
    };
    let nav = navs
        .iter()
        .copied()
        .find(|n| is_toc(*n))
        .or_else(|| navs.first().copied())
        .or_else(|| dom.body());
    let Some(nav) = nav else { return Vec::new() };
    dom.elements(nav, "a")
        .into_iter()
        .filter(|a| dom.attr(*a, "href").is_some())
        .filter_map(|a| {
            let (path, fragment) =
                resolve_href(dir_name(nav_path), dom.attr(a, "href").unwrap_or_default())?;
            Some(TocEntry {
                label: collapse(&dom.text_content(a)),
                path,
                fragment,
            })
        })
        .collect()
}

/// `readNcxEntries`.
fn read_ncx(bytes: &[u8], ncx_path: &str) -> Vec<TocEntry> {
    let Ok(dom) = parse_xml(bytes, "table of contents") else {
        return Vec::new();
    };
    dom.elements(0, "navPoint")
        .into_iter()
        .filter_map(|point| {
            let label = dom
                .elements(point, "text")
                .first()
                .map(|t| collapse(&dom.text_content(*t)))
                .unwrap_or_default();
            let content = dom
                .child_elements(point)
                .into_iter()
                .find(|c| dom.local(*c) == "content");
            let (path, fragment) = resolve_href(
                dir_name(ncx_path),
                content.and_then(|c| dom.attr(c, "src")).unwrap_or_default(),
            )?;
            Some(TocEntry {
                label,
                path,
                fragment,
            })
        })
        .collect()
}

/// `embeddedImageDataUrl`: PNG, JPEG or WebP within the size limit, else empty.
fn embedded_image_data_url(bytes: Option<&Vec<u8>>, path: &str) -> String {
    let extension = path
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default();
    let media = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => return String::new(),
    };
    match bytes {
        Some(b) if !b.is_empty() && b.len() <= MAX_EMBEDDED_IMAGE_BYTES => format!(
            "data:{media};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(b)
        ),
        _ => String::new(),
    }
}

fn image_data_url(
    src: &str,
    document_path: &str,
    archive: &HashMap<String, Vec<u8>>,
) -> Option<String> {
    if src.is_empty() || src.starts_with("data:") {
        return None;
    }
    let (path, _) = resolve_href(dir_name(document_path), src)?;
    Some(embedded_image_data_url(archive.get(&path), &path)).filter(|u| !u.is_empty())
}

/// `rewriteImages`: images inlined as data URLs; what can't be is removed and counted.
fn rewrite_images(
    dom: &mut Dom,
    block: usize,
    document_path: &str,
    archive: &HashMap<String, Vec<u8>>,
    skipped: &mut usize,
) {
    let svgs = if dom.local(block) == "svg" {
        vec![block]
    } else {
        dom.elements(block, "svg")
    };
    for svg in svgs {
        let image = dom.elements(svg, "image").first().copied();
        let href = image
            .and_then(|i| dom.attr(i, "href").or_else(|| dom.attr(i, "xlink:href")))
            .unwrap_or_default()
            .to_owned();
        let data_url = if href.is_empty() {
            None
        } else {
            image_data_url(&href, document_path, archive)
        };
        match data_url {
            Some(url) => {
                let alt = image
                    .and_then(|i| dom.attr(i, "alt"))
                    .unwrap_or_default()
                    .to_owned();
                let img = dom.create("img");
                dom.set_attr(img, "src", &url);
                dom.set_attr(img, "alt", &alt);
                dom.replace(svg, img);
            }
            None => {
                dom.detach(svg);
                if !href.is_empty() {
                    *skipped += 1;
                }
            }
        }
    }
    let imgs = if dom.local(block) == "img" {
        vec![block]
    } else {
        dom.elements(block, "img")
    };
    for img in imgs {
        let src = dom.attr(img, "src").unwrap_or_default().to_owned();
        if src.starts_with("data:") {
            // The sanitizer drops anything that is not a supported inline image.
            let lower = src.to_lowercase();
            if ![
                "data:image/png;base64,",
                "data:image/jpeg;base64,",
                "data:image/webp;base64,",
            ]
            .iter()
            .any(|p| lower.starts_with(p))
            {
                dom.detach(img);
                *skipped += 1;
            }
            continue;
        }
        match image_data_url(&src, document_path, archive) {
            Some(url) => dom.set_attr(img, "src", &url),
            None => {
                dom.detach(img);
                *skipped += 1;
            }
        }
    }
}

fn clean_title(value: &str) -> String {
    let units: Vec<u16> = collapse(value)
        .encode_utf16()
        .take(MAX_CHAPTER_TITLE_LENGTH)
        .collect();
    String::from_utf16_lossy(&units)
}

/// `labelFromPath`.
fn label_from_path(path: &str) -> String {
    let stem = stem(base_name(path));
    let lower = stem.to_lowercase();
    for prefix in ["chapter", "section", "scene", "part", "sec", "ch"] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            let rest = rest.strip_prefix(['-', '_', ' ']).unwrap_or(rest);
            if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) {
                return String::new();
            }
        }
    }
    let cleaned = collapse(&stem.replace(['_', '-'], " "));
    let mut chars = cleaned.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn normalize_heading_text(value: &str) -> String {
    collapse(value)
        .to_lowercase()
        .chars()
        .filter(|c| !is_space(*c))
        .collect()
}

/// `dropChapterHeading`.
fn drop_chapter_heading(book: &Book, blocks: &[Block], label: &str) -> Vec<Block> {
    let Some(first) = blocks.first() else {
        return Vec::new();
    };
    let dom = book.dom(*first);
    if !is_heading_name(dom.local(first.node)) {
        return blocks.to_vec();
    }
    let text = collapse(&dom.text_content(first.node));
    if text.is_empty() || normalize_heading_text(&text) == normalize_heading_text(label) {
        blocks[1..].to_vec()
    } else {
        blocks.to_vec()
    }
}

/// `isMeaningful`.
fn is_meaningful(html: &str) -> bool {
    if !js_trim(&html_to_plain_text(html, &PlainTextOptions::default())).is_empty() {
        return true;
    }
    let lower = html.to_lowercase();
    lower.match_indices("<img").any(|(i, _)| {
        let tag_end = lower[i..].find('>').map_or(lower.len(), |e| i + e);
        let tag = &lower[i..tag_end];
        tag[4..].starts_with(|c: char| !c.is_alphanumeric())
            && (tag.contains("src=\"data:image/") || tag.contains("src='data:image/"))
    })
}

/// `buildScenes`.
fn build_scenes(
    book: &mut Book,
    blocks: &[Block],
    label: &str,
    archive: &HashMap<String, Vec<u8>>,
    skipped: &mut usize,
) -> Vec<EpubScene> {
    let mut groups: Vec<Vec<Block>> = Vec::new();
    let mut current = Vec::new();
    for entry in blocks {
        if is_scene_break(book.dom(*entry), entry.node) {
            if !current.is_empty() {
                groups.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(*entry);
    }
    if !current.is_empty() {
        groups.push(current);
    }
    let mut scenes = Vec::new();
    for group in groups {
        // A title page often appears as a heading plus a byline and no prose.
        if !group.iter().any(|e| is_narrative(book.dom(*e), e.node)) {
            continue;
        }
        for entry in &group {
            let doc = &mut book.docs[entry.doc];
            rewrite_images(&mut doc.dom, entry.node, &doc.path, archive, skipped);
        }
        let joined = group
            .iter()
            .map(|e| book.dom(*e).outer_html(e.node))
            .collect::<Vec<_>>()
            .join("\n");
        let html = js_trim(&sanitize_html(&joined)).to_owned();
        if !is_meaningful(&html) {
            continue;
        }
        let title = if scenes.is_empty() {
            if label.is_empty() {
                "Scene 1".to_owned()
            } else {
                label.to_owned()
            }
        } else {
            format!("Scene {}", scenes.len() + 1)
        };
        scenes.push(EpubScene {
            title,
            content: html,
        });
    }
    scenes
}

struct Range {
    label: String,
    blocks: Vec<Block>,
}

/// `chaptersFromToc`.
fn chapters_from_toc(
    entries: &[TocEntry],
    book: &Book,
    all: &[Block],
    starts_at: &[usize],
) -> Vec<Range> {
    // A nested TOC often repeats the same anchor for a part and its first chapter; the later
    // (child) entry carries the more useful label, so it overwrites.
    let mut starts: BTreeMap<usize, String> = BTreeMap::new();
    for entry in entries {
        let Some(index) = book.docs.iter().position(|d| d.path == entry.path) else {
            continue;
        };
        let doc = &book.docs[index];
        if doc.blocks.is_empty() {
            continue;
        }
        let mut block_index = 0;
        if !entry.fragment.is_empty() {
            let Some(target) = doc.dom.element_by_id(&entry.fragment) else {
                continue;
            };
            let Some(found) = block_index_for_target(&doc.dom, target, &doc.blocks) else {
                continue;
            };
            block_index = found;
        }
        starts.insert(starts_at[index] + block_index, entry.label.clone());
    }
    let ordered: Vec<(usize, String)> = starts.into_iter().collect();
    let mut chapters = Vec::new();
    for (i, (from, label)) in ordered.iter().enumerate() {
        let to = ordered.get(i + 1).map_or(all.len(), |(n, _)| *n);
        if to <= *from {
            continue;
        }
        // Page furniture gets a TOC entry of its own in many books; its whole range is dropped.
        if skipped_toc_label(&collapse(label)) {
            continue;
        }
        chapters.push(Range {
            label: label.clone(),
            blocks: all[*from..to].to_vec(),
        });
    }
    chapters
}

/// `chaptersFromSpine`: one chapter per document, titled by an early heading or the file name.
fn chapters_from_spine(book: &Book, all: &[Block], starts_at: &[usize]) -> Vec<Range> {
    let mut chapters = Vec::new();
    for (index, doc) in book.docs.iter().enumerate() {
        if doc.blocks.is_empty() {
            continue;
        }
        let start = starts_at[index];
        let mut blocks = all[start..start + doc.blocks.len()].to_vec();
        let heading = blocks
            .iter()
            .take(3)
            .position(|b| is_heading(&doc.dom, b.node));
        let mut label = String::new();
        if let Some(h) = heading {
            label = collapse(&doc.dom.text_content(blocks[h].node));
            blocks.remove(h);
        }
        if label.is_empty() {
            label = label_from_path(&doc.path);
        }
        if blocks.is_empty() {
            continue;
        }
        chapters.push(Range { label, blocks });
    }
    chapters
}

/// `parseEpub`: the book's chapters and scenes, or why it can't be imported.
/// `fallback_title` (typically the file name) names a book whose package has no title.
pub fn parse_epub(data: &[u8], fallback_title: Option<&str>) -> Result<EpubManuscript, EpubError> {
    if data.is_empty() {
        return fail("That file is empty.");
    }
    if data.len() > MAX_ARCHIVE_BYTES {
        return fail("That EPUB is too large to import (100 MB limit).");
    }
    let mut archive = Archive::open(data)?;
    let container_files =
        archive.read(|p| p == CONTAINER_FILE || p == ENCRYPTION_FILE || p == RIGHTS_FILE)?;
    let Some(container) = container_files.get(CONTAINER_FILE) else {
        return fail("The EPUB is missing its container file.");
    };
    if looks_encrypted(&container_files) {
        return fail("This EPUB is DRM-protected and can't be imported.");
    }
    let container = parse_xml(container, "container file")?;
    let opf_path = container
        .elements(0, "rootfile")
        .into_iter()
        .filter_map(|r| container.attr(r, "full-path"))
        .filter(|p| !p.is_empty())
        .map(|p| normalize_archive_path(p).unwrap_or_default())
        .find(|p| p.to_lowercase().ends_with(".opf"));
    let Some(opf_path) = opf_path else {
        return fail("The EPUB is missing its package document.");
    };
    let package_files = archive.read(|p| p == opf_path)?;
    let Some(opf) = package_files.get(&opf_path) else {
        return fail("The EPUB is missing its package document.");
    };
    let package = read_package(&parse_xml(opf, "package document")?, &opf_path);

    let mut spine_items: Vec<ManifestItem> = Vec::new();
    for (id, linear) in &package.spine {
        if !linear {
            continue;
        }
        let Some(item) = package.manifest.iter().find(|i| i.id == *id) else {
            continue;
        };
        if spine_items.iter().any(|s| s.path == item.path) {
            continue;
        }
        spine_items.push(item.clone());
    }
    if spine_items.is_empty() {
        return fail("This EPUB doesn't contain any readable chapters.");
    }
    if spine_items.len() > MAX_SPINE_ITEMS {
        return fail("This EPUB has too many documents to import.");
    }
    let mut wanted: Vec<String> = spine_items.iter().map(|i| i.path.clone()).collect();
    for extra in [&package.nav_path, &package.ncx_path] {
        if !extra.is_empty() {
            wanted.push(extra.clone());
        }
    }
    let image_paths: Vec<String> = package
        .manifest
        .iter()
        .filter(|i| i.media_type.starts_with("image/"))
        .map(|i| i.path.clone())
        .collect();
    let content = archive.read(|p| {
        wanted.iter().any(|w| w == p) || image_paths.iter().any(|w| w == p) || is_image_extension(p)
    })?;
    let cover_image = if package.cover_path.is_empty() {
        String::new()
    } else {
        embedded_image_data_url(content.get(&package.cover_path), &package.cover_path)
    };

    let mut book = Book { docs: Vec::new() };
    for item in &spine_items {
        if should_skip(item, &package) {
            continue;
        }
        let Some(bytes) = content.get(&item.path) else {
            continue;
        };
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return fail(format!(
                "The chapter file {} is too large to import.",
                base_name(&item.path)
            ));
        }
        let mut dom = parse_content(bytes);
        let blocks = collect_blocks(&mut dom);
        if blocks.is_empty() {
            continue;
        }
        book.docs.push(ContentDocument {
            path: item.path.clone(),
            dom,
            blocks,
        });
    }
    if book.docs.is_empty() {
        return fail("This EPUB doesn't contain any readable chapters.");
    }

    let mut toc = Vec::new();
    if !package.nav_path.is_empty()
        && let Some(nav) = content.get(&package.nav_path)
    {
        toc = read_nav(&parse_content(nav), &package.nav_path);
    }
    if toc.is_empty()
        && !package.ncx_path.is_empty()
        && let Some(ncx) = content.get(&package.ncx_path)
    {
        toc = read_ncx(ncx, &package.ncx_path);
    }

    let mut all = Vec::new();
    let mut starts_at = Vec::new();
    for (index, doc) in book.docs.iter().enumerate() {
        starts_at.push(all.len());
        all.extend(doc.blocks.iter().map(|node| Block {
            doc: index,
            node: *node,
        }));
    }
    let mut ranges = chapters_from_toc(&toc, &book, &all, &starts_at);
    if ranges.is_empty() {
        ranges = chapters_from_spine(&book, &all, &starts_at);
    }

    let mut skipped = 0;
    let mut chapters = Vec::new();
    for range in ranges {
        let label = clean_title(&range.label);
        let blocks = if label.is_empty() {
            range.blocks.clone()
        } else {
            drop_chapter_heading(&book, &range.blocks, &label)
        };
        let scenes = build_scenes(&mut book, &blocks, &label, &content, &mut skipped);
        if scenes.is_empty() {
            continue;
        }
        let title = if label.is_empty() {
            format!("Chapter {}", chapters.len() + 1)
        } else {
            label
        };
        chapters.push(EpubChapter { title, scenes });
    }
    if chapters.is_empty() {
        return fail("This EPUB doesn't contain any readable chapters.");
    }
    let fallback = fallback_title
        .map(|f| {
            js_trim(
                &stem(base_name(f))
                    .split(['_', '-'])
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "),
            )
            .to_owned()
        })
        .unwrap_or_default();
    let title = collapse(&package.title);
    Ok(EpubManuscript {
        title: if !title.is_empty() {
            title
        } else if !fallback.is_empty() {
            fallback
        } else {
            "Imported EPUB".into()
        },
        author: collapse(&package.author),
        chapters,
        cover_image,
        skipped_images: skipped,
    })
}

/// `buildEpubProject`: the book as an ordinary project (`create_id` makes each id, in
/// ProseCraft's order: each chapter then its scenes, the project, the storyboard row).
pub fn build_epub_project(
    manuscript: &EpubManuscript,
    create_id: &mut dyn FnMut() -> String,
) -> JsValue {
    let mut chapters = Vec::new();
    let mut documents = Vec::new();
    for chapter in &manuscript.chapters {
        let chapter_id = create_id();
        let mut c = JsObject::new();
        c.insert("id", JsValue::str(&chapter_id));
        c.insert("title", JsValue::str(&chapter.title));
        c.insert("expanded", JsValue::Bool(true));
        c.insert("summary", JsValue::str(""));
        chapters.push(JsValue::Object(c));
        for scene in &chapter.scenes {
            let mut d = JsObject::new();
            d.insert("id", JsValue::str(&create_id()));
            d.insert("title", JsValue::str(&scene.title));
            d.insert("content", JsValue::str(&scene.content));
            d.insert("chapterId", JsValue::str(&chapter_id));
            documents.push(JsValue::Object(d));
        }
    }
    let mut publish = crate::starters::default_publish_options();
    if !manuscript.cover_image.is_empty()
        && let Some(cover) = publish
            .as_object_mut()
            .and_then(|o| o.get_mut("cover"))
            .and_then(JsValue::as_object_mut)
    {
        cover.insert("mode", JsValue::str("image"));
        cover.insert("image", JsValue::str(&manuscript.cover_image));
    }
    let first = documents
        .first()
        .and_then(|d| d.get("id"))
        .and_then(JsValue::as_js_string)
        .map(JsString::to_string_lossy)
        .unwrap_or_default();
    let mut p = JsObject::new();
    p.insert("id", JsValue::str(&create_id()));
    let name = collapse(&manuscript.title);
    p.insert(
        "name",
        JsValue::str(if name.is_empty() {
            "Imported EPUB"
        } else {
            &name
        }),
    );
    p.insert("author", JsValue::str(&collapse(&manuscript.author)));
    p.insert("documents", JsValue::Array(documents));
    p.insert("chapters", JsValue::Array(chapters));
    p.insert("activeDocumentId", JsValue::str(&first));
    p.insert("projectGoal", JsValue::Number(50_000.0));
    p.insert("chapterGoals", JsValue::Object(JsObject::new()));
    p.insert("activity", JsValue::Object(JsObject::new()));
    p.insert("storyboardCards", JsValue::Array(Vec::new()));
    let mut row = JsObject::new();
    row.insert("id", JsValue::str(&create_id()));
    row.insert("title", JsValue::str("Timeline"));
    p.insert("storyboardRows", JsValue::Array(vec![JsValue::Object(row)]));
    p.insert("timelineNodes", JsValue::Array(Vec::new()));
    p.insert("noteFolders", JsValue::Array(Vec::new()));
    p.insert("researchFolderName", JsValue::str(""));
    p.insert("researchFolders", JsValue::Array(Vec::new()));
    p.insert("researchItems", JsValue::Array(Vec::new()));
    p.insert("publishOptions", publish);
    JsValue::Object(p)
}
