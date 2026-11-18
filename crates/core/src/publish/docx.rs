//! `exportDocx`: the book as a Word document (ProseCraft `src/lib/publish/docx.ts`, upstream
//! `2d1cf18`).
//!
//! ProseCraft builds the document with the `docx` npm package. Here it is built on the
//! craft-wordmodel document model and written by craft-docx. The paragraphs, runs, styles,
//! sections, notes, headers and footers are ProseCraft's; every measurement is computed in the
//! unit ProseCraft computes it in (twips, half-points, pixels, each rounded as JavaScript's
//! `Math.round` does) and only then turned into the model's points, so the file says the same
//! numbers.
//!
//! The model writes some things its own way: a drop cap is one paragraph whose first letter
//! craft-docx writes as the framed paragraph, and pictures are named by the writer rather than
//! after their alt text (the alt text itself is kept). The fixture test in
//! `crates/core/tests/docx_fixtures.rs` compares both files as craft-docx reads them back.

use std::collections::HashMap;
use std::sync::Arc;

use craft_wordmodel::para::NoteKind;
use craft_wordmodel::props::{LineSpacing, TabAlign, TabLeader, TabStop, Underline, VertAlign};
use craft_wordmodel::section::{NoteRestart, PageVAlign, SectionStart};
use craft_wordmodel::{
    Align, Block, CharProps, ContentControl, ControlKind, Document, InlineObject, ParaProps,
    Paragraph, PartKind, Rgb, SectionProps, Style, StyleKind, StyleSheet, TextColor,
};

use super::citations::resolve_book_citations;
use super::embedded_image::{EmbeddedImage, decode_embedded_image, natural_size};
use super::footnotes::{BookFootnotes, ResolvedFootnote, resolve_book_footnotes};
use super::html::{StyleTokens, matter_paragraphs, style_tokens};
use super::rich_text::{
    Block as TextBlock, BlockKind, InlineRun, blocks_from_html, inline_runs_from_html, trim_runs,
};
use super::{
    CompiledBook, CompiledChapter, Export, MatterPage, PublishOptions, SceneSeparator,
    slugify_filename,
};
use crate::citations::bibliography::BookBibliography;
use crate::html::sanitize::RICH;
use crate::html::{Node, is_js_whitespace, parse_fragment, serialize};

/// Why a DOCX couldn't be made.
#[derive(Debug, thiserror::Error)]
pub enum DocxError {
    #[error("an embedded image couldn't be read: {0}")]
    Image(String),
    #[error("the document couldn't be written: {0}")]
    Write(String),
}

/// JavaScript's `Math.round`: halves round up, towards positive infinity.
fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// `twip`: points as whole twentieths of a point.
fn twip(points: f64) -> f64 {
    js_round(points * 20.0)
}

/// `halfPoint`: points as whole half-points.
fn half_point(points: f64) -> f64 {
    js_round(points * 2.0)
}

/// The model's points for a length in twips.
fn tw(twips: f64) -> f32 {
    (twips / 20.0) as f32
}

/// The model's points for a size in half-points.
fn hp(half_points: f64) -> f32 {
    (half_points / 2.0) as f32
}

/// The model's points for a picture size in CSS pixels (96 to the inch).
fn px(pixels: f64) -> f32 {
    (pixels * 0.75) as f32
}

const GREY_666: Rgb = Rgb(0x66, 0x66, 0x66);
const GREY_777: Rgb = Rgb(0x77, 0x77, 0x77);

/// Run formatting as the `docx` package writes `IRunOptions`: bold and italics come with their
/// complex-script twins (`w:bCs`, `w:iCs`), a size with `w:szCs`, a font on every script.
#[derive(Clone, Copy, Default)]
struct RunFormat<'a> {
    bold: bool,
    italics: bool,
    all_caps: Option<bool>,
    /// Half-points.
    size: Option<f64>,
    color: Option<Rgb>,
    font: Option<&'a str>,
}

impl RunFormat<'_> {
    fn props(&self) -> CharProps {
        let mut p = CharProps::default();
        if self.bold {
            p.bold = Some(true);
            p.bold_cs = Some(true);
        }
        if self.italics {
            p.italic = Some(true);
            p.italic_cs = Some(true);
        }
        p.caps = self.all_caps;
        if let Some(size) = self.size {
            p.size = Some(hp(size));
            p.size_cs = Some(hp(size));
        }
        p.color = self.color.map(TextColor::Rgb);
        if let Some(font) = self.font {
            p.font = Some(font.to_owned());
            p.font_cs = Some(font.to_owned());
            p.font_ea = Some(font.to_owned());
        }
        p
    }
}

/// `runOptions`: a run's bold, italics, underline and strike.
fn run_options(run: &InlineRun) -> CharProps {
    let mut p = RunFormat {
        bold: run.bold,
        italics: run.italic,
        ..RunFormat::default()
    }
    .props();
    if run.underline {
        p.underline = Some(Underline::Single);
    }
    if run.strike {
        p.strike = Some(true);
    }
    p
}

/// Text as a run can hold it. A line ending in the text of a run is whitespace to Word, so it
/// becomes a space, as craft-docx reads it; the other control characters can't be written in
/// XML and are left out. The object marker (U+FFFC) is dropped by `Paragraph::push_text`.
fn run_text(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            '\n' | '\r' => Some(' '),
            '\t' => Some('\t'),
            c if (c as u32) < 0x20 => None,
            c => Some(c),
        })
        .collect()
}

fn push_text(p: &mut Paragraph, text: &str, props: &CharProps) {
    p.push_text(&run_text(text), props);
}

/// `new TextRun({ break: 1 })`: a line break, unformatted.
fn push_line_break(p: &mut Paragraph) {
    p.push_text("\n", &CharProps::default());
}

/// `new Paragraph({ children: [new PageBreak()] })`.
fn page_break_paragraph() -> Paragraph {
    let mut p = Paragraph::new();
    p.push_text("\u{c}", &CharProps::default());
    p
}

fn paragraph(props: ParaProps) -> Paragraph {
    Paragraph {
        props,
        ..Paragraph::new()
    }
}

fn styled(style: &str) -> Paragraph {
    paragraph(ParaProps {
        style: Some(style.to_owned()),
        ..ParaProps::default()
    })
}

/// `linesToTextRuns`: the text's lines, joined by line breaks.
fn lines_to_text_runs(p: &mut Paragraph, text: &str, props: &CharProps) {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            push_line_break(p);
        }
        push_text(p, line, props);
    }
}

