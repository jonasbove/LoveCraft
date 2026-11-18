//! PDF export (ProseCraft `src/lib/publish/pdf.ts`, upstream `2d1cf18`).
//!
//! ProseCraft lays the book out itself with jsPDF and the PDF standard fonts: words are measured
//! with the fonts' metrics ([`super::pdf_text`]), wrapped, justified and drawn line by line,
//! with running headers, page numbers, chapter endnotes and a table of contents filled in at the
//! end. This is that layout, step for step, producing pages of drawing operations
//! ([`Page`]); [`super::pdf_writer`] writes them as a PDF file.
//!
//! Differences:
//! - Text the standard fonts can't show (outside Windows-1252) is drawn as "?" and counted in
//!   [`Layout::unshown`]; jsPDF writes such a string's UTF-16 bytes, which read as other
//!   characters (jennraeross/ProseCraft#53).
//! - Covers aren't ported yet, so there is no cover page.

use std::collections::HashMap;

use super::footnotes::{ResolvedFootnote, resolve_book_footnotes};
use super::html::{matter_paragraphs, style_tokens};
use super::pdf_text::{Face, Family, win_ansi};
use super::rich_text::{
    Block, BlockKind, InlineImage, InlineRun, blocks_from_html, inline_runs_from_html,
};
use super::{CompiledBook, PublishOptions, SceneSeparator};

pub type Rgb = [u8; 3];

const INK: Rgb = [29, 33, 29];
const MUTED: Rgb = [115, 119, 111];
const RULE: Rgb = [185, 189, 181];

/// A drawing operation, in points from the page's top left (as jsPDF takes them).
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// Text whose baseline starts at (`x`, `y`).
    Text {
        x: f64,
        y: f64,
        face: Face,
        size: f64,
        color: Rgb,
        text: String,
    },
    Line {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        color: Rgb,
    },
    /// An embedded image (a `data:` URL) in the box from (`x`, `y`).
    Image {
        src: String,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Page {
    pub ops: Vec<Op>,
}

/// The laid-out book.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub width: f64,
    pub height: f64,
    pub pages: Vec<Page>,
    pub title: String,
    pub author: String,
    pub subtitle: String,
    /// Characters drawn as "?" because the standard fonts can't show them.
    pub unshown: usize,
}

/// A word, space, break, footnote number or inline image, measured.
#[derive(Clone, Debug)]
struct Token {
    text: String,
    width: f64,
    space: bool,
    line_break: bool,
    superscript: bool,
    run: InlineRun,
    image: Option<TokenImage>,
}

#[derive(Clone, Debug)]
struct TokenImage {
    src: String,
    width: f64,
    height: f64,
}

#[derive(Clone, Debug)]
struct Line {
    tokens: Vec<Token>,
    width: f64,
    available: f64,
    last: bool,
}

#[derive(Clone, Debug, Default)]
struct ParagraphOptions {
    size: f64,
    line_height: f64,
    first_indent: f64,
    left_indent: f64,
    justify: bool,
    center: bool,
    bold: bool,
    italic: bool,
    spacing_before: f64,
    spacing_after: f64,
    color: Option<Rgb>,
    drop_cap: bool,
}

/// The size of an embedded image, as `getImageProperties` reads it.
pub trait ImageSizes {
    fn size(&self, src: &str) -> Option<(f64, f64)>;
}

/// JavaScript's `\s`.
fn is_space(c: char) -> bool {
    crate::html::is_js_whitespace(c)
}

/// `run.text.split(/(\s+)/)` without empty parts.
fn split_spaces(text: &str) -> Vec<(String, bool)> {
    let mut parts: Vec<(String, bool)> = Vec::new();
    for c in text.chars() {
        let space = is_space(c);
        match parts.last_mut() {
            Some((part, s)) if *s == space => part.push(c),
            _ => parts.push((c.to_string(), space)),
        }
    }
    parts
}

struct Writer<'a> {
    family: Family,
    width: f64,
    height: f64,
    content_left: f64,
    content_right: f64,
    content_top: f64,
    content_bottom: f64,
    content_width: f64,
    body_size: f64,
    body_line: f64,
    running_header: bool,
    page_numbers: bool,
    header: String,
    margin_top: f64,
    margin_bottom: f64,
    pages: Vec<Page>,
    current: usize,
    page_number: usize,
    y: f64,
    body_started: bool,
    page_has_content: bool,
    front_matter_mode: bool,
    images: &'a dyn ImageSizes,
    unshown: usize,
    no_notes: HashMap<String, ResolvedFootnote>,
    /// jsPDF's current font and size: the last `setFont`/`setFontSize` (measuring sets it too).
    font: std::cell::Cell<(Face, f64)>,
}

