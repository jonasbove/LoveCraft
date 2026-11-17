//! marked's `Lexer` and `Tokenizer` (`src/Lexer.ts`, `src/Tokenizer.ts`) with its default GFM
//! options and the block and inline extensions ProseCraft adds.
//!
//! Positions are bytes, not UTF-16 units. The masked copies of inline text keep the byte length
//! of the text they mask, so positions found in one stay valid in the other.

use super::helpers::{
    decode_numeric_character_references, expand_tabs, find_closing_bracket, js_slice_units,
    js_trim, js_trim_end, normalize_label, rtrim, skip, split_cells, trim_trailing_blank_lines,
};
use super::rules as r;
use crate::cite::{CiteRefItem, parse_pandoc_citation_cluster};
use crate::js::regexp::Re;
use std::collections::HashMap;
use std::sync::OnceLock;

/// The kinds of token marked produces, and ProseCraft's extension tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Kind {
    #[default]
    Space,
    Code,
    Heading,
    Hr,
    Blockquote,
    List,
    ListItem,
    Checkbox,
    Html,
    Def,
    Table,
    TableCell,
    Paragraph,
    Text,
    Escape,
    Link,
    Image,
    Strong,
    Em,
    Codespan,
    Br,
    Del,
    FootnoteDef,
    FootnoteRef,
    CiteRef,
    MathBlock,
    MathInline,
}

impl Kind {
    /// marked's `type` name.
    #[cfg(test)]
    pub fn name(self) -> &'static str {
        match self {
            Kind::Space => "space",
            Kind::Code => "code",
            Kind::Heading => "heading",
            Kind::Hr => "hr",
            Kind::Blockquote => "blockquote",
            Kind::List => "list",
            Kind::ListItem => "list_item",
            Kind::Checkbox => "checkbox",
            Kind::Html => "html",
            Kind::Def => "def",
            Kind::Table => "table",
            Kind::TableCell => "table_cell",
            Kind::Paragraph => "paragraph",
            Kind::Text => "text",
            Kind::Escape => "escape",
            Kind::Link => "link",
            Kind::Image => "image",
            Kind::Strong => "strong",
            Kind::Em => "em",
            Kind::Codespan => "codespan",
            Kind::Br => "br",
            Kind::Del => "del",
            Kind::FootnoteDef => "footnoteDef",
            Kind::FootnoteRef => "footnoteRef",
            Kind::CiteRef => "citeRef",
            Kind::MathBlock => "mathBlock",
            Kind::MathInline => "mathInline",
        }
    }
}

/// One token; which fields mean something depends on its kind, as in marked.
#[derive(Clone, Debug, Default)]
pub(crate) struct Token {
    pub kind: Kind,
    pub raw: String,
    pub text: String,
    pub tokens: Vec<Token>,
    /// Whether it has a `tokens` list (a block `text` token does, an inline one does not).
    pub has_tokens: bool,
    /// The inline queue entry whose tokens fill `tokens`.
    slot: Option<usize>,
    pub depth: usize,
    pub lang: String,
    pub href: String,
    pub title: Option<String>,
    pub ordered: bool,
    pub start: u64,
    pub loose: bool,
    pub items: Vec<Token>,
    pub task: bool,
    pub checked: bool,
    pub header: Vec<Token>,
    pub align: Vec<Option<&'static str>>,
    pub rows: Vec<Vec<Token>>,
    /// An `html` token at block level; for an inline `text` token, `escaped`.
    pub block: bool,
    pub escaped: bool,
    pub autolink: bool,
    pub id: String,
    pub cites: Vec<CiteRefItem>,
    pub display: bool,
    pub tag: String,
}

impl Token {
    fn new(kind: Kind, raw: impl Into<String>) -> Self {
        Token {
            kind,
            raw: raw.into(),
            ..Token::default()
        }
    }

    fn with_text(kind: Kind, raw: impl Into<String>, text: impl Into<String>) -> Self {
        Token {
            text: text.into(),
            ..Token::new(kind, raw)
        }
    }
}

struct Entry {
    src: String,
    tokens: Vec<Token>,
}

#[derive(Clone)]
struct LinkDef {
    href: String,
    title: Option<String>,
}

/// How many levels quotes and lists, and inline tokens (emphasis, links), may nest. marked
/// recurses without a limit and throws a `RangeError` some thousands of levels down; past this,
/// the port reads what is left as plain text, so a hostile file cannot overflow the stack. Up to
/// it (200 nested quotes, lists or emphasis), the two agree.
const MAX_NESTING: usize = 200;

pub(crate) struct Lexer {
    links: HashMap<String, LinkDef>,
    depth: usize,
    queue: Vec<Entry>,
    in_link: bool,
    in_raw_block: bool,
    link_emitted: bool,
    link_paren_possible: bool,
    top: bool,
}

fn cached(
    cell: &'static OnceLock<[Re; 4]>,
    make: fn(usize) -> String,
    flags: &str,
) -> &'static [Re; 4] {
    cell.get_or_init(|| [0, 1, 2, 3].map(|k| Re::new(&make(k), flags)))
}

/// `cachedIndentRegex`: the rule is built with `^ {0,k}`, `k` the indent less one, from 0 to 3.
fn indent_rule(
    cell: &'static OnceLock<[Re; 4]>,
    make: fn(usize) -> String,
    flags: &str,
    indent: usize,
) -> &'static Re {
    let k = indent.saturating_sub(1).min(3);
    &cached(cell, make, flags)[k]
}

fn next_bullet_rule(indent: usize) -> &'static Re {
    static CELL: OnceLock<[Re; 4]> = OnceLock::new();
    indent_rule(
        &CELL,
        |k| format!("^ {{0,{k}}}(?:[*+-]|\\d{{1,9}}[.)])((?:[ \t][^\\n]*)?(?:\\n|$))"),
        "",
        indent,
    )
}

fn hr_rule(indent: usize) -> &'static Re {
    static CELL: OnceLock<[Re; 4]> = OnceLock::new();
    indent_rule(
        &CELL,
        |k| {
            format!(
                "^ {{0,{k}}}((?:-[ \t]*){{3,}}|(?:_[ \t]*){{3,}}|(?:\\*[ \t]*){{3,}})(?:\\n+|$)"
            )
        },
        "",
        indent,
    )
}

fn fences_begin_rule(indent: usize) -> &'static Re {
    static CELL: OnceLock<[Re; 4]> = OnceLock::new();
    indent_rule(&CELL, |k| format!("^ {{0,{k}}}(?:```|~~~)"), "", indent)
}

fn heading_begin_rule(indent: usize) -> &'static Re {
    static CELL: OnceLock<[Re; 4]> = OnceLock::new();
    indent_rule(&CELL, |k| format!("^ {{0,{k}}}#"), "", indent)
}

fn html_begin_rule(indent: usize) -> &'static Re {
    static CELL: OnceLock<[Re; 4]> = OnceLock::new();
    indent_rule(
        &CELL,
        |k| format!("^ {{0,{k}}}{}", r::HTML_BEGIN_TAIL),
        "i",
        indent,
    )
}

fn blockquote_begin_rule(indent: usize) -> &'static Re {
    static CELL: OnceLock<[Re; 4]> = OnceLock::new();
    indent_rule(&CELL, |k| format!("^ {{0,{k}}}>"), "", indent)
}

macro_rules! ext_rule {
    ($name:ident, $source:expr) => {
        fn $name() -> &'static Re {
            static RULE: OnceLock<Re> = OnceLock::new();
            RULE.get_or_init(|| Re::new($source, ""))
        }
    };
}

