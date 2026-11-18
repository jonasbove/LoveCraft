//! The citation library page's helpers (ProseCraft `src/lib/citations/library.ts`, upstream
//! `7dae544`): source kinds and type labels, author lists as edited and shown, readable
//! citation keys, merging imported items, where a source is cited, and the library search.

use crate::cite::citation_ids_in_html;
use crate::csl::{
    MAX_CITATION_ID_LENGTH, MAX_CITATION_ITEMS, parse_person_name, sanitize_citation_id,
};
use crate::html::is_js_whitespace;
use crate::js::regexp::Re;
use crate::js::{JsValue, number_to_string};
use crate::project::Document;
use std::collections::HashSet;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

/// `ManualSourceKind`: the kinds of source the manual entry form offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualSourceKind {
    Book,
    Article,
    Chapter,
    Website,
}

impl ManualSourceKind {
    /// ProseCraft's name for the kind: `book`, `article`, `chapter` or `website`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Book => "book",
            Self::Article => "article",
            Self::Chapter => "chapter",
            Self::Website => "website",
        }
    }

    /// The kind ProseCraft calls `name`.
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Book, Self::Article, Self::Chapter, Self::Website]
            .into_iter()
            .find(|k| k.as_str() == name)
    }
}

/// `manualKindToCslType`: the CSL item type of a manual form kind.
pub fn manual_kind_to_csl_type(kind: ManualSourceKind) -> &'static str {
    match kind {
        ManualSourceKind::Book => "book",
        ManualSourceKind::Article => "article-journal",
        ManualSourceKind::Chapter => "chapter",
        ManualSourceKind::Website => "webpage",
    }
}

/// `cslTypeToManualKind`: the manual form kind of a CSL item type, if it has one.
pub fn csl_type_to_manual_kind(csl_type: &str) -> Option<ManualSourceKind> {
    match csl_type.to_lowercase().as_str() {
        "book" => Some(ManualSourceKind::Book),
        "article-journal" | "article" | "article-magazine" | "article-newspaper" => {
            Some(ManualSourceKind::Article)
        }
        "chapter" | "paper-conference" => Some(ManualSourceKind::Chapter),
        "webpage" | "post-weblog" | "post" => Some(ManualSourceKind::Website),
        _ => None,
    }
}

