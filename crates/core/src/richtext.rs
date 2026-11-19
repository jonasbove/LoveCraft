//! Rich text as an editor holds it: one line per block, inline formatting per character, and
//! footnote markers, citations and equations as single characters that keep their exact HTML.
//!
//! ProseCraft stores scenes as sanitized HTML (`src/lib/sanitize.ts`, upstream `7dae544`). An
//! editor here works on a [`RichDoc`]: the text with one `\n` per block, each block's kind (and,
//! where the kinds can't say it, the containers it sits in: a [`Frame`]), each character's stack
//! of inline tags, and the atoms. [`RichDoc::from_html`] reads a scene only if
//! [`RichDoc::to_html`] gives back **exactly** the same HTML, so editing can never drop or
//! rewrite what the model doesn't understand. A scene it can't hold exactly may still open in a
//! normalized shape ([`RichDoc::from_html_normalized`]: what ProseCraft's sanitizer stores, then
//! a few changes of this editor's own), only if no character, footnote, citation, picture or
//! block is lost, and written back only once it is edited; anything else stays read-only.

use crate::html::{Element, Node, parse_fragment};

/// One character standing for an atom (footnote marker, citation, inline equation): the object
/// replacement character.
pub const ATOM: char = '\u{FFFC}';
/// A line break inside a block (a newline in a text node, as ProseCraft's Markdown writes).
pub const SOFT_BREAK: char = '\u{2028}';
/// A `<br>` inside a block (Shift+Enter in ProseCraft's editor). Like [`SOFT_BREAK`] it ends
/// the row; unlike it, it isn't text (it adds nothing to the block's `textContent`).
pub const HARD_BREAK: char = '\u{2029}';

/// The characters the editor gives its own meaning (and `\r`, which it doesn't hold as text).
/// One of them in a scene's text is held as an atom whose HTML is that character, so it is
/// written back as it was.
const OWN_CHARS: [char; 4] = [ATOM, SOFT_BREAK, HARD_BREAK, '\r'];

/// The character a text atom stands for: one of the editor's own characters found in a scene's
/// text (a line separator, say), which is text there, not the editor's break.
pub fn text_atom(html: &str) -> Option<char> {
    let mut chars = html.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if OWN_CHARS.contains(&c) => Some(c),
        _ => None,
    }
}

/// What a line (block) is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Paragraph,
    Heading1,
    Heading2,
    Heading3,
    /// A paragraph inside a blockquote; neighbours share one blockquote.
    Quote,
    /// A bulleted list item; neighbours share one list.
    Bullet,
    /// A numbered list item; neighbours share one list.
    Numbered,
    /// A list item inside another list's item: `depth` lists deep (1 or more; [`Self::Bullet`]
    /// and [`Self::Numbered`] are depth 0), bit `n` of `ordered` set when the list at depth `n`
    /// is numbered.
    ListItem {
        depth: u8,
        ordered: u8,
    },
    /// ProseCraft's small caption above a heading: `<p class="eyebrow">`.
    Eyebrow,
    /// A block the editor keeps whole, as one atom alone on its line: a rule, a code block, a
    /// table, a block equation, a diagram or a calculation (see [`BlockPreview`]).
    Block,
    /// A screenplay element: `<p data-sp="…">`. `bare` is an empty one written without the
    /// editor's `<br>` (as a new screenplay scene starts: `<p data-sp="scene-heading"></p>`).
    /// `eyebrow` is one that is a caption too, `<p class="eyebrow" data-sp="…">` (a caption
    /// given an element type in ProseCraft's screenplay editor keeps its class).
    Screenplay {
        kind: ScreenplayKind,
        bare: bool,
        eyebrow: bool,
    },
    /// `<p></p>`: an empty paragraph written without the editor's `<br>` (as an import writes
    /// one). Typed into, it is a [`Self::Paragraph`].
    EmptyParagraph,
    /// Inline content in no block: text or a picture at the top of the scene, or directly in a
    /// blockquote when `quote` (as an import or the sanitizer leaves it, and as ProseCraft's
    /// editor keeps it). Emptied, it is a paragraph.
    Bare {
        quote: bool,
    },
}

/// The `data-sp` values ProseCraft keeps (its sanitizer's `screenplayTypes`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenplayKind {
    SceneHeading,
    Action,
    Character,
    Dialogue,
    Parenthetical,
    Transition,
    Centered,
    Section,
    Synopsis,
    Note,
    PageBreak,
}

impl ScreenplayKind {
    pub const ALL: [Self; 11] = [
        Self::SceneHeading,
        Self::Action,
        Self::Character,
        Self::Dialogue,
        Self::Parenthetical,
        Self::Transition,
        Self::Centered,
        Self::Section,
        Self::Synopsis,
        Self::Note,
        Self::PageBreak,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SceneHeading => "scene-heading",
            Self::Action => "action",
            Self::Character => "character",
            Self::Dialogue => "dialogue",
            Self::Parenthetical => "parenthetical",
            Self::Transition => "transition",
            Self::Centered => "centered",
            Self::Section => "section",
            Self::Synopsis => "synopsis",
            Self::Note => "note",
            Self::PageBreak => "page-break",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == name)
    }
}

impl LineKind {
    /// The block element and the container it sits in, if any.
    fn tags(self) -> (&'static str, Option<&'static str>) {
        match self {
            Self::Paragraph => ("p", None),
            Self::Heading1 => ("h1", None),
            Self::Heading2 => ("h2", None),
            Self::Heading3 => ("h3", None),
            Self::Quote => ("p", Some("blockquote")),
            Self::Bullet | Self::Numbered | Self::ListItem { .. } => ("li", None),
            Self::Eyebrow | Self::Screenplay { .. } | Self::EmptyParagraph => ("p", None),
            Self::Block => ("", None),
            Self::Bare { quote } => ("", quote.then_some("blockquote")),
        }
    }
}

/// How deep nesting can go (bits of [`LineKind::ListItem`]'s `ordered`).
const MAX_LIST_DEPTH: usize = 8;

impl LineKind {
    /// A list item at the end of `path` (each list's numbered-ness, outermost first).
    fn list_item(path: &[bool]) -> Option<Self> {
        match path {
            [] => None,
            [false] => Some(Self::Bullet),
            [true] => Some(Self::Numbered),
            _ if path.len() <= MAX_LIST_DEPTH => Some(Self::ListItem {
                depth: (path.len() - 1) as u8,
                ordered: path
                    .iter()
                    .enumerate()
                    .fold(0, |mask, (n, o)| mask | (u8::from(*o) << n)),
            }),
            _ => None,
        }
    }

    /// The lists a list item sits in, outermost first (`ul` or `ol`).
    pub fn lists(self) -> Vec<&'static str> {
        let tag = |ordered: bool| if ordered { "ol" } else { "ul" };
        match self {
            Self::Bullet => vec!["ul"],
            Self::Numbered => vec!["ol"],
            Self::ListItem { depth, ordered } => {
                (0..=depth).map(|n| tag(ordered >> n & 1 == 1)).collect()
            }
            _ => Vec::new(),
        }
    }

    /// How deep a list item is (0 for an outermost one).
    pub fn list_depth(self) -> Option<usize> {
        match self {
            Self::Bullet | Self::Numbered => Some(0),
            Self::ListItem { depth, .. } => Some(usize::from(depth)),
            _ => None,
        }
    }

    /// The kind a line looks like and is styled as: inline content in no block and an empty
    /// paragraph without its `<br>` are paragraphs (or quotes, in a blockquote).
    pub fn shown(self) -> Self {
        match self {
            Self::Bare { quote: true } => Self::Quote,
            Self::Bare { quote: false } | Self::EmptyParagraph => Self::Paragraph,
            kind => kind,
        }
    }

    /// Whether a list item's own list is numbered.
    pub fn numbered(self) -> bool {
        self.lists().last() == Some(&"ol")
    }
}

/// A container element a line sits in (see [`Frame`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContainerTag {
    Div,
    Blockquote,
    Ul,
    Ol,
    Li,
}

impl ContainerTag {
    fn name(self) -> &'static str {
        match self {
            Self::Div => "div",
            Self::Blockquote => "blockquote",
            Self::Ul => "ul",
            Self::Ol => "ol",
            Self::Li => "li",
        }
    }

    /// The container an element is (one without attributes, as ProseCraft's sanitizer keeps
    /// them).
    fn of(element: &Element) -> Option<Self> {
        if !element.attrs.is_empty() || is_kept_div(element) {
            return None;
        }
        Some(match element.name.as_str() {
            "div" => Self::Div,
            "blockquote" => Self::Blockquote,
            "ul" => Self::Ul,
            "ol" => Self::Ol,
            "li" => Self::Li,
            _ => return None,
        })
    }
}

/// One container element. Neighbouring lines whose containers have the same id at the same
/// place sit in the same element.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Container {
    pub tag: ContainerTag,
    pub id: u32,
}

/// How deep a line's containers can nest.
const MAX_CONTAINERS: usize = 32;

/// Where a line sits when its kind can't say it, in the shapes ProseCraft keeps: a `<div>` (a
/// block WebKit's Enter makes, an import's wrapper), paragraphs and other blocks in a list
/// item, a paragraph straight in a list, a `<div>` or a list in a blockquote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// The containers, outermost first.
    pub containers: Vec<Container>,
    /// What the line is written as in them: a paragraph (or an empty one), a heading, a
    /// caption, a screenplay element, a kept block, or inline content straight in the
    /// innermost container ([`LineKind::Bare`]).
    pub block: LineKind,
    /// Empty inline content written as nothing (an empty container), not the editor's `<br>`.
    pub no_br: bool,
}

impl Frame {
    /// The kind the line looks like: a list item in a list, a quote in a blockquote.
    pub fn shown(&self) -> LineKind {
        if !matches!(
            self.block,
            LineKind::Paragraph | LineKind::EmptyParagraph | LineKind::Bare { .. }
        ) {
            return self.block;
        }
        let lists: Vec<bool> = self
            .containers
            .iter()
            .filter_map(|c| match c.tag {
                ContainerTag::Ul => Some(false),
                ContainerTag::Ol => Some(true),
                _ => None,
            })
            .collect();
        let deepest = lists
            .get(..lists.len().min(MAX_LIST_DEPTH))
            .unwrap_or_default();
        if let Some(kind) = LineKind::list_item(deepest) {
            return kind;
        }
        if self
            .containers
            .iter()
            .any(|c| c.tag == ContainerTag::Blockquote)
        {
            return LineKind::Quote;
        }
        self.block
    }

    /// The frame of a line Enter starts after this one: in a `<div>` block or a list item
    /// holding its text, the next one (as WebKit's Enter and a list do); else the next block in
    /// the same containers.
    fn follow(&self, fresh: u32) -> Self {
        let mut next = self.clone();
        next.no_br = false;
        match (self.block, next.containers.last_mut()) {
            (LineKind::Bare { .. }, Some(last))
                if matches!(last.tag, ContainerTag::Div | ContainerTag::Li) =>
            {
                last.id = fresh;
            }
            (block, _) => next.block = follow_kind(block),
        }
        next
    }

    /// The same line, as a kept block, or as text (a kept block's line typed into).
    fn with_block(&self, block: LineKind) -> Self {
        Self {
            containers: self.containers.clone(),
            block,
            no_br: false,
        }
    }
}

/// The kind of the line Enter starts after a line of `kind`.
fn follow_kind(kind: LineKind) -> LineKind {
    match kind {
        LineKind::Heading1
        | LineKind::Heading2
        | LineKind::Heading3
        | LineKind::Eyebrow
        | LineKind::Block
        | LineKind::EmptyParagraph
        | LineKind::Bare { quote: false } => LineKind::Paragraph,
        LineKind::Bare { quote: true } => LineKind::Quote,
        // A new screenplay element is written as the editor writes it (with its `<br>`, and
        // without a caption's class, as ProseCraft's Enter makes a plain `<p data-sp>`).
        LineKind::Screenplay { kind, .. } => LineKind::Screenplay {
            kind,
            bare: false,
            eyebrow: false,
        },
        other => other,
    }
}

/// An inline element (`em`, `strong`, `a href=…`) wrapping a character.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Tag {
    pub name: String,
    pub attrs: Vec<(String, String)>,
}

impl Tag {
    pub fn plain(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            attrs: Vec::new(),
        }
    }
}

const INLINE: &[&str] = &[
    "a", "b", "code", "del", "em", "i", "s", "strike", "strong", "u",
];

/// A scene for editing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichDoc {
    /// The text: one `\n` between blocks, [`ATOM`] for each atom, [`SOFT_BREAK`] for a newline
    /// in the text and [`HARD_BREAK`] for a `<br>`.
    pub text: String,
    /// One kind per line.
    pub lines: Vec<LineKind>,
    /// One tag stack per character of `text` (outermost first); `\n` has an empty one.
    pub marks: Vec<Vec<Tag>>,
    /// The HTML of each atom, in the order their characters appear.
    pub atoms: Vec<String>,
    /// Whether blocks are separated by newlines (ProseCraft's Markdown output) or not (its
    /// editor's).
    pub newline_blocks: bool,
    /// With newlines between blocks, none after the last (as the Wiki migration joins a
    /// profile and a page's text).
    pub no_final_newline: bool,
    /// The scene was empty (`""`) and hasn't been written in; it stays `""`.
    pub empty_source: bool,
    /// With newlines between blocks, none inside a blockquote or a list (as ProseCraft's EPUB
    /// import joins the blocks it reads).
    pub compact_containers: bool,
    /// Each line's [`Frame`], for a scene with containers its kinds can't say; empty otherwise.
    /// A line without one is written by its kind alone, outside them.
    pub frames: Vec<Option<Frame>>,
}

/// The formatting buttons: each toggles one inline element (and its synonyms).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Bold,
    Italic,
    Underline,
    Strike,
}

impl Mark {
    /// The element ProseCraft's own HTML uses for it, and the ones that mean the same.
    fn names(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Bold => ("strong", &["strong", "b"]),
            Self::Italic => ("em", &["em", "i"]),
            Self::Underline => ("u", &["u"]),
            Self::Strike => ("s", &["s", "strike", "del"]),
        }
    }

    pub fn applies(self, stack: &[Tag]) -> bool {
        let (_, names) = self.names();
        stack.iter().any(|t| names.contains(&t.name.as_str()))
    }
}

fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            SOFT_BREAK => out.push('\n'),
            c => out.push(c),
        }
    }
    out
}

fn escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\u{a0}', "&nbsp;")
}

fn open_tag(name: &str, attrs: &[(String, String)]) -> String {
    let mut out = format!("<{name}");
    for (key, value) in attrs {
        out.push_str(&format!(" {key}=\"{}\"", escape_attr(value)));
    }
    out.push('>');
    out
}

/// An element serialized as the browser's `outerHTML` would (for atoms).
fn outer_html(element: &Element) -> String {
    let mut out = open_tag(&element.name, &element.attrs);
    if element.name == "img" || element.name == "br" || element.name == "hr" {
        return out;
    }
    for child in &element.children {
        match child {
            Node::Text(text) => out.push_str(&escape_text(text)),
            Node::Element(e) => out.push_str(&outer_html(e)),
        }
    }
    out.push_str(&format!("</{}>", element.name));
    out
}

/// `addLink`: the address typed for a link as the link's `href` (`https://` added when it has
/// no scheme), or ProseCraft's message for why it can't be one.
pub fn link_href(entered: &str) -> Result<String, &'static str> {
    let value = entered.trim_matches(crate::html::is_js_whitespace);
    let has_scheme = value.split_once(':').is_some_and(|(scheme, _)| {
        scheme
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
    });
    let href = if has_scheme {
        value.to_owned()
    } else {
        format!("https://{value}")
    };
    let url = url::Url::parse(&href).map_err(|_| "Please enter a valid link.")?;
    if !matches!(url.scheme(), "http" | "https" | "mailto") {
        return Err("Please use a web link or email address.");
    }
    Ok(url.to_string())
}

/// The rich placeholders that stand as blocks (`src/lib/rich/model.ts`).
const RICH_BLOCK_CLASSES: [&str; 3] = ["math-block", "mermaid-diagram", "calc-worksheet"];
/// ProseCraft's scene break between the scenes of a chapter (`SCENE_BREAK_CLASS`).
const SCENE_BREAK_CLASS: &str = "scene-break";

/// Whether an atom's HTML is a block (a [`LineKind::Block`] line's), not an inline marker.
pub fn is_block_atom(html: &str) -> bool {
    ["<hr", "<pre", "<table", "<div"]
        .iter()
        .any(|tag| html.starts_with(tag))
}

