//! `search_documents` (`mcp/src/searchTools.ts`, upstream `8cdc694`): the app's matching rules (`src/lib/search.ts`, ported in
//! `prosecraft_core::search`) over the titles and Markdown bodies in the open folder.

use std::collections::HashMap;
use std::sync::OnceLock;

use prosecraft_core::js::{JsString, JsValue};
use prosecraft_core::search::{
    Haystack, build_snippet, collapse_whitespace, make_haystack, ranges,
};

use crate::jsx::{self, object};
use crate::read_tools::{read_stored_chapters, read_stored_documents};
use crate::store::{Found, find_project_dir, str_or_empty, str_or_null};

pub const DEFAULT_SEARCH_LIMIT: f64 = 30.0;
pub const MAX_SEARCH_LIMIT: f64 = 100.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    Scene,
    Wiki,
}

impl Space {
    fn name(self) -> &'static str {
        match self {
            Self::Scene => "scene",
            Self::Wiki => "wiki",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SearchOptions {
    pub space: Option<Space>,
    pub chapter_id: Option<String>,
    pub folder_id: Option<String>,
    pub label_id: Option<String>,
    pub include_trashed: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub limit: Option<f64>,
}

/// One of `markdownToPlainText`'s replacements: a pattern and what replaces each match (`$1`
/// for the first group).
struct Rule {
    regex: Option<regress::Regex>,
    replacement: &'static str,
}

fn rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        [
            (r"```[\s\S]*?```", "g", " "),
            (r"`([^`]*)`", "g", "$1"),
            (r"!\[([^\]]*)\]\([^)]*\)", "g", "$1"),
            (r"\[([^\]]*)\]\([^)]*\)", "g", "$1"),
            (r"^\s{0,3}#{1,6}\s+", "gm", ""),
            (r"^\s{0,3}>\s?", "gm", ""),
            (r"^\s*(?:[-*+]|\d+[.)])\s+", "gm", ""),
            (r"^\s*(?:-{3,}|\*{3,}|_{3,})\s*$", "gm", " "),
            (r"\*{1,3}([^*\n]+)\*{1,3}", "g", "$1"),
        ]
        .into_iter()
        .map(|(pattern, flags, replacement)| Rule {
            // The `g` flag is how `replace` walks the text; regress takes the others.
            regex: regress::Regex::with_flags(pattern, flags.trim_start_matches('g')).ok(),
            replacement,
        })
        .collect()
    })
}

/// `text.replace(regex, replacement)` with the `g` flag.
fn replace_all(text: &str, rule: &Rule) -> String {
    let Some(regex) = &rule.regex else {
        return text.to_owned();
    };
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for found in regex.find_iter(text) {
        let range = found.range();
        out.push_str(text.get(last..range.start).unwrap_or_default());
        if rule.replacement == "$1" {
            if let Some(group) = found.group(1) {
                out.push_str(text.get(group).unwrap_or_default());
            }
        } else {
            out.push_str(rule.replacement);
        }
        last = range.end;
    }
    out.push_str(text.get(last..).unwrap_or_default());
    out
}

/// `markdownToPlainText`: the body with its common Markdown syntax dropped, each marker a space
/// so words do not fuse, whitespace collapsed.
pub fn markdown_to_plain_text(markdown: &str) -> String {
    let mut text = markdown.to_owned();
    for rule in rules() {
        text = replace_all(&text, rule);
    }
    collapse_whitespace(&text)
}

