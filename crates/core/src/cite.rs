//! Citation markers in scene HTML (ProseCraft `src/lib/citations/cite-marker.ts`): the
//! `data-cites` JSON of a `<span class="cite-ref">`, validated and normalized, and the
//! Pandoc syntax it is written as in Markdown.

use crate::html::is_js_whitespace;
use crate::js::{JsObject, JsString, JsValue};

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

fn item(raw: &JsValue) -> Option<CiteRefItem> {
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
fn cite_json(items: &[CiteRefItem]) -> String {
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