/// The footnote reference mark: the `docx` package's `FootnoteReference` character style.
fn note_ref(part: u32) -> (InlineObject, CharProps) {
    (
        InlineObject::NoteRef {
            kind: NoteKind::Footnote,
            id: part,
            custom: String::new(),
        },
        CharProps {
            style: Some("FootnoteReference".into()),
            ..CharProps::default()
        },
    )
}

/// What the runs of one scene, note or bibliography refer to.
struct InlineContext<'a> {
    images: &'a HashMap<String, Picture>,
    body_size_pt: f64,
    /// The scene's footnotes by id, and each note's story by its book-wide index.
    scene_footnotes: Option<&'a HashMap<String, ResolvedFootnote>>,
    note_parts: &'a HashMap<usize, u32>,
}

/// `inlineToChildren`: styled text, links, line breaks, inline images and footnote references.
fn inline_to_children(
    doc: &mut Document,
    p: &mut Paragraph,
    runs: &[InlineRun],
    cx: &InlineContext<'_>,
) {
    for run in runs {
        if let Some(id) = &run.footnote_id {
            let part = cx
                .scene_footnotes
                .and_then(|notes| notes.get(id))
                .and_then(|note| cx.note_parts.get(&note.index));
            if let Some(&part) = part {
                let (object, props) = note_ref(part);
                p.push_object(object, &props);
            }
            continue;
        }
        if run.line_break {
            push_line_break(p);
            continue;
        }
        if let Some(image) = &run.image {
            let Some(picture) = cx.images.get(&image.src) else {
                let alt = if image.alt.is_empty() {
                    "[image]"
                } else {
                    &image.alt
                };
                push_text(p, alt, &CharProps::default());
                continue;
            };
            // The image stands as tall as a line of text (body size at 1.15), keeping the
            // aspect ratio of its stated or natural size.
            let target_height = cx.body_size_pt * 96.0 / 72.0 * 1.15;
            let aspect = image.width.unwrap_or(f64::from(picture.width))
                / image.height.unwrap_or(f64::from(picture.height));
            // A zero natural height would make ProseCraft write NaN; a square stands in.
            let aspect = if aspect.is_finite() && aspect > 0.0 {
                aspect
            } else {
                1.0
            };
            let height = js_round(target_height).max(1.0);
            let width = js_round(height * aspect).max(1.0);
            let alt = image.alt.clone();
            push_image(doc, p, &picture.asset, width, height, alt);
            continue;
        }
        let mut props = run_options(run);
        props.link.clone_from(&run.link);
        push_text(p, &run.text, &props);
    }
}

/// `new ImageRun(…)`, `width` × `height` pixels.
fn push_image(
    doc: &mut Document,
    p: &mut Paragraph,
    asset: &EmbeddedImage,
    width: f64,
    height: f64,
    alt: String,
) {
    let media = doc.add_media(asset.bytes.clone(), asset.extension);
    p.push_object(
        InlineObject::Image {
            media,
            w: px(width),
            h: px(height),
            alt,
            float: Default::default(),
            crop: [0.0; 4],
            fx: Default::default(),
            ole: None,
        },
        &CharProps::default(),
    );
}

/// An embedded picture and its natural size in pixels (`decodeEmbeddedImage`).
struct Picture {
    asset: EmbeddedImage,
    width: u32,
    height: u32,
}

fn decode_picture(src: &str) -> Result<Picture, DocxError> {
    let asset = decode_embedded_image(src).map_err(DocxError::Image)?;
    let (width, height) = natural_size(&asset).map_err(DocxError::Image)?;
    Ok(Picture {
        asset,
        width,
        height,
    })
}

/// The first letter of `runs` lifted out for a drop cap, and the runs left after it
/// (`blockToParagraph`'s drop cap). ProseCraft's `/^(\s*)(\S)/` takes one UTF-16 unit, half of
/// a letter outside the Basic Multilingual Plane; this takes the whole character.
fn lift_drop_cap(runs: &[InlineRun]) -> Option<(char, Vec<InlineRun>)> {
    let first = runs.iter().position(|run| {
        !run.line_break
            && run.image.is_none()
            && run.footnote_id.is_none()
            && run.text.chars().any(|c| !is_js_whitespace(c))
    })?;
    let run = &runs[first];
    let rest = run.text.trim_start_matches(is_js_whitespace);
    let letter = rest.chars().next()?;
    let rest = &rest[letter.len_utf8()..];
    let mut runs = runs.to_vec();
    if rest.is_empty() {
        runs.remove(first);
    } else {
        runs[first].text = rest.to_owned();
    }
    Some((letter, trim_runs(&runs)))
}

/// `blockToParagraph`.
fn block_to_paragraphs(
    doc: &mut Document,
    block: &TextBlock,
    first_in_scene: bool,
    drop_cap: bool,
    cx: &InlineContext<'_>,
) -> Vec<Paragraph> {
    let style = match block.kind {
        BlockKind::Image => return Vec::new(),
        BlockKind::Heading => match block.level {
            Some(1) => "SceneHeading1",
            Some(2) => "SceneHeading2",
            _ => "SceneHeading3",
        },
        BlockKind::Quote => "Quote",
        BlockKind::List => {
            let mut p = styled("ListParagraph");
            let marker = format!("{} ", block.marker.as_deref().unwrap_or_default());
            push_text(&mut p, &marker, &CharProps::default());
            inline_to_children(doc, &mut p, &block.runs, cx);
            return vec![p];
        }
        BlockKind::Paragraph if block.eyebrow => "Eyebrow",
        BlockKind::Paragraph => {
            let mut p = styled("BodyText");
            if first_in_scene || drop_cap {
                p.props.indent_first = Some(0.0);
            }
            // The drop cap: the first letter, bold, in a frame three lines deep. craft-docx
            // writes a `drop_cap` paragraph's first character as that framed paragraph.
            let lifted = drop_cap.then(|| lift_drop_cap(&block.runs)).flatten();
            match lifted {
                // A paragraph of one letter can't be split into a frame and the text beside
                // it, so it is written as it is.
                Some((_, rest)) if rest.is_empty() => {
                    inline_to_children(doc, &mut p, &block.runs, cx)
                }
                Some((letter, rest)) => {
                    p.props.drop_cap = Some(3);
                    let bold = RunFormat {
                        bold: true,
                        ..RunFormat::default()
                    };
                    p.push_text(&letter.to_string(), &bold.props());
                    inline_to_children(doc, &mut p, &rest, cx);
                }
                None => inline_to_children(doc, &mut p, &block.runs, cx),
            }
            return vec![p];
        }
    };
    let mut p = styled(style);
    inline_to_children(doc, &mut p, &block.runs, cx);
    vec![p]
}

