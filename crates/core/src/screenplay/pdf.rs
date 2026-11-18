//! Screenplay PDF (ProseCraft `src/lib/screenplay/pdf.ts`, upstream `2d1cf18`): a title page,
//! then 55 lines of 12 pt Courier per US Letter page in standard screenplay columns, cues and
//! parentheticals kept with what follows them.

use crate::html::is_js_whitespace;
use crate::publish::pdf::{Layout, Op, Page, Rgb};
use crate::publish::pdf_text::{Face, Family, win_ansi};

use super::fountain::ScreenplayElement;
use super::layout::wrap_screenplay_text;

const INCH: f64 = 72.0;
const PAGE_WIDTH: f64 = 8.5 * INCH;
const PAGE_HEIGHT: f64 = 11.0 * INCH;
const LEFT_MARGIN: f64 = 1.5 * INCH;
const RIGHT_MARGIN: f64 = INCH;
const TOP_MARGIN: f64 = INCH;
const BOTTOM_MARGIN: f64 = INCH;
const FONT_SIZE: f64 = 12.0;
const LINES_PER_PAGE: usize = 55;
const CHARACTER_CUE: f64 = 3.7 * INCH;
const DIALOGUE: f64 = 2.5 * INCH;
const DIALOGUE_WIDTH: f64 = 3.3 * INCH;
const PARENTHETICAL: f64 = 3.1 * INCH;
const PARENTHETICAL_WIDTH: f64 = 2.5 * INCH;

