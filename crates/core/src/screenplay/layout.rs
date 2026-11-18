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

/// The printed page: 12 pt Courier, 55 lines; column widths in characters.
pub const LINES_PER_PAGE: usize = 55;
const ACTION_WIDTH: usize = 60;
const DIALOGUE_WIDTH: usize = 35;
const PARENTHETICAL_WIDTH: usize = 25;

/// `wrapScreenplayText`: greedy word wrap at a width in characters (UTF-16 units).
pub fn wrap_screenplay_text(text: &str, width: usize) -> Vec<String> {
    let mut wrapped = Vec::new();
    for line in text.split('\n') {
        let words: Vec<&str> = line
            .split(is_js_whitespace)
            .filter(|w| !w.is_empty())
            .collect();
        if words.is_empty() {
            wrapped.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in words {
            let length = |s: &str| s.encode_utf16().count();
            if current.is_empty() {
                current = word.to_owned();
            } else if length(&current) + 1 + length(word) <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                wrapped.push(std::mem::replace(&mut current, word.to_owned()));
            }
        }
        wrapped.push(current);
    }
    wrapped
}

const PRINTED: [&str; 7] = [
    "scene-heading",
    "action",
    "character",
    "dialogue",
    "parenthetical",
    "transition",
    "centered",
];

fn content_lines(e: &ScreenplayElement) -> Vec<String> {
    match e.kind.as_str() {
        "action" | "scene-heading" => wrap_screenplay_text(&e.text, ACTION_WIDTH),
        "dialogue" => wrap_screenplay_text(&e.text, DIALOGUE_WIDTH),
        "parenthetical" => wrap_screenplay_text(&e.text, PARENTHETICAL_WIDTH),
        "character" | "transition" | "centered" => vec![e.text.clone()],
        _ => Vec::new(),
    }
}

fn next_printed(elements: &[ScreenplayElement], index: usize) -> Option<&ScreenplayElement> {
    for e in &elements[index + 1..] {
        if e.kind == "page-break" {
            return None;
        }
        if PRINTED.contains(&e.kind.as_str()) {
            return Some(e);
        }
    }
    None
}

/// `requiredLines`: an element's lines, with what must share its page (a heading or cue is
/// never left alone at the bottom).
fn required_lines(
    e: &ScreenplayElement,
    content: usize,
    following: Option<&ScreenplayElement>,
) -> usize {
    let mut required = content;
    if e.kind == "scene-heading" {
        required += 1;
        if let Some(f) = following {
            required += if f.kind == "character" { 2 } else { 1 };
        }
    } else if e.kind == "character"
        && following.is_some_and(|f| f.kind == "dialogue" || f.kind == "parenthetical")
    {
        required += 1;
    }
    required
}

/// A scene's place in the estimate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenePages {
    pub heading: String,
    pub start_page: usize,
    pub pages: usize,
}

/// `estimatePages`: paper pages, lines, and each scene's pages.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageEstimate {
    pub pages: usize,
    pub lines: usize,
    pub by_scene: Vec<ScenePages>,
}

pub fn estimate_pages(elements: &[ScreenplayElement]) -> PageEstimate {
    let mut by_scene = Vec::new();
    let mut scene: Option<(String, usize, usize)> = None;
    let (mut page, mut line, mut lines, mut last_page) = (1, 1, 0, 1);
    let mut separated = false;
    let mut previous: Option<&ScreenplayElement> = None;
    let close = |scene: &mut Option<(String, usize, usize)>, by_scene: &mut Vec<ScenePages>| {
        if let Some((heading, start, last)) = scene.take() {
            by_scene.push(ScenePages {
                heading,
                start_page: start,
                pages: last - start + 1,
            });
        }
    };
    for (index, current) in elements.iter().enumerate() {
        if current.kind == "page-break" {
            page += 1;
            line = 1;
            separated = false;
            previous = None;
            continue;
        }
        if !PRINTED.contains(&current.kind.as_str()) {
            continue;
        }
        let continues = matches!(current.kind.as_str(), "dialogue" | "parenthetical")
            && previous.is_some_and(|p| {
                matches!(p.kind.as_str(), "character" | "dialogue" | "parenthetical")
            });
        if !continues && separated && line > 1 {
            lines += 1;
            line += 1;
            if line > LINES_PER_PAGE {
                page += 1;
                line = 1;
            }
        }
        let content = content_lines(current);
        let required = required_lines(current, content.len(), next_printed(elements, index));
        if matches!(current.kind.as_str(), "scene-heading" | "character")
            && line + required - 1 > LINES_PER_PAGE
        {
            page += 1;
            line = 1;
        }
        let start = page;
        for _ in 0..content.len() {
            if line > LINES_PER_PAGE {
                page += 1;
                line = 1;
            }
            lines += 1;
            last_page = page;
            line += 1;
        }
        if current.kind == "scene-heading" {
            close(&mut scene, &mut by_scene);
            scene = Some((current.text.clone(), start, last_page));
        } else if let Some(s) = scene.as_mut() {
            s.2 = last_page;
        }
        separated = true;
        previous = Some(current);
    }
    close(&mut scene, &mut by_scene);
    PageEstimate {
        pages: if lines == 0 { 0 } else { last_page },
        lines,
        by_scene,
    }
}

/// `estimateRuntime`: about a minute a page.
pub fn estimate_runtime(pages: usize) -> usize {
    pages
}