impl Writer<'_> {
    fn face(&self, run: &InlineRun, bold: bool, italic: bool) -> Face {
        Face::new(self.family, run.bold || bold, run.italic || italic)
    }

    fn measure(&self, text: &str, run: &InlineRun, size: f64, bold: bool, italic: bool) -> f64 {
        let face = self.face(run, bold, italic);
        self.font.set((face, size));
        face.text_width(text, size)
    }

    fn add_page(&mut self) {
        self.pages.push(Page::default());
        self.current = self.pages.len() - 1;
    }

    /// `doc.text(text, x, y)`, with `align` "center" or "right" moving `x` as jsPDF does.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        text: &str,
        x: f64,
        y: f64,
        face: Face,
        size: f64,
        color: Rgb,
        align: Option<&str>,
    ) {
        self.font.set((face, size));
        // jsPDF aligns with unkerned widths.
        let width = face.aligned_width(text, size);
        let x = match align {
            Some("center") => x - width / 2.0,
            Some("right") => x - width,
            _ => x,
        };
        let mut shown = String::with_capacity(text.len());
        for c in text.chars() {
            if win_ansi(c).is_some() {
                shown.push(c);
            } else {
                self.unshown += 1;
                shown.push('?');
            }
        }
        self.pages[self.current].ops.push(Op::Text {
            x,
            y,
            face,
            size,
            color,
            text: shown,
        });
    }

    fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, color: Rgb) {
        self.pages[self.current].ops.push(Op::Line {
            x1,
            y1,
            x2,
            y2,
            color,
        });
    }

    fn image(&mut self, src: &str, x: f64, y: f64, width: f64, height: f64) {
        self.pages[self.current].ops.push(Op::Image {
            src: src.to_owned(),
            x,
            y,
            width,
            height,
        });
    }

    fn draw_header_footer(&mut self) {
        let face = Face::new(self.family, false, false);
        if self.running_header {
            let header = self.header.clone();
            self.text(
                &header,
                self.width / 2.0,
                self.margin_top / 2.0 + 3.0,
                face,
                8.5,
                MUTED,
                Some("center"),
            );
        }
        if self.page_numbers {
            let number = self.page_number.to_string();
            self.text(
                &number,
                self.width / 2.0,
                self.height - self.margin_bottom / 2.0 + 3.0,
                face,
                8.5,
                MUTED,
                Some("center"),
            );
        }
    }

    fn start_front_matter_page(&mut self) {
        if self.page_has_content {
            self.add_page();
            self.page_number += 1;
        }
        self.page_has_content = true;
        self.y = self.content_top + self.body_size;
    }

    fn new_body_page(&mut self) {
        self.add_page();
        self.page_number += 1;
        self.y = self.content_top + self.body_size;
        self.draw_header_footer();
    }

    fn begin_body(&mut self) {
        if self.body_started {
            return;
        }
        if self.page_has_content {
            self.add_page();
            self.page_number += 1;
        }
        self.page_has_content = true;
        self.body_started = true;
        self.y = self.content_top + self.body_size;
        self.draw_header_footer();
    }

    fn ensure_space(&mut self, height: f64) {
        if self.y + height <= self.content_bottom {
            return;
        }
        if self.front_matter_mode {
            self.start_front_matter_page();
        } else {
            self.new_body_page();
        }
    }

    fn tokenize(
        &self,
        runs: &[InlineRun],
        size: f64,
        bold: bool,
        italic: bool,
        notes: &HashMap<String, ResolvedFootnote>,
    ) -> Vec<Token> {
        let mut tokens = Vec::new();
        let token = |text: String, width: f64, run: &InlineRun| Token {
            text,
            width,
            space: false,
            line_break: false,
            superscript: false,
            run: run.clone(),
            image: None,
        };
        for run in runs {
            if let Some(id) = &run.footnote_id {
                if let Some(note) = notes.get(id) {
                    let text = note.number.to_string();
                    let width = self.measure(&text, run, size * 0.68, bold, italic);
                    let mut t = token(text, width, run);
                    t.superscript = true;
                    tokens.push(t);
                }
                continue;
            }
            if run.line_break {
                let mut t = token(String::new(), 0.0, run);
                t.line_break = true;
                tokens.push(t);
                continue;
            }
            if let Some(image) = &run.image {
                let (natural_w, natural_h) = self.natural_size(image);
                let height = size * 1.15;
                let width = (height * (natural_w / natural_h)).max(1.0);
                let mut t = token(String::new(), width, run);
                t.image = Some(TokenImage {
                    src: image.src.clone(),
                    width,
                    height,
                });
                tokens.push(t);
                continue;
            }
            for (part, space) in split_spaces(&run.text) {
                let text = if space { " ".to_owned() } else { part };
                let width = self.measure(&text, run, size, bold, italic);
                let mut t = token(text, width, run);
                t.space = space;
                tokens.push(t);
            }
        }
        tokens
    }

    /// `run.image.width ?? properties.width` and the same for the height.
    fn natural_size(&self, image: &InlineImage) -> (f64, f64) {
        let (w, h) = self.images.size(&image.src).unwrap_or((1.0, 1.0));
        (image.width.unwrap_or(w), image.height.unwrap_or(h))
    }

    /// `splitToken`: as many characters as fit in `max`, at least one, and the rest.
    fn split_token(
        &self,
        token: &Token,
        max: f64,
        size: f64,
        bold: bool,
        italic: bool,
    ) -> (Token, Option<Token>) {
        let units: Vec<u16> = token.text.encode_utf16().collect();
        if units.len() <= 1 {
            return (token.clone(), None);
        }
        let slice = |r: std::ops::Range<usize>| String::from_utf16_lossy(&units[r]);
        let mut index = 1;
        while index < units.len() {
            let candidate = slice(0..index + 1);
            if self.measure(&candidate, &token.run, size, bold, italic) > max {
                break;
            }
            index += 1;
        }
        let head = slice(0..index);
        let rest = slice(index..units.len());
        let mut head_token = token.clone();
        head_token.width = self.measure(&head, &token.run, size, bold, italic);
        head_token.text = head;
        let rest_token = (!rest.is_empty()).then(|| {
            let mut t = token.clone();
            t.width = self.measure(&rest, &token.run, size, bold, italic);
            t.text = rest;
            t
        });
        (head_token, rest_token)
    }

    #[allow(clippy::too_many_arguments)]
    fn wrap_tokens(
        &self,
        input: Vec<Token>,
        max_width: f64,
        first_indent: f64,
        size: f64,
        bold: bool,
        italic: bool,
        offset_lines: usize,
        offset_width: f64,
    ) -> Vec<Line> {
        let mut lines: Vec<Line> = Vec::new();
        let mut current: Vec<Token> = Vec::new();
        let mut line_width = 0.0;
        let mut line_index = 0;
        let mut available = max_width
            - first_indent
            - if line_index < offset_lines {
                offset_width
            } else {
                0.0
            };
        let push_line = |current: &mut Vec<Token>,
                         line_width: &mut f64,
                         line_index: &mut usize,
                         available: &mut f64,
                         lines: &mut Vec<Line>| {
            while current.last().is_some_and(|t| t.space) {
                if let Some(t) = current.pop() {
                    *line_width -= t.width;
                }
            }
            lines.push(Line {
                tokens: std::mem::take(current),
                width: *line_width,
                available: *available,
                last: false,
            });
            *line_width = 0.0;
            *line_index += 1;
            *available = max_width
                - if *line_index < offset_lines {
                    offset_width
                } else {
                    0.0
                };
        };
        for original in input {
            let mut token = original;
            if token.line_break {
                push_line(
                    &mut current,
                    &mut line_width,
                    &mut line_index,
                    &mut available,
                    &mut lines,
                );
                continue;
            }
            if token.image.is_some() {
                if line_width + token.width > available && current.iter().any(|t| !t.space) {
                    push_line(
                        &mut current,
                        &mut line_width,
                        &mut line_index,
                        &mut available,
                        &mut lines,
                    );
                }
                if token.width > available {
                    let scale = available / token.width;
                    token.width = available;
                    if let Some(image) = token.image.as_mut() {
                        image.width = available;
                        image.height *= scale;
                    }
                }
                line_width += token.width;
                current.push(token);
                continue;
            }
            if token.space {
                if current.is_empty() {
                    continue;
                }
                if line_width + token.width > available {
                    push_line(
                        &mut current,
                        &mut line_width,
                        &mut line_index,
                        &mut available,
                        &mut lines,
                    );
                    continue;
                }
                line_width += token.width;
                current.push(token);
                continue;
            }
            if line_width + token.width > available && current.iter().any(|t| !t.space) {
                push_line(
                    &mut current,
                    &mut line_width,
                    &mut line_index,
                    &mut available,
                    &mut lines,
                );
            }
            while token.width > available {
                let (head, rest) = self.split_token(&token, available, size, bold, italic);
                line_width = head.width;
                current.push(head);
                push_line(
                    &mut current,
                    &mut line_width,
                    &mut line_index,
                    &mut available,
                    &mut lines,
                );
                match rest {
                    Some(rest) => token = rest,
                    None => {
                        token.text.clear();
                        token.width = 0.0;
                        break;
                    }
                }
            }
            if !token.text.is_empty() {
                line_width += token.width;
                current.push(token);
            }
        }
        if !current.is_empty() || lines.is_empty() {
            push_line(
                &mut current,
                &mut line_width,
                &mut line_index,
                &mut available,
                &mut lines,
            );
        }
        if let Some(last) = lines.last_mut() {
            last.last = true;
        }
        lines
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_line(
        &mut self,
        line: &Line,
        x: f64,
        baseline: f64,
        size: f64,
        bold: bool,
        italic: bool,
        justify: bool,
        color: Rgb,
    ) {
        let gaps = line.tokens.iter().filter(|t| t.space).count();
        let extra = if justify && !line.last && gaps > 0 {
            ((line.available - line.width) / gaps as f64).max(0.0)
        } else {
            0.0
        };
        let mut cursor = x;
        for token in &line.tokens {
            if let Some(image) = &token.image {
                self.image(
                    &image.src,
                    cursor,
                    baseline - image.height * 0.82,
                    image.width,
                    image.height,
                );
                cursor += token.width;
                continue;
            }
            let face = self.face(&token.run, bold, italic);
            if token.superscript {
                self.text(
                    &token.text,
                    cursor,
                    baseline - size * 0.35,
                    face,
                    size * 0.68,
                    color,
                    None,
                );
                cursor += token.width;
                continue;
            }
            self.text(&token.text, cursor, baseline, face, size, color, None);
            if token.run.underline {
                self.line(
                    cursor,
                    baseline + 2.0,
                    cursor + token.width,
                    baseline + 2.0,
                    INK,
                );
            }
            if token.run.strike {
                self.line(
                    cursor,
                    baseline - size * 0.3,
                    cursor + token.width,
                    baseline - size * 0.3,
                    INK,
                );
            }
            cursor += token.width + if token.space { extra } else { 0.0 };
        }
    }

    fn render_paragraph(
        &mut self,
        runs: &[InlineRun],
        opts: &ParagraphOptions,
        notes: &HashMap<String, ResolvedFootnote>,
    ) {
        if runs.is_empty() {
            return;
        }
        let max_width = self.content_width - opts.left_indent;
        let mut draw_runs: Vec<InlineRun> = runs.to_vec();
        let mut letter = String::new();
        let mut drop_width = 0.0;
        let drop_lines = 3;
        if opts.drop_cap
            && let Some(first) = draw_runs
                .iter()
                .position(|r| !r.line_break && r.text.chars().any(|c| !is_space(c)))
        {
            let run = draw_runs[first].clone();
            let leading: String = run.text.chars().take_while(|c| is_space(*c)).collect();
            if let Some(c) = run.text[leading.len()..].chars().next() {
                letter = c.to_string();
                let rest = &run.text[leading.len() + c.len_utf8()..];
                if rest.is_empty() {
                    draw_runs.remove(first);
                } else {
                    draw_runs[first].text = rest.to_owned();
                }
                if draw_runs.is_empty() {
                    return;
                }
                let face = Face::new(self.family, true, false);
                self.font.set((face, self.body_size * 5.2));
                drop_width = face.text_width(&letter, self.body_size * 5.2) + 3.0;
            }
        }
        let tokens = self.tokenize(&draw_runs, opts.size, opts.bold, opts.italic, notes);
        let first_indent = if letter.is_empty() {
            opts.first_indent
        } else {
            0.0
        };
        let offset_lines = if letter.is_empty() { 0 } else { drop_lines };
        let lines = self.wrap_tokens(
            tokens,
            max_width,
            first_indent,
            opts.size,
            opts.bold,
            opts.italic,
            offset_lines,
            drop_width,
        );
        if lines.is_empty() {
            return;
        }
        self.y += opts.spacing_before;
        self.ensure_space(
            if letter.is_empty() {
                1.0
            } else {
                drop_lines as f64
            } * opts.line_height,
        );
        let color = opts.color.unwrap_or(INK);
        let start_y = self.y;
        for (index, line) in lines.iter().enumerate() {
            self.ensure_space(opts.line_height);
            let offset = if index < offset_lines {
                drop_width
            } else {
                0.0
            };
            let indent = if index == 0 { first_indent } else { 0.0 };
            let x = if opts.center {
                self.content_left
                    + opts.left_indent
                    + offset
                    + ((max_width - offset - indent - line.width) / 2.0).max(0.0)
            } else {
                self.content_left + opts.left_indent + indent + offset
            };
            let y = self.y;
            self.draw_line(
                line,
                x,
                y,
                opts.size,
                opts.bold,
                opts.italic,
                opts.justify && !opts.center,
                color,
            );
            self.y += opts.line_height;
        }
        if !letter.is_empty() {
            let baseline = start_y + (drop_lines - 1) as f64 * opts.line_height;
            let face = Face::new(self.family, true, false);
            let x = self.content_left + opts.left_indent;
            self.text(
                &letter,
                x,
                baseline,
                face,
                self.body_size * 5.2,
                color,
                None,
            );
            if lines.len() < drop_lines {
                self.y += (drop_lines - lines.len()) as f64 * opts.line_height;
            }
        }
        self.y += opts.spacing_after;
    }

    fn render_heading(
        &mut self,
        block: &Block,
        size: f64,
        notes: &HashMap<String, ResolvedFootnote>,
    ) {
        let runs: Vec<InlineRun> = block
            .runs
            .iter()
            .map(|r| InlineRun {
                bold: true,
                ..r.clone()
            })
            .collect();
        let opts = ParagraphOptions {
            size,
            line_height: size * 1.3,
            spacing_before: size * 0.8,
            spacing_after: size * 0.35,
            bold: true,
            ..ParagraphOptions::default()
        };
        self.render_paragraph(&runs, &opts, notes);
    }

    fn render_image(&mut self, block: &Block) {
        let Some(image) = &block.image else {
            return;
        };
        let (natural_w, natural_h) = self.images.size(&image.src).unwrap_or((1.0, 1.0));
        let width = image.width.unwrap_or(natural_w);
        let height = image.height.unwrap_or(natural_h);
        let max_height = 288f64.min(self.content_bottom - self.content_top - self.body_line * 0.5);
        let scale = 1f64
            .min(self.content_width / width)
            .min(max_height / height);
        let (w, h) = (width * scale, height * scale);
        self.ensure_space(h + self.body_line * 0.5);
        let x = self.content_left + (self.content_width - w) / 2.0;
        let y = self.y;
        self.image(&image.src, x, y, w, h);
        self.y += h + self.body_line * 0.5;
    }

    #[allow(clippy::too_many_arguments)]
    fn render_block(
        &mut self,
        block: &Block,
        first_in_scene: bool,
        drop_cap: bool,
        indent_em: f64,
        spacing_em: f64,
        justify: bool,
        notes: &HashMap<String, ResolvedFootnote>,
    ) {
        let body = self.body_size;
        match block.kind {
            BlockKind::Image => self.render_image(block),
            BlockKind::Heading => {
                let factor = match block.level {
                    Some(1) => 1.45,
                    Some(2) => 1.28,
                    _ => 1.12,
                };
                self.render_heading(block, body * factor, notes);
            }
            BlockKind::Quote => {
                let runs: Vec<InlineRun> = block
                    .runs
                    .iter()
                    .map(|r| InlineRun {
                        italic: true,
                        ..r.clone()
                    })
                    .collect();
                let opts = ParagraphOptions {
                    size: body,
                    line_height: self.body_line,
                    left_indent: body * 1.5,
                    spacing_before: body * 0.4,
                    spacing_after: body * 0.4,
                    color: Some(MUTED),
                    ..ParagraphOptions::default()
                };
                self.render_paragraph(&runs, &opts, notes);
            }
            BlockKind::List => {
                let mut runs = vec![InlineRun {
                    text: format!("{} ", block.marker.as_deref().unwrap_or("•")),
                    ..InlineRun::default()
                }];
                runs.extend(block.runs.iter().cloned());
                let opts = ParagraphOptions {
                    size: body,
                    line_height: self.body_line,
                    left_indent: body * 1.5,
                    spacing_after: body * 0.2,
                    ..ParagraphOptions::default()
                };
                self.render_paragraph(&runs, &opts, notes);
            }
            BlockKind::Paragraph if block.eyebrow => {
                let runs: Vec<InlineRun> = block
                    .runs
                    .iter()
                    .map(|r| InlineRun {
                        text: r.text.to_uppercase(),
                        bold: true,
                        ..r.clone()
                    })
                    .collect();
                let opts = ParagraphOptions {
                    size: body * 0.68,
                    line_height: body * 1.15,
                    spacing_before: body * 0.2,
                    spacing_after: body * 0.45,
                    color: Some(MUTED),
                    ..ParagraphOptions::default()
                };
                self.render_paragraph(&runs, &opts, notes);
            }
            BlockKind::Paragraph => {
                let opts = ParagraphOptions {
                    size: body,
                    line_height: self.body_line,
                    first_indent: if first_in_scene {
                        0.0
                    } else {
                        indent_em * body
                    },
                    justify,
                    spacing_after: spacing_em * body,
                    drop_cap,
                    ..ParagraphOptions::default()
                };
                self.render_paragraph(&block.runs, &opts, notes);
            }
        }
    }

    /// `renderChapterEndnotes`: the chapter's footnotes after it, under a short rule.
    fn render_chapter_endnotes(&mut self, notes: &[ResolvedFootnote]) {
        if notes.is_empty() {
            return;
        }
        let body = self.body_size;
        let note_size = body * 0.85;
        let note_line = note_size * 1.4;
        self.ensure_space(self.body_line * 2.5);
        self.y += body * 0.7;
        let (x, y) = (self.content_left, self.y);
        let end = self.content_left + 96f64.min(self.content_width * 0.3);
        self.line(x, y, end, y, RULE);
        self.y += body * 1.1;
        let heading = [InlineRun {
            text: "Notes".into(),
            bold: true,
            ..InlineRun::default()
        }];
        let opts = ParagraphOptions {
            size: note_size,
            line_height: note_line,
            spacing_after: note_size * 0.4,
            bold: true,
            color: Some(MUTED),
            ..ParagraphOptions::default()
        };
        let none = self.no_notes.clone();
        self.render_paragraph(&heading, &opts, &none);
        for note in notes {
            let mut runs = vec![InlineRun {
                text: format!("{}. ", note.number),
                bold: true,
                ..InlineRun::default()
            }];
            runs.extend(inline_runs_from_html(&note.html));
            let opts = ParagraphOptions {
                size: note_size,
                line_height: note_line,
                left_indent: note_size * 0.9,
                spacing_after: note_size * 0.35,
                color: Some(INK),
                ..ParagraphOptions::default()
            };
            self.render_paragraph(&runs, &opts, &none);
        }
    }

    fn render_scene_title(&mut self, title: &str) {
        let body = self.body_size;
        let runs = [InlineRun {
            text: title.to_owned(),
            bold: true,
            italic: true,
            ..InlineRun::default()
        }];
        let opts = ParagraphOptions {
            size: body,
            line_height: self.body_line,
            bold: true,
            italic: true,
            spacing_before: body * 0.7,
            spacing_after: body * 0.35,
            ..ParagraphOptions::default()
        };
        let none = self.no_notes.clone();
        self.render_paragraph(&runs, &opts, &none);
    }

    fn render_separator(&mut self, separator: SceneSeparator, spacing_em: f64) {
        if separator == SceneSeparator::None {
            return;
        }
        let body = self.body_size;
        let spacing = body * spacing_em;
        self.ensure_space(spacing);
        self.y += spacing / 2.0;
        let face = Face::new(self.family, false, false);
        let middle = self.width / 2.0;
        let y = self.y;
        match separator {
            SceneSeparator::Hash => {
                self.text("#", middle, y, face, body * 0.9, MUTED, Some("center"))
            }
            SceneSeparator::Asterisks => {
                self.text("* * *", middle, y, face, body * 0.9, MUTED, Some("center"))
            }
            SceneSeparator::Line => self.line(
                middle - 32.0,
                y - body * 0.3,
                middle + 32.0,
                y - body * 0.3,
                RULE,
            ),
            _ => {}
        }
        self.y += spacing / 2.0;
    }

    fn plain_runs(text: &str) -> Vec<InlineRun> {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut runs = Vec::new();
        for (index, line) in normalized.split('\n').enumerate() {
            if index > 0 {
                runs.push(InlineRun {
                    line_break: true,
                    ..InlineRun::default()
                });
            }
            runs.push(InlineRun {
                text: line.to_owned(),
                ..InlineRun::default()
            });
        }
        runs
    }

    fn render_matter_page(&mut self, page: &super::MatterPage, justify: bool) {
        let body = self.body_size;
        let none = self.no_notes.clone();
        if !page.title.is_empty() {
            let opts = ParagraphOptions {
                size: body * 1.5,
                line_height: body * 1.8,
                bold: true,
                center: true,
                spacing_after: body * 0.8,
                ..ParagraphOptions::default()
            };
            self.render_paragraph(&Self::plain_runs(&page.title), &opts, &none);
        }
        for paragraph in matter_paragraphs(&page.body) {
            let opts = ParagraphOptions {
                size: body,
                line_height: self.body_line,
                justify,
                spacing_after: body * 0.6,
                ..ParagraphOptions::default()
            };
            self.render_paragraph(&Self::plain_runs(&paragraph), &opts, &none);
        }
    }

    /// The title page, the copyright page and the dedication (`renderTitlePage`,
    /// `renderCopyrightPage`, `renderDedication`).
    fn render_opening_pages(&mut self, book: &CompiledBook, options: &PublishOptions) {
        let regular = Face::new(self.family, false, false);
        let has_copyright_page = options.include_copyright_page && !book.copyright.is_empty();
        if options.include_title_page {
            self.start_front_matter_page();
            let title_size = (self.body_size * 2.7).min(34.0);
            let mut y = self.height * 0.3;
            for line in regular.split_text_to_size(&book.title, self.content_width, title_size) {
                self.text(
                    &line,
                    self.width / 2.0,
                    y,
                    regular,
                    title_size,
                    INK,
                    Some("center"),
                );
                y += title_size * 1.18;
            }
            if !book.subtitle.is_empty() {
                let face = Face::new(Family::Times, false, true);
                self.text(
                    &book.subtitle,
                    self.width / 2.0,
                    y + 16.0,
                    face,
                    self.body_size * 1.1,
                    MUTED,
                    Some("center"),
                );
            }
            if !book.author.is_empty() {
                self.text(
                    &format!("by {}", book.author),
                    self.width / 2.0,
                    self.height * 0.55,
                    regular,
                    self.body_size,
                    MUTED,
                    Some("center"),
                );
            }
            if !book.copyright.is_empty() && !options.include_copyright_page {
                let lines =
                    regular.split_text_to_size(&book.copyright, self.content_width * 0.9, 8.5);
                let mut y = self.height - self.margin_bottom - (lines.len() as f64 - 1.0) * 11.0;
                for line in lines {
                    self.text(
                        &line,
                        self.width / 2.0,
                        y,
                        regular,
                        8.5,
                        MUTED,
                        Some("center"),
                    );
                    y += 11.0;
                }
            }
        }
        if has_copyright_page {
            self.start_front_matter_page();
            let size = self.body_size * 0.82;
            let line_height = size * 1.6;
            let lines =
                regular.split_text_to_size(&book.copyright, self.content_width * 0.85, size);
            let mut y = (self.height * 0.45)
                .min(self.content_bottom - (lines.len() as f64 - 1.0) * line_height);
            for line in lines {
                self.text(
                    &line,
                    self.width / 2.0,
                    y,
                    regular,
                    size,
                    MUTED,
                    Some("center"),
                );
                y += line_height;
            }
        }
        if !book.dedication.is_empty() {
            self.start_front_matter_page();
            let face = Face::new(Family::Times, false, true);
            let size = self.body_size * 1.05;
            let mut y = self.height * 0.45;
            for line in face.split_text_to_size(&book.dedication, self.content_width * 0.7, size) {
                self.text(
                    &line,
                    self.width / 2.0,
                    y,
                    face,
                    size,
                    MUTED,
                    Some("center"),
                );
                y += self.body_size * 1.55;
            }
            self.y = self.content_top + self.body_size;
        }
    }

    /// `renderTableOfContents`: "Contents" and each chapter's label with its first page, on
    /// the pages kept for them.
    fn render_contents(
        &mut self,
        c: &Contents,
        pages: &[usize],
        entries: &[Vec<String>],
        start_pages: &[usize],
    ) {
        let Some(&first) = pages.first() else {
            return;
        };
        self.current = first;
        let bold = Face::new(self.family, true, false);
        let regular = Face::new(self.family, false, false);
        let (middle, size) = (self.width / 2.0, self.body_size);
        self.text(
            "Contents",
            middle,
            c.heading_baseline,
            bold,
            c.heading_size,
            INK,
            Some("center"),
        );
        let mut page_index = 0;
        let mut cursor = c.first_baseline;
        for (index, lines) in entries.iter().enumerate() {
            let h = c.entry_height(lines);
            if cursor + h > c.bottom && page_index < pages.len() - 1 {
                page_index += 1;
                self.current = pages[page_index];
                cursor = self.content_top + size;
            }
            for (line_index, line) in lines.iter().enumerate() {
                let left = self.content_left;
                self.text(line, left, cursor, regular, size, INK, None);
                if line_index == lines.len() - 1 {
                    let number = start_pages.get(index).copied().unwrap_or(0).to_string();
                    let right = self.content_right;
                    self.text(&number, right, cursor, regular, size, INK, Some("right"));
                }
                cursor += c.line_height;
            }
            cursor += c.entry_gap;
        }
    }
}

