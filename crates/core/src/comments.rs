//! Comments (ProseCraft `src/lib/comments.ts` and `comment-dom.ts`, upstream `7dae544`): notes
//! anchored to a passage by its text and the text around it, so they find the passage again
//! after edits. Positions are in the "comment text": every editable text node of the scene
//! joined with nothing between, skipping footnote markers, citations, equations and diagrams.
//! Like JavaScript, these count UTF-16 code units.

use crate::edit::EditError;
use crate::html::{Node, is_js_whitespace, parse_fragment};
use crate::js::{JsObject, JsValue};

/// How many units before and after the quote are kept to tell repeated phrases apart.
pub const CONTEXT_LENGTH: usize = 24;
const MAX_QUOTE_LENGTH: usize = 2000;
const MAX_BODY_LENGTH: usize = 4000;
const MAX_AUTHOR_LENGTH: usize = 60;

/// A passage: its text and what surrounds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub quote: Vec<u16>,
    pub prefix: Vec<u16>,
    pub suffix: Vec<u16>,
}

fn widget(e: &crate::html::Element) -> bool {
    e.attr("contenteditable") == Some("false")
        || e.name == "svg"
        || [
            "math-inline",
            "math-block",
            "mermaid-diagram",
            "calc-worksheet",
        ]
        .iter()
        .any(|c| e.has_class(c))
}

fn collect(nodes: &[Node], out: &mut Vec<u16>) {
    for node in nodes {
        match node {
            Node::Text(t) => out.extend(t.encode_utf16()),
            Node::Element(e) if !widget(e) => collect(&e.children, out),
            Node::Element(_) => {}
        }
    }
}

/// `commentText` for a scene's HTML as the editor shows it.
pub fn comment_text(html: &str) -> Vec<u16> {
    let mut out = Vec::new();
    collect(&parse_fragment(html), &mut out);
    out
}

fn is_blank(units: &[u16]) -> bool {
    char::decode_utf16(units.iter().copied()).all(|c| c.is_ok_and(is_js_whitespace))
}

/// `buildAnchor` for `text[start..end]`; `None` when nothing but whitespace is selected.
pub fn build_anchor(text: &[u16], start: usize, end: usize) -> Option<Anchor> {
    let from = start.min(end).min(text.len());
    let to = start.max(end).min(text.len());
    let quote = &text[from..to];
    if is_blank(quote) {
        return None;
    }
    Some(Anchor {
        quote: quote[..quote.len().min(MAX_QUOTE_LENGTH)].to_vec(),
        prefix: text[from.saturating_sub(CONTEXT_LENGTH)..from].to_vec(),
        suffix: text[to..(to + CONTEXT_LENGTH).min(text.len())].to_vec(),
    })
}

