//! ProseCraft's scene HTML: a small tree over `craft-html`'s tokenizer, and the text derived from it.
//!
//! ProseCraft stores every scene as sanitized HTML and reads it with the browser's `DOMParser`.
//! This builds a comparable tree for that limited dialect: void elements, a `<p>` that a block
//! closes, a `<li>` that the next item closes, end tags that close up to their element. Nesting
//! is bounded, so hostile input can't exhaust the stack.

pub mod image_layout;
mod plain;
pub mod sanitize;

pub use plain::{
    PlainTextOptions, RichText, count_words, html_to_plain_text, html_word_count, inner_text,
    is_js_whitespace,
};
pub use sanitize::{sanitize_html, sanitize_inline_html, sanitize_nodes};

/// Deepest element nesting kept; anything deeper is attached to the deepest kept element.
pub const MAX_DEPTH: usize = 128;

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

/// Elements whose start closes an open `<p>` (the HTML parsing rules for the tags ProseCraft uses).
const CLOSES_P: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "div",
    "dl",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "ul",
];

/// A node of the tree.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Text(String),
    Element(Element),
}

/// An element: lower-case name, attributes (decoded), children.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn has_class(&self, class: &str) -> bool {
        self.attr("class")
            .is_some_and(|classes| classes.split_ascii_whitespace().any(|c| c == class))
    }
}

/// The elements `innerHTML` writes without an end tag.
const SERIALIZED_VOID: &[&str] = &[
    "area", "base", "basefont", "bgsound", "br", "col", "embed", "frame", "hr", "img", "input",
    "keygen", "link", "meta", "param", "source", "track", "wbr",
];

/// `innerHTML` for these nodes, as the browser (and jsdom) writes it: text escapes `&`, `<`,
/// `>` and no-break spaces; attribute values escape `&`, `"` and no-break spaces.
pub fn serialize(nodes: &[Node]) -> String {
    fn text(out: &mut String, value: &str, attribute: bool) {
        for c in value.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '\u{a0}' => out.push_str("&nbsp;"),
                '"' if attribute => out.push_str("&quot;"),
                '<' if !attribute => out.push_str("&lt;"),
                '>' if !attribute => out.push_str("&gt;"),
                c => out.push(c),
            }
        }
    }
    fn write(out: &mut String, node: &Node) {
        match node {
            Node::Text(value) => text(out, value, false),
            Node::Element(e) => {
                out.push('<');
                out.push_str(&e.name);
                for (key, value) in &e.attrs {
                    out.push(' ');
                    out.push_str(key);
                    out.push_str("=\"");
                    text(out, value, true);
                    out.push('"');
                }
                out.push('>');
                if SERIALIZED_VOID.contains(&e.name.as_str()) {
                    return;
                }
                for child in &e.children {
                    write(out, child);
                }
                out.push_str("</");
                out.push_str(&e.name);
                out.push('>');
            }
        }
    }
    let mut out = String::new();
    for n in nodes {
        write(&mut out, n);
    }
    out
}

/// Elements whose content is dropped (see the sanitizer's module notes).
const DROPPED: [&str; 3] = ["script", "style", "template"];

/// `html` without the start tags of dropped elements that are never closed. A browser reads
/// everything after such a tag as its content, so the sanitizer would drop the rest of the
/// scene with it: Markdown passes raw HTML through, and "She wrote <script> on the board."
/// lost the board and every paragraph after it. Such a tag is left out instead, and the writing
/// after it read as usual.
fn without_unclosed_dropped(html: &str) -> std::borrow::Cow<'_, str> {
    let lower = html.to_ascii_lowercase();
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    for name in DROPPED {
        let open = format!("<{name}");
        let close = format!("</{name}");
        let mut from = 0;
        while let Some(at) = lower[from..].find(&open).map(|i| i + from) {
            let after = lower.as_bytes().get(at + open.len()).copied();
            from = at + open.len();
            if !matches!(
                after,
                Some(b'>' | b'/' | b' ' | b'\t' | b'\n' | b'\r' | 0x0c) | None
            ) {
                continue;
            }
            let Some(end) = lower[at..].find('>').map(|i| i + at + 1) else {
                break;
            };
            if lower[end..].contains(&close) {
                // Closed: dropped with its content, and the scan goes on after its end tag.
                from = lower[end..].find(&close).map_or(lower.len(), |i| i + end);
                continue;
            }
            cuts.push((at, end));
            from = end;
        }
    }
    if cuts.is_empty() {
        return std::borrow::Cow::Borrowed(html);
    }
    cuts.sort_unstable();
    let mut out = String::with_capacity(html.len());
    let mut last = 0;
    for (start, end) in cuts {
        if start >= last {
            out.push_str(&html[last..start]);
            last = end;
        }
    }
    out.push_str(&html[last..]);
    std::borrow::Cow::Owned(out)
}