/// `imageToParagraph`: a picture on a line of its own, centred, no larger than the text area
/// (and no taller than 384 pixels).
fn image_to_paragraph(
    doc: &mut Document,
    block: &TextBlock,
    max_width: f64,
    max_height: f64,
) -> Result<Paragraph, DocxError> {
    let image = block
        .image
        .as_ref()
        .ok_or_else(|| DocxError::Image("an image without source data".into()))?;
    let picture = decode_picture(&image.src)?;
    let width = image.width.unwrap_or(f64::from(picture.width));
    let height = image.height.unwrap_or(f64::from(picture.height));
    let scale = 1f64.min(max_width / width).min(max_height / height);
    let display_width = js_round(width * scale).max(1.0);
    let display_height = js_round(height * scale).max(1.0);
    let mut p = paragraph(ParaProps {
        align: Some(Align::Center),
        ..ParaProps::default()
    });
    push_image(
        doc,
        &mut p,
        &picture.asset,
        display_width,
        display_height,
        image.alt.clone(),
    );
    Ok(p)
}

/// `separatorParagraph`: the mark between scenes, or nothing.
fn separator_paragraph(separator: SceneSeparator) -> Option<Paragraph> {
    let symbol = match separator {
        SceneSeparator::None => return None,
        SceneSeparator::Hash => "#",
        SceneSeparator::Asterisks => "* * *",
        SceneSeparator::Line => "—",
        SceneSeparator::Blank => " ",
    };
    let mut p = styled("SceneBreak");
    let format = RunFormat {
        size: Some(half_point(10.0)),
        ..RunFormat::default()
    };
    push_text(&mut p, symbol, &format.props());
    Some(p)
}

/// The page size and margins every section but the cover has.
fn page_section(tokens: &StyleTokens) -> SectionProps {
    let [width, height, top, right, bottom, left] = tokens.page;
    SectionProps {
        page_w: tw(twip(width)),
        page_h: tw(twip(height)),
        margin_top: tw(twip(top)),
        margin_right: tw(twip(right)),
        margin_bottom: tw(twip(bottom)),
        margin_left: tw(twip(left)),
        header: tw(720.0),
        footer: tw(720.0),
        ..SectionProps::default()
    }
}

/// `coverSection`: the cover filling the page as far as its shape allows, with no margins.
fn cover_section(
    doc: &mut Document,
    data_url: &str,
    tokens: &StyleTokens,
) -> Result<(Vec<Paragraph>, SectionProps), DocxError> {
    let image = decode_picture(data_url)?;
    let [width, height, ..] = tokens.page;
    let page_width_px = width * 96.0 / 72.0;
    let page_height_px = height * 96.0 / 72.0;
    let scale =
        (page_width_px / f64::from(image.width)).min(page_height_px / f64::from(image.height));
    let display_width = js_round(f64::from(image.width) * scale).max(1.0);
    let display_height = js_round(f64::from(image.height) * scale).max(1.0);
    let mut p = paragraph(ParaProps {
        align: Some(Align::Center),
        space_before: Some(0.0),
        space_after: Some(0.0),
        ..ParaProps::default()
    });
    push_image(
        doc,
        &mut p,
        &image.asset,
        display_width,
        display_height,
        String::new(),
    );
    let section = SectionProps {
        page_w: tw(twip(width)),
        page_h: tw(twip(height)),
        margin_top: 0.0,
        margin_right: 0.0,
        margin_bottom: 0.0,
        margin_left: 0.0,
        header: 0.0,
        footer: 0.0,
        ..SectionProps::default()
    };
    Ok((vec![p], section))
}

/// A front- or back-matter page: its title, its paragraphs, or an empty paragraph for a blank
/// page.
fn matter_page(page: &MatterPage, out: &mut Vec<Paragraph>) {
    let paragraphs = matter_paragraphs(&page.body);
    if !page.title.is_empty() {
        let mut p = styled("MatterTitle");
        lines_to_text_runs(&mut p, &page.title, &CharProps::default());
        out.push(p);
    }
    for text in &paragraphs {
        let mut p = styled("MatterBody");
        lines_to_text_runs(&mut p, text, &CharProps::default());
        out.push(p);
    }
    if page.blank && page.title.is_empty() && paragraphs.is_empty() {
        out.push(Paragraph::new());
    }
}

/// ProseCraft's own fallback for an equation, diagram or calculation it can't draw
/// (`prepareRichHtml` and `showRichFallback` in `src/lib/rich/`): the placeholder shows its
/// source text. Drawing them isn't ported, so every placeholder takes the fallback.
fn show_rich_sources(html: &str) -> String {
    if !RICH.iter().any(|(class, ..)| html.contains(class)) {
        return html.to_owned();
    }
    fn walk(nodes: &mut [Node], depth: usize) {
        // The sanitizer bounds nesting; this keeps the walk bounded on its own.
        if depth > 256 {
            return;
        }
        for node in nodes {
            if let Node::Element(e) = node {
                // `richKindOf`: the first kind whose class it has, and that kind's source.
                let source = RICH
                    .iter()
                    .find(|(class, ..)| e.has_class(class))
                    .and_then(|(_, _, attr, _)| e.attr(attr))
                    .map(|s| s.trim_matches(is_js_whitespace).to_owned());
                match source {
                    Some(source) if !source.is_empty() => e.children = vec![Node::Text(source)],
                    _ => walk(&mut e.children, depth + 1),
                }
            }
        }
    }
    let mut nodes = parse_fragment(html);
    walk(&mut nodes, 0);
    serialize(&nodes)
}

/// Every image embedded in the runs, decoded once.
fn decode_run_images<'a>(
    runs: impl Iterator<Item = &'a InlineRun>,
    images: &mut HashMap<String, Picture>,
) -> Result<(), DocxError> {
    for run in runs {
        if let Some(image) = &run.image
            && !images.contains_key(&image.src)
        {
            images.insert(image.src.clone(), decode_picture(&image.src)?);
        }
    }
    Ok(())
}

/// A paragraph style as `docx.ts` defines its own: based on Normal or Body Text, in the gallery.
fn paragraph_style(id: &str, name: &str, based_on: &str, next: &str) -> Style {
    Style {
        id: id.into(),
        name: name.into(),
        kind: StyleKind::Paragraph,
        based_on: Some(based_on.into()),
        next: Some(next.into()),
        quick: true,
        builtin: true,
        ..Style::default()
    }
}

/// Spacing before and after, in twips.
fn spacing(before: Option<f64>, after: Option<f64>) -> ParaProps {
    ParaProps {
        space_before: before.map(tw),
        space_after: after.map(tw),
        ..ParaProps::default()
    }
}