// ProseCraft's extensions (`configureMarked` in `src/lib/publish/markdown.ts`).
ext_rule!(
    footnote_def_start,
    r"(?:^|\n)[ \t]{0,3}\[\^[A-Za-z0-9_-]{1,32}\]:"
);
ext_rule!(
    footnote_def_head,
    r"^[ \t]{0,3}\[\^([A-Za-z0-9_-]{1,32})\]:[ \t]*([^\n]*)(?:\n|$)"
);
ext_rule!(footnote_def_next, r"^[ \t]{0,3}\[\^[A-Za-z0-9_-]{1,32}\]:");
ext_rule!(footnote_def_cont, r"^(?: {2,4}|\t)([^\n]*)(?:\n|$)");
ext_rule!(footnote_def_blank_cont, r"^\n(?: {2,4}|\t)([^\n]*)(?:\n|$)");
ext_rule!(footnote_ref_start, r"\[\^[A-Za-z0-9_-]{1,32}\]");
ext_rule!(footnote_ref_rule, r"^\[\^([A-Za-z0-9_-]{1,32})\]");
ext_rule!(cite_ref_start, r"\[(?!\^)[^\]\n]*@");
ext_rule!(math_block_start, r"(?:^|\n)[ \t]*(?:\$\$|\\\[)");
ext_rule!(
    math_block_dollar,
    r"^\$\$[ \t]*(?:\r?\n)?([\s\S]+?)(?:\r?\n)?[ \t]*\$\$[ \t]*(?:\n|$)"
);
ext_rule!(
    math_block_bracket,
    r"^\\\[[ \t]*(?:\r?\n)?([\s\S]+?)(?:\r?\n)?[ \t]*\\\][ \t]*(?:\n|$)"
);
ext_rule!(math_inline_start, r"\$|\\\(");
ext_rule!(math_inline_paren, r"^\\\(([\s\S]+?)\\\)");
ext_rule!(math_inline_double, r"^\$\$([^$\n]+?)\$\$");
ext_rule!(
    math_inline_single,
    r"^\$(?![\s\d])([^$\n]+?)(?<!\s)\$(?!\d)"
);

/// The first line of `s` (`s.split('\n', 1)[0]`).
fn first_line(s: &str) -> &str {
    s.split('\n').next().unwrap_or_default()
}

/// The last character of a string as JavaScript's `slice(-1)` sees it: an astral character's
/// low surrogate stands alone, which no rule reads as a letter, digit, space or punctuation.
fn last_unit(s: &str) -> String {
    match s.chars().next_back() {
        Some(c) if c.len_utf16() == 2 => '\u{e000}'.to_string(),
        Some(c) => c.to_string(),
        None => String::new(),
    }
}

/// A byte range of `s`, clamped and kept on character boundaries.
fn cut(s: &str, from: usize, to: usize) -> &str {
    let mut to = to.min(s.len());
    while !s.is_char_boundary(to) {
        to -= 1;
    }
    let mut from = from.min(to);
    while !s.is_char_boundary(from) {
        from -= 1;
    }
    &s[from..to]
}

/// `startBlock` and `startInline`: where, after the first character, an extension's token
/// could begin, the earliest of the rules' `search` in `src.slice(1)`. When the first
/// character is astral, JavaScript's `slice(1)` leaves its low surrogate standing alone at the
/// start, so a rule's `^` cannot match at the next character; a private-use character stands
/// in for it.
fn extension_start(src: &str, rules: &[&Re]) -> Option<usize> {
    let first = src.chars().next()?;
    let rest = &src[first.len_utf8()..];
    if first.len_utf16() == 2 {
        let stand_in = '\u{e000}';
        let temp = format!("{stand_in}{rest}");
        let at = rules.iter().filter_map(|rule| rule.search(&temp)).min()?;
        Some(first.len_utf8() + at.saturating_sub(stand_in.len_utf8()))
    } else {
        let at = rules.iter().filter_map(|rule| rule.search(rest)).min()?;
        Some(first.len_utf8() + at)
    }
}