/// Where the table of contents goes on its pages.
struct Contents {
    heading_size: f64,
    heading_baseline: f64,
    first_baseline: f64,
    line_height: f64,
    entry_gap: f64,
    bottom: f64,
}

impl Contents {
    fn entry_height(&self, lines: &[String]) -> f64 {
        lines.len().max(1) as f64 * self.line_height + self.entry_gap
    }
}

/// `exportPdf` up to the drawing: the book laid out on pages.
pub fn layout_pdf(
    book: &CompiledBook,
    options: &PublishOptions,
    images: &dyn ImageSizes,
) -> Layout {
    let (book, bibliography) = super::citations::resolve_book_citations(book, options);
    let tokens = style_tokens(&options.style);
    let [
        width,
        height,
        margin_top,
        margin_right,
        margin_bottom,
        margin_left,
    ] = tokens.page;
    // All the built-in styles use Times (`pdfFont`).
    let (running_header, page_numbers) = (tokens.running_header, tokens.page_numbers);
    let body_size = tokens.body_size;
    let content_top = margin_top;
    let content_bottom = height - margin_bottom;
    let mut w = Writer {
        family: Family::Times,
        width,
        height,
        content_left: margin_left,
        content_right: width - margin_right,
        content_top,
        content_bottom,
        content_width: width - margin_right - margin_left,
        body_size,
        body_line: body_size * tokens.line_height,
        running_header,
        page_numbers,
        header: if book.author.is_empty() {
            book.title.clone()
        } else {
            format!("{} / {}", book.author, book.title)
        },
        margin_top,
        margin_bottom,
        pages: vec![Page::default()],
        current: 0,
        page_number: 1,
        y: content_top + body_size,
        body_started: false,
        page_has_content: false,
        front_matter_mode: false,
        images,
        unshown: 0,
        no_notes: HashMap::new(),
        font: std::cell::Cell::new((Face::new(Family::Times, false, false), 16.0)),
    };
    let regular = Face::new(Family::Times, false, false);
    let has_toc = options.include_table_of_contents && !book.chapters.is_empty();

    let toc_heading_size = body_size * 1.5;
    let toc_heading_baseline = content_top + toc_heading_size;
    let toc_first_baseline = toc_heading_baseline + body_size * 1.7;
    let toc_line_height = w.body_line;
    let toc_entry_gap = body_size * 0.35;
    let toc_max_width = w.content_width - 48.0;
    let toc_bottom = content_bottom - body_size * 0.5;
    // `setFont("normal", bodySize)` before measuring the contents.
    w.font.set((regular, body_size));
    let toc_entries: Vec<Vec<String>> = if has_toc {
        book.chapters
            .iter()
            .map(|c| regular.split_text_to_size(&c.label, toc_max_width, body_size))
            .collect()
    } else {
        Vec::new()
    };
    let entry_height =
        |lines: &Vec<String>| lines.len().max(1) as f64 * toc_line_height + toc_entry_gap;
    let mut toc_page_count = 0;
    if has_toc {
        toc_page_count = 1;
        let mut cursor = toc_first_baseline;
        for entry in &toc_entries {
            let h = entry_height(entry);
            if cursor + h > toc_bottom {
                toc_page_count += 1;
                cursor = content_top + body_size;
            }
            cursor += h;
        }
    }
    let mut toc_pages: Vec<usize> = Vec::new();
    let mut chapter_start_pages: Vec<usize> = Vec::new();

    w.render_opening_pages(&book, options);
    for _ in 0..toc_page_count {
        w.start_front_matter_page();
        toc_pages.push(w.current);
    }
    for page in &book.front_matter {
        w.start_front_matter_page();
        w.front_matter_mode = true;
        w.render_matter_page(page, tokens.justify);
        w.front_matter_mode = false;
    }

    let notes = resolve_book_footnotes(&book, options.footnotes_restart_per_chapter);
    let none = HashMap::new();
    for (c, chapter) in book.chapters.iter().enumerate() {
        if c == 0 {
            w.begin_body();
        } else if options.page_break_before_chapters {
            w.new_body_page();
        } else {
            w.ensure_space(w.body_line * 2.0);
            w.y += body_size * 1.4;
        }
        let mut start_page = w.page_number;
        if options.include_chapter_titles {
            let size = tokens.chapter_size;
            let text = if tokens.chapter_uppercase {
                chapter.label.to_uppercase()
            } else {
                chapter.label.clone()
            };
            // Split before `setFont("bold", headingSize)`: in whatever font was set last.
            let (state_face, state_size) = w.font.get();
            let lines = state_face.split_text_to_size(&text, w.content_width, state_size);
            let face = Face::new(Family::Times, true, false);
            w.ensure_space(lines.len() as f64 * size * 1.25 + w.body_line * 2.0);
            start_page = w.page_number;
            w.y += tokens.chapter_before * size;
            let center = tokens.chapter_align == "center";
            for line in lines {
                let (x, align) = if center {
                    (width / 2.0, Some("center"))
                } else {
                    (margin_left, None)
                };
                let y = w.y;
                w.text(&line, x, y, face, size, INK, align);
                w.y += size * 1.25;
            }
            w.y += tokens.chapter_after * size;
        }
        chapter_start_pages.push(start_page);
        for (s, scene) in chapter.scenes.iter().enumerate() {
            if options.include_scene_titles {
                w.render_scene_title(&scene.title);
            }
            let blocks = blocks_from_html(&scene.html);
            let scene_notes = notes.by_scene.get(&(c, s)).unwrap_or(&none);
            let drop_cap = if options.drop_caps && tokens.drop_cap && s == 0 {
                blocks
                    .iter()
                    .position(|b| b.kind == BlockKind::Paragraph && !b.eyebrow)
            } else {
                None
            };
            for (b, block) in blocks.iter().enumerate() {
                w.render_block(
                    block,
                    b == 0,
                    Some(b) == drop_cap,
                    tokens.indent,
                    tokens.spacing,
                    tokens.justify,
                    scene_notes,
                );
            }
            if s + 1 < chapter.scenes.len() {
                w.render_separator(options.scene_separator, tokens.scene_break);
            }
        }
        let chapter_notes = notes.by_chapter.get(c).cloned().unwrap_or_default();
        w.render_chapter_endnotes(&chapter_notes);
    }

    for page in &book.back_matter {
        if w.body_started {
            w.new_body_page();
        } else {
            w.begin_body();
        }
        w.render_matter_page(page, tokens.justify);
    }

    if let Some(bibliography) = bibliography.filter(|b| !b.entries.is_empty()) {
        if w.body_started {
            w.new_body_page();
        } else {
            w.begin_body();
        }
        let opts = ParagraphOptions {
            size: body_size * 1.5,
            line_height: body_size * 1.8,
            bold: true,
            center: true,
            spacing_after: body_size * 0.8,
            ..ParagraphOptions::default()
        };
        w.render_paragraph(&Writer::plain_runs(&bibliography.heading), &opts, &none);
        let hanging = body_size * 1.8;
        for entry in &bibliography.entries {
            let opts = ParagraphOptions {
                size: body_size,
                line_height: w.body_line,
                first_indent: -hanging,
                left_indent: hanging,
                spacing_after: body_size * 0.5,
                ..ParagraphOptions::default()
            };
            w.render_paragraph(&inline_runs_from_html(&entry.html), &opts, &none);
        }
    }

    // `renderTableOfContents`, on the pages kept for it.
    let contents = Contents {
        heading_size: toc_heading_size,
        heading_baseline: toc_heading_baseline,
        first_baseline: toc_first_baseline,
        line_height: toc_line_height,
        entry_gap: toc_entry_gap,
        bottom: toc_bottom,
    };
    w.render_contents(&contents, &toc_pages, &toc_entries, &chapter_start_pages);

    Layout {
        width,
        height,
        pages: w.pages,
        title: book.title.clone(),
        author: book.author.clone(),
        subtitle: book.subtitle.clone(),
        unshown: w.unshown,
    }
}