/// The style sheet: the document defaults, `docx.ts`'s styles, and the `docx` package's own
/// styles that the text uses (Normal, the footnote reference mark, the contents entries).
fn style_sheet(tokens: &StyleTokens) -> StyleSheet {
    let font = Some(tokens.docx_font);
    let body = tokens.body_size;
    let body_size = half_point(body);
    let chapter_size = half_point(tokens.chapter_size);
    let line = LineSpacing::Multiple((js_round(tokens.line_height * 240.0) / 240.0) as f32);
    let paragraph_spacing = js_round(tokens.spacing * body * 20.0);
    let first_line_indent = js_round(tokens.indent * body * 20.0);
    let body_run = RunFormat {
        size: Some(body_size),
        font,
        ..RunFormat::default()
    };
    let with_body_lines = |after: f64| ParaProps {
        line_spacing: Some(line),
        ..spacing(None, Some(after))
    };

    let mut sheet = StyleSheet::empty();
    sheet.default_chr = body_run.props();
    sheet.default_para = with_body_lines(paragraph_spacing);
    sheet.default_para_present = true;

    let mut styles = vec![Style {
        id: "Normal".into(),
        name: "Normal".into(),
        kind: StyleKind::Paragraph,
        quick: true,
        builtin: true,
        default: true,
        ..Style::default()
    }];
    // The `docx` package's contents entry and footnote reference styles.
    styles.push(Style {
        priority: Some(39),
        hidden: true,
        quick: false,
        para: spacing(None, Some(100.0)),
        ..paragraph_style("TOC1", "toc 1", "Normal", "Normal")
    });
    styles.push(Style {
        id: "FootnoteReference".into(),
        name: "footnote reference".into(),
        kind: StyleKind::Character,
        based_on: Some("DefaultParagraphFont".into()),
        priority: Some(99),
        hidden: true,
        builtin: true,
        chr: CharProps {
            vert_align: Some(VertAlign::Superscript),
            ..CharProps::default()
        },
        ..Style::default()
    });

    let first_zero = |mut p: ParaProps| {
        p.indent_first = Some(0.0);
        p
    };
    let keep_next = |mut p: ParaProps| {
        p.keep_next = Some(true);
        p
    };

    styles.push(Style {
        chr: body_run.props(),
        para: ParaProps {
            align: Some(if tokens.justify {
                Align::Justify
            } else {
                Align::Left
            }),
            indent_first: Some(tw(first_line_indent)),
            ..with_body_lines(paragraph_spacing)
        },
        ..paragraph_style("BodyText", "Body Text", "Normal", "BodyText")
    });
    styles.push(Style {
        chr: RunFormat {
            size: Some(chapter_size),
            bold: true,
            all_caps: Some(tokens.chapter_uppercase),
            font,
            ..RunFormat::default()
        }
        .props(),
        para: keep_next(ParaProps {
            align: Some(if tokens.chapter_align == "center" {
                Align::Center
            } else {
                Align::Left
            }),
            outline_level: Some(0),
            ..spacing(
                Some(js_round(tokens.chapter_before * tokens.chapter_size * 20.0)),
                Some(js_round(tokens.chapter_after * tokens.chapter_size * 20.0)),
            )
        }),
        ..paragraph_style("ChapterTitle", "Chapter Title", "Normal", "BodyText")
    });
    styles.push(Style {
        chr: RunFormat {
            size: Some(half_point(body * 1.5)),
            bold: true,
            font,
            ..RunFormat::default()
        }
        .props(),
        para: keep_next(first_zero(ParaProps {
            align: Some(Align::Center),
            ..spacing(Some(1200.0), Some(360.0))
        })),
        ..paragraph_style("MatterTitle", "Matter Title", "Normal", "MatterBody")
    });
    styles.push(Style {
        chr: body_run.props(),
        para: first_zero(with_body_lines(paragraph_spacing)),
        ..paragraph_style("MatterBody", "Matter Body", "BodyText", "MatterBody")
    });
    styles.push(Style {
        chr: body_run.props(),
        para: ParaProps {
            align: Some(Align::Left),
            indent_left: Some(tw(720.0)),
            indent_first: Some(-tw(720.0)),
            ..with_body_lines(paragraph_spacing.max(120.0))
        },
        ..paragraph_style(
            "BibliographyEntry",
            "Bibliography Entry",
            "BodyText",
            "BibliographyEntry",
        )
    });
    styles.push(Style {
        chr: RunFormat {
            size: Some(half_point(body * 0.68)),
            bold: true,
            all_caps: Some(true),
            color: Some(GREY_777),
            font,
            ..RunFormat::default()
        }
        .props(),
        para: keep_next(first_zero(spacing(Some(120.0), Some(120.0)))),
        ..paragraph_style("Eyebrow", "Eyebrow", "Normal", "BodyText")
    });
    styles.push(Style {
        chr: RunFormat {
            bold: true,
            italics: true,
            ..body_run
        }
        .props(),
        para: first_zero(spacing(Some(240.0), Some(120.0))),
        ..paragraph_style("SceneTitle", "Scene Title", "BodyText", "BodyText")
    });
    for (level, scale, before, after) in [
        (1, 1.45, 240.0, 120.0),
        (2, 1.28, 220.0, 110.0),
        (3, 1.12, 200.0, 100.0),
    ] {
        styles.push(Style {
            chr: RunFormat {
                size: Some(half_point(body * scale)),
                bold: true,
                font,
                ..RunFormat::default()
            }
            .props(),
            para: keep_next(first_zero(spacing(Some(before), Some(after)))),
            ..paragraph_style(
                &format!("SceneHeading{level}"),
                &format!("Scene Heading {level}"),
                "BodyText",
                "BodyText",
            )
        });
    }
    styles.push(Style {
        chr: RunFormat {
            italics: true,
            color: Some(GREY_666),
            ..body_run
        }
        .props(),
        para: ParaProps {
            indent_left: Some(tw(720.0)),
            ..first_zero(spacing(Some(120.0), Some(120.0)))
        },
        ..paragraph_style("Quote", "Quote", "BodyText", "BodyText")
    });
    styles.push(Style {
        // `docx.ts` sets both a hanging indent and a zero first line, which the model can't
        // hold. The hanging one is kept: the standard ignores the first line when both are
        // given (ECMA-376 §17.3.1.12), as craft-docx reads ProseCraft's file.
        para: ParaProps {
            indent_left: Some(tw(720.0)),
            indent_first: Some(-tw(360.0)),
            ..spacing(None, Some(120.0))
        },
        ..paragraph_style("ListParagraph", "List Paragraph", "BodyText", "BodyText")
    });
    let scene_break = js_round(tokens.scene_break * body * 20.0);
    styles.push(Style {
        chr: RunFormat {
            size: Some(half_point(10.0)),
            color: Some(GREY_777),
            font,
            ..RunFormat::default()
        }
        .props(),
        para: first_zero(ParaProps {
            align: Some(Align::Center),
            ..spacing(Some(scene_break), Some(scene_break))
        }),
        ..paragraph_style("SceneBreak", "Scene Break", "BodyText", "BodyText")
    });
    sheet.styles = styles;
    sheet
}

