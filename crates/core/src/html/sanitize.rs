//! ProseCraft's sanitizer (`src/lib/sanitize.ts`): scene HTML against a small allowlist, as
//! it is stored, previewed and exported. Unknown elements are unwrapped (their children
//! kept); equations, diagrams, footnote markers and citations are rebuilt from their
//! validated source; links keep only web and mail addresses; images only embedded PNG,
//! JPEG or WebP data.
//!
//! One deliberate difference: `<script>`, `<style>` and `<template>` content is dropped
//! wherever it is. (A browser drops it before the first content, which goes in the
//! document head, but keeps a later `<style>`'s text as text.)

use super::{Element, Node, image_layout, is_js_whitespace, parse_fragment, serialize};
use crate::cite::{CITE_REF_CLASS, normalize_cite_data_attr};

const SAFE: &[&str] = &[
    "a",
    "b",
    "blockquote",
    "br",
    "code",
    "del",
    "div",
    "em",
    "h1",
    "h2",
    "h3",
    "hr",
    "i",
    "img",
    "li",
    "ol",
    "p",
    "pre",
    "s",
    "strike",
    "strong",
    "table",
    "tbody",
    "td",
    "th",
    "thead",
    "tr",
    "u",
    "ul",
];
const INLINE_SAFE: &[&str] = &[
    "a", "b", "br", "code", "del", "em", "i", "s", "strike", "strong", "u",
];
const SCREENPLAY: &[&str] = &[
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

pub const FOOTNOTE_REF_CLASS: &str = "footnote-ref";
const MAX_EMBEDDED_IMAGE_BYTES: usize = 1_500_000;

/// `FOOTNOTE_ID_RE`: 1–32 of `A–Z a–z 0–9 _ -`.
pub fn is_valid_footnote_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `isEmbeddedImageSrc`: a base64 PNG, JPEG or WebP within the export limit.
pub fn is_embedded_image_src(src: &str) -> bool {
    let data = ["png", "jpeg", "webp"]
        .iter()
        .find_map(|kind| src.strip_prefix(&format!("data:image/{kind};base64,")));
    let Some(data) = data else { return false };
    let body = data.trim_end_matches('=');
    let padding = data.len() - body.len();
    !body.is_empty()
        && padding <= 2
        && body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
        && data.len() <= MAX_EMBEDDED_IMAGE_BYTES.div_ceil(3) * 4
}

/// The rich-content kinds: class, element, source attribute, source limit.
pub(crate) const RICH: [(&str, &str, &str, usize); 4] = [
    ("math-inline", "span", "data-tex", 4_000),
    ("math-block", "div", "data-tex", 4_000),
    ("mermaid-diagram", "div", "data-source", 20_000),
    ("calc-worksheet", "div", "data-source", 8_000),
];

/// The first `n` UTF-16 code units of `text` (`slice(0, n)`), not splitting a character.
fn js_slice(text: &str, n: usize) -> String {
    let mut units = 0;
    let mut out = String::new();
    for c in text.chars() {
        units += c.len_utf16();
        if units > n {
            break;
        }
        out.push(c);
    }
    out
}

/// Whether `new URL(href, page)` parses and has an http, https or mailto protocol. A relative
/// address resolves against the page, which is http(s). An address the URL Standard rejects (a
/// host with a forbidden character, a port out of range) throws in ProseCraft, so its link is
/// kept as plain text.
fn is_safe_href(href: &str) -> bool {
    let Ok(page) = url::Url::parse("http://localhost/") else {
        return false;
    };
    url::Url::options()
        .base_url(Some(&page))
        .parse(href)
        .is_ok_and(|parsed| ["http", "https", "mailto"].contains(&parsed.scheme()))
}

fn element(name: &str, attrs: Vec<(String, String)>, children: Vec<Node>) -> Node {
    Node::Element(Element {
        name: name.to_owned(),
        attrs,
        children,
    })
}

fn attr(key: &str, value: &str) -> (String, String) {
    (key.to_owned(), value.to_owned())
}

fn clean_children(children: &[Node], inline: bool, out: &mut Vec<Node>) {
    for child in children {
        clean(child, inline, out);
    }
}

/// `cleanNode`: appends what `node` becomes (nothing, itself, or its unwrapped children).
fn clean(node: &Node, inline: bool, out: &mut Vec<Node>) {
    let e = match node {
        Node::Text(text) => {
            out.push(Node::Text(text.clone()));
            return;
        }
        Node::Element(e) => e,
    };
    // SVG and MathML aren't HTML elements: the sanitizer drops them with their content.
    if e.name == "svg" || e.name == "math" {
        return;
    }
    if let Some((index, (class, tag, source_attr, limit))) = RICH
        .iter()
        .enumerate()
        .find(|(_, (class, ..))| e.has_class(class))
    {
        if inline && index != 0 {
            return;
        }
        let source = e
            .attr(source_attr)
            .unwrap_or_default()
            .trim_matches(is_js_whitespace);
        if !source.is_empty() {
            out.push(element(
                tag,
                vec![
                    attr("class", class),
                    attr(source_attr, &js_slice(source, *limit)),
                ],
                Vec::new(),
            ));
        }
        return;
    }
    if e.name == "sup" && e.has_class(FOOTNOTE_REF_CLASS) {
        let id = e.attr("data-footnote-id").unwrap_or_default();
        if !inline && is_valid_footnote_id(id) {
            out.push(element(
                "sup",
                vec![
                    attr("class", FOOTNOTE_REF_CLASS),
                    attr("data-footnote-id", id),
                    attr("contenteditable", "false"),
                ],
                Vec::new(),
            ));
        }
        return;
    }
    if e.name == "span" && e.has_class(CITE_REF_CLASS) {
        if let Some(data) = normalize_cite_data_attr(e.attr("data-cites")) {
            out.push(element(
                "span",
                vec![
                    attr("class", CITE_REF_CLASS),
                    attr("data-cites", &data),
                    attr("contenteditable", "false"),
                ],
                Vec::new(),
            ));
        }
        return;
    }
    if e.name == "div" && e.has_class("scene-break") {
        if inline {
            return;
        }
        let mut class = "scene-break".to_owned();
        for extra in ["scene-break-line", "scene-break-blank"] {
            if e.has_class(extra) {
                class.push(' ');
                class.push_str(extra);
            }
        }
        let mut children = Vec::new();
        clean_children(&e.children, inline, &mut children);
        out.push(element(
            "div",
            vec![attr("class", &class), attr("contenteditable", "false")],
            children,
        ));
        return;
    }
    let allowed = if inline { INLINE_SAFE } else { SAFE };
    if !allowed.contains(&e.name.as_str()) {
        clean_children(&e.children, inline, out);
        return;
    }
    let name = if e.name == "strike" || e.name == "del" {
        "s"
    } else {
        e.name.as_str()
    };
    let mut attrs = Vec::new();
    if e.name == "p" {
        if !inline && e.has_class("eyebrow") {
            attrs.push(attr("class", "eyebrow"));
        }
        if let Some(kind) = e.attr("data-sp").filter(|k| SCREENPLAY.contains(k)) {
            attrs.push(attr("data-sp", kind));
        }
    }
    if e.name == "a"
        && let Some(href) = e.attr("href").filter(|h| !h.is_empty() && is_safe_href(h))
    {
        attrs.push(attr("href", href));
    }
    if e.name == "img" {
        let src = e.attr("src").unwrap_or_default();
        if !is_embedded_image_src(src) {
            return;
        }
        attrs.push(attr("src", src));
        attrs.push(attr(
            "alt",
            &js_slice(e.attr("alt").unwrap_or_default(), 500),
        ));
        for dimension in ["width", "height"] {
            // Number(null) is 0, which isn't kept.
            let value = e.attr(dimension).map_or(0.0, crate::js::string_to_number);
            if value.fract() == 0.0 && value > 0.0 && value <= 2400.0 {
                attrs.push(attr(dimension, &crate::js::number_to_string(value)));
            }
        }
        attrs.extend(image_layout::attributes(&image_layout::read(e)));
    }
    let mut children = Vec::new();
    clean_children(&e.children, inline, &mut children);
    out.push(element(name, attrs, children));
}

/// The sanitized tree of `nodes`.
pub fn sanitize_nodes(nodes: &[Node], inline: bool) -> Vec<Node> {
    let mut out = Vec::new();
    clean_children(nodes, inline, &mut out);
    out
}

/// The body ProseCraft's `DOMParser.parseFromString(html, "text/html")` builds: as a fragment,
/// except that whitespace before the first element or text is dropped, since a document
/// ignores it (along with stray end tags and comments) until its body begins.
fn document_body(html: &str) -> Vec<Node> {
    let mut nodes = parse_fragment(html);
    let is_html_space = |c: char| matches!(c, '\t' | '\n' | '\x0c' | '\r' | ' ');
    while let Some(Node::Text(text)) = nodes.first_mut() {
        let kept = text.trim_start_matches(is_html_space);
        if kept.is_empty() {
            nodes.remove(0);
        } else {
            *text = kept.to_owned();
            break;
        }
    }
    nodes
}

/// `sanitizeHtml`: scene HTML against the editor's allowlist.
pub fn sanitize_html(html: &str) -> String {
    serialize(&sanitize_nodes(&document_body(html), false))
}

/// `sanitizeInlineHtml`: footnote HTML against the inline allowlist (no headings, images,
/// tables, scene breaks or footnote markers).
pub fn sanitize_inline_html(html: &str) -> String {
    serialize(&sanitize_nodes(&document_body(html), true))
}