/// What a kept block is, and the lines the editor shows for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockPreview {
    pub kind: BlockKind,
    /// Its text: a code block's lines, a table's rows, an equation's or diagram's source.
    pub lines: Vec<String>,
    /// More lines than are shown.
    pub more: bool,
    /// An equation's TeX, or a diagram's or calculation's source (else empty).
    pub source: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Rule,
    Code,
    Table,
    Equation,
    Diagram,
    Calculation,
    /// The break between two scenes of a chapter, with its glyph.
    SceneBreak,
}

impl BlockKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Rule => "Rule",
            Self::Code => "Code",
            Self::Table => "Table",
            Self::Equation => "Equation",
            Self::Diagram => "Diagram",
            Self::Calculation => "Calculation",
            Self::SceneBreak => "Scene break",
        }
    }
}

/// How many lines of a block the editor shows.
const PREVIEW_LINES: usize = 12;

/// A kept block's kind and text, from its HTML.
pub fn block_preview(html: &str) -> Option<BlockPreview> {
    let nodes = parse_fragment(html);
    let Some(Node::Element(element)) = nodes.first() else {
        return None;
    };
    fn text_of(nodes: &[Node], out: &mut String) {
        for node in nodes {
            match node {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => text_of(&e.children, out),
            }
        }
    }
    fn rows(nodes: &[Node], out: &mut Vec<String>) {
        for node in nodes {
            if let Node::Element(e) = node {
                if e.name == "tr" {
                    let cells: Vec<String> = e
                        .children
                        .iter()
                        .filter_map(|c| match c {
                            Node::Element(cell) if cell.name == "td" || cell.name == "th" => {
                                let mut text = String::new();
                                text_of(&cell.children, &mut text);
                                Some(text.split_whitespace().collect::<Vec<_>>().join(" "))
                            }
                            _ => None,
                        })
                        .collect();
                    out.push(cells.join("  │  "));
                } else {
                    rows(&e.children, out);
                }
            }
        }
    }
    let mut raw = String::new();
    let (kind, lines): (BlockKind, Vec<String>) = match element.name.as_str() {
        "hr" => (BlockKind::Rule, Vec::new()),
        "pre" => {
            let mut text = String::new();
            text_of(&element.children, &mut text);
            let text = text.strip_suffix('\n').unwrap_or(&text);
            (
                BlockKind::Code,
                text.split('\n').map(str::to_owned).collect(),
            )
        }
        "table" => {
            let mut out = Vec::new();
            rows(&element.children, &mut out);
            (BlockKind::Table, out)
        }
        "div" if element.has_class(SCENE_BREAK_CLASS) => {
            let mut text = String::new();
            text_of(&element.children, &mut text);
            if element.has_class("scene-break-line") {
                (BlockKind::Rule, Vec::new())
            } else {
                (BlockKind::SceneBreak, vec![text])
            }
        }
        "div" => {
            let (kind, attr) = if element.has_class("math-block") {
                (BlockKind::Equation, "data-tex")
            } else if element.has_class("mermaid-diagram") {
                (BlockKind::Diagram, "data-source")
            } else if element.has_class("calc-worksheet") {
                (BlockKind::Calculation, "data-source")
            } else {
                return None;
            };
            let source = element.attr(attr).unwrap_or_default();
            raw = source.to_owned();
            // A calculation shows its results (`worksheetToPlainText`); drawing it as
            // ProseCraft's typeset worksheet comes with equations.
            let text = if kind == BlockKind::Calculation {
                crate::calc::worksheet_to_plain_text(source)
            } else {
                source.to_owned()
            };
            (kind, text.lines().map(str::to_owned).collect())
        }
        _ => return None,
    };
    let more = lines.len() > PREVIEW_LINES;
    Some(BlockPreview {
        kind,
        lines: lines.into_iter().take(PREVIEW_LINES).collect(),
        more,
        source: raw,
    })
}

fn is_atom(element: &Element) -> bool {
    element.name == "img"
        || (element.name == "sup" && element.has_class("footnote-ref"))
        || (element.name == "span"
            && (element.has_class("cite-ref") || element.has_class("math-inline")))
}

/// The rich placeholders and the scene break, kept whole as blocks.
fn is_kept_div(element: &Element) -> bool {
    element.name == "div"
        && (RICH_BLOCK_CLASSES.iter().any(|c| element.has_class(c))
            || element.has_class(SCENE_BREAK_CLASS))
}

/// The elements that start a block; anything else at the top of a scene (or directly in a
/// blockquote) is inline content in no block ([`LineKind::Bare`]).
const BLOCK_NAMES: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "div",
    "dl",
    "figure",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "ul",
];

fn is_inline_node(node: &Node) -> bool {
    match node {
        Node::Text(_) => !is_whitespace(node),
        Node::Element(e) => !BLOCK_NAMES.contains(&e.name.as_str()),
    }
}

/// Reading HTML into a document; `None` as soon as something isn't representable.
struct Reader {
    doc: RichDoc,
    /// Inline content in no block gives up the newline that separates it from the blocks
    /// around it (a document whose blocks are joined by newlines).
    trim_bare: bool,
    /// Every line is read with its containers ([`Frame`]).
    framed: bool,
    /// The containers open, outermost first.
    path: Vec<Container>,
    /// The last container's id.
    next_id: u32,
}

impl Reader {
    fn push_char(&mut self, c: char, stack: &[Tag]) {
        self.doc.text.push(c);
        self.doc.marks.push(stack.to_vec());
    }

    fn inline(&mut self, nodes: &[Node], stack: &mut Vec<Tag>) -> Option<()> {
        for node in nodes {
            match node {
                Node::Text(text) => {
                    for c in text.chars() {
                        if OWN_CHARS.contains(&c) {
                            self.doc.atoms.push(c.to_string());
                            self.push_char(ATOM, stack);
                            continue;
                        }
                        let c = if c == '\n' { SOFT_BREAK } else { c };
                        self.push_char(c, stack);
                    }
                }
                Node::Element(e) if is_atom(e) => {
                    self.doc.atoms.push(outer_html(e));
                    self.push_char(ATOM, stack);
                }
                Node::Element(e) if e.name == "br" && e.attrs.is_empty() => {
                    self.push_char(HARD_BREAK, stack);
                }
                // An empty inline element (`<em></em>`, `<a></a>`) has no character to carry
                // it, so it is kept as an atom.
                Node::Element(e) if INLINE.contains(&e.name.as_str()) && e.children.is_empty() => {
                    self.doc.atoms.push(outer_html(e));
                    self.push_char(ATOM, stack);
                }
                Node::Element(e) if INLINE.contains(&e.name.as_str()) => {
                    stack.push(Tag {
                        name: e.name.clone(),
                        attrs: e.attrs.clone(),
                    });
                    self.inline(&e.children, stack)?;
                    stack.pop();
                }
                Node::Element(_) => return None,
            }
        }
        Some(())
    }

    /// A `<p data-sp>` paragraph: a screenplay element (empty without a `<br>` too), a caption
    /// too when its class is `eyebrow` (written first, as ProseCraft's sanitizer writes it).
    fn screenplay(&mut self, element: &Element) -> Option<()> {
        let (eyebrow, value) = match element.attrs.as_slice() {
            [(name, value)] if name == "data-sp" => (false, value),
            [(class, eyebrow), (name, value)]
                if class == "class" && eyebrow == "eyebrow" && name == "data-sp" =>
            {
                (true, value)
            }
            _ => return None,
        };
        let kind = ScreenplayKind::from_name(value)?;
        let bare = element.children.is_empty();
        let mut plain = element.clone();
        plain.attrs.clear();
        self.block(
            &plain,
            LineKind::Screenplay {
                kind,
                bare,
                eyebrow,
            },
        )
    }

    /// A list (`ul` or `ol`) at the end of `path`: its items, and the lists inside them.
    fn list(&mut self, element: &Element, path: &mut Vec<bool>) -> Option<()> {
        if !element.attrs.is_empty() {
            return None;
        }
        path.push(element.name == "ol");
        for child in element.children.iter().filter(|n| !is_whitespace(n)) {
            match child {
                Node::Element(li) if li.name == "li" => self.item(li, path)?,
                _ => return None,
            }
        }
        path.pop();
        Some(())
    }

    /// A list item: its own text (a line), then the lists inside it. An item holding only a
    /// list has no line of its own.
    fn item(&mut self, li: &Element, path: &mut Vec<bool>) -> Option<()> {
        let is_list = |n: &Node| matches!(n, Node::Element(e) if e.name == "ul" || e.name == "ol");
        let split = li
            .children
            .iter()
            .position(is_list)
            .unwrap_or(li.children.len());
        let (own, nested) = li.children.split_at(split);
        if !own.is_empty() || nested.is_empty() {
            let mut line = li.clone();
            line.children = own.to_vec();
            self.block(&line, LineKind::list_item(path)?)?;
        }
        for node in nested.iter().filter(|n| !is_whitespace(n)) {
            match node {
                Node::Element(e) if is_list(node) => self.list(e, path)?,
                _ => return None,
            }
        }
        Some(())
    }

    /// Starts a line of `kind` (in the containers open, when framed).
    fn new_line(&mut self, kind: LineKind) {
        if !self.doc.lines.is_empty() {
            self.doc.text.push('\n');
            self.doc.marks.push(Vec::new());
        }
        if self.framed {
            let frame = Frame {
                containers: self.path.clone(),
                block: kind,
                no_br: false,
            };
            self.doc.lines.push(frame.shown());
            self.doc.frames.push(Some(frame));
        } else {
            self.doc.lines.push(kind);
        }
    }

    /// A container and what it holds; an empty one is a line with nothing in it.
    fn container(&mut self, element: &Element, tag: ContainerTag) -> Option<()> {
        if self.path.len() >= MAX_CONTAINERS {
            return None;
        }
        self.next_id = self.next_id.saturating_add(1);
        self.path.push(Container {
            tag,
            id: self.next_id,
        });
        let lines = self.doc.lines.len();
        self.blocks(&element.children, false)?;
        if self.doc.lines.len() == lines {
            self.new_line(LineKind::Bare { quote: false });
            if let Some(Some(frame)) = self.doc.frames.last_mut() {
                frame.no_br = true;
            }
        }
        self.path.pop();
        Some(())
    }

    /// A block kept whole: one atom alone on its line.
    fn block_atom(&mut self, element: &Element) {
        self.new_line(LineKind::Block);
        self.doc.atoms.push(outer_html(element));
        self.push_char(ATOM, &[]);
    }

    /// One block: its children are inline content; ProseCraft's empty block is `<br>` alone.
    fn block(&mut self, element: &Element, kind: LineKind) -> Option<()> {
        if !element.attrs.is_empty() {
            return None;
        }
        let kind = match kind {
            LineKind::Paragraph if element.children.is_empty() => LineKind::EmptyParagraph,
            kind => kind,
        };
        self.new_line(kind);
        match element.children.as_slice() {
            [Node::Element(br)] if br.name == "br" && br.attrs.is_empty() => Some(()),
            children => self.inline(children, &mut Vec::new()),
        }
    }

    /// Inline content in no block: one line of its own.
    fn bare(&mut self, nodes: &[Node], quote: bool) -> Option<()> {
        // In a container, a `<br>` alone is the editor's empty line.
        if self.framed
            && !self.path.is_empty()
            && matches!(nodes, [Node::Element(br)] if br.name == "br" && br.attrs.is_empty())
        {
            self.new_line(LineKind::Bare { quote });
            return Some(());
        }
        let mut nodes = nodes.to_vec();
        if self.trim_bare {
            if let Some(Node::Text(first)) = nodes.first_mut()
                && !self.doc.lines.is_empty()
                && let Some(rest) = first.strip_prefix('\n')
            {
                *first = rest.to_owned();
            }
            if let Some(Node::Text(last)) = nodes.last_mut()
                && let Some(rest) = last.strip_suffix('\n')
            {
                *last = rest.to_owned();
            }
        }
        self.new_line(LineKind::Bare { quote });
        self.inline(&nodes, &mut Vec::new())
    }

    /// A run of blocks and of inline content in no block: the top of the scene, or a
    /// blockquote's children (`quote`).
    fn blocks(&mut self, nodes: &[Node], quote: bool) -> Option<()> {
        let mut i = 0;
        while let Some(node) = nodes.get(i) {
            if is_inline_node(node) {
                let start = i;
                while nodes.get(i).is_some_and(is_inline_node) {
                    i += 1;
                }
                self.bare(nodes.get(start..i).unwrap_or_default(), quote)?;
                continue;
            }
            i += 1;
            match node {
                Node::Element(e) if quote && e.name == "p" => self.block(e, LineKind::Quote)?,
                Node::Element(e) if !quote => self.top_block(e)?,
                Node::Element(_) => return None,
                Node::Text(_) => {}
            }
        }
        Some(())
    }

    /// One block at the top of the scene (or, framed, in a container).
    fn top_block(&mut self, element: &Element) -> Option<()> {
        if self.framed
            && let Some(tag) = ContainerTag::of(element)
        {
            return self.container(element, tag);
        }
        match element.name.as_str() {
            "p" if element.attr("data-sp").is_some() => self.screenplay(element)?,
            "p" if element.attrs == [("class".to_owned(), "eyebrow".to_owned())] => {
                let mut plain = element.clone();
                plain.attrs.clear();
                self.block(&plain, LineKind::Eyebrow)?;
            }
            "hr" | "pre" | "table" => self.block_atom(element),
            "div" if is_kept_div(element) => self.block_atom(element),
            "p" => self.block(element, LineKind::Paragraph)?,
            "h1" => self.block(element, LineKind::Heading1)?,
            "h2" => self.block(element, LineKind::Heading2)?,
            "h3" => self.block(element, LineKind::Heading3)?,
            "blockquote" if element.attrs.is_empty() => {
                let lines = self.doc.lines.len();
                self.blocks(&element.children, true)?;
                // An empty blockquote has no line to hold it.
                if self.doc.lines.len() == lines {
                    return None;
                }
            }
            "ul" | "ol" => self.list(element, &mut Vec::new())?,
            _ => return None,
        }
        Some(())
    }
}

fn is_whitespace(node: &Node) -> bool {
    matches!(node, Node::Text(t) if t.chars().all(|c| c == '\n'))
}

/// What a scene holds, whatever its shape: its text without white space, and the HTML of
/// each atom and kept block, in order.
fn content_of(html: &str) -> (String, Vec<String>) {
    fn walk(nodes: &[Node], text: &mut String, atoms: &mut Vec<String>) {
        for node in nodes {
            match node {
                Node::Text(t) => text.extend(t.chars().filter(|c| !c.is_whitespace())),
                Node::Element(e)
                    if is_atom(e)
                        || is_kept_div(e)
                        || ["hr", "pre", "table"].contains(&e.name.as_str()) =>
                {
                    atoms.push(outer_html(e))
                }
                Node::Element(e) => walk(&e.children, text, atoms),
            }
        }
    }
    let mut text = String::new();
    let mut atoms = Vec::new();
    walk(&parse_fragment(html), &mut text, &mut atoms);
    (text, atoms)
}

/// What an atom or kept block is, whatever its attributes: its element and classes.
fn atom_kind(html: &str) -> String {
    match parse_fragment(html).first() {
        Some(Node::Element(e)) => format!("{} {}", e.name, e.attr("class").unwrap_or_default()),
        _ => String::new(),
    }
}

/// [`RichDoc::from_html_normalized`]'s own changes: a `<div>` that isn't a kept block gives
/// way to its children, and a paragraph in a list (in an item, or straight in the list)
/// becomes an item of its own.
fn normalize_nodes(nodes: Vec<Node>) -> Vec<Node> {
    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node {
            Node::Element(e) if e.name == "div" && !is_kept_div(&e) => {
                out.extend(normalize_nodes(e.children));
            }
            Node::Element(mut e) if e.name == "ul" || e.name == "ol" => {
                let items = std::mem::take(&mut e.children);
                e.children = items
                    .into_iter()
                    .flat_map(|item| match item {
                        Node::Element(li) if li.name == "li" => {
                            split_item(li).into_iter().map(Node::Element).collect()
                        }
                        // A paragraph straight in a list (as a stray end tag leaves one).
                        Node::Element(p) if p.name == "p" && p.attrs.is_empty() => {
                            vec![Node::Element(Element {
                                name: "li".to_owned(),
                                ..p
                            })]
                        }
                        other => vec![other],
                    })
                    .collect();
                out.push(Node::Element(e));
            }
            Node::Element(mut e) if e.name == "blockquote" => {
                e.children = normalize_nodes(std::mem::take(&mut e.children));
                out.push(Node::Element(e));
            }
            other => out.push(other),
        }
    }
    out
}