/// `indentCodeCompensation`.
fn indent_code_compensation(raw: &str, text: &str) -> String {
    let Some(m) = r::other_indent_code_compensation().exec(raw) else {
        return text.to_owned();
    };
    let indent_to_code = m.at(1).encode_utf16().count();
    text.split('\n')
        .map(|node| match r::other_beginning_space().exec(node) {
            None => node.into(),
            Some(space) => js_slice_units(
                node,
                space.whole().encode_utf16().count().min(indent_to_code),
            ),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `isLabelEndInsideToken`.
fn is_label_end_inside_token(src: &str, label: &str, label_start: usize) -> bool {
    if !label.contains('<') {
        return false;
    }
    let bytes = label.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'`' => {
                if let Some(code) = r::inline_code().exec(&label[i..]) {
                    i += code.whole().len();
                    continue;
                }
            }
            _ => {}
        }
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let token_src = skip(src, label_start + i);
        let found = r::inline_tag()
            .exec(token_src)
            .or_else(|| r::inline_autolink().exec(token_src));
        let Some(found) = found else {
            i += 1;
            continue;
        };
        if found.whole().len() > label.len() - i {
            return true;
        }
        i += found.whole().len();
    }
    false
}

fn is_text_like(token: Option<&Token>) -> bool {
    token.is_some_and(|t| matches!(t.kind, Kind::Paragraph | Kind::Text))
}

impl Lexer {
    pub fn new() -> Self {
        Lexer {
            links: HashMap::new(),
            depth: 0,
            queue: Vec::new(),
            in_link: false,
            in_raw_block: false,
            link_emitted: false,
            link_paren_possible: true,
            top: true,
        }
    }

    /// `lex`: block tokens, then the queued inline runs, which see every link definition.
    pub fn lex(mut self, src: &str) -> Vec<Token> {
        let src = r::other_carriage_return().replace(src, true, "\n");
        let mut tokens = Vec::new();
        self.block_tokens(&src, &mut tokens, false);
        for i in 0..self.queue.len() {
            let src = std::mem::take(&mut self.queue[i].src);
            let mut inline = std::mem::take(&mut self.queue[i].tokens);
            self.inline_tokens(&src, &mut inline);
            self.queue[i].tokens = inline;
        }
        let mut queue: Vec<Option<Vec<Token>>> =
            self.queue.into_iter().map(|e| Some(e.tokens)).collect();
        attach(&mut tokens, &mut queue);
        tokens
    }

    /// `lexInline`.
    pub fn lex_inline(mut self, src: &str) -> Vec<Token> {
        let mut tokens = Vec::new();
        self.inline_tokens(src, &mut tokens);
        tokens
    }

    /// `inline`: queue a run of inline text, lexed once every block is.
    fn inline(&mut self, src: &str) -> Option<usize> {
        self.queue.push(Entry {
            src: src.to_owned(),
            tokens: Vec::new(),
        });
        Some(self.queue.len() - 1)
    }

    fn set_last_queued(&mut self, src: &str) {
        if let Some(last) = self.queue.last_mut() {
            last.src = src.to_owned();
        }
    }

    /// The block extensions, in the order marked tries them (the last registered first).
    fn block_extension(&mut self, src: &str) -> Option<Token> {
        math_block(src).or_else(|| footnote_def(src))
    }

    /// Appends a token's raw text and text to the paragraph or text token before it, as marked
    /// joins them, and points the last queued inline run at the joined text. With `pop`, the
    /// joined token's own queued run (the last one) is dropped first.
    fn merge_into_last(&mut self, tokens: &mut [Token], raw: &str, text: &str, pop: bool) {
        let Some(last) = tokens.last_mut() else {
            return;
        };
        if !last.raw.ends_with('\n') {
            last.raw.push('\n');
        }
        last.raw.push_str(raw);
        last.text.push('\n');
        last.text.push_str(text);
        let joined = last.text.clone();
        if pop {
            self.queue.pop();
        }
        self.set_last_queued(&joined);
    }

    /// The block tokenizers between `code` and `def` in marked's order, each kept as it is.
    fn plain_block(&mut self, src: &str) -> Option<Token> {
        fences(src)
            .or_else(|| self.heading(src))
            .or_else(|| hr(src))
            .or_else(|| self.blockquote(src))
            .or_else(|| self.list(src))
            .or_else(|| html(src))
    }

    /// A link definition: joined to a paragraph before it, else recorded (the first of a label
    /// wins) and kept.
    fn push_def(&mut self, tokens: &mut Vec<Token>, token: Token) {
        if is_text_like(tokens.last()) {
            self.merge_into_last(tokens, &token.raw, &token.raw, false);
        } else if !self.links.contains_key(&token.tag) {
            self.links.insert(
                token.tag.clone(),
                LinkDef {
                    href: token.href.clone(),
                    title: token.title.clone(),
                },
            );
            tokens.push(token);
        }
    }

    /// A level of nesting is counted where it opens (a quote, a list), so the guard here only
    /// backs that up.
    fn block_tokens(&mut self, src: &str, tokens: &mut Vec<Token>, last_paragraph_clipped: bool) {
        if self.depth > MAX_NESTING {
            tokens.push(Token::with_text(Kind::Text, src, src));
            self.top = true;
            return;
        }
        self.block_tokens_inner(src, tokens, last_paragraph_clipped);
    }

    fn block_tokens_inner(
        &mut self,
        src: &str,
        tokens: &mut Vec<Token>,
        mut last_paragraph_clipped: bool,
    ) {
        let mut src = src;
        let mut src_length = usize::MAX;
        while !src.is_empty() && src.len() < src_length {
            src_length = src.len();
            if let Some(token) = self.block_extension(src) {
                src = skip(src, token.raw.len());
                tokens.push(token);
            } else if let Some(token) = space(src) {
                src = skip(src, token.raw.len());
                match tokens.last_mut() {
                    Some(last) if token.raw.len() == 1 => last.raw.push('\n'),
                    _ => tokens.push(token),
                }
            } else if let Some(token) = code(src) {
                src = skip(src, token.raw.len());
                if is_text_like(tokens.last()) {
                    self.merge_into_last(tokens, &token.raw, &token.text, false);
                } else {
                    tokens.push(token);
                }
            } else if let Some(token) = self.plain_block(src) {
                src = skip(src, token.raw.len());
                tokens.push(token);
            } else if let Some(token) = def(src) {
                src = skip(src, token.raw.len());
                self.push_def(tokens, token);
            } else if let Some(token) = self.table(src).or_else(|| self.lheading(src)) {
                src = skip(src, token.raw.len());
                tokens.push(token);
            } else {
                // `startBlock`: a paragraph stops where an extension's block could begin.
                let cut_src = extension_start(src, &[footnote_def_start(), math_block_start()])
                    .map_or(src, |end| cut(src, 0, end));
                if self.top
                    && let Some(token) = self.paragraph(cut_src)
                {
                    let consumed = token.raw.len();
                    if last_paragraph_clipped
                        && tokens.last().is_some_and(|t| t.kind == Kind::Paragraph)
                    {
                        self.merge_into_last(tokens, &token.raw, &token.text, true);
                    } else {
                        tokens.push(token);
                    }
                    last_paragraph_clipped = cut_src.len() != src.len();
                    src = skip(src, consumed);
                } else if let Some(token) = self.text(src) {
                    src = skip(src, token.raw.len());
                    if tokens.last().is_some_and(|t| t.kind == Kind::Text) {
                        self.merge_into_last(tokens, &token.raw, &token.text, true);
                    } else {
                        tokens.push(token);
                    }
                } else {
                    break;
                }
            }
        }
        self.top = true;
    }

    fn heading(&mut self, src: &str) -> Option<Token> {
        let cap = r::block_heading().exec(src)?;
        let mut text = js_trim(cap.at(2)).to_owned();
        if r::other_ending_hash().test(&text) {
            let trimmed = rtrim(&text, '#');
            if trimmed.is_empty() || r::other_ending_space_tab_char().test(trimmed) {
                text = js_trim(trimmed).to_owned();
            }
        }
        let mut token = Token::with_text(Kind::Heading, rtrim(cap.whole(), '\n'), text);
        token.depth = cap.at(1).len();
        token.has_tokens = true;
        token.slot = self.inline(&token.text);
        Some(token)
    }

    fn blockquote(&mut self, src: &str) -> Option<Token> {
        if self.depth >= MAX_NESTING {
            return None;
        }
        self.depth += 1;
        let token = self.blockquote_inner(src);
        self.depth -= 1;
        token
    }

    fn blockquote_inner(&mut self, src: &str) -> Option<Token> {
        let cap = r::block_blockquote().exec(src)?;
        let mut lines: Vec<String> = rtrim(cap.whole(), '\n')
            .split('\n')
            .map(str::to_owned)
            .collect();
        let mut raw = String::new();
        let mut text = String::new();
        let mut tokens: Vec<Token> = Vec::new();
        while !lines.is_empty() {
            let mut in_blockquote = false;
            let mut current = Vec::new();
            let mut i = 0;
            while i < lines.len() {
                if r::other_blockquote_start().test(&lines[i]) {
                    current.push(lines[i].clone());
                    in_blockquote = true;
                } else if !in_blockquote {
                    current.push(lines[i].clone());
                } else {
                    break;
                }
                i += 1;
            }
            lines.drain(..i);
            let current_raw = current.join("\n");
            let current_text =
                r::other_blockquote_setext_replace().replace(&current_raw, true, "\n    $1");
            let current_text =
                r::other_blockquote_setext_replace2().replace(&current_text, true, "");
            raw = if raw.is_empty() {
                current_raw
            } else {
                format!("{raw}\n{current_raw}")
            };
            text = if text.is_empty() {
                current_text.clone()
            } else {
                format!("{text}\n{current_text}")
            };
            let top = self.top;
            self.top = true;
            self.block_tokens(&current_text, &mut tokens, true);
            self.top = top;
            if lines.is_empty() {
                break;
            }
            let last_kind = tokens.last().map(|t| t.kind);
            match last_kind {
                Some(Kind::Code | Kind::Heading | Kind::Hr | Kind::Html) => break,
                Some(Kind::Blockquote) => {
                    let Some(old) = tokens.pop() else { break };
                    let continuation = lines.join("\n");
                    let new_text = format!(
                        "{}\n{}",
                        old.raw,
                        r::other_blockquote_setext_replace2().replace(&continuation, true, "")
                    );
                    let Some(new_token) = self.blockquote(&new_text) else {
                        tokens.push(old);
                        break;
                    };
                    let leftover = skip(&new_text, new_token.raw.len());
                    let leftover = leftover.strip_prefix('\n').unwrap_or(leftover);
                    let leftover_count = if leftover.is_empty() {
                        0
                    } else {
                        leftover.split('\n').count()
                    };
                    let consumed = &lines[..lines.len().saturating_sub(leftover_count)];
                    if !consumed.is_empty() {
                        raw = format!("{raw}\n{}", consumed.join("\n"));
                    }
                    text = format!(
                        "{}{}",
                        cut(&text, 0, text.len().saturating_sub(old.text.len())),
                        new_token.text
                    );
                    tokens.push(new_token);
                    break;
                }
                Some(Kind::List) => {
                    let Some(old) = tokens.pop() else { break };
                    let new_text = format!("{}\n{}", old.raw, lines.join("\n"));
                    let Some(new_token) = self.list(&new_text) else {
                        tokens.push(old);
                        break;
                    };
                    raw = format!(
                        "{}{}",
                        cut(&raw, 0, raw.len().saturating_sub(old.raw.len())),
                        new_token.raw
                    );
                    text = format!(
                        "{}{}",
                        cut(&text, 0, text.len().saturating_sub(old.raw.len())),
                        new_token.raw
                    );
                    lines = skip(&new_text, new_token.raw.len())
                        .split('\n')
                        .map(str::to_owned)
                        .collect();
                    tokens.push(new_token);
                    continue;
                }
                _ => {}
            }
        }
        let mut token = Token::with_text(Kind::Blockquote, raw, text);
        token.tokens = tokens;
        token.has_tokens = true;
        Some(token)
    }

    fn list(&mut self, src: &str) -> Option<Token> {
        if self.depth >= MAX_NESTING {
            return None;
        }
        self.depth += 1;
        let token = self.list_inner(src);
        self.depth -= 1;
        token
    }

    fn list_inner(&mut self, src: &str) -> Option<Token> {
        let cap = r::block_list().exec(src)?;
        let list_src = src;
        let mut src = src;
        let bull = js_trim(cap.at(1)).to_owned();
        let ordered = bull.len() > 1;
        let mut list = Token::new(Kind::List, "");
        list.ordered = ordered;
        if ordered {
            list.start = bull[..bull.len() - 1].parse().unwrap_or(0);
        }
        let last = bull.chars().next_back().unwrap_or('-');
        let bull = if ordered {
            format!("\\d{{1,9}}\\{last}")
        } else {
            format!("\\{bull}")
        };
        let item_rule = Re::new(
            &format!("^( {{0,3}}{bull})((?:[\t ][^\\n]*)?(?:\\n|$))"),
            "",
        );
        let taken = |rest: &str| list_src.len() - rest.len();
        let mut ends_with_blank_line = false;
        while !src.is_empty() {
            let mut end_early = false;
            let Some(cap) = item_rule.exec(src) else {
                break;
            };
            if r::block_hr().test(src) {
                break;
            }
            let item_start = taken(src);
            let bullet = cap.at(1);
            let rest_of_line = cap.at(2);
            src = skip(src, cap.whole().len());
            let bullet_indent = bullet.len();
            let first = first_line(rest_of_line);
            let line = r::other_leading_space_tab()
                .replace_with(first, false, |ws| expand_tabs(ws.whole(), bullet_indent));
            let next_line = first_line(src).to_owned();
            let blank_line = js_trim(&line).is_empty();
            let mut item_contents = String::new();
            let mut indent;
            if blank_line {
                indent = bullet_indent + 1;
            } else {
                indent = r::other_non_space_char().search(&line).unwrap_or(0);
                indent = if indent > 4 { 1 } else { indent };
                item_contents = js_slice_units(&line, indent).into_owned();
                indent += bullet_indent;
            }
            if blank_line && r::other_blank_line().test(&next_line) {
                src = skip(src, next_line.len() + 1);
                end_early = true;
            }
            if !end_early {
                src = item_continuation(src, line, blank_line, indent, &mut item_contents);
            }
            let raw = &list_src[item_start..taken(src)];
            if !list.loose {
                if ends_with_blank_line {
                    list.loose = true;
                } else if r::other_double_blank_line().test(raw) {
                    ends_with_blank_line = true;
                }
            }
            let mut item = Token::with_text(Kind::ListItem, raw, item_contents);
            item.task = r::other_list_is_task().test(&item.text);
            item.has_tokens = true;
            list.items.push(item);
        }
        let last_item = list.items.last_mut()?;
        last_item.raw = js_trim_end(&last_item.raw).to_owned();
        last_item.text = js_trim_end(&last_item.text).to_owned();
        list.raw = js_trim_end(&list_src[..taken(src)]).to_owned();
        for i in 0..list.items.len() {
            self.top = false;
            let text = list.items[i].text.clone();
            let mut tokens = Vec::new();
            self.block_tokens(&text, &mut tokens, false);
            if !list.loose {
                list.loose = tokens
                    .iter()
                    .filter(|t| t.kind == Kind::Space)
                    .any(|t| r::other_any_line().test(&t.raw));
            }
            list.items[i].tokens = tokens;
        }
        self.apply_tasks(&mut list);
        let loose = list.loose;
        if loose {
            for item in &mut list.items {
                item.loose = true;
                for token in &mut item.tokens {
                    if token.kind == Kind::Text {
                        token.kind = Kind::Paragraph;
                    }
                }
            }
        }
        Some(list)
    }

    /// Task list items: the `[ ]` or `[x]` taken from the item's text and its first block, and
    /// a checkbox token put first (in a paragraph of its own or the first one when the list is
    /// loose).
    fn apply_tasks(&mut self, list: &mut Token) {
        let loose = list.loose;
        for item in &mut list.items {
            let first_is_text = is_text_like(item.tokens.first());
            if item.task && first_is_text {
                let replace_task = |s: &str| r::other_list_replace_task().replace(s, false, "");
                item.text = replace_task(&item.text);
                if let Some(first) = item.tokens.first_mut() {
                    first.raw = replace_task(&first.raw);
                    first.text = replace_task(&first.text);
                }
                if let Some(entry) = self
                    .queue
                    .iter_mut()
                    .rev()
                    .find(|e| r::other_list_is_task().test(&e.src))
                {
                    entry.src = replace_task(&entry.src);
                }
                if let Some(task_raw) = r::other_list_task_checkbox().exec(&item.raw) {
                    let mut checkbox = Token::new(Kind::Checkbox, format!("{} ", task_raw.whole()));
                    checkbox.checked = task_raw.whole() != "[ ]";
                    item.checked = checkbox.checked;
                    if loose {
                        match item.tokens.first_mut() {
                            Some(first) if first.has_tokens => {
                                first.raw = format!("{}{}", checkbox.raw, first.raw);
                                first.text = format!("{}{}", checkbox.raw, first.text);
                                first.tokens.insert(0, checkbox);
                            }
                            _ => {
                                let mut paragraph = Token::with_text(
                                    Kind::Paragraph,
                                    checkbox.raw.clone(),
                                    checkbox.raw.clone(),
                                );
                                paragraph.has_tokens = true;
                                paragraph.tokens = vec![checkbox];
                                item.tokens.insert(0, paragraph);
                            }
                        }
                    } else {
                        item.tokens.insert(0, checkbox);
                    }
                }
            } else if item.task {
                item.task = false;
            }
        }
    }

    fn table(&mut self, src: &str) -> Option<Token> {
        let cap = r::block_table().exec(src)?;
        if !r::other_table_delimiter().test(cap.at(2)) {
            return None;
        }
        let headers = split_cells(cap.at(1), None);
        let aligns_src = r::other_table_align_chars().replace(cap.at(2), true, "");
        let aligns: Vec<&str> = aligns_src.split('|').collect();
        let rows: Vec<String> = match cap.get(3) {
            Some(rows) if !js_trim(rows).is_empty() => r::other_table_row_blank_line()
                .replace(rows, false, "")
                .split('\n')
                .map(str::to_owned)
                .collect(),
            _ => Vec::new(),
        };
        let mut table = Token::new(Kind::Table, rtrim(cap.whole(), '\n'));
        if headers.len() != aligns.len() {
            return None;
        }
        for align in aligns {
            table
                .align
                .push(if r::other_table_align_right().test(align) {
                    Some("right")
                } else if r::other_table_align_center().test(align) {
                    Some("center")
                } else if r::other_table_align_left().test(align) {
                    Some("left")
                } else {
                    None
                });
        }
        for (i, header) in headers.iter().enumerate() {
            let mut cell = Token::with_text(Kind::TableCell, "", header.clone());
            cell.slot = self.inline(header);
            cell.align.push(table.align[i]);
            table.header.push(cell);
        }
        for row in rows {
            let cells = split_cells(&row, Some(table.header.len()));
            let mut out = Vec::new();
            for (i, text) in cells.into_iter().enumerate() {
                let mut cell = Token::with_text(Kind::TableCell, "", text);
                cell.slot = self.inline(&cell.text.clone());
                cell.align.push(table.align.get(i).copied().flatten());
                out.push(cell);
            }
            table.rows.push(out);
        }
        Some(table)
    }

    fn lheading(&mut self, src: &str) -> Option<Token> {
        let cap = r::block_lheading().exec(src)?;
        let mut token =
            Token::with_text(Kind::Heading, rtrim(cap.whole(), '\n'), js_trim(cap.at(1)));
        token.depth = if cap.at(2).starts_with('=') { 1 } else { 2 };
        token.has_tokens = true;
        token.slot = self.inline(&token.text.clone());
        Some(token)
    }

    fn paragraph(&mut self, src: &str) -> Option<Token> {
        let cap = r::block_paragraph().exec(src)?;
        let body = cap.at(1);
        let text = body.strip_suffix('\n').unwrap_or(body);
        let mut token = Token::with_text(Kind::Paragraph, cap.whole(), text);
        token.has_tokens = true;
        token.slot = self.inline(text);
        Some(token)
    }

    fn text(&mut self, src: &str) -> Option<Token> {
        let cap = r::block_text().exec(src)?;
        let mut token = Token::with_text(Kind::Text, cap.whole(), cap.whole());
        token.has_tokens = true;
        token.block = true;
        token.slot = self.inline(cap.whole());
        Some(token)
    }

    /// `linkInText`.
    fn link_in_text(&self, text: &str) -> bool {
        self.link_in_text_at(text, 0)
    }

    fn link_in_text_at(&self, text: &str, depth: usize) -> bool {
        if !text.contains('[') || depth >= MAX_NESTING {
            return false;
        }
        for skip_match in r::inline_block_skip().all(text) {
            let before = text[..skip_match.start()].chars().next_back();
            if r::inline_link().test(skip_match.whole()) && before != Some('!') {
                return true;
            }
        }
        for found in r::inline_reflink_search().all(text) {
            let whole = found.whole();
            let ref_start = whole.rfind('[').unwrap_or(0);
            let label = cut(whole, ref_start + 1, whole.len().saturating_sub(1));
            if whole.starts_with('!') || !self.links.contains_key(&normalize_label(label)) {
                continue;
            }
            if ref_start > 1 && self.link_in_text_at(cut(whole, 1, ref_start - 1), depth + 1) {
                continue;
            }
            return true;
        }
        false
    }

    fn mask_reflink(&self, whole: &str) -> String {
        self.mask_reflink_at(whole, 0)
    }

    /// One `reflinkSearch` match masked to the same length, so emphasis cannot close inside a
    /// reference link.
    fn mask_reflink_at(&self, whole: &str, depth: usize) -> String {
        if depth >= MAX_NESTING {
            return whole.to_owned();
        }
        let ref_start = whole.rfind('[').unwrap_or(0);
        let label = cut(whole, ref_start + 1, whole.len().saturating_sub(1));
        if !self.links.contains_key(&normalize_label(label)) {
            return whole.to_owned();
        }
        if ref_start > 1 && !whole.starts_with('!') {
            let text = cut(whole, 1, ref_start - 1);
            if self.link_in_text(text) {
                let inner = r::inline_reflink_search()
                    .replace_with(text, true, |m| self.mask_reflink_at(m.whole(), depth + 1));
                return format!(
                    "[{inner}][{}]",
                    "a".repeat(whole.len().saturating_sub(ref_start + 2))
                );
            }
        }
        format!("[{}]", "a".repeat(whole.len().saturating_sub(2)))
    }

    pub(crate) fn inline_tokens(&mut self, src: &str, tokens: &mut Vec<Token>) {
        if self.depth >= MAX_NESTING {
            if !src.is_empty() {
                tokens.push(Token::with_text(Kind::Text, src, src));
            }
            return;
        }
        self.depth += 1;
        let previous = self.link_paren_possible;
        self.link_paren_possible = previous && src.contains(')');
        self.inline_tokens_inner(src, tokens);
        self.link_paren_possible = previous;
        self.depth -= 1;
    }

    /// The inline extensions, in the order marked tries them (the last registered first).
    fn inline_extension(&mut self, src: &str) -> Option<Token> {
        math_inline(src)
            .or_else(|| cite_ref(src))
            .or_else(|| footnote_ref(src))
    }

    fn inline_tokens_inner(&mut self, src: &str, tokens: &mut Vec<Token>) {
        let mut masked = src.to_owned();
        if src.contains('[') {
            masked = r::inline_reflink_search()
                .replace_with(&masked, true, |m| self.mask_reflink(m.whole()));
        }
        masked = r::inline_any_punctuation_outside_code().replace_with(&masked, true, |m| {
            let whole = m.whole();
            if m.get(2).is_some() {
                return "+".repeat(whole.len());
            }
            if let Some(code) = m.get(1).filter(|c| !c.is_empty()) {
                let middle = cut(whole, code.len(), whole.len() - code.len()).replace('`', "+");
                return format!("{code}{middle}{code}");
            }
            whole.to_owned()
        });
        masked = r::inline_block_skip().replace_with(&masked, true, |m| {
            let whole = m.whole();
            let offset = m.get(2).map_or(0, str::len);
            format!(
                "{}[{}]",
                cut(whole, 0, offset),
                "a".repeat(whole.len().saturating_sub(offset + 2))
            )
        });

        let mut src = src;
        let mut keep_prev_char = false;
        let mut prev_char = String::new();
        let mut src_length = usize::MAX;
        while !src.is_empty() {
            if src.len() < src_length {
                src_length = src.len();
            } else {
                break;
            }
            if !keep_prev_char {
                prev_char.clear();
            }
            keep_prev_char = false;

            if let Some(token) = self.inline_extension(src) {
                src = skip(src, token.raw.len());
                tokens.push(token);
                continue;
            }
            if let Some(cap) = r::inline_escape().exec(src) {
                src = skip(src, cap.whole().len());
                tokens.push(Token::with_text(Kind::Escape, cap.whole(), cap.at(1)));
                continue;
            }
            if let Some(token) = self.tag(src) {
                src = skip(src, token.raw.len());
                tokens.push(token);
                continue;
            }
            if let Some(token) = self.link(src) {
                src = skip(src, token.raw.len());
                tokens.push(token);
                continue;
            }
            if let Some(token) = self.reflink(src) {
                src = skip(src, token.raw.len());
                match tokens.last_mut() {
                    Some(last) if token.kind == Kind::Text && last.kind == Kind::Text => {
                        last.raw.push_str(&token.raw);
                        last.text.push_str(&token.text);
                    }
                    _ => tokens.push(token),
                }
                continue;
            }
            if let Some(token) = self.em_strong(src, &masked, &prev_char) {
                src = skip(src, token.raw.len());
                tokens.push(token);
                continue;
            }
            if let Some(token) = codespan(src) {
                src = skip(src, token.raw.len());
                tokens.push(token);
                continue;
            }
            if let Some(cap) = r::inline_br().exec(src) {
                src = skip(src, cap.whole().len());
                tokens.push(Token::new(Kind::Br, cap.whole()));
                continue;
            }
            if let Some(token) = self.del(src, &masked, &prev_char) {
                src = skip(src, token.raw.len());
                tokens.push(token);
                continue;
            }
            if let Some(token) = autolink(src) {
                src = skip(src, token.raw.len());
                tokens.push(token);
                continue;
            }
            if !self.in_link
                && let Some(token) = url(src)
            {
                src = skip(src, token.raw.len());
                tokens.push(token);
                continue;
            }
            // `startInline`: text stops where an extension's token could begin.
            let mut cut_src = src;
            if let Some(end) = extension_start(
                src,
                &[footnote_ref_start(), cite_ref_start(), math_inline_start()],
            ) {
                cut_src = cut(src, 0, end);
            }
            if let Some(cap) = r::inline_text().exec(cut_src) {
                let raw = cap.whole();
                src = skip(src, raw.len());
                if !raw.ends_with('_') {
                    prev_char = last_unit(raw);
                }
                keep_prev_char = true;
                let text = if self.in_raw_block {
                    raw.to_owned()
                } else {
                    decode_numeric_character_references(raw)
                };
                match tokens.last_mut() {
                    Some(last) if last.kind == Kind::Text => {
                        last.raw.push_str(raw);
                        last.text.push_str(&text);
                    }
                    _ => {
                        let mut token = Token::with_text(Kind::Text, raw, text);
                        token.escaped = self.in_raw_block;
                        tokens.push(token);
                    }
                }
                continue;
            }
            break;
        }
    }

    fn tag(&mut self, src: &str) -> Option<Token> {
        let cap = r::inline_tag().exec(src)?;
        let whole = cap.whole();
        if !self.in_link && r::other_start_a_tag().test(whole) {
            self.in_link = true;
        } else if self.in_link && r::other_end_a_tag().test(whole) {
            self.in_link = false;
        }
        if !self.in_raw_block && r::other_start_pre_script_tag().test(whole) {
            self.in_raw_block = true;
        } else if self.in_raw_block && r::other_end_pre_script_tag().test(whole) {
            self.in_raw_block = false;
        }
        Some(Token::with_text(Kind::Html, whole, whole))
    }

    /// `outputLink`.
    fn output_link(
        &mut self,
        label: &str,
        href: String,
        title: Option<String>,
        raw: &str,
    ) -> Option<Token> {
        let text = r::other_output_link_replace().replace(label, true, "$1");
        let is_image = raw.starts_with('!');
        let outer_in_link = self.in_link;
        let outer_link_emitted = self.link_emitted;
        let outer_in_raw_block = self.in_raw_block;
        self.in_link = true;
        self.link_emitted = false;
        let mut tokens = Vec::new();
        self.inline_tokens(&text, &mut tokens);
        let text_has_link = self.link_emitted;
        self.link_emitted = outer_link_emitted;
        self.in_link = outer_in_link;
        if !is_image {
            if text_has_link {
                self.in_raw_block = outer_in_raw_block;
                return None;
            }
            self.link_emitted = true;
        }
        let mut token =
            Token::with_text(if is_image { Kind::Image } else { Kind::Link }, raw, text);
        token.href = href;
        token.title = title.filter(|t| !t.is_empty());
        token.tokens = tokens;
        token.has_tokens = true;
        Some(token)
    }

    fn link(&mut self, src: &str) -> Option<Token> {
        if !self.link_paren_possible {
            return None;
        }
        let cap = r::inline_link().exec(src)?;
        let label = cap.at(1);
        let label_start = if cap.whole().starts_with('!') { 2 } else { 1 };
        if is_label_end_inside_token(src, label, label_start) {
            return None;
        }
        let mut whole = cap.whole().to_owned();
        let mut href_src = cap.at(2).to_owned();
        let mut title_src = cap.get(3).map(str::to_owned);
        let trimmed_url = js_trim(&href_src).to_owned();
        if r::other_start_angle_bracket().test(&trimmed_url) {
            if !r::other_end_angle_bracket().test(&trimmed_url) {
                return None;
            }
            let without_end = &trimmed_url[..trimmed_url.len() - 1];
            let rtrim_slash = rtrim(without_end, '\\');
            if (trimmed_url.len() - rtrim_slash.len()).is_multiple_of(2) {
                return None;
            }
        } else {
            let last_paren = find_closing_bracket(&href_src, b'(', b')');
            if last_paren == -2 {
                return None;
            }
            if last_paren > -1 {
                let last_paren = last_paren as usize;
                let start = if whole.find('!') == Some(0) { 5 } else { 4 };
                let link_len = start + label.len() + last_paren;
                href_src = cut(&href_src, 0, last_paren).to_owned();
                whole = js_trim(cut(&whole, 0, link_len)).to_owned();
                title_src = Some(String::new());
            }
        }
        let title = match &title_src {
            Some(t) if !t.is_empty() => {
                let mut chars = t.chars();
                chars.next();
                chars.next_back();
                chars.as_str().to_owned()
            }
            _ => String::new(),
        };
        let mut href = js_trim(&href_src).to_owned();
        if r::other_start_angle_bracket().test(&href) {
            let mut chars = href.chars();
            chars.next();
            chars.next_back();
            href = chars.as_str().to_owned();
        }
        let href = r::inline_any_punctuation().replace(&href, true, "$1");
        let title = r::inline_any_punctuation().replace(&title, true, "$1");
        self.output_link(label, href, Some(title), &whole)
    }

    fn reflink(&mut self, src: &str) -> Option<Token> {
        let cap = r::inline_reflink()
            .exec(src)
            .or_else(|| r::inline_nolink().exec(src))?;
        let label_start = if cap.whole().starts_with('!') { 2 } else { 1 };
        if is_label_end_inside_token(src, cap.at(1), label_start) {
            return None;
        }
        let link_string = cap.get(2).filter(|s| !s.is_empty()).unwrap_or(cap.at(1));
        let link_string = r::other_multiple_space_global().replace(link_string, true, " ");
        let Some(link) = self.links.get(&normalize_label(&link_string)).cloned() else {
            let text: String = cap.whole().chars().take(1).collect();
            return Some(Token::with_text(Kind::Text, text.clone(), text));
        };
        self.output_link(cap.at(1), link.href, link.title, cap.whole())
    }

    fn em_strong(&mut self, src: &str, masked: &str, prev_char: &str) -> Option<Token> {
        let found = r::inline_em_strong_l_delim().exec(src)?;
        let group = |i| found.get(i).filter(|g: &&str| !g.is_empty());
        if group(1).is_none() && group(2).is_none() && group(3).is_none() && group(4).is_none() {
            return None;
        }
        if group(4).is_some() && r::other_unicode_alpha_numeric().test(prev_char) {
            return None;
        }
        let next_char = group(1).or(group(3)).unwrap_or_default();
        if !(next_char.is_empty()
            || prev_char.is_empty()
            || r::inline_punctuation().test(prev_char))
        {
            return None;
        }
        let l_length = found.whole().chars().count() - 1;
        let delim_char = found.whole().chars().next()?;
        let mid_run = prev_char == delim_char.encode_utf8(&mut [0; 4]);
        let end_rule = if delim_char == '*' {
            r::inline_em_strong_r_delim_ast()
        } else {
            r::inline_em_strong_r_delim_und()
        };
        let masked = masked_tail(masked, src.len(), l_length);
        let mut delim_total = l_length as i64;
        let mut mid_delim_total = 0i64;
        for m in end_rule.each(masked) {
            let Some((index, r_delim)) =
                (1..=6).find_map(|i| m.get(i).filter(|g| !g.is_empty()).map(|g| (i, g)))
            else {
                continue;
            };
            let mut r_length = r_delim.chars().count() as i64;
            if index == 3 || index == 4 {
                delim_total += r_length;
                continue;
            } else if index == 5 || index == 6 {
                let l = l_length as i64;
                if l % 3 != 0 && (l + r_length) % 3 == 0 {
                    mid_delim_total += r_length;
                    continue;
                }
                if mid_run {
                    break;
                }
            }
            delim_total -= r_length;
            if delim_total > 0 {
                continue;
            }
            r_length = r_length.min(r_length + delim_total + mid_delim_total);
            let last_char_length = m.whole().chars().next().map_or(0, char::len_utf8);
            let end = l_length as i64 + m.start() as i64 + last_char_length as i64 + r_length;
            let raw = cut(src, 0, end.max(0) as usize);
            let (kind, trim) = if (l_length as i64).min(r_length) % 2 != 0 {
                (Kind::Em, 1)
            } else {
                (Kind::Strong, 2)
            };
            let text = cut(raw, trim, raw.len().saturating_sub(trim)).to_owned();
            let mut tokens = Vec::new();
            self.inline_tokens(&text, &mut tokens);
            let mut token = Token::with_text(kind, raw, text);
            token.tokens = tokens;
            token.has_tokens = true;
            return Some(token);
        }
        None
    }

    fn del(&mut self, src: &str, masked: &str, prev_char: &str) -> Option<Token> {
        let found = r::inline_del_l_delim().exec(src)?;
        let next_char = found.at(1);
        if !(next_char.is_empty()
            || prev_char.is_empty()
            || r::inline_punctuation().test(prev_char))
        {
            return None;
        }
        let l_length = found.whole().chars().count() - 1;
        let masked = masked_tail(masked, src.len(), l_length);
        let mut delim_total = l_length as i64;
        for m in r::inline_del_r_delim().each(masked) {
            let Some((index, r_delim)) =
                (1..=6).find_map(|i| m.get(i).filter(|g| !g.is_empty()).map(|g| (i, g)))
            else {
                continue;
            };
            let mut r_length = r_delim.chars().count() as i64;
            if r_length != l_length as i64 {
                continue;
            }
            if index == 3 || index == 4 {
                delim_total += r_length;
                continue;
            }
            delim_total -= r_length;
            if delim_total > 0 {
                continue;
            }
            r_length = r_length.min(r_length + delim_total);
            let last_char_length = m.whole().chars().next().map_or(0, char::len_utf8);
            let end = l_length as i64 + m.start() as i64 + last_char_length as i64 + r_length;
            let raw = cut(src, 0, end.max(0) as usize);
            let text = cut(raw, l_length, raw.len().saturating_sub(l_length)).to_owned();
            let mut tokens = Vec::new();
            self.inline_tokens(&text, &mut tokens);
            let mut token = Token::with_text(Kind::Del, raw, text);
            token.tokens = tokens;
            token.has_tokens = true;
            return Some(token);
        }
        None
    }
}

/// The lines after a list item's first that still belong to it, added to `contents`: what is
/// left of the source after them. `line` is the item's first line and `blank_line` whether it
/// is blank.
fn item_continuation<'s>(
    mut src: &'s str,
    mut line: String,
    mut blank_line: bool,
    indent: usize,
    item_contents: &mut String,
) -> &'s str {
    let next_bullet = next_bullet_rule(indent);
    let hr = hr_rule(indent);
    let fences_begin = fences_begin_rule(indent);
    let heading_begin = heading_begin_rule(indent);
    let html_begin = html_begin_rule(indent);
    let blockquote_begin = blockquote_begin_rule(indent);
    while !src.is_empty() {
        let raw_line = first_line(src);
        let next_line = raw_line;
        let without_tabs = r::other_leading_space_tab()
            .replace_with(next_line, false, |ws| expand_tabs(ws.whole(), 0));
        if [
            fences_begin,
            heading_begin,
            html_begin,
            blockquote_begin,
            next_bullet,
            hr,
        ]
        .iter()
        .any(|rule| rule.test(next_line))
        {
            break;
        }
        let first_non_space = r::other_non_space_char().search(&without_tabs);
        if first_non_space.is_some_and(|i| i >= indent) || js_trim(next_line).is_empty() {
            item_contents.push('\n');
            item_contents.push_str(&js_slice_units(&without_tabs, indent));
        } else {
            let tabs_expanded = r::other_tab_char_global().replace(&line, true, "    ");
            let indented_code = r::other_non_space_char()
                .search(&tabs_expanded)
                .is_some_and(|i| i >= 4);
            if blank_line
                || indented_code
                || [fences_begin, heading_begin, hr]
                    .iter()
                    .any(|rule| rule.test(&line))
            {
                break;
            }
            item_contents.push('\n');
            item_contents.push_str(next_line);
        }
        blank_line = js_trim(next_line).is_empty();
        src = skip(src, raw_line.len() + 1);
        line = js_slice_units(&without_tabs, indent).into_owned();
    }
    src
}