const LINE_HEIGHT: f64 = (PAGE_HEIGHT - TOP_MARGIN - BOTTOM_MARGIN) / LINES_PER_PAGE as f64;
/// Courier is 0.6 em wide, so a 12 pt character is 7.2 pt wide.
const CHAR_WIDTH: f64 = FONT_SIZE * 0.6;
const TEXT_RIGHT: f64 = PAGE_WIDTH - RIGHT_MARGIN;
const TEXT_WIDTH: f64 = TEXT_RIGHT - LEFT_MARGIN;
const TEXT_CENTER: f64 = LEFT_MARGIN + TEXT_WIDTH / 2.0;
const TITLE_WIDTH: f64 = 4.0 * INCH;
/// jsPDF's default text colour.
const BLACK: Rgb = [0, 0, 0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// A printed line (`ScriptLine`).
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptLine {
    pub text: String,
    pub x: f64,
    pub align: Align,
    pub bold: bool,
    /// Stays on the same page as the line after it, so cues keep their dialogue.
    pub keep_with_next: bool,
    pub page_break: bool,
}

fn line(text: String, x: f64, align: Align, bold: bool, keep_with_next: bool) -> ScriptLine {
    ScriptLine {
        text,
        x,
        align,
        bold,
        keep_with_next,
        page_break: false,
    }
}

fn blank() -> ScriptLine {
    line(String::new(), 0.0, Align::Left, false, false)
}

fn chars_for(width: f64) -> usize {
    ((width / CHAR_WIDTH).floor() as usize).max(1)
}

/// `wrapLines`: whitespace runs as one space, trimmed, wrapped to the column's characters.
fn wrap_lines(
    text: &str,
    width: f64,
    x: f64,
    align: Align,
    bold: bool,
    keep: bool,
) -> Vec<ScriptLine> {
    let mut words = String::new();
    let mut space = false;
    for c in text.chars() {
        if is_js_whitespace(c) {
            space = true;
        } else {
            if space && !words.is_empty() {
                words.push(' ');
            }
            space = false;
            words.push(c);
        }
    }
    if words.is_empty() {
        return Vec::new();
    }
    wrap_screenplay_text(&words, chars_for(width))
        .into_iter()
        .map(|l| line(l, x, align, bold, keep))
        .collect()
}

fn element_lines(element: &ScreenplayElement) -> Vec<ScriptLine> {
    let upper = || element.text.to_uppercase();
    match element.kind.as_str() {
        "scene-heading" => wrap_lines(&upper(), TEXT_WIDTH, LEFT_MARGIN, Align::Left, true, true),
        "character" => wrap_lines(
            &upper(),
            TEXT_RIGHT - CHARACTER_CUE,
            CHARACTER_CUE,
            Align::Left,
            false,
            true,
        ),
        "dialogue" => wrap_lines(
            &element.text,
            DIALOGUE_WIDTH,
            DIALOGUE,
            Align::Left,
            false,
            false,
        ),
        "parenthetical" => wrap_lines(
            &element.text,
            PARENTHETICAL_WIDTH,
            PARENTHETICAL,
            Align::Left,
            false,
            true,
        ),
        "transition" => wrap_lines(&upper(), TEXT_WIDTH, TEXT_RIGHT, Align::Right, false, false),
        "centered" => wrap_lines(
            &element.text,
            TEXT_WIDTH,
            TEXT_CENTER,
            Align::Center,
            false,
            false,
        ),
        "action" => wrap_lines(
            &element.text,
            TEXT_WIDTH,
            LEFT_MARGIN,
            Align::Left,
            false,
            false,
        ),
        _ => Vec::new(),
    }
}

/// `screenplayLines`: the printed lines, a blank line between elements except within a
/// cue's dialogue; sections, synopses, notes and page breaks aren't printed.
pub fn screenplay_lines(elements: &[ScreenplayElement]) -> Vec<ScriptLine> {
    let mut lines: Vec<ScriptLine> = Vec::new();
    let mut previous: Option<&str> = None;
    for element in elements {
        if element.kind == "page-break" {
            lines.push(ScriptLine {
                page_break: true,
                ..blank()
            });
            previous = None;
            continue;
        }
        let block = element_lines(element);
        if block.is_empty() {
            continue;
        }
        let glued = matches!(element.kind.as_str(), "dialogue" | "parenthetical")
            && matches!(previous, Some("character" | "dialogue" | "parenthetical"));
        let at_top = lines.last().is_none_or(|l| l.page_break);
        if !glued && !at_top {
            lines.push(blank());
        }
        lines.extend(block);
        previous = Some(element.kind.as_str());
    }
    lines
}

/// `paginateScreenplay`: pages of at most 55 lines, keeping cues with their dialogue.
pub fn paginate_screenplay(lines: &[ScriptLine]) -> Vec<Vec<ScriptLine>> {
    let mut pages: Vec<Vec<ScriptLine>> = Vec::new();
    let mut page: Vec<ScriptLine> = Vec::new();
    let flush = |page: &mut Vec<ScriptLine>, pages: &mut Vec<Vec<ScriptLine>>| {
        while page.last().is_some_and(|l| l.text.is_empty()) {
            page.pop();
        }
        if !page.is_empty() {
            pages.push(std::mem::take(page));
        }
    };
    for (index, l) in lines.iter().enumerate() {
        if l.page_break {
            flush(&mut page, &mut pages);
            continue;
        }
        if l.text.is_empty() && page.is_empty() {
            continue;
        }
        if page.len() == LINES_PER_PAGE {
            flush(&mut page, &mut pages);
        }
        if l.keep_with_next && !page.is_empty() {
            let mut run = 1;
            while index + run < lines.len() && lines[index + run].keep_with_next {
                run += 1;
            }
            if page.len() + run + 1 > LINES_PER_PAGE {
                flush(&mut page, &mut pages);
            }
        }
        page.push(l.clone());
    }
    flush(&mut page, &mut pages);
    pages
}

struct Pages {
    pages: Vec<Page>,
    unshown: usize,
}

impl Pages {
    /// `doc.text(text, x, y, { align })` in Courier at 12 pt.
    fn text(&mut self, text: &str, x: f64, y: f64, bold: bool, align: Align) {
        let face = Face::new(Family::Courier, bold, false);
        let width = face.aligned_width(text, FONT_SIZE);
        let x = match align {
            Align::Left => x,
            Align::Center => x - width / 2.0,
            Align::Right => x - width,
        };
        let shown: String = text
            .chars()
            .map(|c| {
                if win_ansi(c).is_some() {
                    c
                } else {
                    self.unshown += 1;
                    '?'
                }
            })
            .collect();
        if let Some(page) = self.pages.last_mut() {
            page.ops.push(Op::Text {
                x,
                y,
                face,
                size: FONT_SIZE,
                color: BLACK,
                text: shown,
            });
        }
    }
}

/// `buildScreenplayPdf` up to the drawing: the title page, then the script's pages.
pub fn layout_screenplay_pdf(title: &str, author: &str, elements: &[ScreenplayElement]) -> Layout {
    let mut out = Pages {
        pages: vec![Page::default()],
        unshown: 0,
    };
    let trim = |s: &str| s.trim_matches(is_js_whitespace).to_owned();
    let heading = trim(title).to_uppercase();
    if !heading.is_empty() {
        let bold = Face::new(Family::Courier, true, false);
        let mut y = PAGE_HEIGHT * 0.3;
        for l in bold.split_text_to_size(&heading, TITLE_WIDTH, FONT_SIZE) {
            out.text(&l, TEXT_CENTER, y, true, Align::Center);
            y += LINE_HEIGHT;
        }
    }
    let credit = trim(author);
    if !credit.is_empty() {
        let y = PAGE_HEIGHT * 0.6;
        out.text("Written by", TEXT_CENTER, y, false, Align::Center);
        out.text(&credit, TEXT_CENTER, y + LINE_HEIGHT, false, Align::Center);
    }
    for lines in paginate_screenplay(&screenplay_lines(elements)) {
        out.pages.push(Page::default());
        // `doc.getNumberOfPages()`: the title page counts.
        let number = out.pages.len();
        out.text(
            &format!("{number}."),
            TEXT_RIGHT,
            TOP_MARGIN - LINE_HEIGHT,
            false,
            Align::Right,
        );
        for (index, l) in lines.iter().enumerate() {
            if l.text.is_empty() {
                continue;
            }
            let y = TOP_MARGIN + (index + 1) as f64 * LINE_HEIGHT;
            out.text(&l.text, l.x, y, l.bold, l.align);
        }
    }
    Layout {
        width: PAGE_WIDTH,
        height: PAGE_HEIGHT,
        pages: out.pages,
        title: title.to_owned(),
        author: author.to_owned(),
        subtitle: String::new(),
        unshown: out.unshown,
        sections: Vec::new(),
    }
}

/// The book's screenplay as a Screenplay PDF, and how many characters came out as "?".
pub fn export_screenplay_pdf(
    book: &crate::publish::CompiledBook,
) -> Result<(crate::publish::Export, usize), crate::publish::pdf_writer::PdfError> {
    let layout = layout_screenplay_pdf(
        &book.title,
        &book.author,
        &super::export::book_to_elements(book),
    );
    let bytes = crate::publish::pdf_writer::write_pdf(&layout)?;
    Ok((
        crate::publish::Export {
            file_name: format!("{}.pdf", crate::publish::slugify_filename(&book.title)),
            bytes,
        },
        layout.unshown,
    ))
}
