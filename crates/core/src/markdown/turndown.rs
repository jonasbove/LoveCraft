//! A port of Turndown 7.2.4 (MIT, © Dom Christie; its whitespace collapsing adapted from
//! collapse-whitespace, MIT, © Luc Thevenard) with the options and rules ProseCraft
//! configures in `src/lib/publish/markdown.ts`: ATX headings, `-` bullets, `_` emphasis,
//! `**` strong, indented code blocks, inline links, and ProseCraft's rules for eyebrows,
//! laid-out images, equations, diagrams, calculations, footnote markers, citations,
//! underline and strikethrough.
//!
//! Their copyright and permission notices are in `licenses/turndown.txt`.
//!
//! Turndown works on a DOM it changes in place (whitespace is collapsed first, then each
//! node is converted knowing its parent and siblings), so this keeps the tree in an arena.

use crate::cite::{CITE_REF_CLASS, format_pandoc_citations, parse_cite_data};
use crate::html::sanitize::{FOOTNOTE_REF_CLASS, is_valid_footnote_id};
use crate::html::{Element, Node, image_layout, is_js_whitespace};

const BLOCK: &[&str] = &[
    "address",
    "article",
    "aside",
    "audio",
    "blockquote",
    "body",
    "canvas",
    "center",
    "dd",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "html",
    "isindex",
    "li",
    "main",
    "menu",
    "nav",
    "noframes",
    "noscript",
    "ol",
    "output",
    "p",
    "pre",
    "section",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "ul",
];
const VOID: &[&str] = &[
    "area", "base", "br", "col", "command", "embed", "hr", "img", "input", "keygen", "link",
    "meta", "param", "source", "track", "wbr",
];
const MEANINGFUL_WHEN_BLANK: &[&str] = &[
    "a", "table", "thead", "tbody", "tfoot", "th", "td", "iframe", "script", "audio", "video",
];

/// The root's id in the arena.
const ROOT: usize = 0;

enum Kind {
    Text(String),
    Element(Element),
}

struct Item {
    kind: Kind,
    parent: Option<usize>,
    children: Vec<usize>,
}

struct Dom {
    items: Vec<Item>,
}

impl Dom {
    fn new(nodes: &[Node]) -> Self {
        let mut dom = Dom {
            items: vec![Item {
                kind: Kind::Element(Element {
                    name: "x-turndown".into(),
                    ..Element::default()
                }),
                parent: None,
                children: Vec::new(),
            }],
        };
        for node in nodes {
            dom.add(ROOT, node);
        }
        dom
    }

    fn add(&mut self, parent: usize, node: &Node) {
        let id = self.items.len();
        let kind = match node {
            Node::Text(t) => Kind::Text(t.clone()),
            Node::Element(e) => Kind::Element(Element {
                name: e.name.clone(),
                attrs: e.attrs.clone(),
                children: Vec::new(),
            }),
        };
        self.items.push(Item {
            kind,
            parent: Some(parent),
            children: Vec::new(),
        });
        self.items[parent].children.push(id);
        if let Node::Element(e) = node {
            for child in &e.children {
                self.add(id, child);
            }
        }
    }

    fn element(&self, id: usize) -> Option<&Element> {
        match &self.items[id].kind {
            Kind::Element(e) => Some(e),
            Kind::Text(_) => None,
        }
    }

    fn is(&self, id: usize, names: &[&str]) -> bool {
        self.element(id)
            .is_some_and(|e| names.contains(&e.name.as_str()))
    }

    fn text(&self, id: usize) -> Option<&str> {
        match &self.items[id].kind {
            Kind::Text(t) => Some(t),
            Kind::Element(_) => None,
        }
    }

    fn set_text(&mut self, id: usize, value: String) {
        self.items[id].kind = Kind::Text(value);
    }

    fn sibling(&self, id: usize, offset: isize) -> Option<usize> {
        let parent = self.items[id].parent?;
        let siblings = &self.items[parent].children;
        let index = siblings.iter().position(|c| *c == id)?;
        siblings.get(index.checked_add_signed(offset)?).copied()
    }

