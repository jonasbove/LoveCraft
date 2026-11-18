//! Citation markers in scene HTML (ProseCraft `src/lib/citations/cite-marker.ts`): the
//! `data-cites` JSON of a `<span class="cite-ref">`, validated and normalized, and the
//! Pandoc syntax it is written as in Markdown.

use crate::html::is_js_whitespace;
use crate::js::regexp::Re;
use crate::js::{JsObject, JsString, JsValue};
use std::sync::OnceLock;

pub const CITE_REF_CLASS: &str = "cite-ref";
pub const MAX_CITE_ITEMS: usize = 32;
pub const MAX_CITE_ATTR_LENGTH: usize = 8192;
const MAX_LOCATOR: usize = 64;
const MAX_AFFIX: usize = 200;
const MAX_ID: usize = 128;
const LABELS: [&str; 3] = ["page", "chapter", "section"];
const KEYS: [&str; 6] = [
    "id",
    "locator",
    "label",
    "prefix",
    "suffix",
    "suppressAuthor",
];

/// One cited source in a cluster.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CiteRefItem {
    pub id: String,
    pub locator: Option<String>,
    pub label: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub suppress_author: bool,
}

/// Length in UTF-16 code units, as JavaScript counts it.
fn js_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

/// `CITE_ID_RE`: letters, digits and `_` at the ends, and `_ . : / -` between.
pub fn is_valid_cite_id(id: &str) -> bool {
    let end = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let middle = |c: char| end(c) || matches!(c, '.' | ':' | '/' | '-');
    let chars: Vec<char> = id.chars().collect();
    match chars.as_slice() {
        [] => false,
        [only] => end(*only),
        [first, inner @ .., last] => {
            js_len(id) <= MAX_ID
                && chars.len() <= 128
                && end(*first)
                && end(*last)
                && inner.iter().all(|c| middle(*c))
        }
    }
}

fn has_control(text: &str) -> bool {
    text.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}')
}

/// A text field: a string within `limit` without control characters, or the item is invalid.
fn text_field(object: &JsObject, key: &str, limit: usize) -> Result<Option<String>, ()> {
    match object.get(key) {
        None => Ok(None),
        Some(JsValue::String(s)) => {
            let s = s.as_string().ok_or(())?;
            if js_len(&s) > limit || has_control(&s) {
                Err(())
            } else {
                Ok(Some(s))
            }
        }
        Some(_) => Err(()),
    }
}

pub(crate) fn item(raw: &JsValue) -> Option<CiteRefItem> {
    let object = raw.as_object()?;
    if object
        .iter()
        .any(|(k, _)| !KEYS.iter().any(|key| k.eq_str(key)))
    {
        return None;
    }
    let id = object
        .get("id")
        .and_then(JsValue::as_js_string)
        .and_then(JsString::as_string)
        .filter(|id| is_valid_cite_id(id))?;
    let locator = text_field(object, "locator", MAX_LOCATOR)
        .ok()?
        .map(|l| js_trim(&l).to_owned())
        .filter(|l| !l.is_empty());
    let label = match object.get("label") {
        None => None,
        Some(JsValue::String(s)) => {
            let s = s.as_string().filter(|s| LABELS.contains(&s.as_str()))?;
            locator.is_some().then_some(s)
        }
        Some(_) => return None,
    };
    let prefix = text_field(object, "prefix", MAX_AFFIX)
        .ok()?
        .filter(|p| !js_trim(p).is_empty())
        .map(|p| p.trim_start_matches(is_js_whitespace).to_owned());
    let suffix = text_field(object, "suffix", MAX_AFFIX)
        .ok()?
        .map(|s| js_trim(&s).to_owned())
        .filter(|s| !s.is_empty());
    let suppress_author = match object.get("suppressAuthor") {
        None => false,
        Some(JsValue::Bool(b)) => *b,
        Some(_) => return None,
    };
    Some(CiteRefItem {
        id,
        locator,
        label,
        prefix,
        suffix,
        suppress_author,
    })
}