/// A list item holding paragraphs as items: its own text, then one per paragraph, each
/// followed by the lists that came after it.
fn split_item(li: Element) -> Vec<Element> {
    let item = |children: Vec<Node>| Element {
        name: "li".to_owned(),
        attrs: li.attrs.clone(),
        children,
    };
    let mut items: Vec<Element> = Vec::new();
    let mut current: Vec<Node> = Vec::new();
    for child in normalize_nodes(li.children.clone()) {
        match child {
            Node::Element(p) if p.name == "p" && p.attrs.is_empty() => {
                if current.iter().any(|n| !is_whitespace(n)) {
                    items.push(item(std::mem::take(&mut current)));
                }
                current.clear();
                items.push(item(p.children));
            }
            Node::Element(list)
                if (list.name == "ul" || list.name == "ol")
                    && current.iter().all(is_whitespace) =>
            {
                match items.last_mut() {
                    Some(last) => last.children.push(Node::Element(list)),
                    None => current.push(Node::Element(list)),
                }
            }
            other => current.push(other),
        }
    }
    if items.is_empty() || current.iter().any(|n| !is_whitespace(n)) {
        items.push(item(current));
    }
    items
}

/// How the blocks of a document are joined: (`newline_blocks`, `no_final_newline`,
/// `compact_containers`).
const LAYOUTS: [(bool, bool, bool); 5] = [
    (false, false, false),
    (true, false, false),
    (true, true, false),
    (true, false, true),
    (true, true, true),
];

impl RichDoc {
    /// The scene as a document, only if writing it back gives exactly `html`.
    pub fn from_html(html: &str) -> Option<Self> {
        if html.is_empty() {
            return Some(Self {
                lines: vec![LineKind::Paragraph],
                empty_source: true,
                ..Self::default()
            });
        }
        let nodes = parse_fragment(html);
        // By kinds alone first; then with each line's containers.
        [false, true]
            .into_iter()
            .find_map(|framed| Self::read_exactly(html, &nodes, framed))
    }

    /// [`Self::from_html`], read by kinds alone or `framed`.
    fn read_exactly(html: &str, nodes: &[Node], framed: bool) -> Option<Self> {
        for trim_bare in [false, true] {
            let mut reader = Reader {
                doc: RichDoc::default(),
                trim_bare,
                framed,
                path: Vec::new(),
                next_id: 0,
            };
            if reader.blocks(nodes, false).is_none() || reader.doc.lines.is_empty() {
                return None;
            }
            let mut doc = reader.doc;
            let bare = doc.lines.iter().any(|k| matches!(k, LineKind::Bare { .. }))
                || doc
                    .frames
                    .iter()
                    .flatten()
                    .any(|f| matches!(f.block, LineKind::Bare { .. }));
            for (newline_blocks, no_final_newline, compact_containers) in LAYOUTS {
                if trim_bare && !newline_blocks {
                    continue;
                }
                doc.newline_blocks = newline_blocks;
                doc.no_final_newline = no_final_newline;
                doc.compact_containers = compact_containers;
                if doc.to_html() == html {
                    return Some(doc);
                }
            }
            // Without inline content in no block, trimming reads the same.
            if !bare {
                break;
            }
        }
        None
    }

    /// Line `line`'s [`Frame`], if it has one.
    pub fn frame(&self, line: usize) -> Option<&Frame> {
        self.frames.get(line).and_then(Option::as_ref)
    }

    /// An id no container has.
    fn fresh_id(&self) -> u32 {
        self.frames
            .iter()
            .flatten()
            .flat_map(|f| f.containers.iter().map(|c| c.id))
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    /// Whether a list item's line starts its item (has its bullet or number): not a second
    /// block in one item, nor a paragraph straight in a list.
    pub fn starts_item(&self, line: usize) -> bool {
        let Some(frame) = self.frame(line) else {
            return true;
        };
        let is_list = |c: &Container| matches!(c.tag, ContainerTag::Ul | ContainerTag::Ol);
        let item = frame
            .containers
            .iter()
            .rposition(|c| c.tag == ContainerTag::Li);
        let list = frame.containers.iter().rposition(is_list);
        match (item, list) {
            (Some(item), list) if list.is_none_or(|l| l < item) => {
                let open = frame.containers.get(..=item).unwrap_or_default();
                line == 0
                    || !self
                        .frame(line - 1)
                        .is_some_and(|above| above.containers.starts_with(open))
            }
            _ => false,
        }
    }

    /// A scene [`Self::from_html`] can't hold, as a document that writes it in a shape it can:
    /// first as ProseCraft's sanitizer cleans it (what ProseCraft's editor stores once the
    /// scene is edited), then (for containers nested deeper than a [`Frame`] holds) with a
    /// `<div>` wrapper unwrapped and the paragraphs in a list item (or straight in the list)
    /// made items of their own. Only if every character of
    /// text and every footnote, citation, equation, picture and kept block is still there, in
    /// order; else `None`.
    ///
    /// The document's HTML differs from `html`, so it should be stored only once the scene is
    /// really edited: opening the scene must not change it.
    pub fn from_html_normalized(html: &str) -> Option<Self> {
        let sanitized = crate::html::sanitize_html(html);
        let unwrapped = crate::html::serialize(&normalize_nodes(parse_fragment(&sanitized)));
        // The text as it was; the atoms as the sanitizer keeps them (it drops attributes
        // ProseCraft doesn't keep, as its editor does on any edit), but every one of them.
        let (text, atoms) = content_of(html);
        let want = (text, content_of(&sanitized).1);
        let kinds =
            |atoms: &[String]| -> Vec<String> { atoms.iter().map(|a| atom_kind(a)).collect() };
        if kinds(&atoms) != kinds(&want.1) {
            return None;
        }
        [sanitized, unwrapped].into_iter().find_map(|candidate| {
            Self::from_html(&candidate)
                .filter(|doc| !doc.empty_source && content_of(&doc.to_html()) == want)
        })
    }
    /// The text of each line with its character range in `text`.
    pub fn line_ranges(&self) -> Vec<(usize, usize)> {
        let mut ranges = Vec::new();
        let mut start = 0;
        for (i, c) in self.text.chars().enumerate() {
            if c == '\n' {
                ranges.push((start, i));
                start = i + 1;
            }
        }
        ranges.push((start, self.text.chars().count()));
        ranges
    }

    /// The document as ProseCraft's HTML.
    pub fn to_html(&self) -> String {
        self.written_html(&mut Vec::new())
    }

    /// [`Self::to_html`], and where in it each character was written (see
    /// [`Self::write_html`]).
    fn written_html(&self, spans: &mut Vec<std::ops::Range<usize>>) -> String {
        let html = self.write_html(spans);
        match html.strip_suffix('\n') {
            Some(kept) if self.newline_blocks && self.no_final_newline => kept.to_owned(),
            _ => html,
        }
    }

    /// The document as ProseCraft's HTML, and where in it each character was written (byte
    /// ranges; empty for the `\n` between lines, which is written as tags).
    fn write_html(&self, spans: &mut Vec<std::ops::Range<usize>>) -> String {
        let chars: Vec<char> = self.text.chars().collect();
        spans.clear();
        spans.resize(chars.len(), 0..0);
        if self.empty_source && self.text.is_empty() {
            return String::new();
        }
        let nl = if self.newline_blocks { "\n" } else { "" };
        let mut writer = Writer {
            out: String::new(),
            nl,
            inner: if self.compact_containers { "" } else { nl },
            container: None,
            lists: Vec::new(),
            framed: Vec::new(),
            atoms: self.atoms.iter(),
            chars: &chars,
            spans,
        };
        let mut previous: Option<LineKind> = None;
        let mut previous_frame: Option<(Vec<Container>, LineKind)> = None;
        for (index, (start, end)) in self.line_ranges().into_iter().enumerate() {
            if let Some(frame) = self.frame(index) {
                let block = self.written_block(frame, previous_frame.as_ref(), start == end);
                let nothing = frame.no_br && start == end && self.alone(index);
                writer.framed_line(&frame.containers, (block, nothing), start..end, &self.marks);
                previous = Some(frame.with_block(block).shown());
                previous_frame = Some((frame.containers.clone(), block));
                continue;
            }
            previous_frame = None;
            let stored = self
                .lines
                .get(index)
                .copied()
                .unwrap_or(LineKind::Paragraph);
            let kind = self.written_kind(stored, previous, start == end);
            previous = Some(kind);
            writer.line(kind, start..end, &self.marks);
        }
        writer.close_lists(0);
        writer.set_container(None);
        writer.close_framed(0);
        writer.out
    }

    /// How a framed line is written (as [`Self::written_kind`]): emptied inline content at the
    /// top is a paragraph, and so is inline content that would run into the line before it in
    /// the same container; an empty paragraph typed into is an ordinary one.
    fn written_block(
        &self,
        frame: &Frame,
        previous: Option<&(Vec<Container>, LineKind)>,
        empty: bool,
    ) -> LineKind {
        let runs_on = previous.is_some_and(|(containers, block)| {
            *containers == frame.containers && matches!(block, LineKind::Bare { .. })
        }) && (!self.newline_blocks || !frame.containers.is_empty());
        match frame.block {
            LineKind::Bare { .. } if (empty && frame.containers.is_empty()) || runs_on => {
                LineKind::Paragraph
            }
            LineKind::EmptyParagraph if !empty => LineKind::Paragraph,
            block => block,
        }
    }

    /// Whether line `line` is alone in its innermost container (no line beside it shares it).
    fn alone(&self, line: usize) -> bool {
        let Some(frame) = self.frame(line) else {
            return false;
        };
        let shares = |other: Option<&Frame>| {
            other.is_some_and(|o| o.containers.starts_with(&frame.containers))
        };
        !shares(line.checked_sub(1).and_then(|l| self.frame(l))) && !shares(self.frame(line + 1))
    }

    /// How a line of `kind` is written: inline content in no block that was emptied, or that
    /// would run into the line before it, is a paragraph; an empty paragraph typed into is an
    /// ordinary one.
    fn written_kind(&self, kind: LineKind, previous: Option<LineKind>, empty: bool) -> LineKind {
        match kind {
            LineKind::Bare { quote }
                if empty || (!self.newline_blocks && previous == Some(kind)) =>
            {
                if quote {
                    LineKind::Quote
                } else {
                    LineKind::Paragraph
                }
            }
            LineKind::EmptyParagraph if !empty => LineKind::Paragraph,
            kind => kind,
        }
    }
}

/// Writing a document's lines as HTML: what is open, and where each character went.
struct Writer<'a> {
    out: String,
    /// The newline between top-level blocks.
    nl: &'static str,
    /// The newline inside a blockquote or a list.
    inner: &'static str,
    container: Option<&'static str>,
    /// The lists open, outermost first; each has an item open.
    lists: Vec<&'static str>,
    /// The containers of framed lines open, outermost first.
    framed: Vec<Container>,
    atoms: std::slice::Iter<'a, String>,
    chars: &'a [char],
    spans: &'a mut Vec<std::ops::Range<usize>>,
}

impl Writer<'_> {
    fn close_lists(&mut self, keep: usize) {
        while self.lists.len() > keep {
            if let Some(list) = self.lists.pop() {
                let after = if self.lists.is_empty() {
                    self.nl
                } else {
                    self.inner
                };
                self.out
                    .push_str(&format!("</li>{}</{list}>{after}", self.inner));
            }
        }
    }

    fn set_container(&mut self, wanted: Option<&'static str>) {
        if self.container == wanted {
            return;
        }
        if let Some(open) = self.container {
            self.out.push_str(&format!("</{open}>{}", self.nl));
        }
        if let Some(open) = wanted {
            self.out.push_str(&format!("<{open}>{}", self.inner));
        }
        self.container = wanted;
    }

    /// Closes framed lines' containers down to the first `keep`.
    fn close_framed(&mut self, keep: usize) {
        while self.framed.len() > keep {
            if let Some(container) = self.framed.pop() {
                let after = if self.framed.is_empty() {
                    self.nl
                } else {
                    self.inner
                };
                self.out
                    .push_str(&format!("</{}>{after}", container.tag.name()));
            }
        }
    }

    /// A framed line: its containers (those of the line above kept), then its block; inline
    /// content straight in a container ends with it, and an empty one is the editor's `<br>`
    /// unless it is written as `nothing`.
    fn framed_line(
        &mut self,
        containers: &[Container],
        (block, nothing): (LineKind, bool),
        range: std::ops::Range<usize>,
        marks: &[Vec<Tag>],
    ) {
        self.close_lists(0);
        self.set_container(None);
        let common = self
            .framed
            .iter()
            .zip(containers)
            .take_while(|(a, b)| a == b)
            .count();
        self.close_framed(common);
        for container in containers.get(common..).unwrap_or_default() {
            self.out.push_str(&format!("<{}>", container.tag.name()));
            if container.tag != ContainerTag::Li {
                self.out.push_str(self.inner);
            }
            self.framed.push(*container);
        }
        let after = if self.framed.is_empty() {
            self.nl
        } else {
            self.inner
        };
        match block {
            LineKind::Block => {
                self.block_atoms(range);
                self.out.push_str(after);
            }
            LineKind::Bare { .. } => {
                if range.is_empty() && !nothing && !containers.is_empty() {
                    self.out.push_str("<br>");
                }
                self.inline(range, marks);
                if containers.is_empty() {
                    self.out.push_str(self.nl);
                }
            }
            _ => {
                let (tag, _) = block.tags();
                self.open_block(block, tag, range.is_empty());
                self.inline(range, marks);
                self.out.push_str(&format!("</{tag}>{after}"));
            }
        }
    }

    /// A kept block's atom.
    fn block_atoms(&mut self, range: std::ops::Range<usize>) {
        for i in range {
            if self.chars.get(i) == Some(&ATOM) {
                let from = self.out.len();
                let html = self.atoms.next().map(String::as_str).unwrap_or_default();
                self.out.push_str(html);
                if let Some(span) = self.spans.get_mut(i) {
                    *span = from..self.out.len();
                }
            }
        }
    }

    /// The newline after a block: inside its container's, else a top-level one.
    fn after_block(&self) -> &'static str {
        if self.container.is_some() {
            self.inner
        } else {
            self.nl
        }
    }

    fn line(&mut self, kind: LineKind, range: std::ops::Range<usize>, marks: &[Vec<Tag>]) {
        self.close_framed(0);
        let wanted_lists = kind.lists();
        if wanted_lists.is_empty() {
            self.close_lists(0);
        }
        let (block, wanted) = kind.tags();
        self.set_container(wanted);
        if kind == LineKind::Block {
            self.block_atoms(range);
            self.out.push_str(self.nl);
            return;
        }
        let empty = range.is_empty();
        if wanted_lists.is_empty() {
            self.open_block(kind, block, empty);
        } else {
            self.open_item(wanted_lists, empty);
        }
        self.inline(range, marks);
        // A list item stays open: a list inside it may follow.
        if kind.lists().is_empty() {
            if !block.is_empty() {
                self.out.push_str(&format!("</{block}>"));
            }
            self.out.push_str(self.after_block());
        }
    }

    /// A list item: the next of the same list, or the first of lists opened inside the item
    /// open above (or at the top).
    fn open_item(&mut self, wanted_lists: Vec<&'static str>, empty: bool) {
        let common = self
            .lists
            .iter()
            .zip(&wanted_lists)
            .take_while(|(a, b)| a == b)
            .count();
        self.close_lists(common);
        if self.lists.len() == wanted_lists.len() {
            self.out.push_str(&format!("</li>{}<li>", self.inner));
        } else {
            for list in &wanted_lists[self.lists.len()..] {
                self.out.push_str(&format!("<{list}>{}<li>", self.inner));
            }
            self.lists = wanted_lists;
        }
        if empty {
            self.out.push_str("<br>");
        }
    }

    fn open_block(&mut self, kind: LineKind, block: &str, empty: bool) {
        let br = match kind {
            LineKind::Eyebrow => {
                self.out.push_str(r#"<p class="eyebrow">"#);
                empty
            }
            LineKind::Screenplay {
                kind,
                bare,
                eyebrow,
            } => {
                let class = if eyebrow { r#" class="eyebrow""# } else { "" };
                self.out
                    .push_str(&format!(r#"<p{class} data-sp="{}">"#, kind.as_str()));
                empty && !bare
            }
            LineKind::EmptyParagraph => {
                self.out.push_str("<p>");
                false
            }
            LineKind::Bare { .. } => false,
            _ => {
                self.out.push_str(&format!("<{block}>"));
                empty
            }
        };
        if br {
            self.out.push_str("<br>");
        }
    }

    /// A line's characters, each in its inline elements.
    fn inline(&mut self, range: std::ops::Range<usize>, marks: &[Vec<Tag>]) {
        let mut open: Vec<&Tag> = Vec::new();
        for i in range {
            let stack = marks.get(i).map_or(&[][..], Vec::as_slice);
            let common = open
                .iter()
                .zip(stack.iter())
                .take_while(|(a, b)| **a == *b)
                .count();
            while open.len() > common {
                if let Some(tag) = open.pop() {
                    self.out.push_str(&format!("</{}>", tag.name));
                }
            }
            for tag in stack.iter().skip(common) {
                self.out.push_str(&open_tag(&tag.name, &tag.attrs));
                open.push(tag);
            }
            let from = self.out.len();
            match self.chars.get(i) {
                Some(&ATOM) => {
                    let html = self.atoms.next().map(String::as_str).unwrap_or_default();
                    self.out.push_str(html);
                }
                Some(&HARD_BREAK) => self.out.push_str("<br>"),
                Some(c) => self.out.push_str(&escape_text(&c.to_string())),
                None => {}
            }
            if let Some(span) = self.spans.get_mut(i) {
                *span = from..self.out.len();
            }
        }
        while let Some(tag) = open.pop() {
            self.out.push_str(&format!("</{}>", tag.name));
        }
    }
}

/// Where the editor's characters are in ProseCraft's comment text (`commentText`: the text of
/// every text node of the scene's HTML, widgets left out). A scene written from Markdown has
/// newlines between its blocks, which the text holds and the editor doesn't show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommentMap {
    /// The comment text, in UTF-16 units.
    pub text: Vec<u16>,
    /// For each editor character (and the end), the comment-text offset where it starts.
    offsets: Vec<usize>,
}

impl CommentMap {
    /// The comment-text offset of editor character `index`.
    pub fn to_comment(&self, index: usize) -> usize {
        self.offsets
            .get(index)
            .or(self.offsets.last())
            .copied()
            .unwrap_or(0)
    }

    /// The editor character at comment-text offset `offset` (the first one at or after it).
    pub fn to_editor(&self, offset: usize) -> usize {
        self.offsets
            .iter()
            .position(|o| *o >= offset)
            .unwrap_or(self.offsets.len().saturating_sub(1))
    }
}

/// The newlines outside tags in HTML that holds only tags and structural newlines.
fn structural_newlines(html: &str) -> usize {
    let mut in_tag = false;
    let mut count = 0;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            '\n' if !in_tag => count += 1,
            _ => {}
        }
    }
    count
}

