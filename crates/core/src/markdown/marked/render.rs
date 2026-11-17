//! marked's `Parser`, `Renderer` and `TextRenderer` (`src/Parser.ts`, `src/Renderer.ts`,
//! `src/TextRenderer.ts`), with ProseCraft's renderers for its extension tokens and for
//! diagram and calculation code blocks.

use super::helpers::{clean_url, escape_html_entities, js_trim};
use super::lexer::{Kind, Token};
use super::rules as r;
use crate::cite::{CITE_REF_CLASS, cite_json};
use crate::html::sanitize::{FOOTNOTE_REF_CLASS, is_valid_footnote_id};

const MATH_INLINE_CLASS: &str = "math-inline";
const MATH_BLOCK_CLASS: &str = "math-block";
const DIAGRAM_CLASS: &str = "mermaid-diagram";
const CALC_CLASS: &str = "calc-worksheet";

/// ProseCraft's `escapeAttribute`.
fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `Parser.parse`: block tokens as HTML.
pub(crate) fn parse(tokens: &[Token]) -> String {
    let mut out = String::new();
    for token in tokens {
        match token.kind {
            Kind::Heading => out.push_str(&format!(
                "<h{d}>{}</h{d}>\n",
                parse_inline(&token.tokens, false),
                d = token.depth
            )),
            Kind::Hr => out.push_str("<hr>\n"),
            Kind::Code => out.push_str(&code(token)),
            Kind::Table => out.push_str(&table(token)),
            Kind::Blockquote => {
                out.push_str(&format!(
                    "<blockquote>\n{}</blockquote>\n",
                    parse(&token.tokens)
                ));
            }
            Kind::List => out.push_str(&list(token)),
            Kind::Checkbox => out.push_str(&checkbox(token)),
            Kind::Html => out.push_str(&token.text),
            Kind::Paragraph => {
                out.push_str(&format!("<p>{}</p>\n", parse_inline(&token.tokens, false)));
            }
            Kind::Text => out.push_str(&text(token)),
            Kind::MathBlock => out.push_str(&format!(
                "<div class=\"{MATH_BLOCK_CLASS}\" data-tex=\"{}\"></div>\n",
                escape_attribute(&token.text)
            )),
            // `space`, `def` and ProseCraft's `footnoteDef` render as nothing.
            _ => {}
        }
    }
    out
}

/// `Parser.parseInline`, with the `TextRenderer` when `text_only` (an image's alt text).
pub(crate) fn parse_inline(tokens: &[Token], text_only: bool) -> String {
    let mut out = String::new();
    for token in tokens {
        // Extension renderers apply with either renderer.
        match token.kind {
            Kind::FootnoteRef => {
                if is_valid_footnote_id(&token.id) {
                    out.push_str(&format!(
                        "<sup class=\"{FOOTNOTE_REF_CLASS}\" data-footnote-id=\"{}\" contenteditable=\"false\"></sup>",
                        escape_attribute(&token.id)
                    ));
                }
                continue;
            }
            Kind::CiteRef => {
                if !token.cites.is_empty() {
                    out.push_str(&format!(
                        "<span class=\"{CITE_REF_CLASS}\" data-cites=\"{}\" contenteditable=\"false\"></span>",
                        escape_attribute(&cite_json(&token.cites))
                    ));
                }
                continue;
            }
            Kind::MathInline => {
                let class = if token.display {
                    MATH_BLOCK_CLASS
                } else {
                    MATH_INLINE_CLASS
                };
                out.push_str(&format!(
                    "<span class=\"{class}\" data-tex=\"{}\"></span>",
                    escape_attribute(&token.text)
                ));
                continue;
            }
            _ => {}
        }
        if text_only {
            match token.kind {
                Kind::Checkbox => out.push_str(&token.raw),
                Kind::Br => {}
                _ => out.push_str(&token.text),
            }
            continue;
        }
        match token.kind {
            Kind::Escape | Kind::Text => out.push_str(&text(token)),
            Kind::Html => out.push_str(&token.text),
            Kind::Link => out.push_str(&link(token)),
            Kind::Image => out.push_str(&image(token)),
            Kind::Checkbox => out.push_str(&checkbox(token)),
            Kind::Strong => out.push_str(&format!(
                "<strong>{}</strong>",
                parse_inline(&token.tokens, false)
            )),
            Kind::Em => out.push_str(&format!("<em>{}</em>", parse_inline(&token.tokens, false))),
            Kind::Codespan => {
                out.push_str(&format!(
                    "<code>{}</code>",
                    escape_html_entities(&token.text, true)
                ));
            }
            Kind::Br => out.push_str("<br>"),
            Kind::Del => out.push_str(&format!(
                "<del>{}</del>",
                parse_inline(&token.tokens, false)
            )),
            _ => {}
        }
    }
    out
}