/// `maskedSrc.slice(-1 * src.length + lLength)`: the masked text from just after the opening
/// delimiter (all of it when nothing follows the delimiter, as `slice(-0)` is `slice(0)`).
fn masked_tail(masked: &str, src_len: usize, l_length: usize) -> &str {
    let keep = src_len.saturating_sub(l_length);
    if keep == 0 {
        masked
    } else {
        cut(masked, masked.len().saturating_sub(keep), masked.len())
    }
}

/// Fills each token's queued inline tokens in, after the queue has been lexed.
fn attach(tokens: &mut [Token], queue: &mut [Option<Vec<Token>>]) {
    for token in tokens {
        if let Some(slot) = token.slot.take()
            && let Some(inline) = queue.get_mut(slot).and_then(Option::take)
        {
            token.tokens.extend(inline);
        }
        attach(&mut token.tokens, queue);
        attach(&mut token.items, queue);
        attach(&mut token.header, queue);
        for row in &mut token.rows {
            attach(row, queue);
        }
    }
}

fn space(src: &str) -> Option<Token> {
    let cap = r::block_newline().exec(src)?;
    (!cap.whole().is_empty()).then(|| Token::new(Kind::Space, cap.whole()))
}

fn code(src: &str) -> Option<Token> {
    let cap = r::block_code().exec(src)?;
    let raw = trim_trailing_blank_lines(cap.whole());
    let text = r::other_code_remove_indent().replace(&raw, true, "");
    Some(Token::with_text(Kind::Code, raw, text))
}

