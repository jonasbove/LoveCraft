//! Scene HTML as Markdown, as ProseCraft writes it (`src/lib/publish/markdown.ts`), for the
//! Markdown export and open sync folders.

mod turndown;

use crate::html::sanitize::is_valid_footnote_id;
use crate::html::{is_js_whitespace, parse_fragment, sanitize_inline_html};

pub use turndown::turndown_nodes;

/// A footnote's id and its inline HTML.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FootnoteDef {
    pub id: String,
    pub html: String,
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

/// `htmlToInlineMarkdown`: footnote HTML, sanitized to inline elements, as one Markdown line.
pub fn html_to_inline_markdown(html: &str) -> String {
    // Through the string, as in ProseCraft: parsing it again joins the text that unwrapped
    // elements left side by side.
    let clean = sanitize_inline_html(html);
    if clean.is_empty() {
        return String::new();
    }
    js_trim(&turndown_nodes(parse_fragment(&clean))).to_owned()
}

/// A footnote definition line: `[^id]: text`, continuation lines indented.
pub fn footnote_definition(label: &str, html: &str) -> String {
    let text = html_to_inline_markdown(html);
    if text.is_empty() {
        format!("[^{label}]:")
    } else {
        format!("[^{label}]: {}", text.replace('\n', "\n    "))
    }
}

/// `htmlToMarkdown`: a scene as Markdown, with its footnote definitions after it.
pub fn html_to_markdown(html: &str, footnotes: &[FootnoteDef]) -> String {
    let body = js_trim(&turndown_nodes(parse_fragment(html))).to_owned();
    let definitions: Vec<String> = footnotes
        .iter()
        .filter(|f| is_valid_footnote_id(&f.id))
        .map(|f| footnote_definition(&f.id, &f.html))
        .collect();
    if definitions.is_empty() {
        return body;
    }
    if body.is_empty() {
        definitions.join("\n")
    } else {
        format!("{body}\n\n{}", definitions.join("\n"))
    }
}