/// Parses an HTML fragment into the children of an implicit body.
pub fn parse_fragment(html: &str) -> Vec<Node> {
    let cleaned = without_unclosed_dropped(html);
    let html: &str = &cleaned;
    let options = craft_html::Options {
        drop: &DROPPED,
        ..craft_html::Options::default()
    };
    let mut stack: Vec<Element> = vec![Element {
        name: "body".into(),
        ..Element::default()
    }];

    fn close_top(stack: &mut Vec<Element>) {
        if stack.len() > 1
            && let Some(done) = stack.pop()
            && let Some(parent) = stack.last_mut()
        {
            parent.children.push(Node::Element(done));
        }
    }

    fn close_to(stack: &mut Vec<Element>, name: &str) {
        if let Some(position) = stack.iter().rposition(|e| e.name == name)
            && position > 0
        {
            while stack.len() > position {
                close_top(stack);
            }
        }
    }

    for token in craft_html::Lexer::with(html, options) {
        match token {
            craft_html::Token::Text(text) => {
                // The HTML parser ignores NUL characters in the body.
                let text = craft_html::unescape(text).replace('\0', "");
                if let Some(top) = stack.last_mut() {
                    match top.children.last_mut() {
                        Some(Node::Text(previous)) => previous.push_str(&text),
                        _ => top.children.push(Node::Text(text)),
                    }
                }
            }
            craft_html::Token::Start(tag) => {
                let name = tag.name.clone();
                if CLOSES_P.contains(&name.as_str()) && stack.iter().any(|e| e.name == "p") {
                    close_to(&mut stack, "p");
                }
                if name == "li" {
                    // A new item closes the open one, up to its list.
                    if let Some(item) = stack.iter().rposition(|e| e.name == "li") {
                        let list = stack.iter().rposition(|e| e.name == "ul" || e.name == "ol");
                        if list.is_none_or(|l| l < item) {
                            close_to(&mut stack, "li");
                        }
                    }
                }
                let element = Element {
                    name: name.clone(),
                    attrs: tag
                        .attributes()
                        // A NUL in an attribute value is read as U+FFFD.
                        .map(|(k, v)| (k.to_ascii_lowercase(), v.replace('\0', "\u{fffd}")))
                        .collect(),
                    children: Vec::new(),
                };
                if VOID.contains(&name.as_str()) || tag.self_close || stack.len() > MAX_DEPTH {
                    if let Some(top) = stack.last_mut() {
                        top.children.push(Node::Element(element));
                    }
                } else {
                    stack.push(element);
                }
            }
            craft_html::Token::End(name) => {
                if !VOID.contains(&name.as_str()) {
                    close_to(&mut stack, &name);
                }
            }
        }
    }
    while stack.len() > 1 {
        close_top(&mut stack);
    }
    stack.pop().map(|body| body.children).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{Node, parse_fragment};

    fn names(nodes: &[Node]) -> Vec<String> {
        nodes
            .iter()
            .map(|n| match n {
                Node::Text(t) => format!("#{t}"),
                Node::Element(e) => format!("{}[{}]", e.name, names(&e.children).join(",")),
            })
            .collect()
    }

    #[test]
    fn builds_the_tree_html_would() {
        assert_eq!(names(&parse_fragment("<p>a<p>b")), ["p[#a]", "p[#b]"]);
        assert_eq!(
            names(&parse_fragment("<ul><li>a<li>b</ul>")),
            ["ul[li[#a],li[#b]]"]
        );
        assert_eq!(
            names(&parse_fragment("<p>x<br>y</p></span>z")),
            ["p[#x,br[],#y]", "#z"]
        );
        assert_eq!(
            names(&parse_fragment("a &amp; b&nbsp;c")),
            ["#a & b\u{a0}c"]
        );
    }

    #[test]
    fn deep_nesting_is_bounded() {
        let html = "<div>".repeat(100_000);
        let nodes = parse_fragment(&html);
        assert_eq!(nodes.len(), 1);
    }
}