    fn first_child(&self, id: usize) -> Option<usize> {
        self.items[id].children.first().copied()
    }

    /// Removes `id` and returns the next node in document order (`remove` in Turndown).
    fn remove(&mut self, id: usize) -> Option<usize> {
        let next = self.sibling(id, 1).or(self.items[id].parent);
        if let Some(parent) = self.items[id].parent {
            self.items[parent].children.retain(|c| *c != id);
        }
        self.items[id].parent = None;
        next
    }

    fn text_content(&self, id: usize) -> String {
        let mut out = String::new();
        self.collect_text(id, &mut out);
        out
    }

    fn collect_text(&self, id: usize, out: &mut String) {
        match &self.items[id].kind {
            Kind::Text(t) => out.push_str(t),
            Kind::Element(_) => {
                for child in &self.items[id].children {
                    self.collect_text(*child, out);
                }
            }
        }
    }

    /// Whether an element below `id` is one of `names` (`getElementsByTagName`).
    fn has(&self, id: usize, names: &[&str]) -> bool {
        self.items[id]
            .children
            .iter()
            .any(|c| self.is(*c, names) || self.has(*c, names))
    }

    /// `next(prev, current)` of collapse-whitespace.
    fn next(&self, prev: Option<usize>, current: usize) -> Option<usize> {
        if prev.is_some_and(|p| self.items[p].parent == Some(current)) || self.is(current, &["pre"])
        {
            return self.sibling(current, 1).or(self.items[current].parent);
        }
        self.first_child(current)
            .or_else(|| self.sibling(current, 1))
            .or(self.items[current].parent)
    }

    /// `collapseWhitespace`: runs of ASCII whitespace become one space, spaces at block edges
    /// and after a space go, `<pre>` is left alone.
    fn collapse_whitespace(&mut self) {
        if self.items[ROOT].children.is_empty() {
            return;
        }
        let mut prev_text: Option<usize> = None;
        let mut keep_leading = false;
        let mut prev: Option<usize> = None;
        let mut node = self.next(None, ROOT);
        while let Some(id) = node {
            if id == ROOT {
                break;
            }
            if let Some(data) = self.text(id) {
                let mut text = collapse_ascii(data);
                let after_space =
                    prev_text.is_none_or(|p| self.text(p).is_some_and(|t| t.ends_with(' ')));
                if after_space && !keep_leading && text.starts_with(' ') {
                    text.remove(0);
                }
                if text.is_empty() {
                    node = self.remove(id);
                    continue;
                }
                self.set_text(id, text);
                prev_text = Some(id);
            } else if self.is(id, BLOCK) || self.is(id, &["br"]) {
                if let Some(p) = prev_text {
                    let trimmed = self
                        .text(p)
                        .unwrap_or_default()
                        .strip_suffix(' ')
                        .map(str::to_owned);
                    if let Some(t) = trimmed {
                        self.set_text(p, t);
                    }
                }
                prev_text = None;
                keep_leading = false;
            } else if self.is(id, VOID) || self.is(id, &["pre"]) {
                prev_text = None;
                keep_leading = true;
            } else if prev_text.is_some() {
                keep_leading = false;
            }
            let next = self.next(prev, id);
            prev = Some(id);
            node = next;
        }
        if let Some(p) = prev_text {
            let text = self.text(p).unwrap_or_default();
            let trimmed = text.strip_suffix(' ').unwrap_or(text).to_owned();
            if trimmed.is_empty() {
                self.remove(p);
            } else {
                self.set_text(p, trimmed);
            }
        }
    }
}

fn collapse_ascii(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_run = false;
    for c in text.chars() {
        if matches!(c, ' ' | '\r' | '\n' | '\t') {
            if !in_run {
                out.push(' ');
            }
            in_run = true;
        } else {
            out.push(c);
            in_run = false;
        }
    }
    out
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

fn is_ascii_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

/// `edgeWhitespace`: the leading and trailing whitespace, and how much of each is ASCII.
struct Edges<'a> {
    leading: &'a str,
    leading_ascii: &'a str,
    leading_non_ascii: &'a str,
    trailing: &'a str,
    trailing_ascii: &'a str,
    trailing_non_ascii: &'a str,
}

