//! `exportHtml`: the book as one styled, self-contained web page (ProseCraft
//! `src/lib/publish/html.ts`, `sections.ts`, `content.ts` `markDropCapParagraph`,
//! `matter.ts` `matterParagraphs`, `xhtml.ts` escaping; upstream `7dae544`).

use std::collections::{HashMap, HashSet};

use super::citations::resolve_book_citations;
use super::footnotes::{BookFootnotes, ResolvedFootnote, resolve_book_footnotes};
use super::html_css::{CSS, DROP_CAPS};
use super::{
    CompiledBook, CompiledChapter, Export, MatterPage, PublishOptions, SceneSeparator,
    slugify_filename,
};
use crate::html::sanitize::{FOOTNOTE_REF_CLASS, RICH};
use crate::html::{Element, Node, is_js_whitespace, parse_fragment, serialize};
use crate::js::number_to_string;

/// The values of a built-in style the stylesheet uses (`PRESETS`).
pub struct StyleTokens {
    pub font: &'static str,
    pub body_size: f64,
    pub line_height: f64,
    pub indent: f64,
    pub spacing: f64,
    pub justify: bool,
    pub chapter_size: f64,
    pub chapter_uppercase: bool,
    pub chapter_align: &'static str,
    pub chapter_before: f64,
    pub chapter_after: f64,
    pub scene_break: f64,
    pub drop_cap: bool,
    /// Width, height, margins top, right, bottom, left, in points.
    pub page: [f64; 6],
}

const MANUSCRIPT: StyleTokens = StyleTokens {
    font: "Georgia, 'Times New Roman', serif",
    body_size: 12.0,
    line_height: 2.0,
    indent: 2.0,
    spacing: 0.0,
    justify: false,
    chapter_size: 14.0,
    chapter_uppercase: true,
    chapter_align: "center",
    chapter_before: 0.0,
    chapter_after: 1.5,
    scene_break: 1.0,
    drop_cap: false,
    page: [612.0, 792.0, 72.0, 72.0, 72.0, 72.0],
};

const CLASSIC: StyleTokens = StyleTokens {
    font: "Georgia, 'Iowan Old Style', 'Times New Roman', serif",
    body_size: 11.5,
    line_height: 1.5,
    indent: 1.5,
    spacing: 0.0,
    justify: true,
    chapter_size: 18.0,
    chapter_uppercase: true,
    chapter_align: "center",
    chapter_before: 1.2,
    chapter_after: 2.0,
    scene_break: 1.2,
    drop_cap: true,
    page: [432.0, 648.0, 54.0, 54.0, 54.0, 54.0],
};

const MODERN: StyleTokens = StyleTokens {
    font: "'IBM Plex Serif', 'Literata', Georgia, serif",
    body_size: 11.0,
    line_height: 1.45,
    indent: 0.0,
    spacing: 0.65,
    justify: false,
    chapter_size: 24.0,
    chapter_uppercase: false,
    chapter_align: "left",
    chapter_before: 0.8,
    chapter_after: 1.8,
    scene_break: 1.1,
    drop_cap: false,
    page: [432.0, 648.0, 60.0, 54.0, 60.0, 54.0],
};

const EBOOK: StyleTokens = StyleTokens {
    font: "Georgia, 'Times New Roman', serif",
    body_size: 11.5,
    line_height: 1.6,
    indent: 1.4,
    spacing: 0.0,
    justify: false,
    chapter_size: 18.0,
    chapter_uppercase: false,
    chapter_align: "left",
    chapter_before: 1.0,
    chapter_after: 1.4,
    scene_break: 1.1,
    drop_cap: false,
    page: [432.0, 648.0, 42.0, 42.0, 42.0, 42.0],
};

/// `resolveStyleTokens` for the built-in styles. Custom styles aren't ported yet and use the
/// Manuscript style, as ProseCraft does for a style it can't find.
pub fn style_tokens(style: &str) -> &'static StyleTokens {
    match style {
        "classic" => &CLASSIC,
        "modern" => &MODERN,
        "ebook" => &EBOOK,
        _ => &MANUSCRIPT,
    }
}