fn fences(src: &str) -> Option<Token> {
    let cap = r::block_fences().exec(src)?;
    let raw = cap.whole();
    let text = indent_code_compensation(raw, cap.at(3));
    let mut token = Token::with_text(Kind::Code, raw, text);
    token.lang = r::inline_any_punctuation().replace(js_trim(cap.at(2)), true, "$1");
    Some(token)
}

fn hr(src: &str) -> Option<Token> {
    let cap = r::block_hr().exec(src)?;
    Some(Token::new(Kind::Hr, rtrim(cap.whole(), '\n')))
}

fn html(src: &str) -> Option<Token> {
    let cap = r::block_html().exec(src)?;
    let raw = trim_trailing_blank_lines(cap.whole());
    let mut token = Token::with_text(Kind::Html, raw.clone(), raw);
    token.block = true;
    Some(token)
}

fn def(src: &str) -> Option<Token> {
    let cap = r::block_def().exec(src)?;
    let href_src = cap.at(2);
    if !r::other_start_angle_bracket().test(href_src)
        && find_closing_bracket(href_src, b'(', b')') != -1
    {
        return None;
    }
    let mut token = Token::new(Kind::Def, rtrim(cap.whole(), '\n'));
    token.tag = r::other_multiple_space_global().replace(&normalize_label(cap.at(1)), true, " ");
    token.href = if href_src.is_empty() {
        String::new()
    } else {
        let unbracketed = r::other_href_brackets().replace(href_src, false, "$1");
        r::inline_any_punctuation().replace(&unbracketed, true, "$1")
    };
    token.title = cap.get(3).filter(|t| !t.is_empty()).map(|t| {
        let mut chars = t.chars();
        chars.next();
        chars.next_back();
        r::inline_any_punctuation().replace(chars.as_str(), true, "$1")
    });
    Some(token)
}

