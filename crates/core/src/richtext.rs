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
            Self::Bullet => ("li", Some("ul")),
            Self::Numbered => ("li", Some("ol")),
            Self::Screenplay { .. } => ("p", None),
        }
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
    /// The text: one `\n` between blocks, [`ATOM`] for each atom, [`SOFT_BREAK`] for a break.
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

fn is_atom(element: &Element) -> bool {
    (element.name == "sup" && element.has_class("footnote-ref"))
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
                        if c == ATOM || c == '\r' {
                            return None;
                        }
                        self.push_char(c, stack);
                    }
                }
                Node::Element(e) if is_atom(e) => {
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
                "p" => reader.block(element, LineKind::Paragraph)?,
                "h1" => reader.block(element, LineKind::Heading1)?,
                "h2" => reader.block(element, LineKind::Heading2)?,
                "h3" => reader.block(element, LineKind::Heading3)?,
                "blockquote" => container(LineKind::Quote, "p", &mut reader)?,
                "ul" => container(LineKind::Bullet, "li", &mut reader)?,
                "ol" => container(LineKind::Numbered, "li", &mut reader)?,
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
        if self.empty_source && self.text.is_empty() {
            return String::new();
        }
        let chars: Vec<char> = self.text.chars().collect();
        let nl = if self.newline_blocks { "\n" } else { "" };
        let mut out = String::new();
        let mut container: Option<&str> = None;
        let mut atoms = self.atoms.iter();
        for (index, (start, end)) in self.line_ranges().into_iter().enumerate() {
            let kind = self
                .lines
                .get(index)
                .copied()
                .unwrap_or(LineKind::Paragraph);
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
            match kind {
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
                match chars.get(i) {
                    Some(&ATOM) => {
                        out.push_str(atoms.next().map(String::as_str).unwrap_or_default())
                    }
                    Some(c) => out.push_str(&escape_text(&c.to_string())),
                    None => {}
                }
            }
            while let Some(tag) = open.pop() {
                out.push_str(&format!("</{}>", tag.name));
            }
            out.push_str(&format!("</{block}>{nl}"));
        }
        if let Some(open) = container {
            out.push_str(&format!("</{open}>{nl}"));
        }
        out
    }
}

impl RichDoc {
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
            LineKind::Heading1 | LineKind::Heading2 | LineKind::Heading3 => LineKind::Paragraph,
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
        self.text.clone()
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
        let targets: Vec<usize> = range
            .filter(|i| chars.get(*i).is_some_and(|c| *c != '\n'))
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

    /// Sets the kind of every line touched by characters `range`.
    pub fn set_line_kind(&mut self, range: std::ops::Range<usize>, kind: LineKind) {
        let first = self.line_of(range.start);
        let last = self.line_of(range.end.max(range.start));
        for line in first..=last {
            if let Some(slot) = self.lines.get_mut(line) {
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
    use super::{ATOM, LineKind, Mark, RichDoc};

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
            "<p>a<br>b</p>",
            "<table><tr><td>x</td></tr></table>",
            "<p><img src=\"data:image/png;base64,AA==\"></p>",
            "<div>x</div>",
            "<pre>code</pre>",
            "<p>a</p>\n<p>b</p><p>c</p>",
        ] {
            assert!(RichDoc::from_html(html).is_none(), "{html}");
        }
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