/// `parseCiteData`: the items of a `data-cites` value, or `None` if any part is invalid.
pub fn parse_cite_data(raw: Option<&str>) -> Option<Vec<CiteRefItem>> {
    let raw = raw.filter(|r| !r.is_empty() && js_len(r) <= MAX_CITE_ATTR_LENGTH)?;
    let parsed = crate::js::parse(raw).ok()?;
    let entries = parsed
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= MAX_CITE_ITEMS)?;
    entries.iter().map(item).collect()
}

/// The JSON of normalized items, keys in ProseCraft's order.
pub(crate) fn cite_json(items: &[CiteRefItem]) -> String {
    let values = items
        .iter()
        .map(|item| {
            let mut o = JsObject::new();
            o.insert("id", JsValue::str(&item.id));
            for (key, value) in [
                ("locator", &item.locator),
                ("label", &item.label),
                ("prefix", &item.prefix),
                ("suffix", &item.suffix),
            ] {
                if let Some(v) = value {
                    o.insert(key, JsValue::str(v));
                }
            }
            if item.suppress_author {
                o.insert("suppressAuthor", JsValue::Bool(true));
            }
            JsValue::Object(o)
        })
        .collect();
    crate::js::stringify(&JsValue::Array(values))
}

/// `normalizeCiteDataAttr`: the canonical `data-cites` value, or `None`.
pub fn normalize_cite_data_attr(raw: Option<&str>) -> Option<String> {
    let json = cite_json(&parse_cite_data(raw)?);
    (js_len(&json) <= MAX_CITE_ATTR_LENGTH).then_some(json)
}

/// The `data-cites` values of `CITE_REF_SELECTOR` (`span.cite-ref[data-cites]`) elements, in
/// document order.
fn collect_cite_data<'a>(nodes: &'a [crate::html::Node], out: &mut Vec<&'a str>) {
    for node in nodes {
        if let crate::html::Node::Element(e) = node {
            if e.name == "span"
                && e.has_class(CITE_REF_CLASS)
                && let Some(data) = e.attr("data-cites")
            {
                out.push(data);
            }
            collect_cite_data(&e.children, out);
        }
    }
}

/// `citationIdsInHtml`: the citation ids the markers in `html` cite, each once, in order of
/// first appearance (markers with invalid data skipped).
pub fn citation_ids_in_html(html: &str) -> Vec<String> {
    if html.is_empty() || !html.contains(CITE_REF_CLASS) {
        return Vec::new();
    }
    let nodes = crate::html::parse_fragment(html);
    let mut data = Vec::new();
    collect_cite_data(&nodes, &mut data);
    let mut ids: Vec<String> = Vec::new();
    for cites in data.into_iter().filter_map(|d| parse_cite_data(Some(d))) {
        for cite in cites {
            if !ids.contains(&cite.id) {
                ids.push(cite.id);
            }
        }
    }
    ids
}

fn escape_pandoc(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if matches!(c, '\\' | '[' | ']' | ';') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `/^[.,;:!?\])]/`: a suffix that attaches without a space.
fn attaches(text: &str) -> bool {
    text.starts_with(['.', ',', ';', ':', '!', '?', ']', ')'])
}

/// `formatPandocCitations`: `[see @key, p. 42; -@other]`.
pub fn format_pandoc_citations(items: &[CiteRefItem]) -> String {
    let parts: Vec<String> = items
        .iter()
        .map(|entry| {
            let prefix = entry
                .prefix
                .as_deref()
                .map(escape_pandoc)
                .unwrap_or_default();
            let prefix_part = if prefix.is_empty() {
                String::new()
            } else if prefix.ends_with(is_js_whitespace) {
                prefix
            } else {
                format!("{prefix} ")
            };
            let key = format!(
                "{}@{}",
                if entry.suppress_author { "-" } else { "" },
                entry.id
            );
            let suffix = entry
                .suffix
                .as_deref()
                .map(|s| escape_pandoc(js_trim(s)))
                .filter(|s| !s.is_empty());
            let glue = |s: String| if attaches(&s) { s } else { format!(" {s}") };
            let tail = match (&entry.locator, suffix) {
                (Some(locator), suffix) => {
                    let label = match entry.label.as_deref() {
                        Some("chapter") => "ch.",
                        Some("section") => "sec.",
                        _ => "p.",
                    };
                    format!(
                        ", {label} {}{}",
                        escape_pandoc(locator),
                        suffix.map(glue).unwrap_or_default()
                    )
                }
                (None, Some(suffix)) => glue(suffix),
                (None, None) => String::new(),
            };
            format!("{prefix_part}{key}{tail}")
        })
        .collect();
    format!("[{}]", parts.join("; "))
}

macro_rules! pandoc_rule {
    ($name:ident, $source:expr, $flags:expr) => {
        fn $name() -> &'static Re {
            static RULE: OnceLock<Re> = OnceLock::new();
            RULE.get_or_init(|| Re::new($source, $flags))
        }
    };
}

