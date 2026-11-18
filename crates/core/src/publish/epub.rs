//! `exportEpub`: the book as an EPUB 3 package (ProseCraft `src/lib/publish/epub.ts` and
//! `xhtml.ts` `htmlToXhtml`, upstream `7dae544`). The pages, package document, navigation
//! document, NCX and stylesheet are ProseCraft's, byte for byte; craft-zip writes the
//! container (the `mimetype` entry first and stored, as OCF requires).
//!
//! Embedded PNG and JPEG images are copied as they are. A WebP image is re-encoded as PNG,
//! as ProseCraft does, so readers without WebP support still show it.

use std::collections::HashMap;

use super::citations::resolve_book_citations;
use super::epub_css::{CSS, DROP_CAPS};
use super::footnotes::{BookFootnotes, resolve_book_footnotes};
use super::html::{
    escape_xml, escape_xml_attribute, mark_drop_cap_paragraph, matter_paragraphs, separator_markup,
    style_tokens, with_breaks,
};
use super::{CompiledBook, Export, MatterPage, PublishOptions, slugify_filename};
use crate::html::sanitize::{FOOTNOTE_REF_CLASS, is_valid_footnote_id};
use crate::html::{Element, Node, parse_fragment};
use crate::js::number_to_string;

/// Why an EPUB couldn't be made.
#[derive(Debug, thiserror::Error)]
pub enum EpubError {
    #[error("an embedded image couldn't be read: {0}")]
    Image(String),
    #[error("the package couldn't be written: {0}")]
    Zip(String),
}

const VOID: &[&str] = &["br", "hr", "img", "input", "meta", "link"];

/// `SAFE_ID_RE`: a letter, then letters, digits, `_ . : -`.
fn safe_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
}

/// `SAFE_EPUB_TYPE_RE`.
fn safe_epub_type(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ' ' | ':' | '-'))
}

/// A footnote marker's link: number, target, and the id the note links back to.
struct NoteRef {
    number: usize,
    href: String,
    ref_id: Option<String>,
}

/// Resolves a footnote id to its link, if the scene has that note.
type ResolveFootnote<'a> = &'a mut dyn FnMut(&str) -> Option<NoteRef>;

struct XhtmlOptions<'a> {
    image: &'a dyn Fn(&str) -> String,
    footnote: Option<ResolveFootnote<'a>>,
}

fn classes(e: &Element, allowed: &[&str]) -> Option<String> {
    let kept: Vec<&str> = allowed.iter().copied().filter(|c| e.has_class(c)).collect();
    (!kept.is_empty()).then(|| format!("class=\"{}\"", kept.join(" ")))
}

