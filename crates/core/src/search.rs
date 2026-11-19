//! Project-wide find and replace (ProseCraft `src/lib/search.ts` and `replace.ts`, upstream
//! `7dae544`). A literal query, case and diacritics folded unless case matters, optionally
//! whole words only. Offsets count UTF-16 units, as in JavaScript.

use std::collections::HashMap;

use unicode_normalization::UnicodeNormalization;

use crate::html::{
    Node, PlainTextOptions, html_to_plain_text, is_js_whitespace, parse_fragment, serialize,
};
use crate::js::{JsString, JsValue};

/// Documents beyond this many are left out, so huge projects stay scannable.
pub const MAX_SEARCH_HITS: usize = 200;
const SNIPPET_LENGTH: usize = 80;
const SNIPPET_CONTEXT_BEFORE: usize = 40;
const ELLIPSIS: u16 = 0x2026;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub include_trashed: bool,
}

/// A document with matches, and its first match in a snippet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub document_id: String,
    pub title: String,
    pub wiki: bool,
    pub chapter_title: Option<String>,
    /// UTF-16 units.
    pub snippet: Vec<u16>,
    pub match_start: usize,
    pub match_length: usize,
    /// Every occurrence in the title and the text.
    pub matches: usize,
}

/// `fold`: lower case, without the combining diacritics U+0300–U+036F.
fn fold(value: &str) -> String {
    value
        .to_lowercase()
        .nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .collect()
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// What a query is compared against, and the source it maps back to.
pub struct Haystack {
    pub text: Vec<u16>,
    pub source: Vec<u16>,
    /// The source index of each folded unit, when folding changed the length.
    offsets: Option<Vec<usize>>,
}

/// `makeHaystack`.
pub fn make_haystack(value: &str, case_sensitive: bool) -> Haystack {
    let source = units(value);
    if case_sensitive {
        return Haystack {
            text: source.clone(),
            source,
            offsets: None,
        };
    }
    let folded = units(&fold(value));
    if folded.len() == source.len() {
        return Haystack {
            text: folded,
            source,
            offsets: None,
        };
    }
    // `foldWithOffsets`: each character folded on its own, with its source index.
    let mut text = Vec::new();
    let mut offsets = Vec::new();
    let mut index = 0;
    for c in value.chars() {
        let f = units(&fold(&c.to_string()));
        offsets.extend(std::iter::repeat_n(index, f.len()));
        text.extend(f);
        index += c.len_utf16();
    }
    Haystack {
        text,
        source,
        offsets: Some(offsets),
    }
}

impl Haystack {
    /// `sourceIndex`.
    pub fn source_index(&self, index: usize) -> usize {
        match &self.offsets {
            None => index,
            Some(offsets) => offsets.get(index).copied().unwrap_or(self.source.len()),
        }
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphabetic() || c.is_numeric() || c == '_'
}

/// `isWholeWord`: no letter, digit or `_` just before or after (two units of context, so an
/// astral letter counts).
pub fn is_whole_word(text: &[u16], start: usize, length: usize) -> bool {
    let before = &text[start.saturating_sub(2)..start];
    let end = (start + length).min(text.len());
    let after = &text[end..(end + 2).min(text.len())];
    let last = char::decode_utf16(before.iter().copied())
        .last()
        .and_then(Result::ok);
    let first = char::decode_utf16(after.iter().copied())
        .next()
        .and_then(Result::ok);
    !last.is_some_and(is_word_char) && !first.is_some_and(is_word_char)
}

fn index_of(text: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    if needle.is_empty() || needle.len() > text.len() {
        return None;
    }
    (from..=text.len() - needle.len()).find(|&i| text[i..i + needle.len()] == *needle)
}

/// The non-overlapping matches, as `findMatches` and `findRanges` walk them.
pub fn ranges(text: &[u16], needle: &[u16], whole_word: bool) -> Vec<usize> {
    let mut out = Vec::new();
    let mut index = index_of(text, needle, 0);
    while let Some(i) = index {
        if !whole_word || is_whole_word(text, i, needle.len()) {
            out.push(i);
        }
        index = index_of(text, needle, i + needle.len().max(1));
    }
    out
}

/// `collapseWhitespace`: runs of whitespace become one space; the ends are trimmed.
pub fn collapse_whitespace(value: &str) -> String {
    let mut out = String::new();
    let mut in_run = false;
    for c in value.chars() {
        if is_js_whitespace(c) {
            if !in_run {
                out.push(' ');
            }
            in_run = true;
        } else {
            out.push(c);
            in_run = false;
        }
    }
    out.trim_matches(' ').to_owned()
}

/// `buildSnippet`: about 80 units around the match, "…" where it was cut.
pub fn build_snippet(source: &[u16], start: usize, length: usize) -> (Vec<u16>, usize) {
    let mut from = start.saturating_sub(SNIPPET_CONTEXT_BEFORE);
    let mut to = source
        .len()
        .min((start + length).max(from + SNIPPET_LENGTH));
    if to == source.len() {
        from = from.min(source.len().saturating_sub(SNIPPET_LENGTH));
    }
    let space = u16::from(b' ');
    if from > 0 && from < start && source[from] == space {
        from += 1;
    }
    if to < source.len() && to > 0 && source[to - 1] == space {
        to -= 1;
    }
    let mut snippet = Vec::new();
    if from > 0 {
        snippet.push(ELLIPSIS);
    }
    let prefix = snippet.len();
    snippet.extend_from_slice(&source[from..to]);
    if to < source.len() {
        snippet.push(ELLIPSIS);
    }
    (snippet, prefix + (start - from))
}

fn text<'a>(v: &'a JsValue, k: &str) -> Option<&'a JsString> {
    v.get(k).and_then(JsValue::as_js_string)
}

