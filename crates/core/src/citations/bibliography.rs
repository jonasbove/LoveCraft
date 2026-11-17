//! Citations in an exported book (ProseCraft `src/lib/citations/bibliography.ts`, upstream
//! `7dae544`): the sources cited, in reading order (scenes, then the footnotes they point to),
//! each citation marker replaced as the style wants, and the bibliography.
//!
//! - **Author-date styles** (APA, Chicago author-date, MLA, Harvard, DIN 1505): each marker
//!   becomes its formatted citation, with year suffixes worked out over the cited sources.
//! - **Numeric styles** (IEEE, Vancouver): sources are numbered by first citation, and each
//!   marker becomes its number.
//! - **Notes** (Chicago notes and bibliography): each marker in a scene becomes a footnote of
//!   its own, numbered in reading order with the scene's own footnotes; a citation inside a
//!   footnote stays in it, formatted.

use super::format::{
    BibliographyEntry, CitationContext, CitationInput, compute_disambiguation, format_bibliography,
    format_citation,
};
use crate::cite::{CITE_REF_CLASS, CiteRefItem, parse_cite_data};
use crate::html::sanitize::FOOTNOTE_REF_CLASS;
use crate::html::{Element, Node, parse_fragment, serialize};
use crate::js::JsValue;
use crate::publish::{CompiledBook, CompiledChapter, CompiledFootnote, PublishOptions};
use std::collections::{HashMap, HashSet};

/// A book's bibliography: its heading, style, language and entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookBibliography {
    pub heading: String,
    pub style: String,
    pub locale: String,
    pub entries: Vec<BibliographyEntry>,
}

/// `bibliographyHeading`.
pub fn bibliography_heading(locale: &str) -> &'static str {
    if locale == "de" {
        "Literaturverzeichnis"
    } else {
        "Bibliography"
    }
}

fn id_of(item: &JsValue) -> String {
    item.get("id")
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default()
}

/// The library by id, then by cite key (`buildLibraryLookup`, `resolveItem`).
pub(super) struct Lookup<'a> {
    by_id: HashMap<String, &'a JsValue>,
    by_key: HashMap<String, &'a JsValue>,
}

impl<'a> Lookup<'a> {
    pub(super) fn new(items: &'a [JsValue]) -> Self {
        let mut by_id = HashMap::new();
        let mut by_key = HashMap::new();
        for item in items {
            by_id.entry(id_of(item)).or_insert(item);
            if let Some(key) = item
                .get("citeKey")
                .and_then(JsValue::as_js_string)
                .map(|s| s.to_string_lossy())
                .filter(|k| !k.is_empty())
            {
                by_key.entry(key).or_insert(item);
            }
        }
        Self { by_id, by_key }
    }

    pub(super) fn resolve(&self, id: &str) -> Option<&'a JsValue> {
        self.by_id.get(id).or_else(|| self.by_key.get(id)).copied()
    }
}

fn scene_has_cite(html: &str, footnotes: &[CompiledFootnote]) -> bool {
    html.contains(CITE_REF_CLASS) || footnotes.iter().any(|f| f.html.contains(CITE_REF_CLASS))
}

fn book_has_cite(chapters: &[CompiledChapter]) -> bool {
    chapters
        .iter()
        .flat_map(|c| &c.scenes)
        .any(|s| scene_has_cite(&s.html, &s.footnotes))
}

/// What a marker or footnote reference in the text is.
enum Mark {
    /// A citation marker with its `data-cites` (`span.cite-ref[data-cites]`).
    Cite(Option<String>),
    /// A footnote reference (`sup.footnote-ref[data-footnote-id]`).
    Footnote(String),
}

fn mark_of(element: &Element, any_cite_span: bool) -> Option<Mark> {
    if element.name == "span" && element.has_class(CITE_REF_CLASS) {
        let cites = element.attr("data-cites").map(str::to_owned);
        if any_cite_span || cites.is_some() {
            return Some(Mark::Cite(cites));
        }
    }
    if element.name == "sup" && element.has_class(FOOTNOTE_REF_CLASS) {
        return element
            .attr("data-footnote-id")
            .map(|id| Mark::Footnote(id.to_owned()));
    }
    None
}

