//! Rich text as an editor holds it: one line per block, inline formatting per character, and
//! footnote markers, citations and equations as single characters that keep their exact HTML.
//!
//! ProseCraft stores scenes as sanitized HTML (`src/lib/sanitize.ts`, upstream `7dae544`). An
//! editor here works on a [`RichDoc`]: the text with one `\n` per block, each block's kind, each
//! character's stack of inline tags, and the atoms. [`RichDoc::from_html`] reads a scene only if
//! [`RichDoc::to_html`] gives back **exactly** the same HTML, so editing can never drop or
//! rewrite what the model doesn't understand; such a scene stays read-only.

use crate::html::{Element, Node, parse_fragment};

/// One character standing for an atom (footnote marker, citation, inline equation): the object
/// replacement character.
pub const ATOM: char = '\u{FFFC}';
/// A line break inside a block (a newline in a text node, as ProseCraft's Markdown writes).
pub const SOFT_BREAK: char = '\u{2028}';
/// A `<br>` inside a block (Shift+Enter in ProseCraft's editor). Like [`SOFT_BREAK`] it ends
/// the row; unlike it, it isn't text (it adds nothing to the block's `textContent`).
pub const HARD_BREAK: char = '\u{2029}';

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
    Screenplay {
        kind: ScreenplayKind,
        bare: bool,
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
            Self::Eyebrow | Self::Screenplay { .. } => ("p", None),
            Self::Block => ("", None),
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

    /// Whether a list item's own list is numbered.
    pub fn numbered(self) -> bool {
        self.lists().last() == Some(&"ol")
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
    /// The scene was empty (`""`) and hasn't been written in; it stays `""`.
    pub empty_source: bool,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Rule,
    Code,
    Table,
    Equation,
    Diagram,
    Calculation,
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
            (kind, source.lines().map(str::to_owned).collect())
        }
        _ => return None,
    };
    let more = lines.len() > PREVIEW_LINES;
    Some(BlockPreview {
        kind,
        lines: lines.into_iter().take(PREVIEW_LINES).collect(),
        more,
    })
}

fn is_atom(element: &Element) -> bool {
    element.name == "img"
        || (element.name == "sup" && element.has_class("footnote-ref"))
        || (element.name == "span"
            && (element.has_class("cite-ref") || element.has_class("math-inline")))
}

/// Reading HTML into a document; `None` as soon as something isn't representable.
struct Reader {
    doc: RichDoc,
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
                        let c = if c == '\n' { SOFT_BREAK } else { c };
                        if c == ATOM || c == HARD_BREAK || c == '\r' {
                            return None;
                        }
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

