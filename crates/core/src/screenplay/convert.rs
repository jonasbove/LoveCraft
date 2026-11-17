//! Prose into a screenplay (ProseCraft `src/lib/screenplay/convert.ts`, upstream `7dae544`):
//! a scene's lines become elements after a heading placeholder, speech recognized by a speaker
//! prefix ("Mara: …"), a leading dash, or quotation marks, and every piece of text kept.

use super::fountain::ScreenplayElement;
use super::layout::elements_to_html;
use crate::html::{Node, is_js_whitespace, parse_fragment};
use crate::js::{JsObject, JsValue};

const PLACEHOLDER_HEADING: &str = "INT./EXT. LOCATION - DAY";
const PLACEHOLDER_CHARACTER: &str = "CHARACTER";
const SPEAKER_PARTICLES: [&str; 11] = [
    "de", "del", "der", "di", "la", "le", "van", "von", "of", "the", "al",
];
const NON_SPEAKER_WORDS: [&str; 8] = [
    "http", "https", "mailto", "file", "note", "notes", "time", "date",
];
const CONTAINER_BLOCKS: [&str; 6] = ["div", "blockquote", "ul", "ol", "section", "article"];
const LEAF_BLOCKS: [&str; 9] = ["p", "h1", "h2", "h3", "h4", "h5", "h6", "li", "pre"];
const DASHES: [char; 3] = ['—', '–', '-'];

fn trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

/// `[A-Za-zÀ-ÖØ-öø-ÿ]`.
fn is_latin_letter(c: char) -> bool {
    c.is_ascii_alphabetic()
        || matches!(c, '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{f6}' | '\u{f8}'..='\u{ff}')
}

/// `[A-ZÀ-ÖØ-Þ]`.
fn is_latin_capital(c: char) -> bool {
    c.is_ascii_uppercase() || matches!(c, '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{de}')
}

fn inline_text(node: &Node) -> String {
    match node {
        Node::Text(t) => t.clone(),
        Node::Element(e) => {
            if e.name == "br" {
                return "\n".into();
            }
            let inner: String = e.children.iter().map(inline_text).collect();
            match e.name.as_str() {
                "em" | "i" => format!("*{inner}*"),
                "strong" | "b" => format!("**{inner}**"),
                "u" => format!("_{inner}_"),
                _ => inner,
            }
        }
    }
}

fn push_lines(text: &str, lines: &mut Vec<String>) {
    lines.extend(
        text.split('\n')
            .map(trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned),
    );
}

/// `collectLines`: each trimmed line of the scene, block by block.
fn collect_lines(nodes: &[Node], lines: &mut Vec<String>) {
    for node in nodes {
        match node {
            Node::Text(t) => push_lines(t, lines),
            Node::Element(e) => {
                let block_children = e.children.iter().any(|c| matches!(c, Node::Element(c) if CONTAINER_BLOCKS.contains(&c.name.as_str()) || LEAF_BLOCKS.contains(&c.name.as_str())));
                if CONTAINER_BLOCKS.contains(&e.name.as_str()) && block_children {
                    collect_lines(&e.children, lines);
                } else {
                    push_lines(&inline_text(node), lines);
                }
            }
        }
    }
}

/// `stripOuterQuotes`.
fn strip_outer_quotes(text: &str) -> String {
    let trimmed = trim(text);
    for (open, close) in [("\"", "\""), ("“", "”"), ("„", "“"), ("„", "”"), ("«", "»")]
    {
        let length = |s: &str| s.encode_utf16().count();
        if length(trimmed) > length(open) + length(close)
            && trimmed.starts_with(open)
            && trimmed.ends_with(close)
        {
            return trim(&trimmed[open.len()..trimmed.len() - close.len()]).to_owned();
        }
    }
    trimmed.to_owned()
}

/// `isSpeakerName`.
fn is_speaker_name(candidate: &str) -> bool {
    // `replace(/\s*\([^)]*\)\s*$/, "")`: a trailing parenthetical, the leftmost that fits.
    let body = candidate.trim_end_matches(is_js_whitespace);
    let without = match body.strip_suffix(')') {
        Some(inner) => {
            let from = inner.rfind(')').map_or(0, |i| i + 1);
            match inner[from..].find('(') {
                Some(open) => &body[..from + open],
                None => body,
            }
        }
        None => candidate,
    };
    let without = trim(without);
    if without.is_empty() || without.contains([',', ';', '?', '!']) {
        return false;
    }
    let words: Vec<&str> = without
        .split(is_js_whitespace)
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() > 4 {
        return false;
    }
    if let [word] = words.as_slice() {
        return !NON_SPEAKER_WORDS.contains(&word.to_lowercase().as_str())
            && word.chars().next().is_some_and(is_latin_capital);
    }
    words.iter().all(|w| {
        w.chars()
            .next()
            .is_some_and(|c| is_latin_capital(c) || c.is_ascii_digit())
            || SPEAKER_PARTICLES.contains(&w.to_lowercase().as_str())
    })
}