impl RichDoc {
    /// The comment text of the document as ProseCraft's editor holds it, and where each
    /// character is in it. The editor holds a footnote marker, a citation or an inline
    /// equation as an empty widget (the sanitizer rebuilds them), so their stored text isn't
    /// in it; a kept block's text (a table's, a code block's) is.
    pub fn comment_map(&self) -> CommentMap {
        let mut spans = Vec::new();
        let html = self.written_html(&mut spans);
        let chars: Vec<char> = self.text.chars().collect();
        let mut offsets = Vec::with_capacity(chars.len() + 1);
        let mut text: Vec<u16> = Vec::new();
        let mut written = 0;
        let mut atom = 0;
        let newlines = |text: &mut Vec<u16>, html: &str| {
            text.extend(std::iter::repeat_n(
                u16::from(b'\n'),
                structural_newlines(html),
            ));
        };
        for (i, c) in chars.iter().enumerate() {
            let span = spans.get(i).cloned().unwrap_or(0..0);
            if *c == '\n' || span.is_empty() {
                offsets.push(text.len());
                continue;
            }
            newlines(&mut text, html.get(written..span.start).unwrap_or_default());
            offsets.push(text.len());
            match *c {
                ATOM => {
                    let html = self.atoms.get(atom).map(String::as_str).unwrap_or_default();
                    if is_block_atom(html) {
                        text.extend(crate::comments::comment_text(html));
                    } else if let Some(c) = text_atom(html) {
                        let mut buf = [0u16; 2];
                        text.extend_from_slice(c.encode_utf16(&mut buf));
                    }
                    atom += 1;
                }
                HARD_BREAK => {}
                SOFT_BREAK => text.push(u16::from(b'\n')),
                c => {
                    let mut buf = [0u16; 2];
                    text.extend_from_slice(c.encode_utf16(&mut buf));
                }
            }
            written = span.end;
        }
        offsets.push(text.len());
        newlines(&mut text, html.get(written..).unwrap_or_default());
        CommentMap { text, offsets }
    }

    /// What proofreading checks (`extractProofreadText`): the editor's text nodes in order,
    /// without links, code and atoms (equations, diagrams, calculations, footnotes, citations,
    /// pictures and the other blocks). Each checked character's editor character, or `None`
    /// for a structural newline between blocks.
    pub fn proofread_map(&self) -> (Vec<char>, Vec<Option<usize>>) {
        let mut spans = Vec::new();
        let html = self.written_html(&mut spans);
        let chars: Vec<char> = self.text.chars().collect();
        let mut text = Vec::new();
        let mut at = Vec::new();
        let mut written = 0;
        let mut atom = 0;
        let newlines = |text: &mut Vec<char>, at: &mut Vec<Option<usize>>, html: &str| {
            for _ in 0..structural_newlines(html) {
                text.push('\n');
                at.push(None);
            }
        };
        for (i, c) in chars.iter().enumerate() {
            let span = spans.get(i).cloned().unwrap_or(0..0);
            let own = (*c == ATOM)
                .then(|| text_atom(self.atoms.get(atom).map(String::as_str).unwrap_or_default()))
                .flatten();
            if *c == ATOM {
                atom += 1;
            }
            if *c == '\n' || span.is_empty() {
                continue;
            }
            newlines(
                &mut text,
                &mut at,
                html.get(written..span.start).unwrap_or_default(),
            );
            written = span.end;
            let skipped = self
                .marks
                .get(i)
                .is_some_and(|tags| tags.iter().any(|t| t.name == "a" || t.name == "code"));
            match *c {
                // A text atom's character is text in a text node.
                ATOM if !skipped && own.is_some() => {
                    text.extend(own);
                    at.push(Some(i));
                }
                ATOM | HARD_BREAK => {}
                _ if skipped => {}
                SOFT_BREAK => {
                    text.push('\n');
                    at.push(Some(i));
                }
                c => {
                    text.push(c);
                    at.push(Some(i));
                }
            }
        }
        newlines(&mut text, &mut at, html.get(written..).unwrap_or_default());
        (text, at)
    }

    fn line_of(&self, char_index: usize) -> usize {
        self.text
            .chars()
            .take(char_index)
            .filter(|c| *c == '\n')
            .count()
    }

    /// Takes the editor's new text. Inserted characters get the formatting of the character
    /// before them on the same line (else the one after); new lines continue a list or quote and
    /// follow a heading with a paragraph; deleted atoms are dropped; characters that would forge
    /// an atom are removed. Returns the text as kept (the editor shows it).
    pub fn apply_text(&mut self, new_text: &str) -> String {
        self.apply_text_at(new_text, None)
    }

    /// [`Self::apply_text`] knowing where the cursor is after the edit (a character index). Two
    /// edits can give the same text (Enter at the end of one line or at the start of the next),
    /// and only the cursor tells them apart; without it an insertion is placed as early as it can
    /// be.
    pub fn apply_text_at(&mut self, new_text: &str, cursor: Option<usize>) -> String {
        let old: Vec<char> = self.text.chars().collect();
        let mut new: Vec<char> = new_text.chars().collect();
        let mut prefix = old
            .iter()
            .zip(new.iter())
            .take_while(|(a, b)| a == b)
            .count();
        let max_suffix = old.len().min(new.len()) - prefix;
        let mut suffix = old
            .iter()
            .rev()
            .zip(new.iter().rev())
            .take(max_suffix)
            .take_while(|(a, b)| a == b)
            .count();
        if new.len() > old.len() && prefix + suffix == old.len() {
            // A pure insertion of `n` characters: where exactly?
            let n = new.len() - old.len();
            let fits = |start: usize| {
                old.get(..start) == new.get(..start) && old.get(start..) == new.get(start + n..)
            };
            let at = cursor
                .and_then(|c| c.checked_sub(n))
                .filter(|s| fits(*s))
                .unwrap_or_else(|| {
                    let mut start = prefix;
                    while start > 0 && fits(start - 1) {
                        start -= 1;
                    }
                    start
                });
            prefix = at;
            suffix = old.len() - at;
        }
        let removed = prefix..old.len() - suffix;
        let mut inserted: Vec<char> = new
            .get(prefix..new.len() - suffix)
            .unwrap_or_default()
            .to_vec();
        if inserted.contains(&ATOM) {
            inserted.retain(|c| *c != ATOM);
            new = old
                .get(..prefix)
                .unwrap_or_default()
                .iter()
                .chain(&inserted)
                .chain(old.get(old.len() - suffix..).unwrap_or_default())
                .copied()
                .collect();
        }
        // Atoms in the removed range go with their characters.
        let atoms_before = old
            .get(..prefix)
            .unwrap_or_default()
            .iter()
            .filter(|c| **c == ATOM)
            .count();
        let atoms_removed = old
            .get(removed.clone())
            .unwrap_or_default()
            .iter()
            .filter(|c| **c == ATOM)
            .count();
        self.atoms
            .drain(atoms_before..(atoms_before + atoms_removed).min(self.atoms.len()));
        // Lines: the ones whose breaks were removed go; each inserted break adds one.
        let line = self.line_of(prefix);
        let breaks_removed = old
            .get(removed.clone())
            .unwrap_or_default()
            .iter()
            .filter(|c| **c == '\n')
            .count();
        let kind = self.lines.get(line).copied().unwrap_or(LineKind::Paragraph);
        let end = (line + 1 + breaks_removed).min(self.lines.len());
        self.lines.drain((line + 1).min(end)..end);
        let framed = !self.frames.is_empty();
        if framed {
            let end = (line + 1 + breaks_removed).min(self.frames.len());
            self.frames.drain((line + 1).min(end)..end);
        }
        let own = self.frame(line).cloned();
        let breaks_added = inserted.iter().filter(|c| **c == '\n').count();
        for _ in 0..breaks_added {
            let frame = own.as_ref().map(|f| f.follow(self.fresh_id()));
            let follow = frame.as_ref().map_or(follow_kind(kind), Frame::shown);
            self.lines.insert((line + 1).min(self.lines.len()), follow);
            if framed {
                self.frames.insert((line + 1).min(self.frames.len()), frame);
            }
        }
        // Formatting of the inserted characters.
        let inherit = if prefix > 0 && old.get(prefix - 1).is_some_and(|c| *c != '\n') {
            self.marks.get(prefix - 1).cloned()
        } else if old.get(removed.end).is_some_and(|c| *c != '\n') {
            self.marks.get(removed.end).cloned()
        } else {
            None
        }
        .unwrap_or_default();
        let new_marks: Vec<Vec<Tag>> = inserted
            .iter()
            .map(|c| {
                if *c == '\n' {
                    Vec::new()
                } else {
                    inherit.clone()
                }
            })
            .collect();
        let end = removed.end.min(self.marks.len());
        self.marks.splice(removed.start.min(end)..end, new_marks);
        self.text = new.iter().collect();
        if self.lines.is_empty() {
            self.lines.push(LineKind::Paragraph);
        }
        self.keep_blocks_alone();
        self.settle_lines();
        self.text.clone()
    }

    /// Each line's kind as it is written (see [`Self::written_kind`]): an emptied line of
    /// inline content is a paragraph again, a typed-in empty paragraph an ordinary one.
    fn settle_lines(&mut self) {
        let mut previous = None;
        let mut previous_frame: Option<(Vec<Container>, LineKind)> = None;
        for (index, (start, end)) in self.line_ranges().into_iter().enumerate() {
            if let Some(frame) = self.frame(index) {
                let block = self.written_block(frame, previous_frame.as_ref(), start == end);
                let settled = Frame {
                    block,
                    ..frame.clone()
                };
                previous = Some(settled.shown());
                previous_frame = Some((settled.containers.clone(), block));
                if let Some(slot) = self.lines.get_mut(index) {
                    *slot = settled.shown();
                }
                if let Some(slot) = self.frames.get_mut(index) {
                    *slot = Some(settled);
                }
                continue;
            }
            previous_frame = None;
            if let Some(kind) = self.lines.get(index).copied() {
                let written = self.written_kind(kind, previous, start == end);
                if let Some(slot) = self.lines.get_mut(index) {
                    *slot = written;
                }
                previous = Some(written);
            }
        }
    }

    /// Every kept block alone on a [`LineKind::Block`] line, and no such line without one: text
    /// typed beside a block, or a line joined to it, becomes a line of its own; a block's line
    /// whose block was deleted becomes a paragraph.
    fn keep_blocks_alone(&mut self) {
        let chars: Vec<char> = self.text.chars().collect();
        let mut atom = 0;
        let block: Vec<bool> = chars
            .iter()
            .map(|c| {
                if *c != ATOM {
                    return false;
                }
                atom += 1;
                self.atoms.get(atom - 1).is_some_and(|h| is_block_atom(h))
            })
            .collect();
        let ranges = self.line_ranges();
        let settled = ranges.iter().enumerate().all(|(line, (start, end))| {
            let blocks = (*start..*end).filter(|i| block[*i]).count();
            match self.lines.get(line) {
                Some(LineKind::Block) => blocks == 1 && end - start == 1,
                _ => blocks == 0,
            }
        });
        if settled {
            return;
        }
        let mut text = String::new();
        let mut marks = Vec::new();
        let mut lines = Vec::new();
        let mut frames: Vec<Option<Frame>> = Vec::new();
        let mut push = |(kind, frame): (LineKind, Option<Frame>),
                        range: std::ops::Range<usize>,
                        lines: &mut Vec<_>| {
            if !lines.is_empty() {
                text.push('\n');
                marks.push(Vec::new());
            }
            lines.push(kind);
            frames.push(frame);
            for i in range {
                text.push(chars[i]);
                marks.push(if block[i] {
                    Vec::new()
                } else {
                    self.marks.get(i).cloned().unwrap_or_default()
                });
            }
        };
        for (line, (start, end)) in ranges.into_iter().enumerate() {
            let kind = match self.lines.get(line).copied() {
                Some(LineKind::Block) | None => LineKind::Paragraph,
                Some(kind) => kind,
            };
            let own = self.frame(line);
            // The line's text, and its blocks, in its containers.
            let text_line = match own {
                Some(f) if f.block == LineKind::Block => {
                    let f = f.with_block(LineKind::Paragraph);
                    (f.shown(), Some(f))
                }
                Some(f) => (kind, Some(f.clone())),
                None => (kind, None),
            };
            let block_line = (LineKind::Block, own.map(|f| f.with_block(LineKind::Block)));
            let mut from = start;
            let mut pieces = 0;
            for i in (start..end).filter(|i| block[*i]) {
                if i > from {
                    push(text_line.clone(), from..i, &mut lines);
                    pieces += 1;
                }
                push(block_line.clone(), i..i + 1, &mut lines);
                pieces += 1;
                from = i + 1;
            }
            if end > from || pieces == 0 {
                push(text_line, from..end, &mut lines);
            }
        }
        self.text = text;
        self.marks = marks;
        self.lines = lines;
        if !self.frames.is_empty() {
            self.frames = frames;
        }
    }

    /// Puts an atom (its exact HTML, such as a footnote marker) at character `index`, formatted
    /// as text typed there would be. Returns the new text.
    pub fn insert_atom(&mut self, index: usize, html: &str) -> String {
        let chars: Vec<char> = self.text.chars().collect();
        let index = index.min(chars.len());
        let inherit = if index > 0 && chars[index - 1] != '\n' {
            self.marks.get(index - 1).cloned()
        } else if chars.get(index).is_some_and(|c| *c != '\n') {
            self.marks.get(index).cloned()
        } else {
            None
        }
        .unwrap_or_default();
        let atoms_before = chars[..index].iter().filter(|c| **c == ATOM).count();
        let byte = self
            .text
            .char_indices()
            .nth(index)
            .map_or(self.text.len(), |(b, _)| b);
        self.text.insert(byte, ATOM);
        self.marks.insert(index.min(self.marks.len()), inherit);
        self.atoms
            .insert(atoms_before.min(self.atoms.len()), html.to_owned());
        self.empty_source = false;
        if self.lines.is_empty() {
            self.lines.push(LineKind::Paragraph);
        }
        self.text.clone()
    }