fn edges(text: &str) -> Edges<'_> {
    let leading_end = text.len() - text.trim_start_matches(is_js_whitespace).len();
    let leading = &text[..leading_end];
    let ascii_end = leading.len() - leading.trim_start_matches(is_ascii_ws).len();
    // A whitespace-only string is all leading.
    let rest = &text[leading_end..];
    let trailing_start = leading_end + rest.trim_end_matches(is_js_whitespace).len();
    let trailing = &text[trailing_start..];
    let ascii_start = trailing.trim_end_matches(is_ascii_ws).len();
    Edges {
        leading,
        leading_ascii: &leading[..ascii_end],
        leading_non_ascii: &leading[ascii_end..],
        trailing,
        trailing_ascii: &trailing[ascii_start..],
        trailing_non_ascii: &trailing[..ascii_start],
    }
}

fn trim_leading_newlines(s: &str) -> &str {
    s.trim_start_matches('\n')
}

fn trim_trailing_newlines(s: &str) -> &str {
    s.trim_end_matches('\n')
}

fn trim_newlines(s: &str) -> &str {
    trim_trailing_newlines(trim_leading_newlines(s))
}

/// `join`: the two strings with as many newlines between them (at most two) as either had.
fn join(output: &str, replacement: &str) -> String {
    let s1 = trim_trailing_newlines(output);
    let s2 = trim_leading_newlines(replacement);
    let newlines = (output.len() - s1.len())
        .max(replacement.len() - s2.len())
        .min(2);
    format!("{s1}{}{s2}", &"\n\n"[..newlines])
}

/// `escapeMarkdown`: Turndown's escapes, the anchored ones at the start of the text only.
fn escape_markdown(text: &str) -> String {
    let mut s = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => s.push_str("\\\\"),
            '*' => s.push_str("\\*"),
            '`' => s.push_str("\\`"),
            '[' => s.push_str("\\["),
            ']' => s.push_str("\\]"),
            '_' => s.push_str("\\_"),
            c => s.push(c),
        }
    }
    // The anchored escapes, in Turndown's order (after the global ones, which can't create
    // or remove these starts).
    if s.starts_with('-') {
        s.insert(0, '\\');
    }
    if s.starts_with("+ ") {
        s.insert(0, '\\');
    }
    if s.starts_with('=') {
        s.insert(0, '\\');
    }
    let hashes = s.bytes().take_while(|b| *b == b'#').count();
    if (1..=6).contains(&hashes) && s.as_bytes().get(hashes) == Some(&b' ') {
        s.insert(0, '\\');
    }
    if s.starts_with("~~~") {
        s.insert(0, '\\');
    }
    if s.starts_with('>') {
        s.insert(0, '\\');
    }
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && s[digits..].starts_with(". ") {
        s.insert(digits, '\\');
    }
    s
}