fn codespan(src: &str) -> Option<Token> {
    let cap = r::inline_code().exec(src)?;
    let mut text = r::other_new_line_char_global().replace(cap.at(2), true, " ");
    let has_non_space = r::other_non_space_char().test(&text);
    let spaced =
        r::other_starting_space_char().test(&text) && r::other_ending_space_char().test(&text);
    if has_non_space && spaced {
        text = text[1..text.len() - 1].to_owned();
    }
    Some(Token::with_text(Kind::Codespan, cap.whole(), text))
}

fn link_token(raw: &str, text: &str, href: String) -> Token {
    let mut token = Token::with_text(Kind::Link, raw, text);
    token.href = href;
    token.autolink = true;
    token.has_tokens = true;
    token.tokens = vec![Token::with_text(Kind::Text, text, text)];
    token
}

fn autolink(src: &str) -> Option<Token> {
    let cap = r::inline_autolink().exec(src)?;
    let text = cap.at(1);
    let href = if cap.get(2) == Some("@") {
        format!("mailto:{text}")
    } else {
        text.to_owned()
    };
    Some(link_token(cap.whole(), text, href))
}

fn url(src: &str) -> Option<Token> {
    let cap = r::inline_url().exec(src)?;
    if cap.get(2) == Some("@") {
        let text = cap.whole();
        return Some(link_token(text, text, format!("mailto:{text}")));
    }
    let mut whole = cap.whole().to_owned();
    loop {
        let previous = whole.clone();
        whole = r::inline_backpedal()
            .exec(&previous)
            .map(|m| m.whole().to_owned())
            .unwrap_or_default();
        if whole == previous {
            break;
        }
    }
    let href = if cap.get(1) == Some("www.") {
        format!("http://{whole}")
    } else {
        whole.clone()
    };
    Some(link_token(&whole, &whole, href))
}