    /// A `<p data-sp>` paragraph: a screenplay element (empty without a `<br>` too).
    fn screenplay(&mut self, element: &Element) -> Option<()> {
        let [(name, value)] = element.attrs.as_slice() else {
            return None;
        };
        let kind = (name == "data-sp")
            .then(|| ScreenplayKind::from_name(value))
            .flatten()?;
        let bare = element.children.is_empty();
        let mut plain = element.clone();
        plain.attrs.clear();
        self.block(&plain, LineKind::Screenplay { kind, bare })
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

    /// A block kept whole: one atom alone on its line.
    fn block_atom(&mut self, element: &Element) {
        if !self.doc.lines.is_empty() {
            self.doc.text.push('\n');
            self.doc.marks.push(Vec::new());
        }
        self.doc.lines.push(LineKind::Block);
        self.doc.atoms.push(outer_html(element));
        self.push_char(ATOM, &[]);
    }

    /// One block: its children are inline content; ProseCraft's empty block is `<br>` alone.
    fn block(&mut self, element: &Element, kind: LineKind) -> Option<()> {
        if !element.attrs.is_empty() {
            return None;
        }
        if !self.doc.lines.is_empty() {
            self.doc.text.push('\n');
            self.doc.marks.push(Vec::new());
        }
        self.doc.lines.push(kind);
        match element.children.as_slice() {
            [Node::Element(br)] if br.name == "br" && br.attrs.is_empty() => Some(()),
            children => self.inline(children, &mut Vec::new()),
        }
    }
}

fn is_whitespace(node: &Node) -> bool {
    matches!(node, Node::Text(t) if t.chars().all(|c| c == '\n'))
}

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
        let mut reader = Reader {
            doc: RichDoc::default(),
        };
        for node in parse_fragment(html).iter().filter(|n| !is_whitespace(n)) {
            let Node::Element(element) = node else {
                return None;
            };
            let container = |kind: LineKind, item: &str, reader: &mut Reader| -> Option<()> {
                if !element.attrs.is_empty() {
                    return None;
                }
                for child in element.children.iter().filter(|n| !is_whitespace(n)) {
                    match child {
                        Node::Element(e) if e.name == item => reader.block(e, kind)?,
                        _ => return None,
                    }
                }
                Some(())
            };
            match element.name.as_str() {
                "p" if element.attr("data-sp").is_some() => reader.screenplay(element)?,
                "p" if element.attrs == [("class".to_owned(), "eyebrow".to_owned())] => {
                    let mut plain = element.clone();
                    plain.attrs.clear();
                    reader.block(&plain, LineKind::Eyebrow)?;
                }
                "hr" | "pre" | "table" => reader.block_atom(element),
                "div" if RICH_BLOCK_CLASSES.iter().any(|c| element.has_class(c)) => {
                    reader.block_atom(element)
                }
                "p" => reader.block(element, LineKind::Paragraph)?,
                "h1" => reader.block(element, LineKind::Heading1)?,
                "h2" => reader.block(element, LineKind::Heading2)?,
                "h3" => reader.block(element, LineKind::Heading3)?,
                "blockquote" => container(LineKind::Quote, "p", &mut reader)?,
                "ul" | "ol" => reader.list(element, &mut Vec::new())?,
                _ => return None,
            }
        }
        let mut doc = reader.doc;
        if doc.lines.is_empty() {
            return None;
        }
        for newline_blocks in [false, true] {
            doc.newline_blocks = newline_blocks;
            if doc.to_html() == html {
                return Some(doc);
            }
        }
        None
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
        self.write_html(&mut Vec::new())
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
        let mut out = String::new();
        let mut container: Option<&str> = None;
        // The lists open, outermost first; each has an item open.
        let mut lists: Vec<&str> = Vec::new();
        let close_lists = |out: &mut String, lists: &mut Vec<&str>, keep: usize| {
            while lists.len() > keep {
                if let Some(list) = lists.pop() {
                    out.push_str(&format!("</li>{nl}</{list}>{nl}"));
                }
            }
        };
        let mut atoms = self.atoms.iter();
        for (index, (start, end)) in self.line_ranges().into_iter().enumerate() {
            let kind = self
                .lines
                .get(index)
                .copied()
                .unwrap_or(LineKind::Paragraph);
            let wanted_lists = kind.lists();
            if wanted_lists.is_empty() {
                close_lists(&mut out, &mut lists, 0);
            }
            let (block, wanted) = kind.tags();
            if container != wanted {
                if let Some(open) = container {
                    out.push_str(&format!("</{open}>{nl}"));
                }
                if let Some(open) = wanted {
                    out.push_str(&format!("<{open}>{nl}"));
                }
                container = wanted;
            }
            if kind == LineKind::Block {
                for i in start..end {
                    if chars.get(i) == Some(&ATOM) {
                        let from = out.len();
                        out.push_str(atoms.next().map(String::as_str).unwrap_or_default());
                        if let Some(span) = spans.get_mut(i) {
                            *span = from..out.len();
                        }
                    }
                }
                out.push_str(nl);
                continue;
            }
            if !wanted_lists.is_empty() {
                let common = lists
                    .iter()
                    .zip(&wanted_lists)
                    .take_while(|(a, b)| a == b)
                    .count();
                close_lists(&mut out, &mut lists, common);
                if lists.len() == wanted_lists.len() {
                    // The next item of the same list.
                    out.push_str(&format!("</li>{nl}<li>"));
                } else {
                    // Lists opened inside the item open above (or at the top).
                    for list in &wanted_lists[lists.len()..] {
                        out.push_str(&format!("<{list}>{nl}<li>"));
                    }
                    lists = wanted_lists.clone();
                }
                if start == end {
                    out.push_str("<br>");
                }
            }
            match kind {
                _ if !wanted_lists.is_empty() => {}
                LineKind::Eyebrow => {
                    out.push_str(r#"<p class="eyebrow">"#);
                    if start == end {
                        out.push_str("<br>");
                    }
                }
                LineKind::Screenplay { kind, bare } => {
                    out.push_str(&format!(r#"<p data-sp="{}">"#, kind.as_str()));
                    if start == end && !bare {
                        out.push_str("<br>");
                    }
                }
                _ => {
                    out.push_str(&format!("<{block}>"));
                    if start == end {
                        out.push_str("<br>");
                    }
                }
            }
            let mut open: Vec<&Tag> = Vec::new();
            for i in start..end {
                let stack = self.marks.get(i).map_or(&[][..], Vec::as_slice);
                let common = open
                    .iter()
                    .zip(stack.iter())
                    .take_while(|(a, b)| **a == *b)
                    .count();
                while open.len() > common {
                    if let Some(tag) = open.pop() {
                        out.push_str(&format!("</{}>", tag.name));
                    }
                }
                for tag in stack.iter().skip(common) {
                    out.push_str(&open_tag(&tag.name, &tag.attrs));
                    open.push(tag);
                }
                let from = out.len();
                match chars.get(i) {
                    Some(&ATOM) => {
                        out.push_str(atoms.next().map(String::as_str).unwrap_or_default())
                    }
                    Some(&HARD_BREAK) => out.push_str("<br>"),
                    Some(c) => out.push_str(&escape_text(&c.to_string())),
                    None => {}
                }
                if let Some(span) = spans.get_mut(i) {
                    *span = from..out.len();
                }
            }
            while let Some(tag) = open.pop() {
                out.push_str(&format!("</{}>", tag.name));
            }
            // A list item stays open: a list inside it may follow.
            if wanted_lists.is_empty() {
                out.push_str(&format!("</{block}>{nl}"));
            }
        }
        close_lists(&mut out, &mut lists, 0);
        if let Some(open) = container {
            out.push_str(&format!("</{open}>{nl}"));
        }
        out
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
        let html = self.write_html(&mut spans);
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
        let follow = match kind {
            LineKind::Heading1
            | LineKind::Heading2
            | LineKind::Heading3
            | LineKind::Eyebrow
            | LineKind::Block => LineKind::Paragraph,
            // A new screenplay element is written as the editor writes it (with its `<br>`).
            LineKind::Screenplay { kind, .. } => LineKind::Screenplay { kind, bare: false },
            other => other,
        };
        let breaks_added = inserted.iter().filter(|c| **c == '\n').count();
        for _ in 0..breaks_added {
            self.lines.insert((line + 1).min(self.lines.len()), follow);
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
        self.text.clone()
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
        let mut push = |kind: LineKind, range: std::ops::Range<usize>, lines: &mut Vec<_>| {
            if !lines.is_empty() {
                text.push('\n');
                marks.push(Vec::new());
            }
            lines.push(kind);
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
            let mut from = start;
            let mut pieces = 0;
            for i in (start..end).filter(|i| block[*i]) {
                if i > from {
                    push(kind, from..i, &mut lines);
                    pieces += 1;
                }
                push(LineKind::Block, i..i + 1, &mut lines);
                pieces += 1;
                from = i + 1;
            }
            if end > from || pieces == 0 {
                push(kind, from..end, &mut lines);
            }
        }
        self.text = text;
        self.marks = marks;
        self.lines = lines;
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
                    ..RichDoc::default()
                };
            }
            let start = ranges[lines.start].0;
            let end = ranges[lines.end - 1].1;
            let atoms_before = chars[..start].iter().filter(|c| **c == ATOM).count();
            let atoms_in = chars[start..end].iter().filter(|c| **c == ATOM).count();
            RichDoc {
                text: chars[start..end].iter().collect(),
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
                empty_source: false,
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
        }
    }

    /// The kind of the line holding character `index`.
    pub fn kind_at(&self, index: usize) -> LineKind {
        self.lines
            .get(self.line_of(index))
            .copied()
            .unwrap_or(LineKind::Paragraph)
    }
}

#[cfg(test)]
mod tests {
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
            "<div>x</div>",
            "<ul><li><pre>code</pre></li></ul>",
            "<ul><li><p>a</p><ul><li>b</li></ul></li></ul>",
            "<ul><li>a<ul><li>b</li></ul>c</li></ul>",
            "<ul><li></li></ul>",
            "<blockquote><hr></blockquote>",
            r#"<div class="other">x</div>"#,
            r#"<p class="eyebrow big">x</p>"#,
            "<p>a</p>\n<p>b</p><p>c</p>",
            r#"<p>a<br class="x">b</p>"#,
            "<p>a\u{2029}b</p>",
        ] {
            assert!(RichDoc::from_html(html).is_none(), "{html}");
        }
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