fn clean_attribute(value: Option<&str>) -> String {
    // `/(\n+\s*)+/g` → "\n": a newline and the whitespace after it become one newline.
    let Some(value) = value else {
        return String::new();
    };
    let mut out = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' {
            while chars.peek().is_some_and(|c| is_js_whitespace(*c)) {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out
}

fn escape_link_destination(destination: &str) -> String {
    let mut escaped = String::new();
    for c in destination.chars() {
        if matches!(c, '<' | '>' | '(' | ')') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    if escaped.contains(' ') {
        format!("<{escaped}>")
    } else {
        escaped
    }
}

fn escape_link_title(title: &str) -> String {
    title.replace('"', "\\\"")
}

/// `/^/gm`: a prefix before each line, where JavaScript's line terminators are `\n`, `\r`,
/// U+2028 and U+2029.
fn prefix_lines(text: &str, prefix: &str) -> String {
    let mut out = String::from(prefix);
    for c in text.chars() {
        out.push(c);
        if matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
            out.push_str(prefix);
        }
    }
    out
}

fn attr_source(e: &Element, name: &str) -> String {
    js_trim(e.attr(name).unwrap_or_default()).to_owned()
}

fn fenced(kind: &str, source: &str) -> String {
    let fence = if source.contains("```") {
        "~~~~"
    } else {
        "```"
    };
    format!("\n\n{fence}{kind}\n{source}\n{fence}\n\n")
}

fn escape_html_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

struct Converter {
    dom: Dom,
}

impl Converter {
    fn is_blank(&self, id: usize) -> bool {
        !self.dom.is(id, VOID)
            && !self.dom.is(id, MEANINGFUL_WHEN_BLANK)
            && self.dom.text_content(id).chars().all(is_js_whitespace)
            && !self.dom.has(id, VOID)
            && !self.dom.has(id, MEANINGFUL_WHEN_BLANK)
    }

    fn flanked(&self, id: usize, left: bool) -> bool {
        let Some(sibling) = self.dom.sibling(id, if left { -1 } else { 1 }) else {
            return false;
        };
        let test = |s: &str| {
            if left {
                s.ends_with(' ')
            } else {
                s.starts_with(' ')
            }
        };
        if let Some(text) = self.dom.text(sibling) {
            test(text)
        } else if !self.dom.is(sibling, BLOCK) {
            test(&self.dom.text_content(sibling))
        } else {
            false
        }
    }

    /// `flankingWhitespace`: the whitespace an inline element's content had at its edges,
    /// moved outside its delimiters.
    fn flanking(&self, id: usize) -> (String, String) {
        if self.dom.is(id, BLOCK) {
            return (String::new(), String::new());
        }
        let content = self.dom.text_content(id);
        let e = edges(&content);
        let leading = if !e.leading_ascii.is_empty() && self.flanked(id, true) {
            e.leading_non_ascii
        } else {
            e.leading
        };
        let trailing = if !e.trailing_ascii.is_empty() && self.flanked(id, false) {
            e.trailing_non_ascii
        } else {
            e.trailing
        };
        (leading.to_owned(), trailing.to_owned())
    }

    fn process(&self, parent: usize, in_code: bool) -> String {
        let mut output = String::new();
        for &child in &self.dom.items[parent].children {
            let replacement = match self.dom.text(child) {
                Some(text) => {
                    if in_code {
                        text.to_owned()
                    } else {
                        escape_markdown(text)
                    }
                }
                None => self.replacement_for(child, in_code || self.dom.is(child, &["code"])),
            };
            output = join(&output, &replacement);
        }
        output
    }

    fn replacement_for(&self, id: usize, in_code: bool) -> String {
        let mut content = self.process(id, in_code);
        let (leading, trailing) = self.flanking(id);
        if !leading.is_empty() || !trailing.is_empty() {
            content = js_trim(&content).to_owned();
        }
        let body = self.rule(id, &content);
        format!("{leading}{body}{trailing}")
    }

    /// The replacement of the first rule that matches, in ProseCraft's order (its rules,
    /// newest first, then Turndown's), or the blank and default rules.
    fn rule(&self, id: usize, content: &str) -> String {
        let dom = &self.dom;
        let Some(e) = dom.element(id) else {
            return String::new();
        };
        let block = dom.is(id, BLOCK);
        if self.is_blank(id) {
            return if block { "\n\n".into() } else { String::new() };
        }
        let name = e.name.as_str();
        // ProseCraft's rules.
        if matches!(name, "s" | "del" | "strike") {
            return format!("~~{content}~~");
        }
        if name == "u" {
            return format!("<u>{content}</u>");
        }
        if name == "span" && e.has_class(CITE_REF_CLASS) {
            return parse_cite_data(e.attr("data-cites"))
                .map(|c| format_pandoc_citations(&c))
                .unwrap_or_default();
        }
        if name == "sup"
            && e.has_class(FOOTNOTE_REF_CLASS)
            && is_valid_footnote_id(e.attr("data-footnote-id").unwrap_or_default())
        {
            return format!("[^{}]", e.attr("data-footnote-id").unwrap_or_default());
        }
        if e.has_class("calc-worksheet") {
            return fenced("calc", &attr_source(e, "data-source"));
        }
        if e.has_class("mermaid-diagram") {
            return fenced("mermaid", &attr_source(e, "data-source"));
        }
        if e.has_class("math-block") {
            return format!("\n\n$$\n{}\n$$\n\n", attr_source(e, "data-tex"));
        }
        if name == "span" && e.has_class("math-inline") {
            return format!("${}$", attr_source(e, "data-tex"));
        }
        let layout = image_layout::attributes(&image_layout::read(e));
        if name == "img" && !layout.is_empty() {
            let Some(src) = e.attr("src").filter(|s| !s.is_empty()) else {
                return String::new();
            };
            let mut attributes = vec![
                format!("src=\"{}\"", escape_html_attribute(src)),
                format!(
                    "alt=\"{}\"",
                    escape_html_attribute(e.attr("alt").unwrap_or_default())
                ),
            ];
            attributes.extend(
                layout
                    .iter()
                    .map(|(k, v)| format!("{k}=\"{}\"", escape_html_attribute(v))),
            );
            return format!("<img {}>", attributes.join(" "));
        }
        if name == "p" && e.has_class("eyebrow") {
            return format!("**{}**\n\n", js_trim(content));
        }
        // Turndown's rules.
        match name {
            "p" => return format!("\n\n{content}\n\n"),
            "br" => return "  \n".into(),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = usize::from(name.as_bytes()[1] - b'0');
                return format!("\n\n{} {content}\n\n", "#".repeat(level));
            }
            "blockquote" => {
                return format!("\n\n{}\n\n", prefix_lines(trim_newlines(content), "> "));
            }
            "ul" | "ol" => {
                let parent = dom.items[id].parent;
                let last_element = parent.and_then(|p| {
                    dom.items[p]
                        .children
                        .iter()
                        .rev()
                        .find(|c| dom.element(**c).is_some())
                        .copied()
                });
                return if parent.is_some_and(|p| dom.is(p, &["li"])) && last_element == Some(id) {
                    format!("\n{content}")
                } else {
                    format!("\n\n{content}\n\n")
                };
            }
            "li" => {
                let parent = dom.items[id].parent;
                let mut prefix = "-   ".to_owned();
                if let Some(p) = parent.filter(|p| dom.is(*p, &["ol"])) {
                    let index = dom.items[p]
                        .children
                        .iter()
                        .filter(|c| dom.element(**c).is_some())
                        .position(|c| *c == id)
                        .unwrap_or(0);
                    let start = dom
                        .element(p)
                        .and_then(|l| l.attr("start"))
                        .filter(|s| !s.is_empty());
                    let number = match start {
                        Some(s) => crate::js::number_to_string(
                            crate::js::string_to_number(s) + index as f64,
                        ),
                        None => (index + 1).to_string(),
                    };
                    prefix = format!("{number}.  ");
                }
                let paragraph = content.ends_with('\n');
                let body = format!(
                    "{}{}",
                    trim_newlines(content),
                    if paragraph { "\n" } else { "" }
                );
                let body = body.replace(
                    '\n',
                    &format!("\n{}", " ".repeat(prefix.encode_utf16().count())),
                );
                let next = if dom.sibling(id, 1).is_some() {
                    "\n"
                } else {
                    ""
                };
                return format!("{prefix}{body}{next}");
            }
            "pre" => {
                if let Some(code) = dom.first_child(id).filter(|c| dom.is(*c, &["code"])) {
                    return format!(
                        "\n\n    {}\n\n",
                        dom.text_content(code).replace('\n', "\n    ")
                    );
                }
            }
            "hr" => return "\n\n* * *\n\n".into(),
            "a" => {
                if let Some(href) = e.attr("href").filter(|h| !h.is_empty()) {
                    let title = escape_link_title(&clean_attribute(e.attr("title")));
                    let title = if title.is_empty() {
                        String::new()
                    } else {
                        format!(" \"{title}\"")
                    };
                    return format!("[{content}]({}{title})", escape_link_destination(href));
                }
            }
            "em" | "i" => {
                return if js_trim(content).is_empty() {
                    String::new()
                } else {
                    format!("_{content}_")
                };
            }
            "strong" | "b" => {
                return if js_trim(content).is_empty() {
                    String::new()
                } else {
                    format!("**{content}**")
                };
            }
            "code" => {
                let parent = dom.items[id].parent;
                let has_siblings = dom.sibling(id, -1).is_some() || dom.sibling(id, 1).is_some();
                let code_block = parent.is_some_and(|p| dom.is(p, &["pre"])) && !has_siblings;
                if !code_block {
                    return inline_code(content);
                }
            }
            "img" => {
                let alt = escape_markdown(&clean_attribute(e.attr("alt")));
                let src = escape_link_destination(e.attr("src").unwrap_or_default());
                let title = clean_attribute(e.attr("title"));
                let title = if title.is_empty() {
                    String::new()
                } else {
                    format!(" \"{}\"", escape_link_title(&title))
                };
                return if src.is_empty() {
                    String::new()
                } else {
                    format!("![{alt}]({src}{title})")
                };
            }
            _ => {}
        }
        if block {
            format!("\n\n{content}\n\n")
        } else {
            content.to_owned()
        }
    }
}