struct Candidate {
    hit: JsValue,
    title_match: bool,
    kind_rank: u8,
    chapter_index: i64,
    document_index: usize,
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `foldWithOffsets` when folding changed the length, otherwise the index itself.
fn source_index(haystack: &Haystack, index: usize) -> usize {
    haystack.source_index(index)
}

/// `searchDocuments`: one hit per matching document; title matches first, then scenes in
/// chapter order, then Wiki pages.
pub fn search_documents(
    root: &str,
    project_id: &str,
    query: &JsString,
    options: &SearchOptions,
) -> Result<JsValue, String> {
    let needle = jsx::trim_units(query.units());
    if needle.is_empty() {
        return Ok(JsValue::Array(Vec::new()));
    }
    let needle = String::from_utf16_lossy(needle);
    let Found { dir, header } = find_project_dir(root, project_id)?;
    let documents = read_stored_documents(&dir, &header)?;
    let chapters: HashMap<String, _> = read_stored_chapters(&dir, &header)?
        .into_iter()
        .map(|c| (c.id.clone(), c))
        .collect();
    let chapter_order = chapter_order(&header)?;

    let search_for: Vec<u16> = if options.case_sensitive {
        units(&needle)
    } else {
        make_haystack(&needle, false).text
    };
    let limit = options
        .limit
        .unwrap_or(DEFAULT_SEARCH_LIMIT)
        .clamp(1.0, MAX_SEARCH_LIMIT);

    let mut candidates = Vec::new();
    for (document_index, document) in documents.iter().enumerate() {
        let fields = &document.fields;
        let stored_chapter = jsx::as_str(fields.get("chapterId"))
            .filter(|c| !c.is_empty())
            .map(JsString::to_string_lossy);
        let kind = if stored_chapter.is_some() {
            Space::Scene
        } else if jsx::is_str(fields.get("space"), "Wiki") {
            Space::Wiki
        } else {
            continue;
        };
        if options.space.is_some_and(|s| s != kind) {
            continue;
        }
        let chapter = stored_chapter.as_ref().and_then(|c| chapters.get(c));
        if !options.include_trashed
            && (jsx::is_true(fields.get("trashed")) || chapter.is_some_and(|c| c.trashed))
        {
            continue;
        }
        if options
            .chapter_id
            .as_ref()
            .is_some_and(|wanted| stored_chapter.as_ref() != Some(wanted))
        {
            continue;
        }
        let strict = |key: &str, wanted: &Option<String>| {
            wanted
                .as_deref()
                .is_some_and(|w| !jsx::is_str(fields.get(key), w))
        };
        if strict("folderId", &options.folder_id) || strict("labelId", &options.label_id) {
            continue;
        }

        let title = jsx::as_str(fields.get("title"))
            .map(JsString::to_string_lossy)
            .unwrap_or_default();
        let title_haystack = make_haystack(&collapse_whitespace(&title), options.case_sensitive);
        let body_haystack = make_haystack(
            &markdown_to_plain_text(&document.body),
            options.case_sensitive,
        );
        let title_matches = ranges(&title_haystack.text, &search_for, options.whole_word);
        let body_matches = ranges(&body_haystack.text, &search_for, options.whole_word);
        let matches = title_matches.len() + body_matches.len();
        if matches == 0 {
            continue;
        }
        let (container, first) = match title_matches.first() {
            Some(first) => (&title_haystack, *first),
            None => match body_matches.first() {
                Some(first) => (&body_haystack, *first),
                None => continue,
            },
        };
        let start = source_index(container, first);
        let end = source_index(container, first + search_for.len());
        let (snippet, _) = build_snippet(&container.source, start, end.saturating_sub(start));

        let chapter_title = match (kind, chapter) {
            (Space::Scene, Some(chapter)) => chapter.title.clone(),
            _ => JsValue::Null,
        };
        candidates.push(Candidate {
            hit: object(vec![
                ("id", JsValue::str(&document.id)),
                ("title", str_or_empty(fields.get("title"))),
                ("space", JsValue::str(kind.name())),
                ("chapterTitle", chapter_title),
                ("labelId", str_or_null(fields.get("labelId"))),
                ("matches", JsValue::Number(matches as f64)),
                ("snippet", jsx::units(&snippet)),
            ]),
            title_match: !title_matches.is_empty(),
            kind_rank: u8::from(kind == Space::Wiki),
            chapter_index: stored_chapter
                .as_ref()
                .and_then(|c| chapter_order.get(c).copied())
                .unwrap_or(MAX_SAFE_INTEGER),
            document_index,
        });
    }

    candidates.sort_by(|a, b| {
        b.title_match
            .cmp(&a.title_match)
            .then(a.kind_rank.cmp(&b.kind_rank))
            .then(a.chapter_index.cmp(&b.chapter_index))
            .then(a.document_index.cmp(&b.document_index))
    });
    // `limit` is a whole number from 1 to 100 here.
    let limit = limit as usize;
    Ok(JsValue::Array(
        candidates.into_iter().take(limit).map(|c| c.hit).collect(),
    ))
}

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// Each chapter's place in `header.order.chapters`; a repeated id keeps its last place.
fn chapter_order(header: &JsValue) -> Result<HashMap<String, i64>, String> {
    let list = jsx::nullish(jsx::prop(jsx::prop(Some(header), "order")?, "chapters")?);
    let items = match list {
        None => Vec::new(),
        Some(JsValue::Array(items)) => items.clone(),
        Some(_) => return Err("(header.order.chapters ?? []).map is not a function".into()),
    };
    Ok(items
        .iter()
        .enumerate()
        .filter_map(|(index, id)| {
            Some((
                jsx::as_str(Some(id))?.to_string_lossy(),
                i64::try_from(index).ok()?,
            ))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_loses_its_markers() {
        assert_eq!(
            markdown_to_plain_text(
                "# Docks\n\nIt was dark in the **harbor** and [a](b) `x`.\n\n- item\n> quote\n\n---\n"
            ),
            "Docks It was dark in the harbor and a x. item quote"
        );
    }
}