fn string(v: &JsValue, k: &str) -> String {
    text(v, k)
        .map(JsString::to_string_lossy)
        .unwrap_or_default()
}

fn is_true(v: &JsValue, k: &str) -> bool {
    matches!(v.get(k), Some(JsValue::Bool(true)))
}

/// A document's kind: a scene (it has a chapter), a wiki page, or neither.
fn kind(document: &JsValue) -> Option<bool> {
    if text(document, "chapterId").is_some_and(|c| !c.is_empty()) {
        Some(false)
    } else if text(document, "space").is_some_and(|s| s.eq_str("Wiki")) {
        Some(true)
    } else {
        None
    }
}

/// `searchDocuments` over a project's `documents` and `chapters`: title matches first, then
/// scenes in chapter order, then wiki pages.
pub fn search_documents(
    documents: &[JsValue],
    chapters: &[JsValue],
    query: &str,
    options: SearchOptions,
) -> Vec<SearchHit> {
    let needle = query.trim_matches(is_js_whitespace);
    if needle.is_empty() {
        return Vec::new();
    }
    let search_for = if options.case_sensitive {
        units(needle)
    } else {
        units(&fold(needle))
    };
    let mut info: HashMap<String, (usize, String, bool)> = HashMap::new();
    for (index, chapter) in chapters.iter().enumerate() {
        info.entry(string(chapter, "id")).or_insert((
            index,
            string(chapter, "title"),
            is_true(chapter, "trashed"),
        ));
    }
    // (hit, title match, kind rank, chapter index, document index)
    let mut candidates: Vec<(SearchHit, bool, usize, usize, usize)> = Vec::new();
    for (document_index, document) in documents.iter().enumerate() {
        let Some(wiki) = kind(document) else { continue };
        let chapter = text(document, "chapterId").and_then(|c| info.get(&c.to_string_lossy()));
        if !options.include_trashed
            && (is_true(document, "trashed") || chapter.is_some_and(|c| c.2))
        {
            continue;
        }
        let title_text = string(document, "title");
        let title = make_haystack(&collapse_whitespace(&title_text), options.case_sensitive);
        let body = make_haystack(
            &collapse_whitespace(&html_to_plain_text(
                &string(document, "content"),
                &PlainTextOptions::default(),
            )),
            options.case_sensitive,
        );
        let title_matches = ranges(&title.text, &search_for, options.whole_word);
        let body_matches = ranges(&body.text, &search_for, options.whole_word);
        let matches = title_matches.len() + body_matches.len();
        if matches == 0 {
            continue;
        }
        let (container, first) = match (title_matches.first(), body_matches.first()) {
            (Some(&t), _) => (&title, t),
            (None, Some(&b)) => (&body, b),
            (None, None) => continue,
        };
        let start = container.source_index(first);
        let end = container.source_index(first + search_for.len());
        let (snippet, match_start) = build_snippet(&container.source, start, end - start);
        candidates.push((
            SearchHit {
                document_id: string(document, "id"),
                title: title_text,
                wiki,
                chapter_title: if wiki {
                    None
                } else {
                    chapter.map(|c| c.1.clone())
                },
                snippet,
                match_start,
                match_length: end - start,
                matches,
            },
            !title_matches.is_empty(),
            usize::from(wiki),
            chapter.map_or(usize::MAX, |c| c.0),
            document_index,
        ));
    }
    candidates.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(a.2.cmp(&b.2))
            .then(a.3.cmp(&b.3))
            .then(a.4.cmp(&b.4))
    });
    candidates
        .into_iter()
        .take(MAX_SEARCH_HITS)
        .map(|c| c.0)
        .collect()
}

/// `findFirstMatch`: the first match in `text` with the search's rules, as UTF-16 offsets
/// into it.
pub fn find_first_match(
    text_value: &str,
    query: &str,
    options: SearchOptions,
) -> Option<(usize, usize)> {
    let needle = query.trim_matches(is_js_whitespace);
    if needle.is_empty() {
        return None;
    }
    let haystack = make_haystack(text_value, options.case_sensitive);
    let search_for = if options.case_sensitive {
        units(needle)
    } else {
        make_haystack(needle, false).text
    };
    let first = ranges(&haystack.text, &search_for, options.whole_word)
        .into_iter()
        .next()?;
    Some((
        haystack.source_index(first),
        haystack.source_index(first + search_for.len()),
    ))
}