/// `escapeXml`.
pub fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `escapeXmlAttribute`.
pub fn escape_xml_attribute(value: &str) -> String {
    escape_xml(value)
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub(super) fn with_breaks(text: &str) -> String {
    escape_xml(text).replace('\n', "<br />")
}

/// `(points / 72).toFixed(3)` inches.
fn inch(points: f64) -> String {
    format!("{:.3}in", points / 72.0)
}

fn css(tokens: &StyleTokens, drop_caps: bool) -> String {
    let n = number_to_string;
    let [width, height, top, right, bottom, left] = tokens.page;
    let pairs: [(&str, String); 25] = [
        ("{{BODY_FONT}}", tokens.font.to_owned()),
        ("{{BODY_SIZE}}", n(tokens.body_size)),
        ("{{LINE_HEIGHT}}", n(tokens.line_height)),
        ("{{INDENT}}", n(tokens.indent)),
        ("{{SPACING}}", n(tokens.spacing)),
        ("{{CHAPTER_SIZE}}", n(tokens.chapter_size)),
        ("{{CHAPTER_ALIGN}}", tokens.chapter_align.to_owned()),
        ("{{CHAPTER_BEFORE}}", n(tokens.chapter_before)),
        ("{{CHAPTER_AFTER}}", n(tokens.chapter_after)),
        ("{{SCENE_BREAK}}", n(tokens.scene_break)),
        ("{{PAGE_WIDTH}}", n(width)),
        ("{{PAGE_HEIGHT}}", n(height)),
        ("{{MARGIN_TOP}}", n(top)),
        ("{{MARGIN_RIGHT}}", n(right)),
        ("{{MARGIN_BOTTOM}}", n(bottom)),
        ("{{MARGIN_LEFT}}", n(left)),
        ("{{INCH_WIDTH}}", inch(width)),
        ("{{INCH_HEIGHT}}", inch(height)),
        ("{{INCH_TOP}}", inch(top)),
        ("{{INCH_RIGHT}}", inch(right)),
        ("{{INCH_BOTTOM}}", inch(bottom)),
        ("{{INCH_LEFT}}", inch(left)),
        (
            "{{JUSTIFY}}",
            if tokens.justify { "justify" } else { "left" }.to_owned(),
        ),
        (
            "{{HEADING_TRACKING}}",
            if tokens.chapter_uppercase {
                "0.1em"
            } else {
                "0"
            }
            .to_owned(),
        ),
        (
            "{{HEADING_CASE}}",
            if tokens.chapter_uppercase {
                "uppercase"
            } else {
                "none"
            }
            .to_owned(),
        ),
    ];
    let mut out = CSS.replace("{{DROP_CAPS}}", if drop_caps { DROP_CAPS } else { "" });
    for (marker, value) in pairs {
        out = out.replace(marker, &value);
    }
    out
}

pub(super) fn separator_markup(separator: SceneSeparator) -> &'static str {
    match separator {
        SceneSeparator::Hash => r##"<div class="scene-break" aria-hidden="true">#</div>"##,
        SceneSeparator::Asterisks => r#"<div class="scene-break" aria-hidden="true">* * *</div>"#,
        SceneSeparator::Line => {
            r#"<div class="scene-break scene-break-line" aria-hidden="true"><span></span></div>"#
        }
        SceneSeparator::Blank => {
            r#"<div class="scene-break scene-break-blank" aria-hidden="true"></div>"#
        }
        SceneSeparator::None => "",
    }
}