/// The table of contents: one `TOC1` entry per chapter, inside a content control named
/// "Contents" and a `TOC` field Word refreshes on opening, as the `docx` package writes it.
fn table_of_contents(book: &CompiledBook) -> Vec<Paragraph> {
    let count = book.chapters.len();
    book.chapters
        .iter()
        .enumerate()
        .map(|(index, chapter)| {
            let mut p = paragraph(ParaProps {
                style: Some("TOC1".into()),
                tabs: Some(vec![
                    TabStop {
                        pos: tw(9026.0),
                        align: TabAlign::Clear,
                        leader: TabLeader::None,
                    },
                    TabStop {
                        pos: tw(9025.0),
                        align: TabAlign::Right,
                        leader: TabLeader::Dot,
                    },
                ]),
                ..ParaProps::default()
            });
            let plain = CharProps::default();
            if index == 0 {
                let control = ContentControl {
                    title: "Contents".into(),
                    kind: ControlKind::RichText,
                    block: true,
                    ..ContentControl::default()
                };
                p.push_object(
                    InlineObject::ControlStart {
                        control: Box::new(control),
                    },
                    &plain,
                );
                p.push_object(
                    InlineObject::FieldStart {
                        instr: r#"TOC \h \o "1-1""#.into(),
                        locked: false,
                        dirty: true,
                    },
                    &plain,
                );
            }
            push_text(&mut p, &chapter.label, &plain);
            p.push_text("\t", &plain);
            if index + 1 == count {
                p.push_object(InlineObject::FieldEnd, &plain);
                p.push_object(InlineObject::ControlEnd, &plain);
            }
            p
        })
        .collect()
}

/// `exportDocx`: the book as a `.docx` file.
pub fn export_docx(book: &CompiledBook, options: &PublishOptions) -> Result<Export, DocxError> {
    let doc = docx_document(book, options)?;
    craft_docx::set_application_name("prosecraft-rs");
    let bytes = craft_docx::write(&doc).map_err(|e| DocxError::Write(e.to_string()))?;
    Ok(Export {
        file_name: format!("{}.docx", slugify_filename(&book.title)),
        bytes,
    })
}

/// A run in grey, `size` half-points.
fn grey(size: f64) -> RunFormat<'static> {
    RunFormat {
        size: Some(size),
        color: Some(GREY_666),
        ..RunFormat::default()
    }
}

/// A centred paragraph with space before and after, in twips.
fn centred(before: Option<f64>, after: Option<f64>) -> ParaProps {
    ParaProps {
        align: Some(Align::Center),
        ..spacing(before, after)
    }
}

/// The title page (title, subtitle, author, copyright line), the copyright page and the
/// dedication, each after a page break.
fn title_matter(
    book: &CompiledBook,
    options: &PublishOptions,
    tokens: &StyleTokens,
) -> Vec<Paragraph> {
    let mut out: Vec<Paragraph> = Vec::new();
    if options.include_title_page {
        let mut p = paragraph(centred(Some(2400.0), Some(240.0)));
        let title = RunFormat {
            bold: true,
            size: Some(56.0),
            ..RunFormat::default()
        };
        push_text(&mut p, &book.title, &title.props());
        out.push(p);
        if !book.subtitle.is_empty() {
            let mut p = paragraph(centred(None, Some(480.0)));
            let subtitle = RunFormat {
                italics: true,
                ..grey(26.0)
            };
            push_text(&mut p, &book.subtitle, &subtitle.props());
            out.push(p);
        }
        if !book.author.is_empty() {
            let mut p = paragraph(centred(Some(720.0), None));
            let author = RunFormat {
                all_caps: Some(true),
                ..grey(24.0)
            };
            push_text(&mut p, &format!("by {}", book.author), &author.props());
            out.push(p);
        }
        if !book.copyright.is_empty() && !options.include_copyright_page {
            let mut p = paragraph(centred(Some(2400.0), None));
            lines_to_text_runs(&mut p, &book.copyright, &grey(18.0).props());
            out.push(p);
        }
    }
    if options.include_copyright_page && !book.copyright.is_empty() {
        if !out.is_empty() {
            out.push(page_break_paragraph());
        }
        let mut p = paragraph(centred(Some(3600.0), None));
        lines_to_text_runs(&mut p, &book.copyright, &grey(18.0).props());
        out.push(p);
    }
    if !book.dedication.is_empty() {
        if !out.is_empty() {
            out.push(page_break_paragraph());
        }
        let mut p = paragraph(centred(Some(3600.0), None));
        let dedication = RunFormat {
            italics: true,
            size: Some(half_point(tokens.body_size)),
            ..RunFormat::default()
        };
        lines_to_text_runs(&mut p, &book.dedication, &dedication.props());
        out.push(p);
    }
    out
}

/// The contents and the front-matter pages, a page break between each.
fn front_matter(book: &CompiledBook, options: &PublishOptions) -> Vec<Paragraph> {
    let mut out: Vec<Paragraph> = Vec::new();
    if options.include_table_of_contents && !book.chapters.is_empty() {
        out.extend(table_of_contents(book));
    }
    for page in &book.front_matter {
        if !out.is_empty() {
            out.push(page_break_paragraph());
        }
        matter_page(page, &mut out);
    }
    // A section's properties go on its last paragraph. The contents are a content control, not
    // a paragraph, so when they end the section the `docx` package adds an empty paragraph to
    // carry them. (This section is never the last: the body follows.)
    if !out.is_empty() && book.front_matter.is_empty() {
        out.push(Paragraph::new());
    }
    out
}

