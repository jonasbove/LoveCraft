//! Plain text and word counts of scene HTML.
//!
//! Ported from ProseCraft `src/lib/publish/content.ts` (`htmlToPlainText`, `collectBlocks`,
//! `inlineText`, `countWords`) at upstream `7dae544`, including JavaScript's whitespace rules.

use super::{Element, Node, parse_fragment};

const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "div",
    "figcaption",
    "figure",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "ul",
];
const RICH_CLASSES: &[&str] = &[
    "math-inline",
    "math-block",
    "mermaid-diagram",
    "calc-worksheet",
];
const FOOTNOTE_REF_CLASS: &str = "footnote-ref";

/// How equations, diagrams and calculations appear in the text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RichText {
    /// Their source: TeX, diagram source, calculation source (text exports).
    #[default]
    Source,
    /// Left out (word counts).
    Omit,
}

#[derive(Clone, Debug, Default)]
pub struct PlainTextOptions {
    pub rich: RichText,
    /// Footnote id → its number in the book: references become `[n]` (`footnoteNumbers`).
    pub footnote_numbers: Option<std::collections::HashMap<String, usize>>,
}

impl PlainTextOptions {
    fn footnote(&self, element: &Element) -> String {
        let id = element.attr("data-footnote-id").unwrap_or_default();
        self.footnote_numbers
            .as_ref()
            .and_then(|m| m.get(id))
            .map(|n| format!("[{n}]"))
            .unwrap_or_default()
    }
}

/// JavaScript's `\s` (and what `String.prototype.trim` removes): WhiteSpace and LineTerminator.
/// Not quite Rust's `char::is_whitespace`: U+FEFF is in, U+0085 is out.
pub fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

/// `text.trim().match(/\S+/g)?.length ?? 0`.
pub fn count_words(text: &str) -> usize {
    text.split(is_js_whitespace)
        .filter(|word| !word.is_empty())
        .count()
}

/// ProseCraft's word count for a scene: plain text without equations and diagrams.
pub fn html_word_count(html: &str) -> usize {
    count_words(&html_to_plain_text(
        html,
        &PlainTextOptions {
            rich: RichText::Omit,
            ..Default::default()
        },
    ))
}

/// `replace(/\s+/g, " ")`
fn collapse_js_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if is_js_whitespace(c) {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// `buffer.replace(/[ \t]+/g, " ").replace(/ *\n */g, "\n").trim()`
fn clean_block(buffer: &str) -> String {
    let mut spaced = String::with_capacity(buffer.len());
    let mut in_run = false;
    for c in buffer.chars() {
        if c == ' ' || c == '\t' {
            if !in_run {
                spaced.push(' ');
            }
            in_run = true;
        } else {
            spaced.push(c);
            in_run = false;
        }
    }
    let lines: Vec<&str> = spaced.split('\n').collect();
    let last = lines.len().saturating_sub(1);
    let joined: Vec<&str> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let line = if i > 0 {
                line.trim_start_matches(' ')
            } else {
                line
            };
            if i < last {
                line.trim_end_matches(' ')
            } else {
                line
            }
        })
        .collect();
    js_trim(&joined.join("\n")).to_owned()
}

fn is_rich(element: &Element) -> bool {
    RICH_CLASSES.iter().any(|class| element.has_class(class))
}

fn rich_representation(element: &Element, mode: RichText) -> Option<String> {
    if !is_rich(element) {
        return None;
    }
    if mode == RichText::Omit {
        return Some(String::new());
    }
    let tex = || element.attr("data-tex").unwrap_or_default();
    let source = || element.attr("data-source").unwrap_or_default();
    Some(if element.has_class("math-inline") {
        format!("${}$", tex())
    } else if element.has_class("math-block") {
        format!("$$\n{}\n$$", tex())
    } else if element.has_class("calc-worksheet") {
        // ProseCraft prints the evaluated worksheet here; until the calculation engine is
        // ported, its source stands in. Word counts (`Omit`) are unaffected.
        source().to_owned()
    } else {
        format!("[Diagram]\n{}", source())
    })
}

fn is_block(element: &Element) -> bool {
    BLOCK_TAGS.contains(&element.name.as_str())
}

