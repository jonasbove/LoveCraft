//! Screenplay elements as editor HTML (ProseCraft `src/lib/screenplay/layout.ts`, upstream
//! `7dae544`): one paragraph per element, marked with `data-sp`.

use super::fountain::ScreenplayElement;
use crate::html::{Node, is_js_whitespace, parse_fragment};

const ELEMENT_TYPES: [&str; 11] = [
    "scene-heading",
    "action",
    "character",
    "dialogue",
    "parenthetical",
    "transition",
    "centered",
    "section",
    "synopsis",
    "note",
    "page-break",
];

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `EMPHASIS` at `text[i..]`: the match's length and its HTML.
fn emphasis_at(text: &str) -> Option<(usize, String)> {
    let run = |marker: &str, stop: char| -> Option<(usize, &str)> {
        let rest = text.strip_prefix(marker)?;
        let inner_len = rest.find(stop).unwrap_or(rest.len());
        if inner_len == 0 {
            return None;
        }
        rest[inner_len..]
            .starts_with(marker)
            .then(|| (marker.len() * 2 + inner_len, &rest[..inner_len]))
    };
    if let Some((len, inner)) = run("***", '*') {
        return Some((
            len,
            format!("<strong><em>{}</em></strong>", escape_html(inner)),
        ));
    }
    if let Some((len, inner)) = run("**", '*') {
        return Some((len, format!("<strong>{}</strong>", escape_html(inner))));
    }
    if let Some((len, inner)) = run("*", '*') {
        return Some((len, format!("<em>{}</em>", escape_html(inner))));
    }
    if let Some((len, inner)) = run("_", '_') {
        return Some((len, format!("<u>{}</u>", escape_html(inner))));
    }
    None
}

/// `inlineToHtml`: emphasis as tags, the rest escaped.
fn inline_to_html(text: &str) -> String {
    let mut html = String::new();
    let mut plain = 0;
    let mut i = 0;
    while i < text.len() {
        if let Some((len, tagged)) = emphasis_at(&text[i..]) {
            html.push_str(&escape_html(&text[plain..i]));
            html.push_str(&tagged);
            i += len;
            plain = i;
        } else {
            i += text[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    html.push_str(&escape_html(&text[plain..]));
    html
}

/// `elementsToHtml`.
pub fn elements_to_html(elements: &[ScreenplayElement]) -> String {
    elements
        .iter()
        .map(|e| format!(r#"<p data-sp="{}">{}</p>"#, e.kind, inline_to_html(&e.text)))
        .collect()
}

fn inline_from_html(node: &Node) -> String {
    match node {
        Node::Text(t) => t.clone(),
        Node::Element(e) => {
            let inner: String = e.children.iter().map(inline_from_html).collect();
            match e.name.as_str() {
                "em" | "i" => format!("*{inner}*"),
                "strong" | "b" => format!("**{inner}**"),
                "u" => format!("_{inner}_"),
                "br" => "\n".into(),
                _ => inner,
            }
        }
    }
}

/// `htmlToElements`: editor HTML back to elements; unknown or unmarked paragraphs are action.
pub fn html_to_elements(html: &str) -> Vec<ScreenplayElement> {
    parse_fragment(html)
        .iter()
        .filter_map(|node| match node {
            Node::Element(e) => {
                let kind = e
                    .attr("data-sp")
                    .filter(|k| ELEMENT_TYPES.contains(k))
                    .unwrap_or("action");
                Some(ScreenplayElement::new(
                    kind,
                    inline_from_html(node).trim_matches(is_js_whitespace),
                ))
            }
            Node::Text(_) => None,
        })
        .collect()
}