/// Every footnote as a story of its own, starting with its number mark; the stories by the
/// notes' book-wide index.
fn footnote_parts(
    doc: &mut Document,
    footnotes: &BookFootnotes,
    tokens: &StyleTokens,
) -> Result<HashMap<usize, u32>, DocxError> {
    let mut note_parts: HashMap<usize, u32> = HashMap::new();
    for note in &footnotes.all {
        let runs = inline_runs_from_html(&note.html);
        let mut images = HashMap::new();
        decode_run_images(runs.iter(), &mut images)?;
        let part = doc
            .try_add_part(PartKind::Footnote, Vec::new())
            .map_err(|e| DocxError::Write(e.to_string()))?;
        let mut p = Paragraph::new();
        let (mark, props) = note_ref(part);
        p.push_object(mark, &props);
        let cx = InlineContext {
            images: &images,
            body_size_pt: tokens.body_size,
            scene_footnotes: None,
            note_parts: &note_parts,
        };
        inline_to_children(doc, &mut p, &runs, &cx);
        if let Some(story) = doc.parts.get_mut(&part) {
            story.blocks = vec![Arc::new(Block::Para(p))];
        }
        note_parts.insert(note.index, part);
    }
    Ok(note_parts)
}

/// What the chapters are written with.
struct BodyContext<'a> {
    options: &'a PublishOptions,
    tokens: &'a StyleTokens,
    footnotes: &'a BookFootnotes,
    note_parts: &'a HashMap<usize, u32>,
    /// The largest picture, in pixels: the text area's width, and its height up to 384.
    image_max: (f64, f64),
}

/// One scene's paragraphs: its title, its blocks with the drop cap, and the separator after it.
fn scene_paragraphs(
    doc: &mut Document,
    bx: &BodyContext<'_>,
    chapter: &CompiledChapter,
    (chapter_index, scene_index): (usize, usize),
    target: &mut Vec<Paragraph>,
) -> Result<(), DocxError> {
    let (options, tokens) = (bx.options, bx.tokens);
    let Some(scene) = chapter.scenes.get(scene_index) else {
        return Ok(());
    };
    if options.include_scene_titles {
        let mut p = styled("SceneTitle");
        push_text(&mut p, &scene.title, &CharProps::default());
        target.push(p);
    }
    let blocks = blocks_from_html(&show_rich_sources(&scene.html));
    let mut images = HashMap::new();
    decode_run_images(blocks.iter().flat_map(|b| b.runs.iter()), &mut images)?;
    let drop_cap_index = (options.drop_caps && tokens.drop_cap && scene_index == 0)
        .then(|| {
            blocks
                .iter()
                .position(|b| b.kind == BlockKind::Paragraph && !b.eyebrow)
        })
        .flatten();
    let cx = InlineContext {
        images: &images,
        body_size_pt: tokens.body_size,
        scene_footnotes: bx.footnotes.by_scene.get(&(chapter_index, scene_index)),
        note_parts: bx.note_parts,
    };
    for (block_index, block) in blocks.iter().enumerate() {
        if block.kind == BlockKind::Image {
            let (width, height) = bx.image_max;
            target.push(image_to_paragraph(doc, block, width, height)?);
            continue;
        }
        target.extend(block_to_paragraphs(
            doc,
            block,
            block_index == 0,
            Some(block_index) == drop_cap_index,
            &cx,
        ));
    }
    if scene_index + 1 < chapter.scenes.len()
        && let Some(separator) = separator_paragraph(options.scene_separator)
    {
        target.push(separator);
    }
    Ok(())
}

/// One chapter's paragraphs: its title (or the page break that stands for it) and its scenes.
fn chapter_paragraphs(
    doc: &mut Document,
    bx: &BodyContext<'_>,
    chapter: &CompiledChapter,
    chapter_index: usize,
    page_break_before: bool,
    target: &mut Vec<Paragraph>,
) -> Result<(), DocxError> {
    if bx.options.include_chapter_titles {
        let mut p = styled("ChapterTitle");
        if page_break_before {
            p.props.page_break_before = Some(true);
        }
        push_text(&mut p, &chapter.label, &CharProps::default());
        target.push(p);
    } else if page_break_before {
        target.push(paragraph(ParaProps {
            page_break_before: Some(true),
            ..spacing(Some(0.0), Some(0.0))
        }));
    }
    for scene_index in 0..chapter.scenes.len() {
        scene_paragraphs(doc, bx, chapter, (chapter_index, scene_index), target)?;
    }
    Ok(())
}

/// The back-matter pages and the bibliography, each after a page break.
fn back_matter(
    doc: &mut Document,
    book: &CompiledBook,
    bibliography: Option<&BookBibliography>,
    bx: &BodyContext<'_>,
    target: &mut Vec<Paragraph>,
) {
    for page in &book.back_matter {
        target.push(page_break_paragraph());
        matter_page(page, target);
    }
    let Some(bibliography) = bibliography.filter(|b| !b.entries.is_empty()) else {
        return;
    };
    if !target.is_empty() {
        target.push(page_break_paragraph());
    }
    let mut p = styled("MatterTitle");
    lines_to_text_runs(&mut p, &bibliography.heading, &CharProps::default());
    target.push(p);
    // No pictures here: an image in an entry is written as its alt text.
    let no_images = HashMap::new();
    let cx = InlineContext {
        images: &no_images,
        body_size_pt: bx.tokens.body_size,
        scene_footnotes: None,
        note_parts: bx.note_parts,
    };
    for entry in &bibliography.entries {
        let mut p = styled("BibliographyEntry");
        inline_to_children(doc, &mut p, &inline_runs_from_html(&entry.html), &cx);
        target.push(p);
    }
}

/// A body section with its running header and page-number footer, each a story of its own as
/// in ProseCraft's file (one header and one footer part per section).
fn body_section(
    doc: &mut Document,
    book: &CompiledBook,
    tokens: &StyleTokens,
    mut section: SectionProps,
) -> SectionProps {
    if tokens.running_header {
        let mut p = paragraph(centred(None, None));
        let format = RunFormat {
            size: Some(18.0),
            color: Some(GREY_777),
            all_caps: Some(true),
            ..RunFormat::default()
        };
        let text = if book.author.is_empty() {
            book.title.clone()
        } else {
            format!("{} / {}", book.author, book.title)
        };
        push_text(&mut p, &text, &format.props());
        section.headers.default =
            Some(doc.add_part(PartKind::Header, vec![Arc::new(Block::Para(p))]));
    }
    if tokens.page_numbers {
        let mut p = paragraph(centred(None, None));
        let format = RunFormat {
            size: Some(18.0),
            color: Some(GREY_777),
            ..RunFormat::default()
        };
        p.push_object(
            InlineObject::Field {
                instr: "PAGE".into(),
                result: String::new(),
                locked: false,
                dirty: false,
                code: false,
                form: String::new(),
            },
            &format.props(),
        );
        section.footers.default =
            Some(doc.add_part(PartKind::Footer, vec![Arc::new(Block::Para(p))]));
    }
    section
}