/// `SPEAKER_PREFIX`: the speaker and what they say.
fn speaker_prefix(line: &str) -> Option<(&str, &str)> {
    let colon = line.find(':')?;
    let name = &line[..colon];
    let mut chars = name.chars();
    if !chars.next().is_some_and(is_latin_letter) || name.chars().count() > 41 {
        return None;
    }
    if !chars.all(|c| {
        is_latin_letter(c) || c.is_ascii_digit() || matches!(c, ' ' | '.' | '\'' | '-' | '(' | ')')
    }) {
        return None;
    }
    let after = &line[colon + 1..];
    if !after.starts_with(is_js_whitespace) {
        return None;
    }
    // `\s+(.+)$`: at least one space, then the rest (lines come trimmed, so never empty).
    let speech = after.trim_start_matches(is_js_whitespace);
    if speech.is_empty() || speech.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
        return None;
    }
    Some((name, speech))
}

/// `DASH_ATTRIBUTION` (`^(.+?[,!?.])\s+[—–-]\s+(.+)$`).
fn dash_attribution(body: &str) -> Option<(&str, &str)> {
    for (i, c) in body.char_indices() {
        if i == 0 || !matches!(c, ',' | '!' | '?' | '.') {
            continue;
        }
        let end = i + c.len_utf8();
        let after = &body[end..];
        let spaced = after.trim_start_matches(is_js_whitespace);
        if spaced.len() == after.len() {
            continue;
        }
        let Some(dash) = spaced.chars().next().filter(|d| DASHES.contains(d)) else {
            continue;
        };
        let tail = &spaced[dash.len_utf8()..];
        let rest = tail.trim_start_matches(is_js_whitespace);
        if rest.len() == tail.len() || rest.is_empty() {
            continue;
        }
        return Some((&body[..end], rest));
    }
    None
}

/// `QUOTED_SPEECH` at the start of `text`: the match's length and what was said.
fn quoted_at(text: &str) -> Option<(usize, &str)> {
    let quote = |open: &str, closes: &[char], inner_excludes: &[char]| -> Option<(usize, &str)> {
        let rest = text.strip_prefix(open)?;
        let inner_len = rest
            .find(|c| inner_excludes.contains(&c))
            .unwrap_or(rest.len());
        if inner_len == 0 {
            return None;
        }
        let close = rest[inner_len..]
            .chars()
            .next()
            .filter(|c| closes.contains(c))?;
        Some((
            open.len() + inner_len + close.len_utf8(),
            &rest[..inner_len],
        ))
    };
    quote("\"", &['"'], &['"'])
        .or_else(|| quote("“", &['”'], &['”']))
        .or_else(|| quote("„", &['“', '”'], &['“', '”']))
        .or_else(|| quote("«", &['»'], &['»']))
}

fn element(kind: &str, text: impl Into<String>) -> ScreenplayElement {
    ScreenplayElement::new(kind, text)
}

/// `convertLine`.
fn convert_line(line: &str) -> Vec<ScreenplayElement> {
    if let Some((name, speech)) = speaker_prefix(line).filter(|(name, _)| is_speaker_name(name)) {
        return vec![
            element("character", trim(name).to_uppercase()),
            element("dialogue", strip_outer_quotes(speech)),
        ];
    }
    // `LEADING_DASH` (`^\s*[—–-]\s+(.+)$`).
    let start = line.trim_start_matches(is_js_whitespace);
    if let Some(dash) = start.chars().next().filter(|d| DASHES.contains(d)) {
        let tail = &start[dash.len_utf8()..];
        let body = tail.trim_start_matches(is_js_whitespace);
        if body.len() != tail.len() && !body.is_empty() {
            let body = trim(body);
            if let Some((said, rest)) = dash_attribution(body) {
                let mut out = vec![
                    element("character", PLACEHOLDER_CHARACTER),
                    element("dialogue", trim(said)),
                ];
                if !trim(rest).is_empty() {
                    out.push(element("action", trim(rest)));
                }
                return out;
            }
            return vec![
                element("character", PLACEHOLDER_CHARACTER),
                element("dialogue", body),
            ];
        }
    }
    let mut out = Vec::new();
    let mut plain = 0;
    let mut i = 0;
    let mut found = false;
    while i < line.len() {
        if let Some((len, spoken)) = quoted_at(&line[i..]) {
            found = true;
            let before = trim(&line[plain..i]);
            if !before.is_empty() {
                out.push(element("action", before));
            }
            if !trim(spoken).is_empty() {
                out.push(element("character", PLACEHOLDER_CHARACTER));
                out.push(element("dialogue", trim(spoken)));
            }
            i += len;
            plain = i;
        } else {
            i += line[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    if found {
        let after = trim(&line[plain..]);
        if !after.is_empty() {
            out.push(element("action", after));
        }
        if !out.is_empty() {
            return out;
        }
    }
    vec![element("action", line)]
}

/// `proseToScreenplayElements`.
pub fn prose_to_screenplay_elements(html: &str) -> Vec<ScreenplayElement> {
    let mut lines = Vec::new();
    collect_lines(&parse_fragment(html), &mut lines);
    let mut elements = vec![element("scene-heading", PLACEHOLDER_HEADING)];
    for line in lines {
        elements.extend(convert_line(&line));
    }
    elements
}

/// `convertDocumentToScreenplay` on a stored scene: its content the screenplay, its format
/// "screenplay", its Markdown source dropped.
pub fn convert_document(document: &mut JsObject) {
    let html = document
        .get("content")
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    document.remove("markdownSource");
    document.insert(
        "content",
        JsValue::str(&elements_to_html(&prose_to_screenplay_elements(&html))),
    );
    document.insert("format", JsValue::str("screenplay"));
}