fn common_suffix(a: &[u16], b: &[u16]) -> usize {
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

fn common_prefix(a: &[u16], b: &[u16]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

/// `locateAnchor`: the passage again after edits; with several occurrences, the one whose
/// surroundings match best (the first, on a tie). `None` when the quote is gone.
pub fn locate_anchor(text: &[u16], anchor: &Anchor) -> Option<(usize, usize)> {
    if anchor.quote.is_empty() || anchor.quote.len() > text.len() {
        return None;
    }
    let mut best: Option<(usize, usize)> = None;
    for index in 0..=text.len() - anchor.quote.len() {
        if text[index..index + anchor.quote.len()] != anchor.quote[..] {
            continue;
        }
        let before = &text[index.saturating_sub(CONTEXT_LENGTH)..index];
        let after_start = index + anchor.quote.len();
        let after = &text[after_start..(after_start + CONTEXT_LENGTH).min(text.len())];
        let score = common_suffix(before, &anchor.prefix) + common_prefix(after, &anchor.suffix);
        if best.is_none_or(|(_, s)| score > s) {
            best = Some((index, score));
        }
    }
    best.map(|(start, _)| (start, start + anchor.quote.len()))
}

fn units(text: &str, limit: usize) -> String {
    let u: Vec<u16> = text.encode_utf16().take(limit).collect();
    String::from_utf16_lossy(&u)
}

fn js(units: &[u16]) -> JsValue {
    JsValue::String(crate::js::JsString::from_units(units.to_vec()))
}

fn comments_mut(project: &mut JsValue) -> Result<&mut Vec<JsValue>, EditError> {
    let object = project.as_object_mut().ok_or(EditError::Shape)?;
    if !matches!(object.get("comments"), Some(JsValue::Array(_))) {
        object.insert("comments", JsValue::Array(Vec::new()));
    }
    match object.get_mut("comments") {
        Some(JsValue::Array(list)) => Ok(list),
        _ => Err(EditError::Shape),
    }
}

fn has_id(v: &JsValue, id: &str) -> bool {
    v.get("id")
        .and_then(JsValue::as_js_string)
        .is_some_and(|i| i.eq_str(id))
}

/// `createComment` added to the project: the body trimmed (4000), the author trimmed (60) or
/// "Me".
pub fn add_comment(
    project: &mut JsValue,
    id: &str,
    document_id: &str,
    anchor: &Anchor,
    body: &str,
    author: &str,
    now: f64,
) -> Result<(), EditError> {
    let author = units(author.trim_matches(is_js_whitespace), MAX_AUTHOR_LENGTH);
    let mut c = JsObject::new();
    c.insert("id", JsValue::str(id));
    c.insert("documentId", JsValue::str(document_id));
    c.insert("quote", js(&anchor.quote));
    c.insert("prefix", js(&anchor.prefix));
    c.insert("suffix", js(&anchor.suffix));
    c.insert(
        "body",
        JsValue::str(&units(body.trim_matches(is_js_whitespace), MAX_BODY_LENGTH)),
    );
    c.insert(
        "author",
        JsValue::str(if author.is_empty() { "Me" } else { &author }),
    );
    c.insert("createdAt", JsValue::Number(now));
    c.insert("resolved", JsValue::Bool(false));
    comments_mut(project)?.push(JsValue::Object(c));
    Ok(())
}

/// `setCommentResolved`.
pub fn set_resolved(project: &mut JsValue, id: &str, resolved: bool) -> Result<(), EditError> {
    for c in comments_mut(project)?.iter_mut().filter(|c| has_id(c, id)) {
        if let Some(o) = c.as_object_mut() {
            o.insert("resolved", JsValue::Bool(resolved));
        }
    }
    Ok(())
}

/// `removeComment`.
pub fn delete_comment(project: &mut JsValue, id: &str) -> Result<(), EditError> {
    comments_mut(project)?.retain(|c| !has_id(c, id));
    Ok(())
}

/// Maps between the editor's text (blocks joined by `\n`, markers as U+FFFC, line breaks
/// as U+2028, `<br>` as U+2029) and the comment text (none of those), in UTF-16 units on the
/// comment side.
pub mod editor_text {
    fn kept(c: char) -> bool {
        !matches!(c, '\n' | '\u{fffc}' | '\u{2028}' | '\u{2029}')
    }

    /// The comment-text offset of editor character `index`.
    pub fn to_comment(editor: &str, index: usize) -> usize {
        editor
            .chars()
            .take(index)
            .filter(|c| kept(*c))
            .map(char::len_utf16)
            .sum()
    }

    /// The editor character index of comment-text offset `offset` (the first character at or
    /// after it).
    pub fn to_editor(editor: &str, offset: usize) -> usize {
        let mut units = 0;
        for (i, c) in editor.chars().enumerate() {
            if kept(c) {
                if units >= offset {
                    return i;
                }
                units += c.len_utf16();
            }
        }
        editor.chars().count()
    }

    /// The comment text of the editor's text.
    pub fn comment_text(editor: &str) -> Vec<u16> {
        editor
            .chars()
            .filter(|c| kept(*c))
            .flat_map(|c| {
                let mut b = [0u16; 2];
                c.encode_utf16(&mut b).to_vec()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::editor_text;

    #[test]
    fn maps_editor_positions() {
        let editor = "One\nT\u{fffc}wo 🌊\u{2028}x";
        let comment: Vec<u16> = "OneTwo 🌊x".encode_utf16().collect();
        assert_eq!(editor_text::comment_text(editor), comment);
        assert_eq!(editor_text::to_comment(editor, 4), 3); // "T"
        assert_eq!(editor_text::to_comment(editor, 6), 4); // past the marker: "w"
        assert_eq!(editor_text::to_editor(editor, 3), 4);
        assert_eq!(editor_text::to_editor(editor, 4), 6);
        assert_eq!(editor_text::to_editor(editor, 9), 11); // "x", after the emoji (2 units)
    }
}