/// ProseCraft's `code` renderer: diagram and calculation blocks as placeholders, any other
/// block as marked renders it.
fn code(token: &Token) -> String {
    let lang = js_trim(&token.lang).to_lowercase();
    if lang == "mermaid" {
        return format!(
            "<div class=\"{DIAGRAM_CLASS}\" data-source=\"{}\"></div>\n",
            escape_attribute(&token.text)
        );
    }
    if lang == "calc" || lang == "mathcad" {
        return format!(
            "<div class=\"{CALC_CLASS}\" data-source=\"{}\"></div>\n",
            escape_attribute(&token.text)
        );
    }
    let lang_string = r::other_not_space_start()
        .exec(&token.lang)
        .map(|m| m.whole())
        .unwrap_or_default();
    let code = if token.text.is_empty() {
        String::new()
    } else {
        format!(
            "{}\n",
            r::other_ending_newline().replace(&token.text, false, "")
        )
    };
    let body = escape_html_entities(&code, true);
    if lang_string.is_empty() {
        format!("<pre><code>{body}</code></pre>\n")
    } else {
        format!(
            "<pre><code class=\"language-{}\">{body}</code></pre>\n",
            escape_html_entities(lang_string, false)
        )
    }
}

fn list(token: &Token) -> String {
    let body: String = token
        .items
        .iter()
        .map(|item| format!("<li>{}</li>\n", parse(&item.tokens)))
        .collect();
    let kind = if token.ordered { "ol" } else { "ul" };
    let start = if token.ordered && token.start != 1 {
        format!(" start=\"{}\"", token.start)
    } else {
        String::new()
    };
    format!("<{kind}{start}>\n{body}</{kind}>\n")
}

fn checkbox(token: &Token) -> String {
    let checked = if token.checked { "checked=\"\" " } else { "" };
    format!("<input {checked}disabled=\"\" type=\"checkbox\"> ")
}

fn table_cell(cell: &Token, header: bool) -> String {
    let content = parse_inline(&cell.tokens, false);
    let kind = if header { "th" } else { "td" };
    match cell.align.first().copied().flatten() {
        Some(align) => format!("<{kind} align=\"{align}\">{content}</{kind}>\n"),
        None => format!("<{kind}>{content}</{kind}>\n"),
    }
}

fn table(token: &Token) -> String {
    let header_cells: String = token.header.iter().map(|c| table_cell(c, true)).collect();
    let header = format!("<tr>\n{header_cells}</tr>\n");
    let mut body: String = token
        .rows
        .iter()
        .map(|row| {
            let cells: String = row.iter().map(|c| table_cell(c, false)).collect();
            format!("<tr>\n{cells}</tr>\n")
        })
        .collect();
    if !body.is_empty() {
        body = format!("<tbody>{body}</tbody>");
    }
    format!("<table>\n<thead>\n{header}</thead>\n{body}</table>\n")
}

fn text(token: &Token) -> String {
    if token.has_tokens {
        parse_inline(&token.tokens, false)
    } else if token.escaped {
        token.text.clone()
    } else {
        escape_html_entities(&token.text, false)
    }
}

fn link(token: &Token) -> String {
    let text = if token.autolink {
        escape_html_entities(&token.text, true)
    } else {
        parse_inline(&token.tokens, false)
    };
    let href = escape_html_entities(&clean_url(&token.href), token.autolink);
    let mut out = format!("<a href=\"{href}\"");
    if let Some(title) = token.title.as_deref().filter(|t| !t.is_empty()) {
        out.push_str(&format!(
            " title=\"{}\"",
            escape_html_entities(title, false)
        ));
    }
    out.push_str(&format!(">{text}</a>"));
    out
}

fn image(token: &Token) -> String {
    let text = parse_inline(&token.tokens, true);
    let href = clean_url(&token.href);
    let mut out = format!(
        "<img src=\"{}\" alt=\"{}\"",
        escape_html_entities(&href, false),
        escape_html_entities(&text, false)
    );
    if let Some(title) = token.title.as_deref().filter(|t| !t.is_empty()) {
        out.push_str(&format!(
            " title=\"{}\"",
            escape_html_entities(title, false)
        ));
    }
    out.push('>');
    out
}