/// The marks in the HTML, in document order.
fn marks(nodes: &[Node], any_cite_span: bool, out: &mut Vec<Mark>) {
    for node in nodes {
        if let Node::Element(element) = node {
            if let Some(mark) = mark_of(element, any_cite_span) {
                out.push(mark);
            }
            marks(&element.children, any_cite_span, out);
        }
    }
}

fn cites_of(raw: Option<&str>) -> Option<Vec<CiteRefItem>> {
    parse_cite_data(raw)
}

/// `collectCitedSources`: each cited source once, in reading order (a scene's citations and
/// the footnotes it points to as it points to them, then its other footnotes).
pub fn collect_cited_sources(chapters: &[CompiledChapter], library: &[JsValue]) -> Vec<JsValue> {
    if library.is_empty() || !book_has_cite(chapters) {
        return Vec::new();
    }
    let lookup = Lookup::new(library);
    let mut seen = HashSet::new();
    let mut collected = Vec::new();
    let record =
        |cites: &[CiteRefItem], seen: &mut HashSet<String>, collected: &mut Vec<JsValue>| {
            for cite in cites {
                if let Some(item) = lookup.resolve(&cite.id)
                    && seen.insert(id_of(item))
                {
                    collected.push(item.clone());
                }
            }
        };
    let from_html = |html: &str, seen: &mut HashSet<String>, collected: &mut Vec<JsValue>| {
        if !html.contains(CITE_REF_CLASS) {
            return;
        }
        let mut found = Vec::new();
        marks(&parse_fragment(html), false, &mut found);
        for mark in found {
            if let Mark::Cite(raw) = mark
                && let Some(cites) = cites_of(raw.as_deref())
            {
                record(&cites, seen, collected);
            }
        }
    };
    for scene in chapters.iter().flat_map(|c| &c.scenes) {
        if !scene_has_cite(&scene.html, &scene.footnotes) {
            continue;
        }
        let by_id: HashMap<&str, &CompiledFootnote> =
            scene.footnotes.iter().map(|f| (f.id.as_str(), f)).collect();
        let mut visited = HashSet::new();
        if scene.html.contains(CITE_REF_CLASS) || scene.html.contains(FOOTNOTE_REF_CLASS) {
            let mut found = Vec::new();
            marks(&parse_fragment(&scene.html), false, &mut found);
            for mark in found {
                match mark {
                    Mark::Cite(raw) => {
                        if let Some(cites) = cites_of(raw.as_deref()) {
                            record(&cites, &mut seen, &mut collected);
                        }
                    }
                    Mark::Footnote(id) => {
                        if let Some(note) = by_id.get(id.as_str())
                            && visited.insert(id.clone())
                        {
                            from_html(&note.html, &mut seen, &mut collected);
                        }
                    }
                }
            }
        }
        for note in &scene.footnotes {
            if !visited.contains(&note.id) {
                from_html(&note.html, &mut seen, &mut collected);
            }
        }
    }
    collected
}

/// `buildBookBibliography`: the cited sources' bibliography, or none when nothing is cited.
pub fn build_book_bibliography(
    chapters: &[CompiledChapter],
    library: &[JsValue],
    style: &str,
    locale: &str,
) -> Option<BookBibliography> {
    let cited = collect_cited_sources(chapters, library);
    if cited.is_empty() {
        return None;
    }
    Some(BookBibliography {
        heading: bibliography_heading(locale).to_owned(),
        style: style.to_owned(),
        locale: locale.to_owned(),
        entries: format_bibliography(style, &cited, locale),
    })
}

/// `hasBookBibliography`: whether the export will end with a bibliography.
pub fn has_book_bibliography(book: &CompiledBook, options: &PublishOptions) -> bool {
    if !options.include_bibliography {
        return false;
    }
    if !book.citations.is_empty() && book_has_cite(&book.chapters) {
        return !collect_cited_sources(&book.chapters, &book.citations).is_empty();
    }
    book.bibliography
        .as_ref()
        .is_some_and(|b| !b.entries.is_empty())
}