    /// Puts a block (a block equation, a diagram, a calculation) where the caret is, as
    /// ProseCraft's `insertRichPlaceholder` does: an empty line becomes the block; otherwise the
    /// block goes after the line. An empty paragraph follows it, and the caret goes there.
    /// Returns the new text and the caret.
    pub fn insert_block(&mut self, index: usize, html: &str) -> (String, usize) {
        let ranges = self.line_ranges();
        let line = ranges
            .iter()
            .position(|(start, end)| *start <= index && index <= *end)
            .unwrap_or(ranges.len().saturating_sub(1));
        let (start, end) = ranges.get(line).copied().unwrap_or((0, 0));
        let empty = start == end;
        let mut chars: Vec<char> = self.text.chars().collect();
        let at = if empty { start } else { end };
        let atoms_before = chars[..at].iter().filter(|c| **c == ATOM).count();
        let (inserted, caret): (Vec<char>, usize) = if empty {
            (vec![ATOM, '\n'], start + 2)
        } else {
            (vec!['\n', ATOM, '\n'], end + 3)
        };
        let count = inserted.len();
        chars.splice(at..at, inserted);
        self.text = chars.into_iter().collect();
        for _ in 0..count {
            self.marks.insert(at.min(self.marks.len()), Vec::new());
        }
        if self.lines.is_empty() {
            self.lines.push(LineKind::Paragraph);
        }
        // In the line's containers, if it has any.
        let own = self.frame(line).cloned();
        let block_frame = own.as_ref().map(|f| f.with_block(LineKind::Block));
        let after_frame = own.as_ref().map(|f| f.with_block(LineKind::Paragraph));
        let after = after_frame
            .as_ref()
            .map_or(LineKind::Paragraph, Frame::shown);
        if empty {
            self.lines[line] = LineKind::Block;
            self.lines.insert(line + 1, after);
        } else {
            self.lines.insert(line + 1, LineKind::Block);
            self.lines.insert(line + 2, after);
        }
        if !self.frames.is_empty() {
            let at = (line + 1).min(self.frames.len());
            if empty {
                if let Some(slot) = self.frames.get_mut(line) {
                    *slot = block_frame;
                }
                self.frames.insert(at, after_frame);
            } else {
                self.frames.insert(at, after_frame);
                self.frames.insert(at, block_frame);
            }
        }
        self.atoms
            .insert(atoms_before.min(self.atoms.len()), html.to_owned());
        self.empty_source = false;
        self.keep_blocks_alone();
        (self.text.clone(), caret)
    }

    /// The HTML of the characters in `range`, with their formatting and atoms (what a copy of
    /// that selection puts on the clipboard).
    pub fn slice_html(&self, range: std::ops::Range<usize>) -> String {
        let chars: Vec<char> = self.text.chars().collect();
        let (start, end) = (range.start.min(chars.len()), range.end.min(chars.len()));
        if start >= end {
            return String::new();
        }
        let mut doc = self.clone();
        doc.no_final_newline = false;
        // The end first, then the start, so each cut is one change at one end.
        let head: String = chars[..end].iter().collect();
        doc.apply_text_at(&head, Some(end));
        let kept: String = chars[start..end].iter().collect();
        doc.apply_text_at(&kept, Some(0));
        doc.empty_source = false;
        doc.newline_blocks = false;
        doc.to_html()
    }

    /// Cleaned HTML as the editor would paste it, if it can hold it. Inline content alone (a
    /// few words copied from a page, or a lone footnote marker) is a paragraph's.
    fn pasted(cleaned: &str) -> Option<Self> {
        RichDoc::from_html(cleaned)
            .or_else(|| RichDoc::from_html(&format!("<p>{cleaned}</p>")))
            .filter(|p| !p.text.is_empty())
    }

    /// How many atoms (footnote markers, citations, pictures, equations, diagrams,
    /// calculations, kept blocks) [`Self::paste_html`] would paste from `html`.
    pub fn paste_atom_count(html: &str) -> usize {
        Self::pasted(&crate::html::sanitize::sanitize_html(html)).map_or(0, |p| p.atoms.len())
    }

    /// Pastes HTML over `range` as the browser pastes into ProseCraft's editor (the HTML
    /// cleaned as its input handler cleans it): the first block joins the caret's line (taking
    /// its own kind only on an empty line), the blocks after it keep theirs, and the text after
    /// the caret joins the last one. HTML the editor can't hold is pasted as its plain text.
    /// Returns the new text and the caret after the pasted text.
    pub fn paste_html(&mut self, range: std::ops::Range<usize>, html: &str) -> (String, usize) {
        let chars: Vec<char> = self.text.chars().collect();
        let (start, end) = (range.start.min(chars.len()), range.end.min(chars.len()));
        let without: String = chars[..start]
            .iter()
            .chain(&chars[end.max(start)..])
            .collect();
        self.apply_text_at(&without, Some(start));
        let cleaned = crate::html::sanitize::sanitize_html(html);
        let Some(pasted) = Self::pasted(&cleaned) else {
            let plain = crate::html::html_to_plain_text(&cleaned, &Default::default());
            let plain = plain.trim_end_matches('\n');
            let chars: Vec<char> = self.text.chars().collect();
            let at = start.min(chars.len());
            let text: String = chars[..at]
                .iter()
                .copied()
                .chain(plain.chars())
                .chain(chars[at..].iter().copied())
                .collect();
            let caret = at + plain.chars().count();
            return (self.apply_text_at(&text, Some(caret)), caret);
        };
        let chars: Vec<char> = self.text.chars().collect();
        let at = start.min(chars.len());
        let line = chars[..at].iter().filter(|c| **c == '\n').count();
        let line_empty =
            chars.get(at).is_none_or(|c| *c == '\n') && (at == 0 || chars[at - 1] == '\n');
        let atoms_before = chars[..at].iter().filter(|c| **c == ATOM).count();
        let pasted_chars = pasted.text.chars().count();
        // The text and each character's inline elements.
        let byte = self
            .text
            .char_indices()
            .nth(at)
            .map_or(self.text.len(), |(b, _)| b);
        self.text.insert_str(byte, &pasted.text);
        let tail = self.marks.split_off(at.min(self.marks.len()));
        self.marks.extend(pasted.marks.iter().cloned());
        self.marks.extend(tail);
        let tail = self.atoms.split_off(atoms_before.min(self.atoms.len()));
        self.atoms.extend(pasted.atoms.iter().cloned());
        self.atoms.extend(tail);
        // The lines: the caret's line split around the pasted blocks.
        if self.lines.is_empty() {
            self.lines.push(LineKind::Paragraph);
        }
        let line = line.min(self.lines.len() - 1);
        let own = self.lines[line];
        // Pasted text in no block (or an empty paragraph without its `<br>`) is a paragraph's,
        // as the browser puts it in the caret's block.
        let first = match pasted.lines.first() {
            Some(kind) if line_empty => kind.shown(),
            _ => own,
        };
        let tail = self.lines.split_off(line + 1);
        self.lines[line] = first;
        self.lines
            .extend(pasted.lines.iter().skip(1).map(|kind| kind.shown()));
        self.lines.extend(tail);
        // Pasted blocks are written by their kinds; the caret's line keeps its containers, in
        // the block pasted into an empty one (or none, for a list item or a quote).
        if !self.frames.is_empty() {
            let tail = self.frames.split_off((line + 1).min(self.frames.len()));
            if let Some(slot) = self.frames.get_mut(line)
                && let Some(frame) = slot.as_mut()
                && first != own
            {
                match first {
                    LineKind::Paragraph | LineKind::EmptyParagraph | LineKind::Bare { .. } => {}
                    kind if kind.list_depth().is_some() || kind == LineKind::Quote => *slot = None,
                    kind => frame.block = kind,
                }
                if let Some(frame) = slot.as_ref()
                    && let Some(kind) = self.lines.get_mut(line)
                {
                    *kind = frame.shown();
                }
            }
            self.frames
                .extend(pasted.lines.iter().skip(1).map(|_| None));
            self.frames.extend(tail);
        }
        self.empty_source = false;
        self.keep_blocks_alone();
        let caret = at + pasted_chars;
        (self.text.clone(), caret)
    }

    /// Replaces atom `index`'s HTML (an edited equation, diagram or calculation); a block that
    /// became inline, or the other way round, moves to or from a line of its own.
    pub fn replace_atom(&mut self, index: usize, html: &str) -> String {
        if let Some(atom) = self.atoms.get_mut(index) {
            *atom = html.to_owned();
        }
        self.keep_blocks_alone();
        self.text.clone()
    }

    /// Removes atom `index` (`removeRichTarget`); returns the new text and where it was.
    pub fn remove_atom(&mut self, index: usize) -> (String, Option<usize>) {
        let chars: Vec<char> = self.text.chars().collect();
        let Some(at) = chars
            .iter()
            .enumerate()
            .filter(|(_, c)| **c == ATOM)
            .nth(index)
            .map(|(i, _)| i)
        else {
            return (self.text.clone(), None);
        };
        let text: String = chars[..at].iter().chain(&chars[at + 1..]).collect();
        let kept = self.apply_text_at(&text, Some(at));
        (kept, Some(at))
    }

    /// Puts a picture (its `<img>`) at character `index` as ProseCraft's `insertRichTextImage`
    /// does: the image, then a `<br>`; on an empty line the browser keeps the line's own `<br>`
    /// after them. Returns the new text and the character after the break.
    pub fn insert_picture(&mut self, index: usize, html: &str) -> (String, usize) {
        let empty_line = self
            .line_ranges()
            .into_iter()
            .any(|(start, end)| start == end && start == index);
        self.insert_atom(index, html);
        let chars: Vec<char> = self.text.chars().collect();
        let index = (index + 1).min(chars.len());
        let breaks = if empty_line { 2 } else { 1 };
        let text: String = chars[..index]
            .iter()
            .copied()
            .chain(std::iter::repeat_n(HARD_BREAK, breaks))
            .chain(chars[index..].iter().copied())
            .collect();
        self.apply_text_at(&text, Some(index + breaks));
        (self.text.clone(), index + 1)
    }

    /// The lines before `line` and the lines after it, as two documents; `line` itself is left
    /// out (ProseCraft's empty paragraph that becomes a scene divider). A side without lines is
    /// one empty paragraph.
    pub fn split_at_line(&self, line: usize) -> (RichDoc, RichDoc) {
        let ranges = self.line_ranges();
        let chars: Vec<char> = self.text.chars().collect();
        let part = |lines: std::ops::Range<usize>| -> RichDoc {
            let lines = lines.start.min(ranges.len())..lines.end.min(ranges.len());
            if lines.is_empty() {
                return RichDoc {
                    lines: vec![LineKind::Paragraph],
                    newline_blocks: self.newline_blocks,
                    no_final_newline: self.no_final_newline,
                    compact_containers: self.compact_containers,
                    ..RichDoc::default()
                };
            }
            let start = ranges[lines.start].0;
            let end = ranges[lines.end - 1].1;
            let atoms_before = chars[..start].iter().filter(|c| **c == ATOM).count();
            let atoms_in = chars[start..end].iter().filter(|c| **c == ATOM).count();
            RichDoc {
                text: chars[start..end].iter().collect(),
                frames: if self.frames.is_empty() {
                    Vec::new()
                } else {
                    lines.clone().map(|l| self.frame(l).cloned()).collect()
                },
                lines: lines
                    .clone()
                    .map(|l| self.lines.get(l).copied().unwrap_or(LineKind::Paragraph))
                    .collect(),
                marks: (start..end)
                    .map(|i| self.marks.get(i).cloned().unwrap_or_default())
                    .collect(),
                atoms: self
                    .atoms
                    .iter()
                    .skip(atoms_before)
                    .take(atoms_in)
                    .cloned()
                    .collect(),
                newline_blocks: self.newline_blocks,
                no_final_newline: self.no_final_newline,
                empty_source: false,
                compact_containers: self.compact_containers,
            }
        };
        (part(0..line), part(line + 1..ranges.len()))
    }

    /// Removes every atom whose HTML `matches`, with its character. Returns the new text and the
    /// character index of the first one removed.
    pub fn remove_atoms(&mut self, matches: impl Fn(&str) -> bool) -> (String, Option<usize>) {
        let mut first = None;
        let mut atom = 0;
        let mut kept_atoms = Vec::new();
        let mut text = String::new();
        let mut marks = Vec::new();
        for (i, c) in self.text.chars().enumerate() {
            if c == ATOM {
                let html = self.atoms.get(atom).cloned().unwrap_or_default();
                atom += 1;
                if matches(&html) {
                    first.get_or_insert(i);
                    continue;
                }
                kept_atoms.push(html);
            }
            text.push(c);
            marks.push(self.marks.get(i).cloned().unwrap_or_default());
        }
        self.text = text;
        self.marks = marks;
        self.atoms = kept_atoms;
        (self.text.clone(), first)
    }

    /// Toggles `mark` over characters `range`: removed where every character has it, else added.
    pub fn toggle_mark(&mut self, range: std::ops::Range<usize>, mark: Mark) {
        let chars: Vec<char> = self.text.chars().collect();
        let ranges = self.line_ranges();
        let in_block = |i: usize| {
            ranges
                .iter()
                .position(|(s, e)| (*s..=*e).contains(&i))
                .and_then(|line| self.lines.get(line))
                == Some(&LineKind::Block)
        };
        let targets: Vec<usize> = range
            .filter(|i| chars.get(*i).is_some_and(|c| *c != '\n') && !in_block(*i))
            .collect();
        if targets.is_empty() {
            return;
        }
        let all = targets
            .iter()
            .all(|i| self.marks.get(*i).is_some_and(|s| mark.applies(s)));
        let (name, names) = mark.names();
        for i in targets {
            if let Some(stack) = self.marks.get_mut(i) {
                if all {
                    stack.retain(|t| !names.contains(&t.name.as_str()));
                } else if !mark.applies(stack) {
                    stack.push(Tag::plain(name));
                }
            }
        }
    }

    /// `createLink`: the characters of `range` become a link to `href` (any link they were in
    /// replaced). With nothing selected the address itself goes in as the link's text, as the
    /// browser does.
    /// The link the character at `index` (or just before it, for a caret at its end) is in:
    /// its characters and its address (`currentLinkElement`).
    pub fn link_at(&self, index: usize) -> Option<(std::ops::Range<usize>, String)> {
        let href = |i: usize| {
            self.marks.get(i).and_then(|stack| {
                stack.iter().find(|t| t.name == "a").map(|t| {
                    t.attrs
                        .iter()
                        .find(|(k, _)| k == "href")
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default()
                })
            })
        };
        let at = [Some(index), index.checked_sub(1)]
            .into_iter()
            .flatten()
            .find(|i| href(*i).is_some())?;
        let address = href(at)?;
        let same = |i: usize| href(i).as_deref() == Some(address.as_str());
        let mut start = at;
        while start > 0 && same(start - 1) {
            start -= 1;
        }
        let mut end = at + 1;
        while same(end) {
            end += 1;
        }
        Some((start..end, address))
    }

    /// `removeLink`: the characters of `range` no longer in a link.
    pub fn unlink(&mut self, range: std::ops::Range<usize>) -> String {
        for i in range {
            if let Some(stack) = self.marks.get_mut(i) {
                stack.retain(|t| t.name != "a");
            }
        }
        self.text.clone()
    }

    /// A link put in at a caret (`applyLink` with nothing selected): `text` linked to `href`,
    /// and the caret after it.
    pub fn insert_link(&mut self, at: usize, text: &str, href: &str) -> (String, usize) {
        let chars: Vec<char> = self.text.chars().collect();
        let at = at.min(chars.len());
        let joined: String = chars[..at]
            .iter()
            .copied()
            .chain(text.chars())
            .chain(chars[at..].iter().copied())
            .collect();
        let end = at + text.chars().count();
        self.apply_text_at(&joined, Some(end));
        self.set_link(at..end, href);
        (self.text.clone(), end)
    }

