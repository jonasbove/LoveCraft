//! Scene HTML as Markdown, as ProseCraft writes it (`src/lib/publish/markdown.ts`), for the
//! Markdown export and open sync folders.

pub(crate) mod marked;
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

/// `markdownToHtml`: Markdown as HTML, as ProseCraft's configured marked renders it. Raw HTML
/// in the Markdown passes through: callers sanitize it, as ProseCraft's do.
pub fn markdown_to_html(markdown: &str) -> String {
    marked::parse(markdown)
}

/// Scene HTML and the footnote definitions taken out of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownDocument {
    pub html: String,
    pub footnotes: Vec<FootnoteDef>,
}

/// `markdownToDocument`: the body as HTML, and each `[^id]:` definition's text as sanitized
/// inline HTML. A repeated id keeps its first place and its last text.
pub fn markdown_to_document(markdown: &str) -> MarkdownDocument {
    let tokens = marked::lex(markdown);
    let mut footnotes: Vec<FootnoteDef> = Vec::new();
    for token in tokens
        .iter()
        .filter(|t| t.kind == marked::Kind::FootnoteDef)
    {
        if !is_valid_footnote_id(&token.id) {
            continue;
        }
        let html = if token.text.is_empty() {
            String::new()
        } else {
            sanitize_inline_html(&marked::parse_inline(&token.text))
        };
        match footnotes.iter_mut().find(|f| f.id == token.id) {
            Some(existing) => existing.html = html,
            None => footnotes.push(FootnoteDef {
                id: token.id.clone(),
                html,
            }),
        }
    }
    MarkdownDocument {
        html: marked::parse_tokens(&tokens),
        footnotes,
    }
}

#[cfg(test)]
mod table_tests {
    //! `markdown tables round trip` (jennraeross/ProseCraft#60).
    use super::{html_to_markdown, markdown_to_html};

    fn md(html: &str) -> String {
        html_to_markdown(html, &[])
    }

    fn round_trip(html: &str) -> String {
        crate::html::sanitize_html(&markdown_to_html(&md(html)))
    }

    /// The structure without the newlines marked puts between tags.
    fn compact(html: &str) -> String {
        let mut out = String::new();
        let mut chars = html.chars().peekable();
        while let Some(c) = chars.next() {
            out.push(c);
            if c == '>' {
                let mut newlines = String::new();
                while chars.peek() == Some(&'\n') {
                    newlines.push('\n');
                    chars.next();
                }
                if chars.peek() != Some(&'<') {
                    out.push_str(&newlines);
                }
            }
        }
        out.trim().to_owned()
    }

    #[test]
    fn a_table_with_a_header_row_is_a_pipe_table_that_reads_back() {
        let html = "<p>Before.</p><table><thead><tr><th>Name</th><th>Role</th></tr></thead><tbody><tr><td>Mara</td><td><em>Keeper</em></td></tr><tr><td>a | b</td><td><a href=\"https://example.test/l\">l</a></td></tr></tbody></table><p>After.</p>";
        assert_eq!(
            md(html),
            "Before.\n\n| Name | Role |\n| --- | --- |\n| Mara | _Keeper_ |\n| a \\| b | [l](https://example.test/l) |\n\nAfter."
        );
        assert_eq!(compact(&round_trip(html)), html);
        let note = "<table><thead><tr><th>H</th></tr></thead><tbody><tr><td>n<sup class=\"footnote-ref\" data-footnote-id=\"fn-1\" contenteditable=\"false\"></sup></td></tr></tbody></table>";
        assert_eq!(md(note), "| H |\n| --- |\n| n[^fn-1] |");
        assert_eq!(compact(&round_trip(note)), note);
    }

    #[test]
    fn any_other_table_is_kept_as_html() {
        for html in [
            "<table><tbody><tr><td>no</td><td>header</td></tr><tr><td>x</td><td>y</td></tr></tbody></table>",
            "<table><tbody><tr><th>H</th></tr><tr><td><p>one</p><p>two</p></td></tr></tbody></table>",
            "<table><tbody><tr><th>H</th></tr><tr><td>a<br>b</td></tr></tbody></table>",
            "<table><tbody><tr><th>A</th><th>B</th></tr><tr><td>only one</td></tr></tbody></table>",
            "<table><thead><tr><th>H</th></tr></thead><tbody><tr><td><h2>Heading</h2></td></tr></tbody></table>",
            "<table><thead><tr><th>H</th></tr></thead><tbody><tr><td><ul><li>a</li></ul></td></tr></tbody></table>",
            "<table><thead><tr><th>H</th></tr></thead><tbody><tr><td><p>one</p></td></tr></tbody></table>",
            "<table><thead><tr><th>H</th></tr></thead><tbody><tr><td><code>x\\|y</code></td></tr></tbody></table>",
            "<table><tbody><tr><td>n<sup class=\"footnote-ref\" data-footnote-id=\"fn-1\" contenteditable=\"false\"></sup></td></tr></tbody></table>",
        ] {
            assert_eq!(md(html), html);
            assert_eq!(round_trip(html), html);
        }
        let blank = "<table><tbody><tr><td><pre><code>a\n\nb</code></pre></td></tr></tbody></table><p><em>after</em></p>";
        assert_eq!(compact(&round_trip(blank)), blank);
    }
}