// ProseCraft's extension tokenizers.

/// The `footnoteDef` block: `[^id]: text`, with indented continuation lines.
fn footnote_def(src: &str) -> Option<Token> {
    let head = footnote_def_head().exec(src)?;
    let mut raw = head.whole().to_owned();
    let mut lines = vec![head.at(2).to_owned()];
    let mut rest = &src[raw.len()..];
    while !rest.is_empty() {
        if footnote_def_next().test(rest) {
            break;
        }
        if let Some(cont) = footnote_def_cont().exec(rest) {
            raw.push_str(cont.whole());
            lines.push(cont.at(1).to_owned());
            rest = &rest[cont.whole().len()..];
            continue;
        }
        if let Some(cont) = footnote_def_blank_cont().exec(rest) {
            raw.push_str(cont.whole());
            lines.push(String::new());
            lines.push(cont.at(1).to_owned());
            rest = &rest[cont.whole().len()..];
            continue;
        }
        break;
    }
    let mut token = Token::with_text(Kind::FootnoteDef, raw, js_trim(&lines.join("\n")));
    token.id = head.at(1).to_owned();
    Some(token)
}

/// The `mathBlock` block: `$$ … $$` or `\[ … \]`.
fn math_block(src: &str) -> Option<Token> {
    let cap = math_block_dollar()
        .exec(src)
        .or_else(|| math_block_bracket().exec(src))?;
    Some(Token::with_text(
        Kind::MathBlock,
        cap.whole(),
        js_trim(cap.at(1)),
    ))
}