fn title_page(book: &CompiledBook, include_copyright: bool) -> String {
    let mut parts = vec![
        r#"<section class="title-page">"#.to_owned(),
        r#"  <div class="title-page-main">"#.to_owned(),
        format!(
            r#"    <h1 class="book-title">{}</h1>"#,
            escape_xml(&book.title)
        ),
    ];
    if !book.subtitle.is_empty() {
        parts.push(format!(
            r#"    <p class="book-subtitle">{}</p>"#,
            escape_xml(&book.subtitle)
        ));
    }
    if !book.author.is_empty() {
        parts.push(format!(
            r#"    <p class="book-author">by {}</p>"#,
            escape_xml(&book.author)
        ));
    }
    parts.push("  </div>".to_owned());
    if include_copyright && !book.copyright.is_empty() {
        parts.push(format!(
            r#"  <p class="book-copyright">{}</p>"#,
            with_breaks(&book.copyright)
        ));
    }
    parts.push("</section>".to_owned());
    parts.join("\n")
}

/// `matterParagraphs`: paragraphs split at blank lines, trimmed, empty ones left out.
pub(super) fn matter_paragraphs(body: &str) -> Vec<String> {
    let body = body.replace("\r\n", "\n").replace('\r', "\n");
    let mut paragraphs = Vec::new();
    let mut current = String::new();
    let mut newlines = 0;
    for c in body.chars() {
        if c == '\n' {
            newlines += 1;
            continue;
        }
        if newlines >= 2 {
            paragraphs.push(std::mem::take(&mut current));
        } else if newlines == 1 {
            current.push('\n');
        }
        newlines = 0;
        current.push(c);
    }
    if newlines == 1 {
        current.push('\n');
    }
    paragraphs.push(current);
    paragraphs
        .into_iter()
        .map(|p| p.trim_matches(is_js_whitespace).to_owned())
        .filter(|p| !p.is_empty())
        .collect()
}

fn matter_page(page: &MatterPage, placement: &str) -> String {
    if page.title.is_empty() && page.body.is_empty() && !page.blank {
        return String::new();
    }
    let title = if page.title.is_empty() {
        String::new()
    } else {
        format!(
            "  <h2 class=\"matter-title\">{}</h2>\n",
            escape_xml(&page.title)
        )
    };
    let paragraphs = matter_paragraphs(&page.body)
        .iter()
        .map(|p| format!("  <p>{}</p>", with_breaks(p)))
        .collect::<Vec<_>>()
        .join("\n");
    let blank = if page.blank && title.is_empty() && paragraphs.is_empty() {
        " blank-matter-page"
    } else {
        ""
    };
    let newline = if paragraphs.is_empty() { "" } else { "\n" };
    format!(
        "<section class=\"matter-page {placement}-matter{blank}\">\n{title}{paragraphs}{newline}</section>"
    )
}