/// `serializeNode`: well-formed XHTML keeping only the attributes ProseCraft's EPUB uses.
fn serialize_node(node: &Node, options: &mut XhtmlOptions<'_>, out: &mut String) {
    let e = match node {
        Node::Text(t) => {
            out.push_str(&escape_xml(t));
            return;
        }
        Node::Element(e) => e,
    };
    let name = e.name.as_str();
    if name == "sup"
        && e.has_class(FOOTNOTE_REF_CLASS)
        && let Some(resolve) = options.footnote.as_mut()
    {
        {
            let id = e.attr("data-footnote-id").unwrap_or_default();
            let Some(note) = is_valid_footnote_id(id).then(|| resolve(id)).flatten() else {
                return;
            };
            let id_attr = note
                .ref_id
                .filter(|r| safe_id(r))
                .map(|r| format!(" id=\"{}\"", escape_xml_attribute(&r)))
                .unwrap_or_default();
            out.push_str(&format!("<a epub:type=\"noteref\" href=\"{}\"{id_attr} class=\"footnote-ref\"><sup>{}</sup></a>", escape_xml_attribute(&note.href), note.number));
            return;
        }
    }
    let mut attributes: Vec<String> = Vec::new();
    match name {
        "p" => attributes.extend(classes(e, &["eyebrow", "drop-cap", "bibliography-entry"])),
        "span" | "div" => attributes.extend(classes(
            e,
            &[
                "math-inline",
                "math-block",
                "mermaid-diagram",
                "calc-worksheet",
                "footnote-number",
            ],
        )),
        "aside" | "section" | "sup" => attributes.extend(classes(
            e,
            &["footnote", "footnotes", "footnote-ref", "bibliography"],
        )),
        _ => {}
    }
    if matches!(name, "a" | "aside" | "section" | "sup")
        && let Some(id) = e.attr("id").filter(|id| safe_id(id))
    {
        attributes.push(format!("id=\"{}\"", escape_xml_attribute(id)));
    }
    if matches!(name, "a" | "aside" | "section")
        && let Some(kind) = e.attr("epub:type").filter(|k| safe_epub_type(k))
    {
        attributes.push(format!("epub:type=\"{}\"", escape_xml_attribute(kind)));
    }
    if name == "a" {
        if let Some(href) = e.attr("href").filter(|h| !h.is_empty()) {
            attributes.push(format!("href=\"{}\"", escape_xml_attribute(href)));
        }
        attributes.extend(classes(e, &["footnote-ref", "footnote-backref"]));
        if let Some(label) = e.attr("aria-label").filter(|l| !l.is_empty()) {
            attributes.push(format!("aria-label=\"{}\"", escape_xml_attribute(label)));
        }
    }
    if name == "img" {
        if let Some(src) = e.attr("src").filter(|s| !s.is_empty()) {
            attributes.push(format!(
                "src=\"{}\"",
                escape_xml_attribute(&(options.image)(src))
            ));
        }
        if let Some(alt) = e.attr("alt") {
            attributes.push(format!("alt=\"{}\"", escape_xml_attribute(alt)));
        }
        for dimension in ["width", "height"] {
            if let Some(v) = e
                .attr(dimension)
                .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
            {
                attributes.push(format!("{dimension}=\"{v}\""));
            }
        }
    }
    let attribute_text = if attributes.is_empty() {
        String::new()
    } else {
        format!(" {}", attributes.join(" "))
    };
    if VOID.contains(&name) {
        out.push_str(&format!("<{name}{attribute_text} />"));
        return;
    }
    out.push_str(&format!("<{name}{attribute_text}>"));
    for child in &e.children {
        serialize_node(child, options, out);
    }
    out.push_str(&format!("</{name}>"));
}

/// `htmlToXhtml`.
fn html_to_xhtml(html: &str, options: &mut XhtmlOptions<'_>) -> String {
    let mut out = String::new();
    for node in parse_fragment(html) {
        serialize_node(&node, options, &mut out);
    }
    out
}

fn page(title: &str, body: &str, body_class: &str) -> String {
    let class = if body_class.is_empty() {
        String::new()
    } else {
        format!(" class=\"{body_class}\"")
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"en\" lang=\"en\">\n<head>\n  <meta charset=\"utf-8\" />\n  <title>{}</title>\n  <link rel=\"stylesheet\" type=\"text/css\" href=\"../styles.css\" />\n</head>\n<body{class}>\n{body}\n</body>\n</html>\n",
        escape_xml(title)
    )
}

fn title_page(book: &CompiledBook, include_copyright: bool) -> String {
    let mut lines = vec![
        "<div class=\"title-page\">".to_owned(),
        format!(
            "  <h1 class=\"book-title\">{}</h1>",
            escape_xml(&book.title)
        ),
    ];
    if !book.subtitle.is_empty() {
        lines.push(format!(
            "  <p class=\"book-subtitle\">{}</p>",
            escape_xml(&book.subtitle)
        ));
    }
    if !book.author.is_empty() {
        lines.push(format!(
            "  <p class=\"book-author\">by {}</p>",
            escape_xml(&book.author)
        ));
    }
    if include_copyright && !book.copyright.is_empty() {
        lines.push(format!(
            "  <p class=\"book-copyright\">{}</p>",
            with_breaks(&book.copyright)
        ));
    }
    lines.push("</div>".to_owned());
    page(&book.title, &lines.join("\n"), "titlepage")
}