/// The `footnoteRef` inline token: `[^id]`.
fn footnote_ref(src: &str) -> Option<Token> {
    let cap = footnote_ref_rule().exec(src)?;
    let mut token = Token::new(Kind::FootnoteRef, cap.whole());
    token.id = cap.at(1).to_owned();
    Some(token)
}

/// The `citeRef` inline token: a Pandoc citation cluster, `[see @key, p. 4]`.
fn cite_ref(src: &str) -> Option<Token> {
    if !src.starts_with('[') || src.starts_with("[^") {
        return None;
    }
    let bytes = src.as_bytes();
    let mut end = None;
    let mut escaped = false;
    for (i, ch) in src.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '[' | '\n' | '\r' => return None,
            ']' => {
                end = Some(i);
                break;
            }
            _ => {}
        }
    }
    let end = end.filter(|&e| e > 1)?;
    let next = bytes.get(end + 1).copied();
    if next == Some(b'(') || (next == Some(b'[') && bytes.get(end + 2) != Some(&b'^')) {
        return None;
    }
    let cites = parse_pandoc_citation_cluster(&src[1..end])?;
    let mut token = Token::new(Kind::CiteRef, &src[..=end]);
    token.cites = cites;
    Some(token)
}

/// The `mathInline` token: `\( … \)`, `$$ … $$` or `$ … $` (not currency).
fn math_inline(src: &str) -> Option<Token> {
    let (cap, display) = if let Some(cap) = math_inline_paren().exec(src) {
        (cap, false)
    } else if let Some(cap) = math_inline_double().exec(src) {
        (cap, true)
    } else {
        (math_inline_single().exec(src)?, false)
    };
    let mut token = Token::with_text(Kind::MathInline, cap.whole(), js_trim(cap.at(1)));
    token.display = display;
    Some(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_compiles() {
        for rule in r::ALL {
            assert!(rule().compiled());
        }
        for indent in 0..6 {
            for rule in [
                next_bullet_rule,
                hr_rule,
                fences_begin_rule,
                heading_begin_rule,
                html_begin_rule,
                blockquote_begin_rule,
            ] {
                assert!(rule(indent).compiled());
            }
        }
        for rule in [
            footnote_def_start,
            footnote_def_head,
            footnote_def_next,
            footnote_def_cont,
            footnote_def_blank_cont,
            footnote_ref_start,
            footnote_ref_rule,
            cite_ref_start,
            math_block_start,
            math_block_dollar,
            math_block_bracket,
            math_inline_start,
            math_inline_paren,
            math_inline_double,
            math_inline_single,
        ] {
            assert!(rule().compiled());
        }
    }
}