fn inline_text(node: &Node, options: &PlainTextOptions) -> String {
    let element = match node {
        Node::Text(text) => return text.clone(),
        Node::Element(element) => element,
    };
    if let Some(rich) = rich_representation(element, options.rich) {
        return rich;
    }
    if element.name == "sup" && element.has_class(FOOTNOTE_REF_CLASS) {
        return options.footnote(element);
    }
    if element.name == "br" {
        return "\n".to_owned();
    }
    element
        .children
        .iter()
        .map(|child| {
            let text = inline_text(child, options);
            match child {
                Node::Element(e) if is_block(e) => format!(" {text} "),
                _ => text,
            }
        })
        .collect()
}

struct Blocks<'o> {
    options: &'o PlainTextOptions,
    blocks: Vec<(String, bool)>,
    buffer: String,
}

impl Blocks<'_> {
    fn flush(&mut self) {
        let text = clean_block(&self.buffer);
        if !text.is_empty() {
            self.blocks.push((text, false));
        }
        self.buffer.clear();
    }

    fn walk(&mut self, node: &Node) {
        let element = match node {
            Node::Text(text) => {
                self.buffer.push_str(text);
                return;
            }
            Node::Element(element) => element,
        };
        if element.has_class("math-inline") {
            self.buffer
                .push_str(&rich_representation(element, self.options.rich).unwrap_or_default());
            return;
        }
        if element.has_class("math-block")
            || element.has_class("mermaid-diagram")
            || element.has_class("calc-worksheet")
        {
            self.flush();
            if let Some(text) =
                rich_representation(element, self.options.rich).filter(|t| !t.is_empty())
            {
                self.blocks.push((text, false));
            }
            return;
        }
        if element.name == "br" {
            self.buffer.push('\n');
            return;
        }
        if element.name == "sup" && element.has_class(FOOTNOTE_REF_CLASS) {
            let marker = self.options.footnote(element);
            self.buffer.push_str(&marker);
            return;
        }
        if element.name == "ul" || element.name == "ol" {
            self.flush();
            let items = element.children.iter().filter_map(|child| match child {
                Node::Element(e) if e.name == "li" => Some(e),
                _ => None,
            });
            for (index, item) in items.enumerate() {
                let marker = if element.name == "ol" {
                    format!("{}.", index + 1)
                } else {
                    "•".to_owned()
                };
                let text = inline_text(&Node::Element(item.clone()), self.options);
                let text = js_trim(&collapse_js_whitespace(&text)).to_owned();
                if !text.is_empty() {
                    self.blocks.push((format!("{marker} {text}"), true));
                }
            }
            return;
        }
        if element.name == "li" {
            self.flush();
            let text = inline_text(node, self.options);
            let text = js_trim(&collapse_js_whitespace(&text)).to_owned();
            if !text.is_empty() {
                self.blocks.push((format!("• {text}"), true));
            }
            return;
        }
        let block = is_block(element);
        if block {
            self.flush();
        }
        for child in &element.children {
            self.walk(child);
        }
        if block {
            self.flush();
        }
    }
}

/// `htmlToPlainText`: readable text with blank lines between paragraphs and single lines between
/// list items.
pub fn html_to_plain_text(html: &str, options: &PlainTextOptions) -> String {
    let nodes = parse_fragment(html);
    let mut collector = Blocks {
        options,
        blocks: Vec::new(),
        buffer: String::new(),
    };
    for node in &nodes {
        collector.walk(node);
    }
    collector.flush();
    let mut output = String::new();
    let mut previous_tight = false;
    for (index, (text, tight)) in collector.blocks.iter().enumerate() {
        if index > 0 {
            output.push_str(if *tight && previous_tight {
                "\n"
            } else {
                "\n\n"
            });
        }
        output.push_str(text);
        previous_tight = *tight;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{count_words, is_js_whitespace};

    #[test]
    fn javascript_whitespace_not_rusts() {
        assert!(is_js_whitespace('\u{feff}'));
        assert!(!is_js_whitespace('\u{85}'));
        assert_eq!(count_words(" a\u{a0}b\u{85}c \u{3000}d "), 3);
    }
}