fn table_of_contents(book: &CompiledBook) -> String {
    if book.chapters.is_empty() {
        return String::new();
    }
    let entries = book
        .chapters
        .iter()
        .map(|c| {
            format!(
                "  <p class=\"toc-entry\"><a href=\"#chapter-{}\">{}</a></p>",
                escape_xml_attribute(&c.id),
                escape_xml(&c.label)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<nav class=\"matter-page toc-page\" aria-label=\"Table of contents\">\n  <h2 class=\"matter-title\">Contents</h2>\n{entries}\n</nav>"
    )
}

fn footnote_suffix(note: &ResolvedFootnote, restart: bool) -> String {
    if restart {
        format!("ch{}-{}", note.chapter + 1, note.number)
    } else {
        note.number.to_string()
    }
}

/// `startsWithRichContent`: the paragraph opens with an equation or diagram.
fn starts_with_rich(paragraph: &Element) -> bool {
    fn is_rich(e: &Element) -> bool {
        RICH.iter().any(|(class, ..)| e.has_class(class))
    }
    fn contains_rich(nodes: &[Node]) -> bool {
        nodes
            .iter()
            .any(|n| matches!(n, Node::Element(e) if is_rich(e) || contains_rich(&e.children)))
    }
    for child in &paragraph.children {
        match child {
            Node::Text(t) if t.trim_matches(is_js_whitespace).is_empty() => continue,
            Node::Text(_) => return false,
            Node::Element(e) => return is_rich(e) || contains_rich(&e.children),
        }
    }
    false
}

/// Marks the first narrative paragraph (not an eyebrow, a quote, a list item, or one that
/// opens with rich content) with `drop-cap`. Returns whether one was found.
fn mark_drop_cap(nodes: &mut [Node], inside: bool) -> bool {
    for node in nodes {
        if let Node::Element(e) = node {
            let inside_here = inside || e.name == "blockquote" || e.name == "li";
            if e.name == "p" && !e.has_class("eyebrow") && !inside && !starts_with_rich(e) {
                match e.attrs.iter_mut().find(|(k, _)| k == "class") {
                    Some((_, value)) => {
                        // classList.add: the token is appended to the cleaned list.
                        let mut tokens: Vec<&str> = value.split_ascii_whitespace().collect();
                        tokens.push("drop-cap");
                        *value = tokens.join(" ");
                    }
                    None => e.attrs.push(("class".into(), "drop-cap".into())),
                }
                return true;
            }
            if mark_drop_cap(&mut e.children, inside_here) {
                return true;
            }
        }
    }
    false
}

/// `markDropCapParagraph`.
pub(super) fn mark_drop_cap_paragraph(html: &str) -> String {
    let mut nodes = parse_fragment(html);
    if mark_drop_cap(&mut nodes, false) {
        serialize(&nodes)
    } else {
        html.to_owned()
    }
}

/// `renderSceneFootnotesHtml`: each marker becomes a numbered link to its note; the first
/// reference to a note in the chapter carries the id the note links back to.
fn render_footnotes(
    nodes: &mut [Node],
    notes: &HashMap<String, ResolvedFootnote>,
    seen: &mut HashSet<String>,
    restart: bool,
) {
    for node in nodes {
        if let Node::Element(e) = node {
            if e.name == "sup"
                && e.has_class(FOOTNOTE_REF_CLASS)
                && e.attr("data-footnote-id").is_some()
            {
                let id = e.attr("data-footnote-id").unwrap_or_default();
                let Some(note) = notes.get(id) else { continue };
                let suffix = footnote_suffix(note, restart);
                let ref_id = format!("fnref-{suffix}");
                let mut attrs = vec![("class".to_owned(), FOOTNOTE_REF_CLASS.to_owned())];
                if seen.insert(ref_id.clone()) {
                    attrs.push(("id".to_owned(), ref_id));
                }
                let link = Element {
                    name: "a".into(),
                    attrs: vec![("href".into(), format!("#fn-{suffix}"))],
                    children: vec![Node::Text(note.number.to_string())],
                };
                *e = Element {
                    name: "sup".into(),
                    attrs,
                    children: vec![Node::Element(link)],
                };
            } else {
                render_footnotes(&mut e.children, notes, seen, restart);
            }
        }
    }
}

fn chapter_html(
    chapter: &CompiledChapter,
    c: usize,
    options: &PublishOptions,
    first: bool,
    drop_caps: bool,
    notes: &BookFootnotes,
) -> String {
    let restart = options.footnotes_restart_per_chapter;
    let mut seen = HashSet::new();
    let count = chapter.scenes.len();
    let scenes = chapter
        .scenes
        .iter()
        .enumerate()
        .map(|(s, scene)| {
            let title = if options.include_scene_titles {
                format!(
                    "<h3 class=\"scene-title\">{}</h3>",
                    escape_xml(&scene.title)
                )
            } else {
                String::new()
            };
            let separator = if s + 1 < count {
                separator_markup(options.scene_separator)
            } else {
                ""
            };
            let base = if drop_caps && s == 0 {
                mark_drop_cap_paragraph(&scene.html)
            } else {
                scene.html.clone()
            };
            let html = match notes.by_scene.get(&(c, s)) {
                Some(map) if base.contains(FOOTNOTE_REF_CLASS) => {
                    let mut nodes = parse_fragment(&base);
                    let before = nodes.clone();
                    render_footnotes(&mut nodes, map, &mut seen, restart);
                    if nodes == before {
                        base
                    } else {
                        serialize(&nodes)
                    }
                }
                _ => base,
            };
            format!("<div class=\"scene\">{title}{html}</div>{separator}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let chapter_notes = &notes.by_chapter[c];
    let footnotes = if chapter_notes.is_empty() {
        String::new()
    } else {
        let items = chapter_notes
            .iter()
            .map(|note| {
                let suffix = footnote_suffix(note, restart);
                format!(
                    "    <li id=\"fn-{suffix}\" value=\"{n}\">{html} <a href=\"#fnref-{suffix}\" class=\"footnote-backref\" aria-label=\"Back to reference {n}\">↩</a></li>",
                    n = note.number,
                    html = note.html
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "\n<section class=\"footnotes\" aria-label=\"Footnotes\">\n  <ol>\n{items}\n  </ol>\n</section>"
        )
    };
    let break_class = if !first && options.page_break_before_chapters {
        " chapter-break"
    } else {
        ""
    };
    let heading = if options.include_chapter_titles {
        format!(
            "<h2 class=\"chapter-title\">{}</h2>",
            escape_xml(&chapter.label)
        )
    } else {
        String::new()
    };
    format!(
        "<section class=\"chapter{break_class}\" id=\"chapter-{}\">{heading}\n{scenes}{footnotes}</section>",
        escape_xml_attribute(&chapter.id)
    )
}

/// `buildManuscriptBody`: every section of the book in order (`listManuscriptSections`).
/// Covers and the bibliography aren't ported yet.
fn manuscript_body(book: &CompiledBook, options: &PublishOptions, drop_caps: bool) -> String {
    let notes = resolve_book_footnotes(book, options.footnotes_restart_per_chapter);
    let mut sections = Vec::new();
    if options.include_title_page {
        sections.push(title_page(book, !options.include_copyright_page));
    }
    if options.include_copyright_page && !book.copyright.is_empty() {
        sections.push(format!(
            "<section class=\"copyright-page\"><p>{}</p></section>",
            with_breaks(&book.copyright)
        ));
    }
    if !book.dedication.is_empty() {
        sections.push(format!(
            "<section class=\"dedication\"><p>{}</p></section>",
            with_breaks(&book.dedication)
        ));
    }
    if options.include_table_of_contents && !book.chapters.is_empty() {
        sections.push(table_of_contents(book));
    }
    for page in &book.front_matter {
        sections.push(matter_page(page, "front"));
    }
    for (c, chapter) in book.chapters.iter().enumerate() {
        sections.push(chapter_html(chapter, c, options, c == 0, drop_caps, &notes));
    }
    for page in &book.back_matter {
        sections.push(matter_page(page, "back"));
    }
    sections
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// `exportHtml`, byte for byte.
pub fn export_html(book: &CompiledBook, options: &PublishOptions) -> Export {
    let book = &resolve_book_citations(book);
    let tokens = style_tokens(&options.style);
    let drop_caps = options.drop_caps && tokens.drop_cap;
    let body = manuscript_body(book, options, drop_caps);
    let html = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\" />\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />\n  <title>{}</title>\n  <style>{}</style>\n</head>\n<body class=\"{}\">\n<div class=\"book\">\n{body}\n</div>\n</body>\n</html>",
        escape_xml(&book.title),
        css(tokens, drop_caps),
        if drop_caps { "drop-caps" } else { "" }
    );
    Export {
        file_name: format!("{}.html", slugify_filename(&book.title)),
        bytes: html.into_bytes(),
    }
}

#[cfg(test)]
mod tests {
    use super::matter_paragraphs;

    #[test]
    fn splits_matter_like_prosecraft() {
        assert_eq!(
            matter_paragraphs("Some words.\n\nMore & more.\r\nSame paragraph."),
            ["Some words.", "More & more.\nSame paragraph."]
        );
        assert_eq!(matter_paragraphs("\n\n  a \n\n\n\nb\n"), ["a", "b"]);
        assert!(matter_paragraphs("").is_empty());
    }
}