/// Turndown's `code` rule: backticks around the code, more than any run inside it.
fn inline_code(content: &str) -> String {
    if content.is_empty() {
        return String::new();
    }
    let content = content.replace("\r\n", " ").replace(['\r', '\n'], " ");
    // `/^`|^ .*?[^ ].* $|`$/`, where `.` doesn't match a line terminator.
    let single_line = !content.contains(['\u{2028}', '\u{2029}']);
    let padded = content.starts_with(' ')
        && content.ends_with(' ')
        && content.len() >= 2
        && content.trim_matches(' ').chars().next().is_some()
        && single_line;
    let extra = if content.starts_with('`') || content.ends_with('`') || padded {
        " "
    } else {
        ""
    };
    let runs: Vec<usize> = content
        .split(|c| c != '`')
        .map(str::len)
        .filter(|n| *n > 0)
        .collect();
    let mut delimiter = 1;
    while runs.contains(&delimiter) {
        delimiter += 1;
    }
    let fence = "`".repeat(delimiter);
    format!("{fence}{extra}{content}{extra}{fence}")
}

/// What `seedRichPlaceholders` does: a zero-width space in each equation, diagram,
/// calculation, footnote marker and citation, so Turndown doesn't skip them as blank.
fn seed(nodes: &mut [Node]) {
    for node in nodes {
        if let Node::Element(e) = node {
            let rich = [
                "math-inline",
                "math-block",
                "mermaid-diagram",
                "calc-worksheet",
            ]
            .iter()
            .any(|c| e.has_class(c));
            let footnote = e.name == "sup"
                && e.has_class(FOOTNOTE_REF_CLASS)
                && e.attr("data-footnote-id").is_some();
            let cite =
                e.name == "span" && e.has_class(CITE_REF_CLASS) && e.attr("data-cites").is_some();
            if rich || footnote || cite {
                e.children = vec![Node::Text("\u{200b}".into())];
            } else {
                seed(&mut e.children);
            }
        }
    }
}

/// `turndown(html)` with ProseCraft's service, the rich content seeded first.
pub fn turndown_nodes(mut nodes: Vec<Node>) -> String {
    seed(&mut nodes);
    let mut dom = Dom::new(&nodes);
    dom.collapse_whitespace();
    let converter = Converter { dom };
    let output = converter.process(ROOT, false);
    // `postProcess`: leading tabs and newlines, and trailing whitespace, go.
    let output = output.trim_start_matches(['\t', '\r', '\n']);
    output
        .trim_end_matches(|c: char| matches!(c, '\t' | '\r' | '\n') || is_js_whitespace(c))
        .to_owned()
}
