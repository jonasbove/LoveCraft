//! A scene's HTML as blocks of styled runs, for the exporters that lay text out themselves
//! (ProseCraft `src/lib/publish/rich-text.ts`, upstream `2d1cf18`): DOCX and PDF.

use crate::html::{
    Element, Node, is_js_whitespace, parse_fragment, sanitize::is_valid_footnote_id,
};
use crate::js::string_to_number;

/// An image in the text: an equation or diagram drawn inline, or an embedded picture.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineImage {
    pub src: String,
    pub alt: String,
    pub width: Option<f64>,
    pub height: Option<f64>,
}

/// A stretch of text with one style, or a line break, an inline image or a footnote reference.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InlineRun {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub link: Option<String>,
    pub line_break: bool,
    /// An inline equation or diagram image; the text is empty.
    pub image: Option<InlineImage>,
    /// A footnote reference (`data-footnote-id`); the text is empty.
    pub footnote_id: Option<String>,
}

impl InlineRun {
    fn styled(text: String, style: &InlineRun) -> Self {
        Self {
            text,
            bold: style.bold,
            italic: style.italic,
            underline: style.underline,
            strike: style.strike,
            link: style.link.clone(),
            ..Self::default()
        }
    }

    /// Whether the run shows something: a break, an image, a reference or non-blank text.
    fn visible(&self) -> bool {
        self.line_break
            || self.image.is_some()
            || self.footnote_id.is_some()
            || !js_trim(&self.text).is_empty()
    }