    pub fn set_link(&mut self, range: std::ops::Range<usize>, href: &str) -> String {
        let tag = Tag {
            name: "a".to_owned(),
            attrs: vec![("href".to_owned(), href.to_owned())],
        };
        let range = if range.is_empty() {
            let chars: Vec<char> = self.text.chars().collect();
            let at = range.start.min(chars.len());
            let text: String = chars[..at]
                .iter()
                .copied()
                .chain(href.chars())
                .chain(chars[at..].iter().copied())
                .collect();
            self.apply_text_at(&text, Some(at + href.chars().count()));
            at..at + href.chars().count()
        } else {
            range
        };
        let chars: Vec<char> = self.text.chars().collect();
        let ranges = self.line_ranges();
        for i in range {
            let in_block = ranges
                .iter()
                .position(|(s, e)| (*s..*e).contains(&i))
                .and_then(|line| self.lines.get(line))
                == Some(&LineKind::Block);
            if chars.get(i).is_none_or(|c| *c == '\n') || in_block {
                continue;
            }
            if let Some(stack) = self.marks.get_mut(i) {
                stack.retain(|t| t.name != "a");
                stack.insert(0, tag.clone());
            }
        }
        self.text.clone()
    }

    /// Sets the kind of every line touched by characters `range`; a kept block stays one.
    pub fn set_line_kind(&mut self, range: std::ops::Range<usize>, kind: LineKind) {
        if kind == LineKind::Block {
            return;
        }
        let first = self.line_of(range.start);
        let last = self.line_of(range.end.max(range.start));
        for line in first..=last {
            if let Some(slot) = self.lines.get_mut(line).filter(|k| **k != LineKind::Block) {
                *slot = kind;
            }
            // A framed line takes the block in its containers; made a list item or a quote, or
            // taken out of the list or quote it is in, it is written by its kind alone.
            if let Some(slot) = self.frames.get_mut(line)
                && let Some(frame) = slot.as_mut()
                && frame.block != LineKind::Block
            {
                let in_container = frame.shown() != frame.block;
                let container_kind = kind.list_depth().is_some() || kind == LineKind::Quote;
                if in_container && frame.shown() == kind {
                    continue;
                }
                if in_container || container_kind {
                    *slot = None;
                } else {
                    frame.block = kind;
                }
            }
        }
    }