/// The sections into the body: each ends with the paragraph that carries its properties (an
/// empty one when it has none), the last with the body's own.
fn assemble(doc: &mut Document, sections: Vec<(Vec<Paragraph>, SectionProps)>) {
    let last = sections.len().saturating_sub(1);
    for (index, (mut paragraphs, section)) in sections.into_iter().enumerate() {
        if index == last {
            doc.last_section = section;
        } else {
            match paragraphs.last_mut() {
                Some(p) => p.section = Some(Box::new(section)),
                None => {
                    let mut p = Paragraph::new();
                    p.section = Some(Box::new(section));
                    paragraphs.push(p);
                }
            }
        }
        doc.body
            .extend(paragraphs.into_iter().map(|p| Arc::new(Block::Para(p))));
    }
}

/// The document `exportDocx` builds, on the word-processing model.
fn docx_document(book: &CompiledBook, options: &PublishOptions) -> Result<Document, DocxError> {
    let (book, bibliography) = resolve_book_citations(book, options);
    let tokens = style_tokens(&options.style);
    let mut doc = Document::new();
    doc.body.clear();
    doc.styles = Arc::new(style_sheet(tokens));
    doc.core.title.clone_from(&book.title);
    doc.core.creator.clone_from(&book.author);
    doc.core.description.clone_from(&book.subtitle);

    let title_matter = title_matter(&book, options, tokens);
    let front_matter = front_matter(&book, options);
    // The notes first, so the text can refer to their stories.
    let footnotes = resolve_book_footnotes(&book, options.footnotes_restart_per_chapter);
    let note_parts = footnote_parts(&mut doc, &footnotes, tokens)?;

    let [width, height, top, right, bottom, left] = tokens.page;
    let bx = BodyContext {
        options,
        tokens,
        footnotes: &footnotes,
        note_parts: &note_parts,
        image_max: (
            (width - left - right) * 96.0 / 72.0,
            384f64.min((height - top - bottom) * 96.0 / 72.0),
        ),
    };
    // With notes numbered per chapter, each chapter is a section that restarts them.
    let split_chapter_sections = options.footnotes_restart_per_chapter
        && !book.chapters.is_empty()
        && !footnotes.all.is_empty();
    let mut chapter_bodies: Vec<Vec<Paragraph>> = Vec::new();
    let mut body: Vec<Paragraph> = Vec::new();
    for (chapter_index, chapter) in book.chapters.iter().enumerate() {
        if split_chapter_sections {
            let mut own = Vec::new();
            chapter_paragraphs(&mut doc, &bx, chapter, chapter_index, false, &mut own)?;
            chapter_bodies.push(own);
        } else {
            let page_break_before = chapter_index > 0 && options.page_break_before_chapters;
            chapter_paragraphs(
                &mut doc,
                &bx,
                chapter,
                chapter_index,
                page_break_before,
                &mut body,
            )?;
        }
    }
    let back_matter_target = match chapter_bodies.last_mut() {
        Some(last) if split_chapter_sections => last,
        _ => &mut body,
    };
    back_matter(
        &mut doc,
        &book,
        bibliography.as_ref(),
        &bx,
        back_matter_target,
    );

    let page = page_section(tokens);
    let mut sections: Vec<(Vec<Paragraph>, SectionProps)> = Vec::new();
    if let Some(cover) = &book.cover {
        sections.push(cover_section(&mut doc, cover, tokens)?);
    }
    if !title_matter.is_empty() {
        let section = SectionProps {
            valign: PageVAlign::Center,
            ..page.clone()
        };
        sections.push((title_matter, section));
    }
    if !front_matter.is_empty() {
        sections.push((front_matter, page.clone()));
    }
    if split_chapter_sections {
        for (chapter_index, chapter_body) in chapter_bodies.into_iter().enumerate() {
            let mut section = page.clone();
            if chapter_index > 0 && !options.page_break_before_chapters {
                section.start = SectionStart::Continuous;
            }
            section.footnote_pr.num_restart = Some(NoteRestart::EachSect);
            let section = body_section(&mut doc, &book, tokens, section);
            sections.push((chapter_body, section));
        }
    } else {
        let section = body_section(&mut doc, &book, tokens, page.clone());
        sections.push((body, section));
    }
    assemble(&mut doc, sections);
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_as_javascript_does() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(0.49999999999999994), 0.0);
        assert_eq!(half_point(11.5 * 0.68), 16.0);
        assert_eq!(twip(595.28), 11906.0);
    }

    #[test]
    fn rich_placeholders_show_their_source() {
        let html = r#"<p>A <span class="math-inline" data-tex="x^2"></span> b</p><div class="mermaid-diagram" data-source="graph TD"></div>"#;
        let blocks = blocks_from_html(&show_rich_sources(html));
        let text: Vec<String> = blocks
            .iter()
            .map(|b| b.runs.iter().map(|r| r.text.as_str()).collect())
            .collect();
        assert_eq!(text, ["A x^2 b", "graph TD"]);
    }

    #[test]
    fn a_drop_cap_takes_a_whole_letter() {
        let run = |text: &str| InlineRun {
            text: text.into(),
            ..InlineRun::default()
        };
        let (letter, rest) = lift_drop_cap(&[run("  𝔐ara "), run("went")]).expect("letter");
        assert_eq!(letter, '𝔐');
        assert_eq!(rest[0].text, "ara ");
        let (letter, rest) = lift_drop_cap(&[run("A"), run(" b")]).expect("letter");
        assert_eq!((letter, rest[0].text.as_str()), ('A', "b"));
        assert!(lift_drop_cap(&[run("   ")]).is_none());
    }

    use crate::publish::{CompiledFootnote, CompiledScene};
    use base64::Engine;
    use craft_wordmodel::para::OBJ;

    fn scene(html: &str, footnotes: &[(&str, &str)]) -> CompiledScene {
        CompiledScene {
            id: "s".into(),
            title: "S".into(),
            html: html.into(),
            words: 0,
            footnotes: footnotes
                .iter()
                .enumerate()
                .map(|(i, (id, html))| CompiledFootnote {
                    id: (*id).into(),
                    html: (*html).into(),
                    number: i + 1,
                })
                .collect(),
        }
    }

    fn book(chapters: Vec<Vec<CompiledScene>>) -> CompiledBook {
        CompiledBook {
            title: "T".into(),
            subtitle: String::new(),
            author: String::new(),
            copyright: String::new(),
            dedication: String::new(),
            front_matter: Vec::new(),
            back_matter: Vec::new(),
            chapters: chapters
                .into_iter()
                .enumerate()
                .map(|(i, scenes)| CompiledChapter {
                    id: format!("c{i}"),
                    label: format!("Chapter {}", i + 1),
                    title: String::new(),
                    scenes,
                    words: 0,
                })
                .collect(),
            words: 0,
            citations: Vec::new(),
            bibliography: None,
            cover: None,
        }
    }

    /// Our file, as craft-docx reads it back.
    fn round_trip(book: &CompiledBook, options: &PublishOptions) -> Document {
        let export = export_docx(book, options).expect("export");
        craft_docx::read(&export.bytes).expect("read")
    }

    fn texts(doc: &Document) -> Vec<String> {
        doc.body
            .iter()
            .filter_map(|b| b.as_para().map(|p| p.text.clone()))
            .collect()
    }

    #[test]
    fn a_one_letter_paragraph_keeps_its_letter_without_a_drop_cap() {
        let options = PublishOptions {
            style: "classic".into(),
            include_title_page: false,
            ..PublishOptions::default()
        };
        let doc = round_trip(
            &book(vec![vec![scene("<p>A</p><p>More.</p>", &[])]]),
            &options,
        );
        let first = doc
            .body
            .iter()
            .filter_map(|b| b.as_para())
            .nth(1)
            .expect("paragraph");
        assert_eq!(first.text, "A");
        assert_eq!(first.props.drop_cap, None);
        assert_eq!(first.props.indent_first, Some(0.0));
    }

    #[test]
    fn webp_pictures_become_png() {
        let image = craft_image_codecs::Image::from_u8(
            3,
            2,
            craft_image_codecs::ChannelLayout::Rgba,
            vec![200; 24],
        )
        .expect("image");
        let webp = craft_image_codecs::encode(
            &image,
            craft_image_codecs::Format::WebP,
            &craft_image_codecs::EncodeOptions::default(),
        )
        .expect("webp");
        let html = format!(
            r#"<p><img src="data:image/webp;base64,{}" alt="w"></p>"#,
            base64::engine::general_purpose::STANDARD.encode(webp)
        );
        let doc = round_trip(
            &book(vec![vec![scene(&html, &[])]]),
            &PublishOptions::default(),
        );
        let media: Vec<&Vec<u8>> = doc.media.values().map(|b| &**b).collect();
        assert_eq!(media.len(), 1);
        assert!(media[0].starts_with(b"\x89PNG"));
        let size = doc
            .body
            .iter()
            .filter_map(|b| b.as_para())
            .flat_map(|p| p.objects.iter())
            .find_map(|o| match o {
                InlineObject::Image { w, h, alt, .. } => Some((*w, *h, alt.clone())),
                _ => None,
            });
        // 3 × 2 pixels, in points.
        assert_eq!(size, Some((2.25, 1.5, "w".into())));
    }

    #[test]
    fn an_unreadable_picture_is_an_error_not_a_crash() {
        let html = r#"<p><img src="data:image/png;base64,iVBORw0KGgo=" alt="broken"></p>"#;
        let result = export_docx(
            &book(vec![vec![scene(html, &[])]]),
            &PublishOptions::default(),
        );
        assert!(matches!(result, Err(DocxError::Image(_))));
    }

    #[test]
    fn a_chapter_with_nothing_to_show_still_gets_its_section() {
        // Notes restart each chapter, so each chapter is a section; the second shows nothing.
        let options = PublishOptions {
            footnotes_restart_per_chapter: true,
            include_chapter_titles: false,
            include_title_page: false,
            ..PublishOptions::default()
        };
        let note = r#"<p>Text<sup class="footnote-ref" data-footnote-id="n1"></sup>.</p>"#;
        let empty = r#"<p><span class="math-inline" data-tex=""></span></p>"#;
        let book = book(vec![
            vec![scene(note, &[("n1", "Note.")])],
            vec![scene(empty, &[])],
        ]);
        let doc = round_trip(&book, &options);
        assert_eq!(doc.sections().len(), 2);
        assert_eq!(
            texts(&doc).first().map(String::as_str),
            Some("Text\u{fffc}.")
        );
    }

    #[test]
    fn hostile_text_stays_in_step_with_its_objects() {
        let mut book = book(vec![vec![scene(
            "<p>a\u{fffc}b\u{1}c<sup class=\"footnote-ref\" data-footnote-id=\"n\"></sup></p>",
            &[("n", "x\u{fffc}y")],
        )]]);
        book.title = "Line\none\u{c}\u{fffc}".into();
        let doc = docx_document(&book, &PublishOptions::default()).expect("document");
        for block in doc
            .body
            .iter()
            .chain(doc.parts.values().flat_map(|p| p.blocks.iter()))
        {
            if let Some(p) = block.as_para() {
                assert_eq!(p.text.matches(OBJ).count(), p.objects.len(), "{:?}", p.text);
                assert_eq!(p.runs.iter().map(|r| r.len).sum::<usize>(), p.text.len());
            }
        }
        let back = round_trip(&book, &PublishOptions::default());
        assert_eq!(texts(&back).first().map(String::as_str), Some("Line one"));
    }

    #[test]
    fn unprepared_equations_show_their_source() {
        let html = r#"<p>Area <span class="math-inline" data-tex="\pi r^2"></span>.</p>"#;
        let doc = round_trip(
            &book(vec![vec![scene(html, &[])]]),
            &PublishOptions::default(),
        );
        assert!(
            texts(&doc).iter().any(|t| t == "Area \\pi r^2."),
            "{:?}",
            texts(&doc)
        );
    }

    #[test]
    fn a_book_with_nothing_in_it_is_one_empty_section() {
        let options = PublishOptions {
            include_title_page: false,
            ..PublishOptions::default()
        };
        let doc = round_trip(&book(Vec::new()), &options);
        assert_eq!(doc.sections().len(), 1);
        assert!(texts(&doc).iter().all(String::is_empty));
    }

    #[test]
    fn a_note_referred_to_twice_is_one_note() {
        let html = r#"<p>a<sup class="footnote-ref" data-footnote-id="n"></sup> b<sup class="footnote-ref" data-footnote-id="n"></sup></p>"#;
        let doc = round_trip(
            &book(vec![vec![scene(html, &[("n", "Once.")])]]),
            &PublishOptions::default(),
        );
        let notes: Vec<u32> = doc
            .body
            .iter()
            .filter_map(|b| b.as_para())
            .flat_map(|p| p.objects.iter())
            .filter_map(|o| match o {
                InlineObject::NoteRef { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0], notes[1], "both references are to the same note");
        let stories = doc
            .parts
            .values()
            .filter(|p| p.kind == PartKind::Footnote)
            .count();
        assert_eq!(stories, 1);
    }
}