fn escape_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// How the book's citations are formatted.
struct Formatter<'a> {
    lookup: Lookup<'a>,
    style: &'a str,
    locale: &'a str,
    numeric: Option<HashMap<String, f64>>,
    year_suffixes: HashMap<String, String>,
}

impl Formatter<'_> {
    /// `formatMarkerCluster`: the citation's HTML, with sources not in the library kept as
    /// `[@key]`.
    fn cluster(&self, cites: &[CiteRefItem]) -> String {
        let mut inputs = Vec::new();
        let mut unresolved = Vec::new();
        for cite in cites {
            let Some(item) = self.lookup.resolve(&cite.id) else {
                unresolved.push(format!("@{}", cite.id));
                continue;
            };
            inputs.push(CitationInput {
                item: item.clone(),
                locator: cite.locator.clone().filter(|l| !l.is_empty()),
                label: cite.label.clone().filter(|l| !l.is_empty()),
                prefix: cite.prefix.clone().filter(|p| !p.is_empty()),
                suffix: cite.suffix.clone().filter(|s| !s.is_empty()),
                suppress_author: cite.suppress_author,
            });
        }
        if inputs.is_empty() {
            let keys: Vec<String> = cites.iter().map(|c| format!("@{}", c.id)).collect();
            return escape_text(&format!("[{}]", keys.join("; ")));
        }
        let formatted = format_citation(
            self.style,
            &inputs,
            &CitationContext {
                indices: self.numeric.clone(),
                locale: self.locale.to_owned(),
                year_suffixes: Some(self.year_suffixes.clone()),
                ..CitationContext::default()
            },
        );
        if unresolved.is_empty() {
            return formatted.html;
        }
        format!(
            "{} {}",
            formatted.html,
            escape_text(&format!("[{}]", unresolved.join("; ")))
        )
    }
}

/// Every `span.cite-ref` replaced by `replace` (none: removed).
fn replace_cites(nodes: &mut Vec<Node>, replace: &mut dyn FnMut(Option<&str>) -> Vec<Node>) {
    let mut i = 0;
    while i < nodes.len() {
        let cite = match &nodes[i] {
            Node::Element(e) if e.name == "span" && e.has_class(CITE_REF_CLASS) => {
                Some(e.attr("data-cites").map(str::to_owned))
            }
            _ => None,
        };
        match cite {
            Some(raw) => {
                let replacement = replace(raw.as_deref());
                let count = replacement.len();
                nodes.splice(i..=i, replacement);
                i += count;
            }
            None => {
                if let Node::Element(e) = &mut nodes[i] {
                    replace_cites(&mut e.children, replace);
                }
                i += 1;
            }
        }
    }
}

/// `resolveInlineCitationsInHtml`.
fn resolve_inline(html: &str, formatter: &Formatter) -> String {
    if !html.contains(CITE_REF_CLASS) {
        return html.to_owned();
    }
    let mut nodes = parse_fragment(html);
    replace_cites(&mut nodes, &mut |raw| match cites_of(raw) {
        Some(cites) => parse_fragment(&formatter.cluster(&cites)),
        None => Vec::new(),
    });
    serialize(&nodes)
}

fn footnote_sup(id: &str) -> Node {
    Node::Element(Element {
        name: "sup".into(),
        attrs: vec![
            ("class".into(), FOOTNOTE_REF_CLASS.into()),
            ("data-footnote-id".into(), id.into()),
            ("contenteditable".into(), "false".into()),
        ],
        children: Vec::new(),
    })
}

/// The scene's marks in reading order once citations have become footnote references.
fn footnote_order(nodes: &[Node], out: &mut Vec<String>) {
    for node in nodes {
        if let Node::Element(e) = node {
            if e.name == "sup"
                && e.has_class(FOOTNOTE_REF_CLASS)
                && let Some(id) = e.attr("data-footnote-id")
            {
                out.push(id.to_owned());
            }
            footnote_order(&e.children, out);
        }
    }
}