    /// The kind of the line holding character `index`, as it looks (see [`LineKind::shown`]).
    pub fn kind_at(&self, index: usize) -> LineKind {
        self.lines
            .get(self.line_of(index))
            .copied()
            .unwrap_or(LineKind::Paragraph)
            .shown()
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn blocks_are_inserted_as_insert_rich_placeholder_puts_them() {
        let block = r#"<div class="math-block" data-tex="E = mc^2"></div>"#;
        // On an empty line: the line becomes the block, an empty paragraph follows.
        let mut doc = RichDoc::from_html("<p>One</p><p><br></p><p>Two</p>").expect("doc");
        let (_, caret) = doc.insert_block(4, block);
        assert_eq!(
            doc.to_html(),
            format!("<p>One</p>{block}<p><br></p><p>Two</p>")
        );
        assert_eq!(caret, 6);
        // In a line with text: after the line, then an empty paragraph.
        let mut doc = RichDoc::from_html("<p>One</p><p>Two</p>").expect("doc");
        let (_, caret) = doc.insert_block(1, block);
        assert_eq!(
            doc.to_html(),
            format!("<p>One</p>{block}<p><br></p><p>Two</p>")
        );
        assert_eq!(doc.text.chars().nth(caret - 1), Some('\n'));
        // Edited, then removed.
        let edited = r#"<div class="math-block" data-tex="a^2"></div>"#;
        doc.replace_atom(0, edited);
        assert!(doc.to_html().contains(edited));
        let (_, at) = doc.remove_atom(0);
        assert_eq!(at, Some(4));
        assert!(!doc.to_html().contains("math-block"));
    }
    use super::{ATOM, HARD_BREAK, LineKind, Mark, RichDoc};

    #[test]
    fn typing_keeps_formatting_and_atoms_follow_their_characters() {
        let mut doc = RichDoc::from_html(r#"<p>It was <em>dark</em><sup class="footnote-ref" data-footnote-id="n1" contenteditable="false"></sup>.</p>"#).unwrap_or_default();
        // A second marker, typed inside the emphasis; then the first removed by its id.
        let mut twice = doc.clone();
        twice.insert_atom(
            9,
            r#"<sup class="footnote-ref" data-footnote-id="n2" contenteditable="false"></sup>"#,
        );
        assert_eq!(
            twice.to_html(),
            r#"<p>It was <em>da<sup class="footnote-ref" data-footnote-id="n2" contenteditable="false"></sup>rk</em><sup class="footnote-ref" data-footnote-id="n1" contenteditable="false"></sup>.</p>"#
        );
        let (_, at) = twice.remove_atoms(|html| html.contains(r#"data-footnote-id="n1""#));
        assert_eq!(at, Some(12));
        assert_eq!(
            twice.to_html(),
            r#"<p>It was <em>da<sup class="footnote-ref" data-footnote-id="n2" contenteditable="false"></sup>rk</em>.</p>"#
        );
        // Type inside the emphasis: "dark" -> "darker".
        let text = doc.text.replace("dark", "darker");
        doc.apply_text(&text);
        assert!(
            doc.to_html().contains("<em>darker</em>"),
            "{}",
            doc.to_html()
        );
        // A pasted object character is not an atom.
        let forged = format!("{}{ATOM}", doc.text);
        let kept = doc.apply_text(&forged);
        assert_eq!(kept.matches(ATOM).count(), 1);
        // Deleting the marker's character removes the footnote reference.
        let without = doc.text.replace(ATOM, "");
        doc.apply_text(&without);
        assert_eq!(doc.to_html(), "<p>It was <em>darker</em>.</p>");
    }

    #[test]
    fn enter_follows_prosecraft() {
        let mut doc = RichDoc::from_html("<h2>Title</h2><ul><li>one</li></ul>").unwrap_or_default();
        // Keystroke by keystroke, with the cursor after each: Enter at the end of the heading,
        // "Body", then Enter at the end of "one", "two".
        for (step, cursor) in [
            ("Title\n\none", 6),
            ("Title\nBody\none", 10),
            ("Title\nBody\none\n", 15),
            ("Title\nBody\none\ntwo", 18),
        ] {
            doc.apply_text_at(step, Some(cursor));
        }
        assert_eq!(
            doc.to_html(),
            "<h2>Title</h2><p>Body</p><ul><li>one</li><li>two</li></ul>"
        );
    }

    #[test]
    fn marks_and_kinds_toggle() {
        let mut doc = RichDoc::from_html("<p>plain words</p>").unwrap_or_default();
        doc.toggle_mark(0..5, Mark::Bold);
        assert_eq!(doc.to_html(), "<p><strong>plain</strong> words</p>");
        doc.toggle_mark(0..5, Mark::Bold);
        assert_eq!(doc.to_html(), "<p>plain words</p>");
        doc.set_line_kind(0..0, LineKind::Quote);
        assert_eq!(doc.to_html(), "<blockquote><p>plain words</p></blockquote>");
        let empty = RichDoc::from_html("").unwrap_or_default();
        assert_eq!(empty.to_html(), "");
    }

    #[test]
    fn round_trips_the_editor_dialect() {
        for html in [
            "<p>It was <em>dark</em>.</p><p><br></p><h2>Next</h2>",
            "<blockquote><p>One</p><p>Two</p></blockquote><p>After</p>",
            "<ul><li>a</li><li><strong>b</strong></li></ul><ol><li>c</li></ol>",
            "<p><em><strong>both</strong></em> and <strong>bold <em>italic</em></strong></p>",
            r#"<p>See <a href="https://example.com/?a=1&amp;b=2">here</a>.</p>"#,
            r#"<p>A note<sup class="footnote-ref" data-footnote-id="n1" contenteditable="false"></sup>.</p>"#,
            "<p>a<br>b</p><p><em>c<br></em><br>d<br><br></p>",
            "<ul><li>one<br>two</li></ul>",
            "<p>a</p><hr><p>b</p>",
            "<p>a</p>\n<hr>\n<pre><code class=\"language-rust\">let a = 1 &lt; 2;\n</code></pre>\n<p>b</p>\n",
            "<table>\n<thead>\n<tr>\n<th>a</th>\n<th>b</th>\n</tr>\n</thead>\n<tbody><tr>\n<td>1</td>\n<td><em>2</em></td>\n</tr>\n</tbody></table>\n",
            r#"<ul><li>x</li></ul><div class="math-block" data-tex="E = mc^2"></div><ol><li>y</li></ol>"#,
            r#"<div class="mermaid-diagram" data-source="graph TD; A-->B"></div><div class="calc-worksheet" data-source="x = 2"></div>"#,
            r#"<p class="eyebrow">Part one</p><h1>Arrival</h1>"#,
            "<hr><hr>",
            "<ul>\n<li>a<ul>\n<li>b</li>\n<li>c</li>\n</ul>\n</li>\n<li>d</li>\n</ul>\n",
            "<ol>\n<li>a<ul>\n<li>b</li>\n</ul>\n</li>\n<li>c</li>\n</ol>\n",
            "<ul>\n<li>a<ul>\n<li>b<ul>\n<li>c</li>\n</ul>\n</li>\n</ul>\n</li>\n</ul>\n",
            "<ul><li>a<ol><li>b</li></ol><ul><li>c</li></ul></li></ul><ol><li>e</li></ol>",
            "<ul><li><ul><li>only nested</li></ul></li></ul>",
            r#"<p>See <img src="data:image/png;base64,AA==" alt="A &quot;map&quot;" width="40" data-size="small" data-align="left"><br>the map.</p>"#,
        ] {
            let doc = RichDoc::from_html(html).unwrap_or_else(|| panic!("not read: {html}"));
            assert_eq!(doc.to_html(), html);
        }
    }

    #[test]
    fn a_caption_that_is_a_screenplay_element_is_both() {
        let html = r#"<p data-sp="scene-heading">INT. ROOM</p><p class="eyebrow" data-sp="dialogue">y</p><p class="eyebrow" data-sp="action"></p>"#;
        let mut doc = RichDoc::from_html(html).unwrap_or_else(|| panic!("not read: {html}"));
        assert_eq!(doc.to_html(), html);
        assert_eq!(
            doc.lines[1],
            LineKind::Screenplay {
                kind: super::ScreenplayKind::Dialogue,
                bare: false,
                eyebrow: true,
            }
        );
        // Typed into, it keeps both; Enter starts a plain element, as ProseCraft's does.
        doc.apply_text_at("INT. ROOM\nyz\n\n", Some(13));
        assert_eq!(
            doc.to_html(),
            r#"<p data-sp="scene-heading">INT. ROOM</p><p class="eyebrow" data-sp="dialogue">yz</p><p data-sp="dialogue"><br></p><p class="eyebrow" data-sp="action"></p>"#
        );
    }

    #[test]
    fn the_editors_own_characters_in_the_text_are_kept() {
        // A line separator is the editor's newline in the text; one in the text itself is
        // held as an atom of its own, and so are the others the editor gives a meaning.
        let html = "<p>Unicode: caf\u{e9} \u{2028} tab\tend\nnext</p>";
        let doc = RichDoc::from_html(html).unwrap_or_else(|| panic!("not read: {html}"));
        assert_eq!(doc.to_html(), html);
        assert_eq!(
            doc.text,
            format!(
                "Unicode: caf\u{e9} {ATOM} tab\tend{}next",
                super::SOFT_BREAK
            )
        );
        assert_eq!(doc.atoms, ["\u{2028}"]);
        assert_eq!(super::text_atom(&doc.atoms[0]), Some(super::SOFT_BREAK));
        for html in [
            "<p>a\u{2029}b<br>c</p>",
            "<p>a\u{fffc}b<sup class=\"footnote-ref\" data-footnote-id=\"n\" contenteditable=\"false\"></sup></p>",
            "<p>a\rb</p>",
            "<ul><li><em>\u{2028}</em></li></ul>",
        ] {
            let doc = RichDoc::from_html(html).unwrap_or_else(|| panic!("not read: {html}"));
            assert_eq!(doc.to_html(), html);
            // It is text: in the comment text and in what proofreading checks.
            assert_eq!(
                doc.comment_map().text,
                crate::comments::comment_text(html),
                "{html}"
            );
        }
        let doc = RichDoc::from_html("<p>a\u{2028}b</p>").unwrap_or_default();
        let (text, at) = doc.proofread_map();
        assert_eq!(text, ['a', '\u{2028}', 'b']);
        assert_eq!(at, [Some(0), Some(1), Some(2)]);
        // Typing beside it keeps it.
        let mut doc = doc;
        doc.apply_text_at(&format!("ax{ATOM}b"), Some(2));
        assert_eq!(doc.to_html(), "<p>ax\u{2028}b</p>");
    }

    /// Container shapes ProseCraft keeps as they are (see `editor-load.json`).
    const CONTAINERS: [&str; 14] = [
        "<div>One</div><div><br></div><div>Two <em>it</em></div>",
        "<div><div><p>a</p></div><p>b</p></div><div><p>c</p></div>",
        "<blockquote><div><p>q</p></div><p>r</p></blockquote>",
        "<ul><li><div>a</div></li><li>b</li></ul>",
        "<ul><li><p>a</p></li></ul>",
        "<ul><li>text<p>b</p></li></ul>",
        "<ul><li><p>a</p><p>b</p><ul><li>c</li></ul></li><li>d</li></ul>",
        "<ol><li>One</li><p>stray</p><li>Two</li></ol>",
        "<ul>\n<li><p>foo</p>\n<p>bar</p>\n</li>\n<li><p>baz</p>\n</li>\n</ul>\n",
        "<ul><li><blockquote><p>q</p></blockquote></li></ul>",
        "<div></div><ul><li></li></ul>",
        "<li>Lone item</li><p>after</p>",
        "<ul><li><pre>code</pre></li><li>a<ul><li>b</li></ul>c</li></ul><blockquote><hr></blockquote>",
        "<h2>Title</h2>\n<div><p class=\"eyebrow\">One</p><h3>Two</h3><p data-sp=\"action\">x</p></div>\n<p>end</p>",
    ];

    #[test]
    fn containers_round_trip_as_prosecraft_keeps_them() {
        for html in CONTAINERS {
            let doc = RichDoc::from_html(html).unwrap_or_else(|| panic!("not read: {html}"));
            assert_eq!(doc.to_html(), html);
            assert_eq!(doc.frames.len(), doc.lines.len(), "{html}");
        }
        // Shapes the kinds say are read by them alone.
        let doc = RichDoc::from_html(
            "<ul><li>a<ul><li>b</li></ul></li></ul><blockquote><p>q</p></blockquote>",
        )
        .unwrap_or_default();
        assert!(doc.frames.is_empty());
        // What the lines look like: items in a list, a quote in a blockquote, and only an
        // item's first line has its bullet.
        let doc = RichDoc::from_html(CONTAINERS[6]).unwrap_or_default();
        assert_eq!(
            doc.lines,
            [
                LineKind::Bullet,
                LineKind::Bullet,
                LineKind::ListItem {
                    depth: 1,
                    ordered: 0
                },
                LineKind::Bullet
            ]
        );
        let starts: Vec<bool> = (0..4).map(|l| doc.starts_item(l)).collect();
        assert_eq!(starts, [true, false, true, true]);
        let doc = RichDoc::from_html(CONTAINERS[7]).unwrap_or_default();
        let starts: Vec<bool> = (0..3).map(|l| doc.starts_item(l)).collect();
        assert_eq!(starts, [true, false, true]);
        let doc = RichDoc::from_html(CONTAINERS[2]).unwrap_or_default();
        assert_eq!(doc.lines, [LineKind::Quote; 2]);
        let doc = RichDoc::from_html(CONTAINERS[0]).unwrap_or_default();
        assert_eq!(doc.text, "One\n\nTwo it");
    }

    #[test]
    fn typing_in_containers_changes_only_that_character() {
        for html in CONTAINERS {
            let doc = RichDoc::from_html(html).unwrap_or_default();
            let chars: Vec<char> = doc.text.chars().collect();
            for (line, (start, end)) in doc.line_ranges().into_iter().enumerate() {
                // Typed beside a kept block, text is a line of its own.
                if start == end || doc.lines[line] == LineKind::Block {
                    continue;
                }
                for at in start..=end {
                    let mut typed = doc.clone();
                    let text: String = chars[..at]
                        .iter()
                        .chain(&['§'])
                        .chain(&chars[at..])
                        .collect();
                    typed.apply_text_at(&text, Some(at + 1));
                    let written = typed.to_html();
                    assert_eq!(written.replacen('§', "", 1), html, "{at} in {written}");
                }
            }
        }
    }

    #[test]
    fn enter_in_containers_follows_the_browser() {
        let enter = |html: &str, at: usize| {
            let mut doc = RichDoc::from_html(html).unwrap_or_default();
            let chars: Vec<char> = doc.text.chars().collect();
            let text: String = chars[..at]
                .iter()
                .chain(&['\n'])
                .chain(&chars[at..])
                .collect();
            doc.apply_text_at(&text, Some(at + 1));
            doc.to_html()
        };
        // A `<div>` block is followed by the next one, as WebKit's Enter makes it.
        assert_eq!(
            enter("<div>One</div><p>x</p>", 3),
            "<div>One</div><div><br></div><p>x</p>"
        );
        // A paragraph in a `<div>` or a list item by the next paragraph there.
        assert_eq!(
            enter("<div><p>a</p></div>", 1),
            "<div><p>a</p><p><br></p></div>"
        );
        assert_eq!(
            enter("<ul><li><p>a</p></li></ul>", 1),
            "<ul><li><p>a</p><p><br></p></li></ul>"
        );
        // Text in a list item by the next item.
        assert_eq!(
            enter("<ul><li>a</li><li>b</li></ul><div>x</div>", 1),
            "<ul><li>a</li><li><br></li><li>b</li></ul><div>x</div>"
        );
    }

    #[test]
    fn a_line_in_containers_takes_a_kind() {
        // A block kind in its containers.
        let mut doc = RichDoc::from_html("<div><p>a</p><p>b</p></div>").unwrap_or_default();
        doc.set_line_kind(0..0, LineKind::Heading2);
        assert_eq!(doc.to_html(), "<div><h2>a</h2><p>b</p></div>");
        // A list item or a quote, or out of its list, by its kind alone.
        doc.set_line_kind(2..2, LineKind::Bullet);
        assert_eq!(doc.to_html(), "<div><h2>a</h2></div><ul><li>b</li></ul>");
        let mut doc = RichDoc::from_html("<ul><li><p>a</p><p>b</p></li></ul>").unwrap_or_default();
        doc.set_line_kind(2..2, LineKind::Bullet);
        assert_eq!(doc.to_html(), "<ul><li><p>a</p><p>b</p></li></ul>");
        doc.set_line_kind(2..2, LineKind::Paragraph);
        assert_eq!(doc.to_html(), "<ul><li><p>a</p></li></ul><p>b</p>");
        // A kept block put in a list item stays in it.
        let mut doc = RichDoc::from_html("<ul><li><p>a</p></li></ul>").unwrap_or_default();
        doc.insert_block(1, r#"<div class="math-block" data-tex="x"></div>"#);
        assert_eq!(
            doc.to_html(),
            r#"<ul><li><p>a</p><div class="math-block" data-tex="x"></div><p><br></p></li></ul>"#
        );
    }

    #[test]
    fn keeps_atoms_and_kinds() {
        let doc = RichDoc::from_html(
            r#"<h1>T</h1><p>x<span class="math-inline" data-tex="x^2"></span>y</p>"#,
        )
        .unwrap_or_default();
        assert_eq!(doc.text, format!("T\nx{ATOM}y"));
        assert_eq!(doc.lines, [LineKind::Heading1, LineKind::Paragraph]);
        assert_eq!(
            doc.atoms,
            [r#"<span class="math-inline" data-tex="x^2"></span>"#]
        );
    }

    #[test]
    fn refuses_what_it_cannot_write_back() {
        for html in [
            r#"<p data-sp="action" class="x">x</p>"#,
            r#"<p data-sp="unknown">x</p>"#,
            r#"<blockquote class="x"><p>x</p></blockquote>"#,
            "<ul><li class=\"x\">a</li></ul>",
            // ProseCraft's sanitizer writes the class first.
            r#"<p data-sp="action" class="eyebrow">x</p>"#,
            r#"<p class="eyebrow big" data-sp="action">x</p>"#,
            r#"<div class="other">x</div>"#,
            r#"<p class="eyebrow big">x</p>"#,
            "<p>a</p>\n<p>b</p><p>c</p>",
            r#"<p>a<br class="x">b</p>"#,
        ] {
            assert!(RichDoc::from_html(html).is_none(), "{html}");
        }
    }

    #[test]
    fn imported_and_sanitized_shapes_round_trip_exactly() {
        let img = r#"<img src="data:image/png;base64,AA==" alt="Map">"#;
        let cases = [
            // An EPUB import's blocks: a newline after each top-level one, none inside a list
            // or blockquote, none at the end.
            "<h2>After</h2>\n<blockquote><p>A quote.</p></blockquote>\n<p>Last.<a href=\"#fn-4\">4</a></p>".to_owned(),
            "<h3>Morning</h3>\n<ul><li>bread</li><li>salt</li></ul>\n<p>Then.</p>".to_owned(),
            // A picture in no block, and text after it.
            format!("{img}\n{img}\n<p></p>\n<p></p>"),
            format!("<p>Before {img} after.</p>\n{img}Caption\n<p>Kept text<a></a></p>"),
            // A blockquote holding text in no paragraph.
            "<p>Second&nbsp;half.</p>\n<blockquote>Quoted <b>bold</b></blockquote>".to_owned(),
            // Text in no block, as the sanitizer leaves it.
            "<em>a</em> b".to_owned(),
            "<h1>One</h1><h2>Two</h2><h3>Three</h3>FourFive".to_owned(),
            "<p>Upper</p><em>loud</em>".to_owned(),
            "<p>after</p>p{}".to_owned(),
            // Empty paragraphs and inline elements.
            "<p></p>".to_owned(),
            "<p>a<br>b</p><p></p><p><br></p>".to_owned(),
            r#"<p>It was <em></em>.<sup class="footnote-ref" data-footnote-id="fn-1" contenteditable="false"></sup></p>"#.to_owned(),
            // Scene breaks.
            r#"<div class="scene-break scene-break-line" contenteditable="false">* * *</div><div class="scene-break" contenteditable="false"><b>#</b></div>"#.to_owned(),
        ];
        for html in &cases {
            let doc = RichDoc::from_html(html).unwrap_or_else(|| panic!("not read: {html}"));
            assert_eq!(doc.to_html(), *html);
        }
        // What each is held as.
        let doc = RichDoc::from_html(&cases[2]).unwrap_or_default();
        assert_eq!(doc.text, format!("{ATOM}\n{ATOM}\n\n"));
        assert_eq!(doc.lines[..2], [LineKind::Bare { quote: false }; 2]);
        assert_eq!(doc.lines[2..], [LineKind::EmptyParagraph; 2]);
        let doc = RichDoc::from_html(&cases[4]).unwrap_or_default();
        assert_eq!(doc.lines[1], LineKind::Bare { quote: true });
        assert_eq!(doc.lines[1].shown(), LineKind::Quote);
        let doc = RichDoc::from_html(&cases[11]).unwrap_or_default();
        assert_eq!(doc.atoms[0], "<em></em>");
        let doc = RichDoc::from_html(&cases[12]).unwrap_or_default();
        assert_eq!(doc.lines, [LineKind::Block; 2]);
        let preview = super::block_preview(&doc.atoms[1]).map(|p| (p.kind, p.lines));
        assert_eq!(
            preview,
            Some((super::BlockKind::SceneBreak, vec!["#".to_owned()]))
        );
    }

    #[test]
    fn normalized_shapes_keep_every_character_or_are_refused() {
        let normalized = |html: &str| RichDoc::from_html_normalized(html).map(|d| d.to_html());
        // ProseCraft's sanitizer: an `h4` is unwrapped, as its editor stores it.
        assert_eq!(
            normalized("<p>a</p><h4>Four</h4>").as_deref(),
            Some("<p>a</p>Four")
        );
        // As ProseCraft's sanitizer balances a stray end tag: the paragraphs go in the list.
        assert_eq!(
            normalized("<ol><li>a</li></ul><p>b</p>").as_deref(),
            Some("<ol><li>a</li><p>b</p></ol>")
        );
        // This editor's own, past the containers it holds: a `div` unwrapped.
        let deep = format!("{}<p>in</p>{}", "<div>".repeat(40), "</div>".repeat(40));
        assert_eq!(normalized(&deep).as_deref(), Some("<p>in</p>"));
        // Refused: quotes nested deeper than it holds.
        let deep = format!(
            "{}<p>in</p>{}",
            "<blockquote>".repeat(40),
            "</blockquote>".repeat(40)
        );
        assert_eq!(RichDoc::from_html(&deep), None);
        assert_eq!(normalized(&deep), None);
        // Refused: a picture or a footnote marker the sanitizer would drop.
        assert_eq!(
            normalized(r#"<h4>x</h4><p>a<img src="https://e.org/x.png" alt=""></p>"#),
            None
        );
        assert_eq!(
            normalized(
                r#"<h4>x</h4><p>a<sup class="footnote-ref" data-footnote-id="bad id!"></sup></p>"#
            ),
            None
        );
    }

    #[test]
    fn inline_html_pastes_into_the_carets_paragraph() {
        let mut doc = RichDoc::from_html("<p><br></p>").unwrap_or_default();
        doc.paste_html(0..0, "hello <em>x</em>");
        assert_eq!(doc.to_html(), "<p>hello <em>x</em></p>");
        let mut doc = RichDoc::from_html("<p>a</p>").unwrap_or_default();
        doc.paste_html(1..1, "<p>b</p>c<p></p>");
        assert_eq!(doc.to_html(), "<p>ab</p><p>c</p><p><br></p>");
    }

    #[test]
    fn typing_in_imported_shapes_writes_what_the_browser_keeps() {
        // Typed into, an empty paragraph is an ordinary one; text in no block stays so.
        let mut doc = RichDoc::from_html("<p></p><em>a</em> b").unwrap_or_default();
        doc.apply_text_at("x\na b", Some(1));
        doc.apply_text_at("x\na bc", Some(6));
        assert_eq!(doc.to_html(), "<p>x</p><em>a</em> bc");
        // Emptied, text in no block is an empty paragraph; Enter in it starts a paragraph.
        let mut doc = RichDoc::from_html("<p>a</p>bc").unwrap_or_default();
        doc.apply_text_at("a\n", Some(2));
        assert_eq!(doc.to_html(), "<p>a</p><p><br></p>");
        let mut doc = RichDoc::from_html("<p>a</p>bc").unwrap_or_default();
        doc.apply_text_at("a\nb\nc", Some(4));
        assert_eq!(doc.to_html(), "<p>a</p>b<p>c</p>");
        // An EPUB import's layout is kept.
        let html = "<h3>M</h3>\n<ul><li>bread</li><li>salt</li></ul>\n<p>Then.</p>";
        let mut doc = RichDoc::from_html(html).unwrap_or_default();
        let text = doc.text.replace("salt", "salty");
        doc.apply_text(&text);
        assert_eq!(doc.to_html(), html.replace("salt", "salty"));
    }

    #[test]
    fn shift_enter_is_a_br() {
        let mut doc = RichDoc::from_html("<p>one two</p>").unwrap_or_default();
        doc.apply_text_at(&format!("one{HARD_BREAK}two"), Some(4));
        assert_eq!(doc.to_html(), "<p>one<br>two</p>");
        // Deleting it joins the words again.
        doc.apply_text("onetwo");
        assert_eq!(doc.to_html(), "<p>onetwo</p>");
        // A block holding only a break is ProseCraft's empty block.
        assert_eq!(
            RichDoc::from_html("<p><br></p>").map(|d| d.text),
            Some(String::new())
        );
    }

    #[test]
    fn kept_blocks_stay_alone_on_their_line() {
        let mut doc = RichDoc::from_html("<p>a</p><hr><p>b</p>").expect("doc");
        assert_eq!(doc.text, format!("a\n{ATOM}\nb"));
        assert_eq!(doc.lines[1], LineKind::Block);
        // Typing right after the rule starts a paragraph of its own.
        doc.apply_text_at(&format!("a\n{ATOM}x\nb"), Some(4));
        assert_eq!(doc.to_html(), "<p>a</p><hr><p>x</p><p>b</p>");
        // Joining the line before into it (Backspace at the rule) keeps both.
        doc.apply_text_at(&format!("a{ATOM}\nx\nb"), Some(1));
        assert_eq!(doc.to_html(), "<p>a</p><hr><p>x</p><p>b</p>");
        // Enter after the rule gives a paragraph, not another rule.
        let at = doc
            .text
            .find(ATOM)
            .map(|b| doc.text[..b].chars().count() + 1);
        let at = at.expect("rule");
        let chars: Vec<char> = doc.text.chars().collect();
        let text: String = chars[..at]
            .iter()
            .chain(&['\n'])
            .chain(&chars[at..])
            .collect();
        doc.apply_text_at(&text, Some(at + 1));
        assert_eq!(doc.to_html(), "<p>a</p><hr><p><br></p><p>x</p><p>b</p>");
        // Deleting the rule's character deletes the rule; its line is a paragraph.
        doc.apply_text(&doc.text.replace(ATOM, ""));
        assert_eq!(
            doc.to_html(),
            "<p>a</p><p><br></p><p><br></p><p>x</p><p>b</p>"
        );
        // Formatting and block kinds leave a kept block as it is.
        let mut doc = RichDoc::from_html("<p>a</p><pre><code>x</code></pre>").expect("doc");
        doc.toggle_mark(0..3, Mark::Bold);
        doc.set_line_kind(0..3, LineKind::Bullet);
        assert_eq!(
            doc.to_html(),
            "<ul><li><strong>a</strong></li></ul><pre><code>x</code></pre>"
        );
    }

    #[test]
    fn a_picture_goes_in_as_prosecraft_puts_it() {
        let img = r#"<img src="data:image/png;base64,AA==" alt="map">"#;
        let mut doc = RichDoc::from_html("<p>One two</p><p><br></p>").expect("doc");
        let (_, after) = doc.insert_picture(4, img);
        assert_eq!(doc.to_html(), format!("<p>One {img}<br>two</p><p><br></p>"));
        assert_eq!(after, 6);
        // On an empty line the line's own break stays.
        let end = doc.text.chars().count();
        doc.insert_picture(end, img);
        assert_eq!(
            doc.to_html(),
            format!("<p>One {img}<br>two</p><p>{img}<br><br></p>")
        );
        assert!(RichDoc::from_html(&doc.to_html()).is_some());
    }

    #[test]
    fn nested_lists_keep_their_depth() {
        let mut doc =
            RichDoc::from_html("<ul><li>a<ol><li>b</li></ol></li><li>c</li></ul>").expect("doc");
        assert_eq!(
            doc.lines,
            [
                LineKind::Bullet,
                LineKind::ListItem {
                    depth: 1,
                    ordered: 0b10
                },
                LineKind::Bullet
            ]
        );
        // Enter at the end of the nested item continues the nested list.
        doc.apply_text_at("a\nb\nb2\nc", Some(6));
        assert_eq!(
            doc.to_html(),
            "<ul><li>a<ol><li>b</li><li>b2</li></ol></li><li>c</li></ul>"
        );
        // Made a paragraph, the nested item leaves both lists; the list goes on after it.
        doc.set_line_kind(4..5, LineKind::Paragraph);
        assert_eq!(
            doc.to_html(),
            "<ul><li>a<ol><li>b</li></ol></li></ul><p>b2</p><ul><li>c</li></ul>"
        );
    }

    #[test]
    fn links_go_on_the_selection_or_in_as_the_address() {
        use super::link_href;
        assert_eq!(
            link_href(" example.com/a b "),
            Ok("https://example.com/a%20b".to_owned())
        );
        assert_eq!(
            link_href("mailto:me@example.com"),
            Ok("mailto:me@example.com".to_owned())
        );
        assert_eq!(
            link_href("HTTP://Example.COM"),
            Ok("http://example.com/".to_owned())
        );
        assert_eq!(
            link_href("javascript:alert(1)"),
            Err("Please use a web link or email address.")
        );
        assert_eq!(link_href("http://"), Err("Please enter a valid link."));
        let mut doc = RichDoc::from_html("<p>See <strong>the map</strong> here.</p>").expect("doc");
        doc.set_link(4..11, "https://example.com/");
        assert_eq!(
            doc.to_html(),
            r#"<p>See <a href="https://example.com/"><strong>the map</strong></a> here.</p>"#
        );
        // Linking again replaces the link.
        doc.set_link(4..7, "https://other.example/");
        assert_eq!(
            doc.to_html(),
            r#"<p>See <a href="https://other.example/"><strong>the</strong></a><a href="https://example.com/"><strong> map</strong></a> here.</p>"#
        );
        let mut doc = RichDoc::from_html("<p>Go </p>").expect("doc");
        doc.set_link(3..3, "https://example.com/");
        assert_eq!(
            doc.to_html(),
            r#"<p>Go <a href="https://example.com/">https://example.com/</a></p>"#
        );
    }

    #[test]
    fn previews_kept_blocks() {
        use super::{BlockKind, block_preview};
        let table = block_preview(
            "<table><thead><tr><th>a</th><th>b c</th></tr></thead><tbody><tr><td>1</td><td>2</td></tr></tbody></table>",
        )
        .expect("table");
        assert_eq!(table.kind, BlockKind::Table);
        assert_eq!(table.lines, ["a  │  b c", "1  │  2"]);
        let code = block_preview("<pre><code>one\ntwo\n</code></pre>").expect("code");
        assert_eq!(
            (code.kind, code.lines),
            (BlockKind::Code, vec!["one".into(), "two".into()])
        );
        let math = block_preview(r#"<div class="math-block" data-tex="x^2"></div>"#).expect("math");
        assert_eq!(
            (math.kind, math.lines),
            (BlockKind::Equation, vec!["x^2".into()])
        );
        assert_eq!(block_preview("<hr>").map(|p| p.kind), Some(BlockKind::Rule));
        // A calculation shows its results.
        let calc = block_preview(
            r#"<div class="calc-worksheet" data-source="v := 60 mph&#10;v = kph&#10;v = kg"></div>"#,
        )
        .expect("calc");
        assert_eq!(calc.kind, BlockKind::Calculation);
        assert_eq!(
            calc.lines,
            [
                "v := 60 mph",
                "v = 96.561 kph",
                "v = kg  [The result is in m·s^-1, which can't be shown in kg.]"
            ]
        );
        assert_eq!(block_preview("<p>x</p>"), None);
    }

    /// Whatever is typed, what the editor writes can be read back into the same document: a
    /// scene never turns read-only after a save.
    #[test]
    fn edits_always_write_html_that_reads_back() {
        let note =
            r#"<sup class="footnote-ref" data-footnote-id="n1" contenteditable="false"></sup>"#;
        let starts = [
            format!("<p>One <em>two</em>{note}</p><ul><li>a<br>b</li></ul><h2>H</h2>"),
            "<blockquote><p>q<br></p></blockquote><ol><li>x</li></ol>".to_owned(),
            "<p><br></p>".to_owned(),
            "<ul><li>a<ol><li>b<ul><li>c</li></ul></li></ol></li><li>d</li></ul><p>e</p>".to_owned(),
            "<p>a</p><hr><table><tr><td>x</td></tr></table><p class=\"eyebrow\">e</p><pre><code>c\n</code></pre>".to_owned(),
            r#"<p>See <img src="data:image/png;base64,AA==" alt="map"><br>it.</p><p><img src="data:image/png;base64,AA==" alt=""><br><br></p>"#.to_owned(),
            // The shapes an import or the sanitizer leaves: blocks joined by newlines with none
            // inside a list or blockquote, inline content in no block, empty elements.
            "<h3>Morning</h3>\n<p>Calm.<a href=\"#fn-2\">2</a></p>\n<ul><li>bread</li><li>salt</li></ul>\n<blockquote><p>q</p></blockquote>\n<p>end</p>".to_owned(),
            "<img src=\"data:image/png;base64,AA==\" alt=\"Map\">\n<img src=\"data:image/png;base64,AA==\" alt=\"\">Caption\n<p></p>\n<p></p>".to_owned(),
            "<em>a</em> b<p>x</p>Tail<blockquote>Quoted <b>bold</b><p>p</p></blockquote>".to_owned(),
            r#"<p>It was <em></em>.</p><p></p><div class="scene-break" contenteditable="false">* * *</div><p>Kept<a></a></p>"#.to_owned(),
            // The editor's own characters in the text.
            "<p>Unicode: caf\u{e9} \u{2028} tab\tend\nnext\u{2029}</p><p>\u{fffc}</p>".to_owned(),
            // Containers the kinds can't say.
            "<div>One</div><div><br></div><div><p>a</p><ul><li><p>b</p><p>c</p></li><li>d</li></ul></div>".to_owned(),
            "<ul>\n<li><p>foo</p>\n<p>bar</p>\n</li>\n<li><p>baz</p>\n</li>\n</ul>\n<p>x</p>\n".to_owned(),
            "<ol><li>One</li><p>stray</p><li>Two</li></ol><blockquote><div><p>q</p></div>r</blockquote>".to_owned(),
            "<div></div><ul><li></li></ul><li>Lone</li><ul><li>a<hr>b<ul><li>c</li></ul>d</li></ul>".to_owned(),
        ];
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n.max(1) as u64) as usize
        };
        let kinds = [
            LineKind::Paragraph,
            LineKind::Heading2,
            LineKind::Quote,
            LineKind::Bullet,
            LineKind::Numbered,
            LineKind::Eyebrow,
            LineKind::ListItem {
                depth: 2,
                ordered: 0b101,
            },
        ];
        for start in &starts {
            let mut doc = RichDoc::from_html(start).expect("start");
            for step in 0..400 {
                let chars: Vec<char> = doc.text.chars().collect();
                let at = next(chars.len() + 1);
                match next(5) {
                    0 | 1 => {
                        let c = ['a', ' ', '\n', HARD_BREAK, '&'][next(5)];
                        let text: String =
                            chars[..at].iter().chain(&[c]).chain(&chars[at..]).collect();
                        doc.apply_text_at(&text, Some(at + 1));
                    }
                    2 => {
                        let end = (at + next(4)).min(chars.len());
                        let text: String = chars[..at].iter().chain(&chars[end..]).collect();
                        doc.apply_text_at(&text, Some(at));
                    }
                    3 => doc.toggle_mark(at..(at + next(6)), Mark::Italic),
                    _ => doc.set_line_kind(at..at, kinds[next(kinds.len())]),
                }
                let html = doc.to_html();
                let again = RichDoc::from_html(&html)
                    .unwrap_or_else(|| panic!("step {step}: not read back: {html}"));
                assert_eq!(again.to_html(), html, "step {step}");
                assert!(
                    doc.frames.is_empty() || doc.frames.len() == doc.lines.len(),
                    "step {step}"
                );
                // The comment text is ProseCraft's for the HTML, and each character's own
                // text is where the map puts it.
                let map = doc.comment_map();
                assert_eq!(
                    map.text,
                    crate::comments::comment_text(&html),
                    "step {step}"
                );
                for (i, c) in doc.text.chars().enumerate() {
                    if !matches!(c, '\n' | ATOM | HARD_BREAK) {
                        let at = map.to_comment(i);
                        let unit = if c == super::SOFT_BREAK { '\n' } else { c };
                        let mut buf = [0u16; 2];
                        let want = unit.encode_utf16(&mut buf);
                        assert_eq!(
                            map.text.get(at..at + want.len()),
                            Some(&*want),
                            "step {step}: char {i} {c:?} in {html}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn comment_text_has_the_newlines_between_markdown_blocks() {
        // ProseCraft's `commentText` for these (checked in its own code).
        let doc = RichDoc::from_html("<p>a</p>\n<p>b</p>\n").expect("doc");
        let map = doc.comment_map();
        assert_eq!(String::from_utf16_lossy(&map.text), "a\nb\n");
        assert_eq!(
            (map.to_comment(0), map.to_comment(2), map.to_comment(3)),
            (0, 2, 3)
        );
        assert_eq!(map.to_editor(2), 2);
        let doc =
            RichDoc::from_html("<ul>\n<li>x</li>\n<li>y</li>\n</ul>\n<p>z\nw</p>\n").expect("doc");
        let map = doc.comment_map();
        assert_eq!(String::from_utf16_lossy(&map.text), "\nx\ny\n\nz\nw\n");
        assert_eq!(map.to_comment(0), 1);
        assert_eq!(map.to_comment(4), 6);
        // A citation stored with its text: the editor holds it as an empty widget.
        let doc = RichDoc::from_html(
            r#"<p>a<br>b<sup class="footnote-ref" data-footnote-id="n" contenteditable="false"></sup>c<span class="cite-ref" data-cites="[]">(Smith 2020)</span></p><table><tr><td>t</td></tr></table>"#,
        )
        .expect("doc");
        let map = doc.comment_map();
        assert_eq!(String::from_utf16_lossy(&map.text), "abct");
        assert_eq!(map.to_comment(4), 2);
    }

    #[test]
    fn splits_around_a_line() {
        let note =
            r#"<sup class="footnote-ref" data-footnote-id="fn-1" contenteditable="false"></sup>"#;
        let doc = RichDoc::from_html(&format!(
            "<p>One<em>a</em></p><p><br></p><ul><li>Two{note}</li></ul><p>Three</p>"
        ))
        .expect("doc");
        let (before, after) = doc.split_at_line(1);
        assert_eq!(before.to_html(), "<p>One<em>a</em></p>");
        assert_eq!(
            after.to_html(),
            format!("<ul><li>Two{note}</li></ul><p>Three</p>")
        );
        let (before, after) = doc.split_at_line(0);
        assert_eq!(before.to_html(), "<p><br></p>");
        assert!(after.to_html().starts_with("<p><br></p><ul>"));
        let (_, after) = doc.split_at_line(3);
        assert_eq!(after.to_html(), "<p><br></p>");
    }
}

#[cfg(test)]
mod paste_tests {
    use super::*;

    fn paste(html: &str, at: std::ops::Range<usize>, pasted: &str) -> (String, usize) {
        let mut doc = RichDoc::from_html(html).expect("doc");
        let (_, caret) = doc.paste_html(at, pasted);
        (doc.to_html(), caret)
    }

    #[test]
    fn formatting_and_links_paste_into_the_line() {
        let (html, caret) = paste(
            "<p>Before after</p>",
            7..7,
            r#"<meta charset="utf-8"><span style="color: red">a <b>bold</b> <a href="https://x.test/">link</a></span>"#,
        );
        assert_eq!(
            html,
            r#"<p>Before a <b>bold</b> <a href="https://x.test/">link</a>after</p>"#
        );
        assert_eq!(caret, 7 + "a bold link".chars().count());
    }

    #[test]
    fn paragraphs_split_the_line_and_keep_their_kinds() {
        let (html, _) = paste(
            "<h2>Start end</h2><p>Next</p>",
            6..6,
            "<p>one</p><blockquote><p>two</p></blockquote><p>three </p>",
        );
        assert_eq!(
            html,
            "<h2>Start one</h2><blockquote><p>two</p></blockquote><p>three end</p><p>Next</p>"
        );
    }

    #[test]
    fn an_empty_line_takes_the_first_block_kind_and_a_selection_is_replaced() {
        let (html, _) = paste("<p>A</p><p><br></p>", 2..2, "<ul><li>x</li><li>y</li></ul>");
        assert_eq!(html, "<p>A</p><ul><li>x</li><li>y</li></ul>");
        let (html, caret) = paste("<p>Keep this word</p>", 5..9, "<em>that</em>");
        assert_eq!(html, "<p>Keep <em>that</em> word</p>");
        assert_eq!(caret, 9);
    }

    #[test]
    fn scripts_are_cleaned_away() {
        let (html, _) = paste(
            "<p>x</p>",
            1..1,
            r#"<script>alert(1)</script><img src="x" onerror="alert(1)">!"#,
        );
        assert!(
            !html.contains("script") && !html.contains("onerror"),
            "{html}"
        );
        assert!(html.contains('!'), "{html}");
    }
}

#[cfg(test)]
mod link_tests {
    use super::RichDoc;

    #[test]
    fn links_are_found_put_in_and_taken_away() {
        let mut doc = RichDoc::from_html(r#"<p>See <a href="https://a.test/">the map</a> now</p>"#)
            .expect("doc");
        let (range, href) = doc.link_at(6).expect("in the link");
        assert_eq!((range, href.as_str()), (4..11, "https://a.test/"));
        // A caret at its end is in it too; one after the space isn't.
        assert_eq!(doc.link_at(11).map(|l| l.0), Some(4..11));
        assert!(doc.link_at(12).is_none());
        doc.unlink(4..11);
        assert_eq!(doc.to_html(), "<p>See the map now</p>");
        let (_, caret) = doc.insert_link(0, "example.com", "https://example.com/");
        assert_eq!(caret, 11);
        assert_eq!(
            doc.to_html(),
            r#"<p><a href="https://example.com/">example.com</a>See the map now</p>"#
        );
    }
}

#[cfg(test)]
mod final_newline_tests {
    use super::RichDoc;

    #[test]
    fn blocks_joined_by_newlines_without_a_final_one_round_trip() {
        for html in [
            "<h2>A</h2>\n<p>b</p>",
            "<p>b</p>\n<hr>\n<p>c</p>",
            "<h2>A</h2>\n<p>b</p>\n",
        ] {
            let doc = RichDoc::from_html(html).expect("held");
            assert_eq!(doc.to_html(), html);
        }
    }
}

#[cfg(test)]
mod slice_tests {
    use super::RichDoc;

    #[test]
    fn a_selection_copies_with_its_formatting() {
        let doc = RichDoc::from_html(
            r#"<p>It was <em>very dark</em> here.</p><h2>Next</h2><p>Then <b>more</b>.</p>"#,
        )
        .expect("doc");
        assert_eq!(doc.slice_html(7..11), "<p><em>very</em></p>");
        assert_eq!(
            doc.slice_html(12..27),
            "<p><em>dark</em> here.</p><h2>Next</h2>"
        );
        assert_eq!(doc.slice_html(3..3), "");
    }
}

#[cfg(test)]
mod atom_paste_tests {
    use super::*;

    /// One atom of each kind, written as ProseCraft's editor writes it.
    const ATOMS: [&str; 9] = [
        r#"<sup class="footnote-ref" data-footnote-id="fn-abc123" contenteditable="false"></sup>"#,
        r#"<span class="cite-ref" data-cites="[{&quot;id&quot;:&quot;smith2020&quot;}]" contenteditable="false"></span>"#,
        r#"<span class="math-inline" data-tex="x^2"></span>"#,
        r#"<img src="data:image/png;base64,AA==" alt="Map">"#,
        r#"<div class="math-block" data-tex="a^2"></div>"#,
        r#"<div class="mermaid-diagram" data-source="graph TD; A-->B"></div>"#,
        r#"<div class="calc-worksheet" data-source="x := 2"></div>"#,
        "<hr>",
        "<pre>let x = 1;</pre>",
    ];

    /// A scene holding `atom`, and the atom's character.
    fn scene_with(atom: &str) -> (RichDoc, usize) {
        let html = if is_block_atom(atom) {
            format!("<p>A</p>{atom}<p>B</p>")
        } else {
            format!("<p>A{atom}B</p>")
        };
        let doc = RichDoc::from_html(&html).expect("held");
        let at = doc.text.chars().position(|c| c == ATOM).expect("an atom");
        (doc, at)
    }

    #[test]
    fn a_lone_atom_copied_pastes_back_as_itself() {
        for atom in ATOMS {
            let (doc, at) = scene_with(atom);
            let copied = crate::html::sanitize::sanitize_html(&doc.slice_html(at..at + 1));
            assert_eq!(RichDoc::paste_atom_count(&copied), 1, "{atom}: {copied}");
            let mut target = RichDoc::from_html("<p>Hello world</p>").expect("held");
            let (_, caret) = target.paste_html(5..5, &copied);
            assert_eq!(caret, 6, "{atom}");
            let expected = if is_block_atom(atom) {
                format!("<p>Hello</p>{atom}<p> world</p>")
            } else {
                format!("<p>Hello{atom} world</p>")
            };
            assert_eq!(target.to_html(), expected);
        }
    }

    #[test]
    fn a_pasted_footnote_marker_keeps_its_id() {
        // As in ProseCraft, the copy refers to the same footnote (and takes its number).
        let (doc, at) = scene_with(ATOMS[0]);
        let copied = doc.slice_html(at..at + 1);
        let mut again = doc.clone();
        let end = again.text.chars().count();
        again.paste_html(end..end, &copied);
        assert_eq!(
            crate::footnotes::extract_ids(&again.to_html()),
            ["fn-abc123", "fn-abc123"]
        );
    }

    #[test]
    fn text_without_atoms_counts_none() {
        assert_eq!(RichDoc::paste_atom_count("<p>Just words</p>"), 0);
        assert_eq!(RichDoc::paste_atom_count(""), 0);
        assert_eq!(
            RichDoc::paste_atom_count(&format!("<p>x{}y{}</p>", ATOMS[0], ATOMS[1])),
            2
        );
    }
}