fn contents_page(items: &[(String, String)]) -> String {
    let entries = items
        .iter()
        .map(|(href, label)| {
            format!(
                "    <li><a href=\"{}\">{}</a></li>",
                href.rsplit('/').next().unwrap_or(href),
                escape_xml(label)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    page(
        "Contents",
        &format!(
            "<nav class=\"contents-page\" aria-label=\"Table of contents\">\n  <h1 class=\"matter-title\">Contents</h1>\n  <ol class=\"toc-list\">\n{entries}\n  </ol>\n</nav>"
        ),
        "contents-page-body",
    )
}

/// `bibliographyXhtml`.
fn bibliography_page(bibliography: &crate::citations::bibliography::BookBibliography) -> String {
    let same = |src: &str| src.to_owned();
    let entries: Vec<String> = bibliography
        .entries
        .iter()
        .map(|e| {
            let xhtml = html_to_xhtml(
                &e.html,
                &mut XhtmlOptions {
                    image: &same,
                    footnote: None,
                },
            );
            format!("  <p class=\"bibliography-entry\">{xhtml}</p>")
        })
        .collect();
    page(
        &bibliography.heading,
        &format!(
            "<section class=\"matter-page back-matter bibliography\" epub:type=\"bibliography\">\n  <h1 class=\"matter-title\">{}</h1>\n{}\n</section>",
            escape_xml(&bibliography.heading),
            entries.join("\n")
        ),
        "back-matter-page bibliography-page",
    )
}

fn matter_page(p: &MatterPage, placement: &str) -> String {
    let title = if p.title.is_empty() {
        String::new()
    } else {
        format!(
            "  <h1 class=\"matter-title\">{}</h1>\n",
            escape_xml(&p.title)
        )
    };
    let paragraphs = matter_paragraphs(&p.body)
        .iter()
        .map(|x| format!("  <p>{}</p>", with_breaks(x)))
        .collect::<Vec<_>>()
        .join("\n");
    let label = if !p.title.is_empty() {
        p.title.clone()
    } else if p.blank {
        "Blank page".to_owned()
    } else if placement == "front" {
        "Front matter".to_owned()
    } else {
        "Back matter".to_owned()
    };
    let newline = if paragraphs.is_empty() { "" } else { "\n" };
    page(
        &label,
        &format!(
            "<section class=\"matter-page {placement}-matter\">\n{title}{paragraphs}{newline}</section>"
        ),
        &format!("{placement}-matter-page"),
    )
}

fn chapter_page(
    book: &CompiledBook,
    options: &PublishOptions,
    c: usize,
    images: &HashMap<String, String>,
    notes: &BookFootnotes,
) -> String {
    let chapter = &book.chapters[c];
    let heading = if options.include_chapter_titles {
        format!(
            "<h2 class=\"chapter-title\">{}</h2>",
            escape_xml(&chapter.label)
        )
    } else {
        String::new()
    };
    let drop_caps = options.drop_caps && style_tokens(&options.style).drop_cap;
    let image = |src: &str| images.get(src).cloned().unwrap_or_else(|| src.to_owned());
    let mut seen: Vec<String> = Vec::new();
    let count = chapter.scenes.len();
    let mut scenes = Vec::new();
    for (s, scene) in chapter.scenes.iter().enumerate() {
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
        let html = if drop_caps && s == 0 {
            mark_drop_cap_paragraph(&scene.html)
        } else {
            scene.html.clone()
        };
        let scene_notes = notes.by_scene.get(&(c, s));
        let mut resolve = |id: &str| {
            let note = scene_notes?.get(id)?;
            let ref_id = format!("fnref-{}", note.number);
            let emit = !seen.contains(&ref_id);
            if emit {
                seen.push(ref_id.clone());
            }
            Some(NoteRef {
                number: note.number,
                href: format!("#fn-{}", note.number),
                ref_id: emit.then_some(ref_id),
            })
        };
        let xhtml = html_to_xhtml(
            &html,
            &mut XhtmlOptions {
                image: &image,
                footnote: Some(&mut resolve),
            },
        );
        scenes.push(format!(
            "<div class=\"scene\">{title}{xhtml}</div>{separator}"
        ));
    }
    let chapter_notes = &notes.by_chapter[c];
    let footnotes = if chapter_notes.is_empty() {
        String::new()
    } else {
        let items = chapter_notes
            .iter()
            .map(|n| {
                let html = html_to_xhtml(&n.html, &mut XhtmlOptions { image: &image, footnote: None });
                format!(
                    "  <aside epub:type=\"footnote\" id=\"fn-{n}\" class=\"footnote\"><p><span class=\"footnote-number\">{n}.</span> {html} <a href=\"#fnref-{n}\" class=\"footnote-backref\" aria-label=\"Back to reference {n}\">↩</a></p></aside>",
                    n = n.number
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!("\n<section class=\"footnotes\" epub:type=\"footnotes\">\n{items}\n</section>")
    };
    let break_class = if c != 0 && options.page_break_before_chapters {
        " chapter-break"
    } else {
        ""
    };
    let drop_class = if drop_caps { " drop-caps" } else { "" };
    page(
        &chapter.label,
        &format!(
            "<section class=\"chapter{break_class}\">{heading}\n{}{footnotes}</section>",
            scenes.join("\n")
        ),
        &format!("chapter-page{drop_class}"),
    )
}

fn stylesheet(options: &PublishOptions) -> String {
    let t = style_tokens(&options.style);
    let n = number_to_string;
    let pairs = [
        ("{{BODY_FONT}}", t.font.to_owned()),
        ("{{LINE_HEIGHT}}", n(t.line_height)),
        ("{{SPACING}}", n(t.spacing)),
        (
            "{{JUSTIFY}}",
            if t.justify { "justify" } else { "left" }.to_owned(),
        ),
        ("{{CHAPTER_BEFORE}}", n(t.chapter_before)),
        ("{{CHAPTER_AFTER}}", n(t.chapter_after)),
        (
            "{{HEADING_EM}}",
            format!("{:.2}", t.chapter_size / t.body_size),
        ),
        (
            "{{HEADING_TRACKING}}",
            if t.chapter_uppercase { "0.1em" } else { "0" }.to_owned(),
        ),
        ("{{CHAPTER_ALIGN}}", t.chapter_align.to_owned()),
        (
            "{{HEADING_CASE}}",
            if t.chapter_uppercase {
                "uppercase"
            } else {
                "none"
            }
            .to_owned(),
        ),
        ("{{INDENT}}", n(t.indent)),
        ("{{SCENE_BREAK}}", n(t.scene_break)),
    ];
    let mut out = CSS.replace(
        "{{DROP_CAPS}}",
        if options.drop_caps && t.drop_cap {
            DROP_CAPS
        } else {
            ""
        },
    );
    for (marker, value) in pairs {
        out = out.replace(marker, &value);
    }
    out
}

fn nav(items: &[(String, String)]) -> String {
    let items = items
        .iter()
        .map(|(href, label)| {
            format!(
                "      <li><a href=\"{href}\">{}</a></li>",
                escape_xml(label)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"en\" lang=\"en\">\n<head>\n  <meta charset=\"utf-8\" />\n  <title>Contents</title>\n  <link rel=\"stylesheet\" type=\"text/css\" href=\"styles.css\" />\n</head>\n<body>\n  <nav epub:type=\"toc\" id=\"toc\">\n    <h1>Contents</h1>\n    <ol>\n{items}\n    </ol>\n  </nav>\n</body>\n</html>\n"
    )
}

fn ncx(book: &CompiledBook, identifier: &str, items: &[(String, String)]) -> String {
    let points = items
        .iter()
        .enumerate()
        .map(|(i, (href, label))| format!("    <navPoint id=\"nav-{n}\" playOrder=\"{n}\">\n      <navLabel><text>{}</text></navLabel>\n      <content src=\"{href}\" />\n    </navPoint>", escape_xml(label), n = i + 1))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n  <head>\n    <meta name=\"dtb:uid\" content=\"{}\" />\n    <meta name=\"dtb:depth\" content=\"1\" />\n    <meta name=\"dtb:totalPageCount\" content=\"0\" />\n    <meta name=\"dtb:maxPageNumber\" content=\"0\" />\n  </head>\n  <docTitle><text>{}</text></docTitle>\n  <navMap>\n{points}\n  </navMap>\n</ncx>\n",
        escape_xml(identifier),
        escape_xml(&book.title)
    )
}

fn collect_images(nodes: &[Node], out: &mut Vec<String>) {
    for node in nodes {
        if let Node::Element(e) = node {
            if e.name == "img"
                && let Some(src) = e.attr("src").filter(|s| !s.is_empty())
            {
                out.push(src.to_owned());
            }
            collect_images(&e.children, out);
        }
    }
}

/// `exportEpub`: the package for book id `uuid` (a random UUID, as ProseCraft's
/// `crypto.randomUUID()`), modified at `now_ms` (Unix milliseconds).
pub fn export_epub(
    book: &CompiledBook,
    options: &PublishOptions,
    uuid: &str,
    now_ms: f64,
) -> Result<Export, EpubError> {
    // `new Date().toISOString()` without milliseconds.
    let now = chrono::DateTime::from_timestamp_millis(now_ms as i64).unwrap_or_default();
    let modified = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    export_epub_with(book, options, &format!("urn:uuid:{uuid}"), &modified)
}

/// The EPUB's files in order: name and bytes (for tests, which fix the identifier and time).
pub fn epub_entries(
    book: &CompiledBook,
    options: &PublishOptions,
    identifier: &str,
    modified: &str,
) -> Result<Vec<(String, Vec<u8>)>, EpubError> {
    let (book, bibliography) = resolve_book_citations(book, options);
    let book = &book;
    let bibliography = bibliography.filter(|b| !b.entries.is_empty());
    let mut files: Vec<(String, Vec<u8>)> =
        vec![("mimetype".into(), b"application/epub+zip".to_vec())];
    let mut images: HashMap<String, String> = HashMap::new();
    let mut image_files = Vec::new();
    let mut image_manifest = Vec::new();
    for scene in book.chapters.iter().flat_map(|c| &c.scenes) {
        let mut sources = Vec::new();
        collect_images(&parse_fragment(&scene.html), &mut sources);
        for src in sources {
            if images.contains_key(&src) {
                continue;
            }
            let super::embedded_image::EmbeddedImage {
                bytes,
                extension,
                media_type,
                ..
            } = super::embedded_image::decode_embedded_image(&src).map_err(EpubError::Image)?;
            let index = images.len() + 1;
            let file = format!("image-{index}.{extension}");
            images.insert(src, format!("../images/{file}"));
            image_files.push((format!("OEBPS/images/{file}"), bytes));
            image_manifest.push(format!("    <item id=\"image-{index}\" href=\"images/{file}\" media-type=\"{media_type}\" />"));
        }
    }
    // Images come first in the archive, as ProseCraft adds them while reading the scenes.
    files.extend(image_files);
    let items: Vec<(String, String)> = book
        .chapters
        .iter()
        .enumerate()
        .map(|(i, c)| (format!("text/chapter-{}.xhtml", i + 1), c.label.clone()))
        .collect();
    let mut manifest = vec![
        "    <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\" />".to_owned(),
        "    <item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\" />".to_owned(),
        "    <item id=\"css\" href=\"styles.css\" media-type=\"text/css\" />".to_owned(),
    ];
    manifest.extend(image_manifest);
    let mut spine = Vec::new();
    let mut add = |id: String, manifest: &mut Vec<String>| {
        manifest.push(format!(
            "    <item id=\"{id}\" href=\"text/{id}.xhtml\" media-type=\"application/xhtml+xml\" />"
        ));
        spine.push(format!("    <itemref idref=\"{id}\" />"));
    };
    let copyright_page = options.include_copyright_page && !book.copyright.is_empty();
    let contents = options.include_table_of_contents && !book.chapters.is_empty();
    if options.include_title_page {
        add("titlepage".into(), &mut manifest);
    }
    if copyright_page {
        add("copyright".into(), &mut manifest);
    }
    if !book.dedication.is_empty() {
        add("dedication".into(), &mut manifest);
    }
    if contents {
        add("contents".into(), &mut manifest);
    }
    for i in 0..book.front_matter.len() {
        add(format!("front-matter-{}", i + 1), &mut manifest);
    }
    for i in 0..book.chapters.len() {
        add(format!("chapter-{}", i + 1), &mut manifest);
    }
    for i in 0..book.back_matter.len() {
        add(format!("back-matter-{}", i + 1), &mut manifest);
    }
    if bibliography.is_some() {
        add("bibliography".into(), &mut manifest);
    }
    let creator = if book.author.is_empty() {
        String::new()
    } else {
        format!("<dc:creator>{}</dc:creator>", escape_xml(&book.author))
    };
    let opf = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"book-id\" xml:lang=\"en\">\n  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n    <dc:identifier id=\"book-id\">{}</dc:identifier>\n    <dc:title>{}</dc:title>\n    {creator}\n    <dc:language>en</dc:language>\n    \n    <meta property=\"dcterms:modified\">{modified}</meta>\n  </metadata>\n  <manifest>\n{}\n  </manifest>\n  <spine toc=\"ncx\">\n{}\n  </spine>\n</package>\n",
        escape_xml(identifier),
        escape_xml(&book.title),
        manifest.join("\n"),
        spine.join("\n")
    );
    let container = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n  <rootfiles>\n    <rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\" />\n  </rootfiles>\n</container>\n";
    let mut text = |name: &str, body: String| files.push((name.to_owned(), body.into_bytes()));
    text("META-INF/container.xml", container.to_owned());
    text("OEBPS/content.opf", opf);
    text("OEBPS/nav.xhtml", nav(&items));
    text("OEBPS/toc.ncx", ncx(book, identifier, &items));
    text("OEBPS/styles.css", stylesheet(options));
    if options.include_title_page {
        text(
            "OEBPS/text/titlepage.xhtml",
            title_page(book, !options.include_copyright_page),
        );
    }
    if copyright_page {
        text(
            "OEBPS/text/copyright.xhtml",
            page(
                "Copyright",
                &format!(
                    "<div class=\"copyright-page\"><p>{}</p></div>",
                    with_breaks(&book.copyright)
                ),
                "copyright-page-body",
            ),
        );
    }
    if !book.dedication.is_empty() {
        text(
            "OEBPS/text/dedication.xhtml",
            page(
                "Dedication",
                &format!(
                    "<div class=\"dedication\"><p>{}</p></div>",
                    with_breaks(&book.dedication)
                ),
                "dedication-page",
            ),
        );
    }
    if contents {
        text("OEBPS/text/contents.xhtml", contents_page(&items));
    }
    for (i, p) in book.front_matter.iter().enumerate() {
        text(
            &format!("OEBPS/text/front-matter-{}.xhtml", i + 1),
            matter_page(p, "front"),
        );
    }
    let notes = resolve_book_footnotes(book, options.footnotes_restart_per_chapter);
    for c in 0..book.chapters.len() {
        text(
            &format!("OEBPS/text/chapter-{}.xhtml", c + 1),
            chapter_page(book, options, c, &images, &notes),
        );
    }
    for (i, p) in book.back_matter.iter().enumerate() {
        text(
            &format!("OEBPS/text/back-matter-{}.xhtml", i + 1),
            matter_page(p, "back"),
        );
    }
    if let Some(bibliography) = &bibliography {
        text(
            "OEBPS/text/bibliography.xhtml",
            bibliography_page(bibliography),
        );
    }
    Ok(files)
}

/// The EPUB with a given identifier and modification time.
pub fn export_epub_with(
    book: &CompiledBook,
    options: &PublishOptions,
    identifier: &str,
    modified: &str,
) -> Result<Export, EpubError> {
    let files = epub_entries(book, options, identifier, modified)?;
    let mut zip = craft_zip::Writer::new();
    for (name, bytes) in &files {
        let compression = if name == "mimetype" {
            craft_zip::Compression::Stored
        } else {
            craft_zip::Compression::Deflated
        };
        zip.add(name, bytes, compression)
            .map_err(|e| EpubError::Zip(e.to_string()))?;
    }
    let bytes = zip.finish().map_err(|e| EpubError::Zip(e.to_string()))?;
    Ok(Export {
        file_name: format!("{}.epub", slugify_filename(&book.title)),
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use base64::Engine;

    use super::epub_entries;
    use crate::publish::{CompiledBook, CompiledChapter, CompiledScene, PublishOptions};

    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    fn webp() -> String {
        let image = craft_image_codecs::Image::from_u8(
            2,
            2,
            craft_image_codecs::ChannelLayout::Rgba,
            vec![200; 16],
        )
        .expect("image");
        let bytes = craft_image_codecs::encode(
            &image,
            craft_image_codecs::Format::WebP,
            &craft_image_codecs::EncodeOptions::default(),
        )
        .expect("webp");
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn images_are_packaged_and_webp_becomes_png() {
        let html = format!(
            r#"<p><img src="data:image/webp;base64,{}" alt="w"></p><p><img src="data:image/png;base64,{PNG}" alt="p" width="20"><img src="data:image/png;base64,{PNG}" alt="again"></p>"#,
            webp()
        );
        let scene = CompiledScene {
            id: "s".into(),
            title: "S".into(),
            html,
            words: 0,
            footnotes: Vec::new(),
        };
        let chapter = CompiledChapter {
            id: "c".into(),
            label: "One".into(),
            title: "One".into(),
            scenes: vec![scene],
            words: 0,
        };
        let book = CompiledBook {
            title: "T".into(),
            subtitle: String::new(),
            author: String::new(),
            copyright: String::new(),
            dedication: String::new(),
            front_matter: Vec::new(),
            back_matter: Vec::new(),
            chapters: vec![chapter],
            words: 0,
            citations: Vec::new(),
            bibliography: None,
            cover: None,
        };
        let entries =
            epub_entries(&book, &PublishOptions::default(), "urn:uuid:ID", "M").expect("entries");
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            &names[..3],
            [
                "mimetype",
                "OEBPS/images/image-1.png",
                "OEBPS/images/image-2.png"
            ]
        );
        assert!(
            entries[1].1.starts_with(b"\x89PNG"),
            "the WebP was re-encoded as PNG"
        );
        assert_eq!(
            entries[2].1,
            base64::engine::general_purpose::STANDARD
                .decode(PNG)
                .expect("png"),
            "a PNG is copied as it is"
        );
        let text = |name: &str| {
            String::from_utf8_lossy(&entries.iter().find(|(n, _)| n == name).expect(name).1)
                .into_owned()
        };
        let opf = text("OEBPS/content.opf");
        assert!(
            opf.contains(
                r#"<item id="image-2" href="images/image-2.png" media-type="image/png" />"#
            ),
            "{opf}"
        );
        let chapter = text("OEBPS/text/chapter-1.xhtml");
        assert!(
            chapter.contains(r#"<img src="../images/image-1.png" alt="w" />"#),
            "{chapter}"
        );
        assert!(chapter.contains(r#"<img src="../images/image-2.png" alt="p" width="20" /><img src="../images/image-2.png" alt="again" />"#), "{chapter}");
    }
}