/// `sourceTypeLabel`: a readable label for a CSL item type.
pub fn source_type_label(csl_type: &str) -> String {
    let label = match csl_type.to_lowercase().as_str() {
        "book" => "Book",
        "article-journal" | "article" | "article-magazine" | "article-newspaper" => "Article",
        "chapter" => "Chapter",
        "paper-conference" => "Conference paper",
        "webpage" | "post-weblog" | "post" => "Website",
        "thesis" => "Thesis",
        "report" => "Report",
        _ => {
            let mut chars = csl_type.chars();
            return match chars.next() {
                None => "Source".into(),
                // `charAt(0)` of a character outside the BMP is half of it, which
                // `toUpperCase` leaves alone.
                Some(first) if first.len_utf16() == 2 => csl_type.to_owned(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            };
        }
    };
    label.into()
}

/// A string field of an object, `None` when missing, not a string, or empty (falsy in JS).
fn field(value: &JsValue, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .filter(|s| !s.is_empty())
}

/// `[a, b].filter(Boolean).join(" ")`.
fn join_present(parts: [Option<String>; 2]) -> String {
    parts.into_iter().flatten().collect::<Vec<_>>().join(" ")
}

/// `formatAuthorList`: names as the form edits them, `Family, Given` separated by `; `.
pub fn format_author_list(names: &[JsValue]) -> String {
    names
        .iter()
        .map(|name| {
            if let Some(literal) = field(name, "literal") {
                return literal;
            }
            let family =
                join_present([field(name, "non-dropping-particle"), field(name, "family")]);
            let given = join_present([field(name, "given"), field(name, "dropping-particle")]);
            if !family.is_empty() && !given.is_empty() {
                return match field(name, "suffix") {
                    Some(suffix) => format!("{family}, {suffix}, {given}"),
                    None => format!("{family}, {given}"),
                };
            }
            if family.is_empty() { given } else { family }
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

fn names<'a>(item: &'a JsValue, key: &str) -> &'a [JsValue] {
    item.get(key)
        .and_then(JsValue::as_array)
        .unwrap_or_default()
}

/// `formatItemContributors`: the authors (else the editors) as a list row shows them.
pub fn format_item_contributors(item: &JsValue) -> String {
    let authors = names(item, "author");
    let list = if authors.is_empty() {
        names(item, "editor")
    } else {
        authors
    };
    list.iter()
        .map(|name| {
            if let Some(literal) = field(name, "literal") {
                return literal;
            }
            let family =
                join_present([field(name, "non-dropping-particle"), field(name, "family")]);
            match field(name, "given") {
                Some(given) if !family.is_empty() => format!("{family}, {given}"),
                given if family.is_empty() => given.unwrap_or_default(),
                _ => family,
            }
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

/// `parseAuthorList`: names separated by semicolons or new lines, `None` when there are none.
pub fn parse_author_list(raw: &str) -> Option<Vec<JsValue>> {
    let names: Vec<JsValue> = raw
        .split([';', '\n'])
        .map(|part| part.trim_matches(is_js_whitespace))
        .filter(|part| !part.is_empty())
        .filter_map(parse_person_name)
        .collect();
    (!names.is_empty()).then_some(names)
}

/// `itemYearText`: the first part of the issued date, else its literal or raw text.
pub fn item_year_text(item: &JsValue) -> String {
    let issued = item.get("issued");
    let first_part = issued
        .and_then(|i| i.get("date-parts"))
        .and_then(JsValue::as_array)
        .and_then(|parts| parts.first())
        .and_then(JsValue::as_array)
        .and_then(|group| group.first());
    match first_part {
        Some(JsValue::Number(n)) => return number_to_string(*n),
        Some(JsValue::String(s)) => return s.to_string_lossy(),
        _ => {}
    }
    let text = |key: &str| {
        issued
            .and_then(|i| i.get(key))
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
    };
    text("literal").or_else(|| text("raw")).unwrap_or_default()
}

fn rule(cell: &'static OnceLock<Re>, source: &str) -> &'static Re {
    cell.get_or_init(|| Re::new(source, ""))
}

fn combining_marks() -> &'static Re {
    static RULE: OnceLock<Re> = OnceLock::new();
    rule(&RULE, r"[̀-ͯ]")
}

/// `transliterateForKey`: German umlauts spelled out, other accents dropped, lower case, and
/// only ASCII letters and digits kept.
fn transliterate_for_key(value: &str) -> String {
    let spelled = value
        .replace(['ä', 'Ä'], "ae")
        .replace(['ö', 'Ö'], "oe")
        .replace(['ü', 'Ü'], "ue")
        .replace('ß', "ss");
    let decomposed: String = spelled.nfkd().collect();
    let plain = combining_marks().replace(&decomposed, true, "");
    plain
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .collect()
}

/// The parts of a source a citation key is made from (`generateCitationKey`'s `params`).
#[derive(Clone, Copy, Debug, Default)]
pub struct CitationKeySource<'a> {
    pub author: Option<&'a [JsValue]>,
    pub editor: Option<&'a [JsValue]>,
    pub title: Option<&'a str>,
    pub year: Option<&'a str>,
}

/// The first `count` UTF-16 units of `text`.
fn first_units(text: &str, count: usize) -> String {
    String::from_utf16_lossy(&text.encode_utf16().take(count).collect::<Vec<_>>())
}

/// `id` cut to leave room for `suffix` within the id length limit, then `suffix`.
fn with_suffix(id: &str, suffix: &str) -> String {
    let keep = MAX_CITATION_ID_LENGTH.saturating_sub(suffix.encode_utf16().count());
    format!("{}{suffix}", first_units(id, keep))
}

fn string_field(value: &JsValue, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
}

/// The surname (or literal name, or first word of the title) a key starts with.
fn key_name(source: &CitationKeySource) -> String {
    let lead = source
        .author
        .and_then(|a| a.first())
        .or_else(|| source.editor.and_then(|e| e.first()));
    lead.and_then(|l| string_field(l, "family").or_else(|| string_field(l, "literal")))
        .or_else(|| {
            source.title.map(|t| {
                let trimmed = t.trim_matches(is_js_whitespace);
                trimmed
                    .split(is_js_whitespace)
                    .next()
                    .unwrap_or_default()
                    .to_owned()
            })
        })
        .unwrap_or_default()
}

/// `generateCitationKey`: a readable key such as `mueller2020` from the first author's or
/// editor's surname (or the title) and the year, made unique among `existing` ids and cite
/// keys (`exclude_id`'s item left out) with `-2`, `-3`….
pub fn generate_citation_key(
    source: &CitationKeySource,
    existing: &[JsValue],
    exclude_id: Option<&str>,
) -> String {
    let slug: String = transliterate_for_key(&key_name(source))
        .chars()
        .take(24)
        .collect();
    let name_slug = if slug.is_empty() {
        "ref".to_owned()
    } else {
        slug
    };
    let year_digits: String = source
        .year
        .unwrap_or_default()
        .chars()
        .filter(char::is_ascii_digit)
        .take(4)
        .collect();
    let sanitized = sanitize_citation_id(&format!("{name_slug}{year_digits}"));
    let base_raw = if sanitized.is_empty() {
        "ref".to_owned()
    } else {
        sanitized
    };
    let base = first_units(&base_raw, MAX_CITATION_ID_LENGTH);
    let mut used = HashSet::new();
    for item in existing {
        let id = string_field(item, "id").unwrap_or_default();
        if exclude_id.is_some_and(|e| !e.is_empty() && e == id) {
            continue;
        }
        used.insert(id);
        if let Some(cite_key) = field(item, "citeKey") {
            used.insert(cite_key);
        }
    }
    if !used.contains(&base) {
        return base;
    }
    for counter in 2..MAX_CITATION_ITEMS + 10 {
        let candidate = with_suffix(&base, &format!("-{counter}"));
        if !used.contains(&candidate) {
            return candidate;
        }
    }
    format!("ref-{}", existing.len() + 1)
}

/// `mergeImportedCitations`' result: the library, how many items were added, and warnings.
#[derive(Clone, Debug, PartialEq)]
pub struct CitationMerge {
    pub items: Vec<JsValue>,
    pub added_count: usize,
    pub warnings: Vec<String>,
}

/// `mergeImportedCitations`: `imported` added after `existing`, an id already used renamed
/// with `-2`, `-3`…, and the library capped at 5,000 items.
pub fn merge_imported_citations(existing: &[JsValue], imported: &[JsValue]) -> CitationMerge {
    let mut items = existing.to_vec();
    let mut seen: HashSet<String> = existing
        .iter()
        .map(|item| string_field(item, "id").unwrap_or_default())
        .collect();
    let mut warnings = Vec::new();
    let mut added_count = 0;
    for entry in imported {
        if items.len() >= MAX_CITATION_ITEMS {
            warnings.push(format!(
                "Citation library capped at {MAX_CITATION_ITEMS} items; remaining entries were skipped."
            ));
            break;
        }
        let original = string_field(entry, "id").unwrap_or_default();
        let mut id = original.clone();
        if seen.contains(&id) {
            let mut counter = 2;
            let mut candidate = id.clone();
            while seen.contains(&candidate) {
                candidate = with_suffix(&id, &format!("-{counter}"));
                counter += 1;
            }
            warnings.push(format!(
                "Duplicate citation id \"{id}\" renamed to \"{candidate}\"."
            ));
            id = candidate;
        }
        seen.insert(id.clone());
        let mut added = entry.clone();
        if id != original
            && let Some(object) = added.as_object_mut()
        {
            object.insert("id", JsValue::str(&id));
        }
        items.push(added);
        added_count += 1;
    }
    CitationMerge {
        items,
        added_count,
        warnings,
    }
}

/// `countSourceCitations`' result: the documents that cite a source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceCitations {
    pub document_count: usize,
    pub doc_ids: Vec<String>,
}

/// `countSourceCitations`: the documents whose text or footnotes cite `item` (by its id or
/// cite key).
pub fn count_source_citations(item: &JsValue, documents: &[Document]) -> SourceCitations {
    let mut match_ids = vec![string_field(item, "id").unwrap_or_default()];
    match_ids.extend(field(item, "citeKey"));
    let cites = |html: &str| {
        citation_ids_in_html(html)
            .iter()
            .any(|id| match_ids.contains(id))
    };
    let doc_ids: Vec<String> = documents
        .iter()
        .filter(|doc| cites(&doc.content) || doc.footnotes.iter().any(|f| cites(&f.html)))
        .map(|doc| doc.id.clone())
        .collect();
    SourceCitations {
        document_count: doc_ids.len(),
        doc_ids,
    }
}

/// `matchesLibraryQuery`: whether the library search `query` finds `item` (in its title,
/// names, year, type, container, publisher, id, cite key or DOI).
pub fn matches_library_query(item: &JsValue, query: &str) -> bool {
    let needle = query.trim_matches(is_js_whitespace).to_lowercase();
    if needle.is_empty() {
        return true;
    }
    let item_type = string_field(item, "type").unwrap_or_default();
    let haystack = [
        field(item, "title"),
        Some(format_item_contributors(item)),
        Some(format_author_list(names(item, "editor"))),
        Some(item_year_text(item)),
        Some(source_type_label(&item_type)),
        Some(item_type),
        field(item, "container-title"),
        field(item, "publisher"),
        field(item, "id"),
        field(item, "citeKey"),
        field(item, "DOI"),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
    .to_lowercase();
    haystack.contains(&needle)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_combining_rule_compiles() {
        assert!(super::combining_marks().compiled());
    }
}