    fn is_text(&self) -> bool {
        !self.line_break && self.image.is_none() && self.footnote_id.is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    Heading,
    Quote,
    List,
    Image,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub kind: BlockKind,
    pub runs: Vec<InlineRun>,
    pub image: Option<InlineImage>,
    /// A heading's level, 1 to 3.
    pub level: Option<u8>,
    /// A list item's marker: "•" or "1.".
    pub marker: Option<String>,
    /// A paragraph with ProseCraft's `eyebrow` class (a small label above a heading).
    pub eyebrow: bool,
}

impl Block {
    fn new(kind: BlockKind, runs: Vec<InlineRun>) -> Self {
        Self {
            kind,
            runs,
            image: None,
            level: None,
            marker: None,
            eyebrow: false,
        }
    }
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

/// `normalizeText`: no-break spaces as spaces, then each run of whitespace as one space.
fn normalize_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut space = false;
    for c in value.chars() {
        let c = if c == '\u{a0}' { ' ' } else { c };
        if is_js_whitespace(c) {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

/// `/^data:image\/(?:png|jpeg|webp);base64,/`.
fn is_embedded_image(src: &str) -> bool {
    ["png", "jpeg", "webp"]
        .iter()
        .any(|kind| src.starts_with(&format!("data:image/{kind};base64,")))
}

/// `Number(value)` as a size, kept when it is a finite positive number.
fn size(value: Option<&str>) -> Option<f64> {
    let n = value.map_or(0.0, string_to_number);
    (n.is_finite() && n > 0.0).then_some(n)
}

fn collect_inline(node: &Node, style: &InlineRun, output: &mut Vec<InlineRun>) {
    let element = match node {
        Node::Text(text) => {
            output.push(InlineRun::styled(normalize_text(text), style));
            return;
        }
        Node::Element(e) => e,
    };
    match element.name.as_str() {
        "br" => {
            let mut run = InlineRun::styled(String::new(), style);
            run.line_break = true;
            output.push(run);
            return;
        }
        "sup" if element.has_class("footnote-ref") => {
            let id = element.attr("data-footnote-id").unwrap_or_default();
            if is_valid_footnote_id(id) {
                let mut run = InlineRun::styled(String::new(), style);
                run.footnote_id = Some(id.to_owned());
                output.push(run);
            }
            return;
        }
        "img" => {
            let src = element.attr("src").unwrap_or_default();
            if is_embedded_image(src) {
                let mut run = InlineRun::styled(String::new(), style);
                run.image = Some(InlineImage {
                    src: src.to_owned(),
                    alt: element.attr("alt").unwrap_or_default().to_owned(),
                    width: size(
                        element
                            .attr("width")
                            .or_else(|| element.attr("data-display-width")),
                    ),
                    height: size(
                        element
                            .attr("height")
                            .or_else(|| element.attr("data-display-height")),
                    ),
                });
                output.push(run);
            }
            return;
        }
        _ => {}
    }
    for_children(element, style, output);
}

/// The element's own style over `style`, then its children (the tail of `collectInline`).
fn for_children(element: &Element, style: &InlineRun, output: &mut Vec<InlineRun>) {
    let mut next = style.clone();
    match element.name.as_str() {
        "strong" | "b" => next.bold = true,
        "em" | "i" => next.italic = true,
        "u" => next.underline = true,
        "s" | "strike" => next.strike = true,
        "a" => {
            if let Some(href) = element.attr("href").filter(|h| !h.is_empty()) {
                next.link = Some(href.to_owned());
            }
        }
        _ => {}
    }
    for child in &element.children {
        collect_inline(child, &next, output);
    }
}

/// `trimRuns`: empty text runs dropped, and the runs cut to the first and last visible one,
/// with the outer text runs trimmed.
pub fn trim_runs(runs: &[InlineRun]) -> Vec<InlineRun> {
    let kept: Vec<InlineRun> = runs
        .iter()
        .filter(|r| !r.is_text() || !r.text.is_empty())
        .cloned()
        .collect();
    let Some(first) = kept.iter().position(InlineRun::visible) else {
        return Vec::new();
    };
    let last = kept.iter().rposition(InlineRun::visible).unwrap_or(first);
    let mut trimmed: Vec<InlineRun> = kept[first..=last].to_vec();
    if let Some(run) = trimmed.first_mut().filter(|r| r.is_text()) {
        run.text = run.text.trim_start_matches(is_js_whitespace).to_owned();
    }
    if let Some(run) = trimmed.last_mut().filter(|r| r.is_text()) {
        run.text = run.text.trim_end_matches(is_js_whitespace).to_owned();
    }
    trimmed
}

/// `blockRuns(node)`: the runs of a block element, its own style included.
fn block_runs(element: &Element) -> Vec<InlineRun> {
    let mut runs = Vec::new();
    match element.name.as_str() {
        "br" | "sup" | "img" => collect_inline(
            &Node::Element(element.clone()),
            &InlineRun::default(),
            &mut runs,
        ),
        _ => for_children(element, &InlineRun::default(), &mut runs),
    }
    trim_runs(&runs)
}

/// `inlineRunsFromHtml`: an inline fragment (a footnote's text) as styled runs.
pub fn inline_runs_from_html(html: &str) -> Vec<InlineRun> {
    if html.is_empty() {
        return Vec::new();
    }
    let mut runs = Vec::new();
    for node in parse_fragment(html) {
        collect_inline(&node, &InlineRun::default(), &mut runs);
    }
    trim_runs(&runs)
}

fn is_inline_image(element: &Element) -> bool {
    element.attr("data-inline") == Some("true")
}

/// `node.querySelector('img:not([data-inline="true"])')`.
fn has_block_image(element: &Element) -> bool {
    element.children.iter().any(|child| match child {
        Node::Element(e) => (e.name == "img" && !is_inline_image(e)) || has_block_image(e),
        Node::Text(_) => false,
    })
}

fn shallow(element: &Element) -> Element {
    Element {
        children: Vec::new(),
        ..element.clone()
    }
}

/// `splitAroundImages`: the node in pieces, cut around every image that isn't inline.
fn split_around_images(node: &Node) -> Vec<Node> {
    let element = match node {
        Node::Element(e) => e,
        Node::Text(_) => return vec![node.clone()],
    };
    if element.name == "img" {
        return vec![node.clone()];
    }
    let mut parts = Vec::new();
    let mut current = shallow(element);
    for child in &element.children {
        for part in split_around_images(child) {
            match &part {
                Node::Element(e) if e.name == "img" && !is_inline_image(e) => {
                    if !current.children.is_empty() {
                        parts.push(Node::Element(std::mem::replace(
                            &mut current,
                            shallow(element),
                        )));
                    } else {
                        current = shallow(element);
                    }
                    parts.push(part);
                }
                _ => current.children.push(part),
            }
        }
    }
    if !current.children.is_empty() {
        parts.push(Node::Element(current));
    }
    parts
}

const NESTED_BLOCKS: [&str; 9] = ["p", "div", "h1", "h2", "h3", "blockquote", "ul", "ol", "li"];

struct Builder {
    blocks: Vec<Block>,
}

impl Builder {
    fn add(&mut self, block: Block) {
        if !block.runs.is_empty() {
            self.blocks.push(block);
        }
    }

    fn node(&mut self, node: &Node) {
        let element = match node {
            Node::Text(text) => {
                let run = InlineRun {
                    text: normalize_text(text),
                    ..InlineRun::default()
                };
                self.add(Block::new(BlockKind::Paragraph, trim_runs(&[run])));
                return;
            }
            Node::Element(e) => e,
        };
        if has_block_image(element) {
            for part in split_around_images(node) {
                match &part {
                    Node::Element(e) if e.name == "img" && !is_inline_image(e) => {
                        let src = e.attr("src").unwrap_or_default();
                        if !is_embedded_image(src) {
                            continue;
                        }
                        let mut block = Block::new(BlockKind::Image, Vec::new());
                        block.image = Some(InlineImage {
                            src: src.to_owned(),
                            alt: e.attr("alt").unwrap_or_default().to_owned(),
                            width: size(e.attr("width")),
                            height: size(e.attr("height")),
                        });
                        self.blocks.push(block);
                    }
                    _ => self.node(&part),
                }
            }
            return;
        }
        if element.name == "img" {
            let src = element.attr("src").unwrap_or_default();
            if is_embedded_image(src) {
                let mut block = Block::new(BlockKind::Image, Vec::new());
                block.image = Some(InlineImage {
                    src: src.to_owned(),
                    alt: element.attr("alt").unwrap_or_default().to_owned(),
                    width: None,
                    height: None,
                });
                self.blocks.push(block);
            }
            return;
        }
        let heading = |level: u8, b: &mut Self| {
            let mut block = Block::new(BlockKind::Heading, block_runs(element));
            block.level = Some(level);
            b.add(block);
        };
        match element.name.as_str() {
            "h1" => heading(1, self),
            "h2" => heading(2, self),
            "h3" => heading(3, self),
            "blockquote" => {
                let paragraphs: Vec<&Element> = element
                    .children
                    .iter()
                    .filter_map(|c| match c {
                        Node::Element(e) if e.name == "p" => Some(e),
                        _ => None,
                    })
                    .collect();
                if paragraphs.is_empty() {
                    self.add(Block::new(BlockKind::Quote, block_runs(element)));
                } else {
                    for p in paragraphs {
                        self.add(Block::new(BlockKind::Quote, block_runs(p)));
                    }
                }
            }
            "ul" | "ol" => {
                let items = element.children.iter().filter_map(|c| match c {
                    Node::Element(e) if e.name == "li" => Some(e),
                    _ => None,
                });
                for (index, item) in items.enumerate() {
                    let mut block = Block::new(BlockKind::List, block_runs(item));
                    block.marker = Some(if element.name == "ol" {
                        format!("{}.", index + 1)
                    } else {
                        "•".to_owned()
                    });
                    self.add(block);
                }
            }
            "li" => {
                let mut block = Block::new(BlockKind::List, block_runs(element));
                block.marker = Some("•".to_owned());
                self.add(block);
            }
            name => {
                let nested = element.children.iter().any(
                    |c| matches!(c, Node::Element(e) if NESTED_BLOCKS.contains(&e.name.as_str())),
                );
                if nested && (name == "div" || name == "p") {
                    for child in &element.children {
                        self.node(child);
                    }
                } else {
                    let mut block = Block::new(BlockKind::Paragraph, block_runs(element));
                    block.eyebrow = name == "p" && element.has_class("eyebrow");
                    self.add(block);
                }
            }
        }
    }
}

/// `blocksFromHtml`: a scene's HTML as paragraphs, headings, quotes, list items and images.
pub fn blocks_from_html(html: &str) -> Vec<Block> {
    let mut builder = Builder { blocks: Vec::new() };
    for node in parse_fragment(html) {
        builder.node(&node);
    }
    builder.blocks
}