/// `replaceInText`: the text with its matches replaced, and how many.
fn replace_in_text(
    source: &str,
    needle: &[u16],
    replacement: &[u16],
    options: SearchOptions,
) -> (String, usize) {
    let haystack = make_haystack(source, options.case_sensitive);
    let found = ranges(&haystack.text, needle, options.whole_word);
    if found.is_empty() {
        return (source.to_owned(), 0);
    }
    let mut out: Vec<u16> = Vec::new();
    let mut cursor = 0;
    for start in &found {
        let s = haystack.source_index(*start);
        let e = haystack.source_index(start + needle.len());
        out.extend_from_slice(&haystack.source[cursor..s]);
        out.extend_from_slice(replacement);
        cursor = e;
    }
    out.extend_from_slice(&haystack.source[cursor..]);
    if out == haystack.source {
        (source.to_owned(), 0)
    } else {
        (String::from_utf16_lossy(&out), found.len())
    }
}

fn replace_in_nodes(
    nodes: &mut [Node],
    needle: &[u16],
    replacement: &[u16],
    options: SearchOptions,
) -> usize {
    let mut count = 0;
    for node in nodes {
        match node {
            Node::Text(t) => {
                let (next, n) = replace_in_text(t, needle, replacement, options);
                if n > 0 {
                    *t = next;
                    count += n;
                }
            }
            Node::Element(e) => {
                count += replace_in_nodes(&mut e.children, needle, replacement, options)
            }
        }
    }
    count
}

/// `replaceInHtml`: replaces in text nodes only; markup with nothing replaced keeps its
/// original string.
fn replace_in_html(
    html: &str,
    needle: &[u16],
    replacement: &[u16],
    options: SearchOptions,
) -> (String, usize) {
    let mut nodes = parse_fragment(html);
    let count = replace_in_nodes(&mut nodes, needle, replacement, options);
    if count == 0 {
        (html.to_owned(), 0)
    } else {
        (serialize(&nodes), count)
    }
}

/// What a replace changed: per document id, how many.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplaceResult {
    pub changed: Vec<(String, usize)>,
    pub total: usize,
}

/// `replaceInDocuments` on a project's `documents` (changed in place): scenes and wiki pages,
/// their text and (with `include_titles`) their titles. A changed body drops the Markdown
/// mirror. `only` limits it to those ids.
pub fn replace_in_documents(
    documents: &mut [JsValue],
    query: &str,
    replacement: &str,
    options: SearchOptions,
    include_titles: bool,
    only: Option<&[String]>,
) -> ReplaceResult {
    let needle = query.trim_matches(is_js_whitespace);
    let mut result = ReplaceResult::default();
    if needle.is_empty() {
        return result;
    }
    let search_for = make_haystack(needle, options.case_sensitive).text;
    let replacement = units(replacement);
    for document in documents.iter_mut() {
        if kind(document).is_none() || (!options.include_trashed && is_true(document, "trashed")) {
            continue;
        }
        let id = string(document, "id");
        if only.is_some_and(|ids| !ids.contains(&id)) {
            continue;
        }
        let (body, body_count) = replace_in_html(
            &string(document, "content"),
            &search_for,
            &replacement,
            options,
        );
        let (title, title_count) = if include_titles {
            replace_in_text(
                &string(document, "title"),
                &search_for,
                &replacement,
                options,
            )
        } else {
            (String::new(), 0)
        };
        let count = body_count + title_count;
        if count == 0 {
            continue;
        }
        if let Some(o) = document.as_object_mut() {
            if include_titles {
                o.insert("title", JsValue::str(&title));
            }
            o.insert("content", JsValue::str(&body));
            if body_count > 0 {
                o.remove("markdownSource");
            }
        }
        result.changed.push((id, count));
        result.total += count;
    }
    result
}

/// The project-wide replace as ProseCraft's dialog confirms it: a "Before replace" snapshot
/// of each document that changes, then the replacement in `project`'s documents.
pub fn replace_in_project(
    project: &mut JsValue,
    query: &str,
    replacement: &str,
    options: SearchOptions,
    only: &[String],
    now: f64,
    new_id: &mut dyn FnMut() -> String,
) -> Result<ReplaceResult, crate::edit::EditError> {
    let mut documents = project
        .get("documents")
        .and_then(JsValue::as_array)
        .map(<[JsValue]>::to_vec)
        .ok_or(crate::edit::EditError::Shape)?;
    let result = replace_in_documents(
        &mut documents,
        query,
        replacement,
        options,
        false,
        Some(only),
    );
    if result.total == 0 {
        return Ok(result);
    }
    let changed: Vec<String> = result.changed.iter().map(|(id, _)| id.clone()).collect();
    crate::snapshots::take_snapshots(project, &changed, now, "Before replace", new_id)?;
    project
        .as_object_mut()
        .ok_or(crate::edit::EditError::Shape)?
        .insert("documents", JsValue::Array(documents));
    Ok(result)
}