/// `resolveBookCitations`: the book with every citation marker resolved for the style, its
/// bibliography (when it has one and the options want it), and the cited sources' ids.
pub fn resolve_book_citations(
    book: &CompiledBook,
    options: &PublishOptions,
) -> (CompiledBook, Option<BookBibliography>, Vec<String>) {
    let style = options.citation_style.as_str();
    let locale = options.citation_locale.as_str();
    if !book_has_cite(&book.chapters) {
        let fallback = book
            .bibliography
            .clone()
            .filter(|b| options.include_bibliography && !b.entries.is_empty());
        let ids = fallback
            .as_ref()
            .map(|b| b.entries.iter().map(|e| e.id.clone()).collect())
            .unwrap_or_default();
        return (book.clone(), fallback, ids);
    }
    let cited = collect_cited_sources(&book.chapters, &book.citations);
    let kind = super::styles::styles()
        .into_iter()
        .find(|s| s.id == style)
        .map(|s| s.kind)
        .unwrap_or_default();
    let formatter = Formatter {
        lookup: Lookup::new(&book.citations),
        style,
        locale,
        numeric: (kind == "numeric").then(|| {
            cited
                .iter()
                .enumerate()
                .map(|(i, item)| (id_of(item), (i + 1) as f64))
                .collect()
        }),
        year_suffixes: compute_disambiguation(style, &cited, locale),
    };
    let mut next_number = 0usize;
    let mut resolved = book.clone();
    for scene in resolved
        .chapters
        .iter_mut()
        .flat_map(|c| c.scenes.iter_mut())
    {
        let footnotes: Vec<CompiledFootnote> = scene
            .footnotes
            .iter()
            .map(|f| CompiledFootnote {
                html: resolve_inline(&f.html, &formatter),
                ..f.clone()
            })
            .collect();
        if !scene.html.contains(CITE_REF_CLASS) {
            next_number += footnotes.len();
            scene.footnotes = footnotes;
            continue;
        }
        if kind != "note" {
            scene.html = resolve_inline(&scene.html, &formatter);
            next_number += footnotes.len();
            scene.footnotes = footnotes;
            continue;
        }
        // Notes: each citation in the scene becomes a footnote of its own.
        let mut used: HashSet<String> = footnotes.iter().map(|f| f.id.clone()).collect();
        let mut counter = 0;
        let mut automatic: Vec<(String, String)> = Vec::new();
        let mut nodes = parse_fragment(&scene.html);
        replace_cites(&mut nodes, &mut |raw| {
            let Some(cites) = cites_of(raw) else {
                return Vec::new();
            };
            let html = formatter.cluster(&cites);
            let id = loop {
                counter += 1;
                let candidate = format!("cite-fn-{counter}");
                if used.insert(candidate.clone()) {
                    break candidate;
                }
            };
            automatic.push((id.clone(), html));
            vec![footnote_sup(&id)]
        });
        let by_id: HashMap<&str, &CompiledFootnote> =
            footnotes.iter().map(|f| (f.id.as_str(), f)).collect();
        let automatic: HashMap<String, String> = automatic.into_iter().collect();
        let mut order = Vec::new();
        footnote_order(&nodes, &mut order);
        let mut ordered = Vec::new();
        let mut seen_manual = HashSet::new();
        for id in order {
            if let Some(html) = automatic.get(&id) {
                next_number += 1;
                ordered.push(CompiledFootnote {
                    id,
                    html: html.clone(),
                    number: next_number,
                });
            } else if let Some(manual) = by_id.get(id.as_str())
                && seen_manual.insert(id.clone())
            {
                next_number += 1;
                ordered.push(CompiledFootnote {
                    number: next_number,
                    ..(*manual).clone()
                });
            }
        }
        for note in &footnotes {
            if !seen_manual.contains(&note.id) {
                next_number += 1;
                ordered.push(CompiledFootnote {
                    number: next_number,
                    ..note.clone()
                });
            }
        }
        scene.html = serialize(&nodes);
        scene.footnotes = ordered;
    }
    let bibliography =
        (options.include_bibliography && !cited.is_empty()).then(|| BookBibliography {
            heading: bibliography_heading(locale).to_owned(),
            style: style.to_owned(),
            locale: locale.to_owned(),
            entries: format_bibliography(style, &cited, locale),
        });
    resolved.bibliography = bibliography.clone();
    (resolved, bibliography, cited.iter().map(id_of).collect())
}