pandoc_rule!(
    pandoc_segment,
    r"^((?:[\s\S]*?\s)?)(-?)@([A-Za-z0-9_](?:[A-Za-z0-9_.:/-]{0,126}[A-Za-z0-9_])?)(?=$|[\s,;.)\]!?])([\s\S]*)$",
    ""
);
pandoc_rule!(
    explicit_locator,
    r"^(pp?\.|pages?|ch\.|chap\.|chapters?|sec\.|sect\.|sections?|§|S\.|Kap\.|Abschn\.)\s*(.+)$",
    "i"
);
pandoc_rule!(
    locator_value,
    r"^([0-9]+(?:[.:][0-9]+)*(?:[A-Za-z]{1,2})?(?:\s*[-–—]\s*[0-9]+(?:[.:][0-9]+)*(?:[A-Za-z]{1,2})?)*(?:,\s*[0-9]+(?:[.:][0-9]+)*(?:[A-Za-z]{1,2})?(?:\s*[-–—]\s*[0-9]+(?:[.:][0-9]+)*(?:[A-Za-z]{1,2})?)*)*|[ivxlcdm]+(?:\s*[-–—]\s*[ivxlcdm]+)*)(?=$|[\s,;.\-–—])([\s\S]*)$",
    "i"
);
pandoc_rule!(
    bare_numeric_locator,
    r"^([0-9]+(?:\s*[-–—]\s*[0-9]+)?)(?=$|[\s,;.)\]!?])([\s\S]*)$",
    ""
);
pandoc_rule!(leading_comma, r"^\s*,\s*", "");

/// `unescapePandocText`: `\\`, `\[`, `\]` and `\;` unescaped.
fn unescape_pandoc(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(&next) = chars.peek()
            && matches!(next, '\\' | '[' | ']' | ';')
        {
            out.push(next);
            chars.next();
            continue;
        }
        out.push(c);
    }
    out
}

/// `splitPandocSegments`: the cluster split at unescaped `;`, escapes kept.
fn split_pandoc_segments(raw: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for c in raw.chars() {
        if escaped {
            current.push('\\');
            current.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == ';' {
            segments.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    if escaped {
        current.push('\\');
    }
    segments.push(current);
    segments
}

/// `mapLocatorPrefix`.
fn locator_label(prefix: &str) -> &'static str {
    let lower = prefix.to_lowercase();
    if lower.starts_with("ch") || lower.starts_with("kap") {
        "chapter"
    } else if lower.starts_with("sec") || lower == "§" || lower.starts_with("abschn") {
        "section"
    } else {
        "page"
    }
}

/// `parsePandocAfter`: the locator, its label and the suffix after a cited key.
fn parse_pandoc_after(raw: &str) -> (Option<String>, Option<&'static str>, Option<String>) {
    let suffix_only = |text: &str| {
        let suffix = js_trim(&unescape_pandoc(text)).to_owned();
        (None, None, (!suffix.is_empty()).then_some(suffix))
    };
    if js_trim(raw).is_empty() {
        return (None, None, None);
    }
    if !raw.trim_start_matches(is_js_whitespace).starts_with(',') {
        return suffix_only(raw);
    }
    let after_comma = leading_comma().replace(raw, false, "");
    if let Some(explicit) = explicit_locator().exec(&after_comma) {
        let label = locator_label(explicit.at(1));
        let body = js_trim(explicit.at(2));
        if let Some(split) = locator_value().exec(body) {
            let locator = unescape_pandoc(js_trim(split.at(1)));
            let suffix = js_trim(&unescape_pandoc(split.at(2))).to_owned();
            return (
                Some(locator),
                Some(label),
                (!suffix.is_empty()).then_some(suffix),
            );
        }
        if let Some(comma) = body.find(',').filter(|&i| i > 0) {
            return (
                Some(unescape_pandoc(js_trim(&body[..comma]))),
                Some(label),
                Some(js_trim(&unescape_pandoc(&body[comma..])).to_owned()),
            );
        }
        return (Some(unescape_pandoc(body)), Some(label), None);
    }
    if let Some(bare) = bare_numeric_locator().exec(&after_comma) {
        let suffix = js_trim(&unescape_pandoc(bare.at(2))).to_owned();
        return (
            Some(unescape_pandoc(js_trim(bare.at(1)))),
            Some("page"),
            (!suffix.is_empty()).then_some(suffix),
        );
    }
    suffix_only(raw)
}

/// `parsePandocSegment`: one cited source, validated as `data-cites` items are.
fn parse_pandoc_segment(segment: &str) -> Option<CiteRefItem> {
    let trimmed = segment.trim_start_matches(is_js_whitespace);
    let found = pandoc_segment().exec(trimmed)?;
    let prefix = unescape_pandoc(found.at(1));
    let (locator, label, suffix) = parse_pandoc_after(found.at(4));
    let mut candidate = JsObject::new();
    candidate.insert("id", JsValue::str(found.at(3)));
    if let Some(locator) = locator.filter(|l| !l.is_empty()) {
        candidate.insert("locator", JsValue::str(&locator));
    }
    if let Some(label) = label {
        candidate.insert("label", JsValue::str(label));
    }
    if !js_trim(&prefix).is_empty() {
        candidate.insert("prefix", JsValue::str(&prefix));
    }
    if let Some(suffix) = suffix.filter(|s| !s.is_empty()) {
        candidate.insert("suffix", JsValue::str(&suffix));
    }
    if found.at(2) == "-" {
        candidate.insert("suppressAuthor", JsValue::Bool(true));
    }
    item(&JsValue::Object(candidate))
}

/// `parsePandocCitationCluster`: the sources of `[see @key, p. 4; -@other]`, without its
/// brackets, or `None` when any part is not a citation.
pub fn parse_pandoc_citation_cluster(raw: &str) -> Option<Vec<CiteRefItem>> {
    if raw.is_empty() || !raw.contains('@') || js_len(raw) > MAX_CITE_ATTR_LENGTH {
        return None;
    }
    let segments = split_pandoc_segments(raw);
    if segments.is_empty() || segments.len() > MAX_CITE_ITEMS {
        return None;
    }
    segments.iter().map(|s| parse_pandoc_segment(s)).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        format_pandoc_citations, is_valid_cite_id, normalize_cite_data_attr, parse_cite_data,
    };

    #[test]
    fn validates_ids() {
        assert!(
            is_valid_cite_id("smith2020")
                && is_valid_cite_id("a")
                && is_valid_cite_id("doi:10.1/x")
        );
        assert!(
            !is_valid_cite_id("")
                && !is_valid_cite_id("-a")
                && !is_valid_cite_id("a.")
                && !is_valid_cite_id("a b")
        );
    }

    #[test]
    fn normalizes_and_formats_clusters() {
        let raw = r#"[{"id":"smith2020","locator":" 42 ","label":"page","prefix":"  see ","suffix":" ff. "}]"#;
        assert_eq!(
            normalize_cite_data_attr(Some(raw)).as_deref(),
            Some(
                r#"[{"id":"smith2020","locator":"42","label":"page","prefix":"see ","suffix":"ff."}]"#
            )
        );
        assert_eq!(parse_cite_data(Some(r#"[{"id":"a","extra":1}]"#)), None);
        let items = parse_cite_data(Some(r#"[{"id":"s","locator":"42","label":"page","prefix":"see","suffix":"ff."},{"id":"doe","suppressAuthor":true,"suffix":"; more [x]"}]"#)).expect("valid");
        assert_eq!(
            format_pandoc_citations(&items),
            r"[see @s, p. 42 ff.; -@doe \; more \[x\]]"
        );
    }
}
