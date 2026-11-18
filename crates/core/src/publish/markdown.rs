//! `exportMarkdown`: the book as Markdown, with YAML front matter and Pandoc footnotes.

use super::citations::resolve_book_citations;
use super::footnotes::{ResolvedFootnote, resolve_book_footnotes};
use super::text::matter_text;
use super::{CompiledBook, Export, PublishOptions, SceneSeparator, slugify_filename};
use crate::html::sanitize::FOOTNOTE_REF_CLASS;
use crate::html::{Node, is_js_whitespace, parse_fragment};
use crate::js::{JsValue, stringify};
use crate::markdown::{footnote_definition, turndown_nodes};

fn label(note: &ResolvedFootnote, restart: bool, chapters: usize) -> String {
    if restart && chapters > 1 {
        format!("{}-{}", note.chapter + 1, note.number)
    } else {
        note.number.to_string()
    }
}

/// `renumberSceneHtmlForMarkdown`: each footnote marker takes its label; markers without a
/// note are removed.
fn renumber(nodes: &mut Vec<Node>, label_of: &dyn Fn(&str) -> Option<String>) {
    let mut i = 0;
    while i < nodes.len() {
        if let Node::Element(e) = &mut nodes[i] {
            if e.name == "sup"
                && e.has_class(FOOTNOTE_REF_CLASS)
                && e.attr("data-footnote-id").is_some()
            {
                let id = e.attr("data-footnote-id").unwrap_or_default().to_owned();
                match label_of(&id) {
                    Some(label) => {
                        for (key, value) in &mut e.attrs {
                            if key == "data-footnote-id" {
                                *value = label.clone();
                            }
                        }
                    }
                    None => {
                        nodes.remove(i);
                        continue;
                    }
                }
            } else {
                renumber(&mut e.children, label_of);
            }
        }
        i += 1;
    }
}

/// `normalizeMarkdown`: lines of only spaces and tabs emptied, runs of blank lines cut to
/// one, the ends trimmed.
fn normalize(markdown: &str) -> String {
    // `/^[ \t]+$/gm`, with JavaScript's line terminators.
    let mut emptied = String::new();
    let mut line = String::new();
    for c in markdown.chars() {
        if matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
            if !line.chars().all(|c| c == ' ' || c == '\t') {
                emptied.push_str(&line);
            }
            line.clear();
            emptied.push(c);
        } else {
            line.push(c);
        }
    }
    if !line.chars().all(|c| c == ' ' || c == '\t') {
        emptied.push_str(&line);
    }
    let mut out = String::new();
    let mut newlines = 0;
    for c in emptied.chars() {
        if c == '\n' {
            newlines += 1;
            continue;
        }
        out.push_str(&"\n".repeat(if newlines >= 3 { 2 } else { newlines }));
        newlines = 0;
        out.push(c);
    }
    out.push_str(&"\n".repeat(if newlines >= 3 { 2 } else { newlines }));
    out.trim_matches(is_js_whitespace).to_owned()
}

fn separator(separator: SceneSeparator) -> &'static str {
    match separator {
        SceneSeparator::Hash | SceneSeparator::Asterisks | SceneSeparator::Line => "* * *",
        SceneSeparator::Blank | SceneSeparator::None => "",
    }
}

/// `exportMarkdown`, byte for byte.
pub fn export_markdown(book: &CompiledBook, options: &PublishOptions) -> Export {
    let (book, bibliography) = resolve_book_citations(book, options);
    let book = &book;
    let restart = options.footnotes_restart_per_chapter;
    let notes = resolve_book_footnotes(book, restart);
    let chapters = book.chapters.len();
    let mut blocks: Vec<String> = Vec::new();
    if options.include_title_page {
        let yaml = |s: &str| stringify(&JsValue::str(s));
        let mut front = vec!["---".to_owned(), format!("title: {}", yaml(&book.title))];
        if !book.author.is_empty() {
            front.push(format!("author: {}", yaml(&book.author)));
        }
        front.push("---".to_owned());
        blocks.push(front.join("\n"));
        blocks.push(format!("# {}", book.title));
        if !book.subtitle.is_empty() {
            blocks.push(format!("_{}_", book.subtitle));
        }
        if !book.author.is_empty() {
            blocks.push(format!("by {}", book.author));
        }
        if !book.copyright.is_empty() && !options.include_copyright_page {
            blocks.push(book.copyright.clone());
        }
    }
    if options.include_copyright_page && !book.copyright.is_empty() {
        blocks.push(book.copyright.clone());
    }
    if !book.dedication.is_empty() {
        blocks.push(format!("> {}", book.dedication.replace('\n', "\n> ")));
    }
    if options.include_table_of_contents && !book.chapters.is_empty() {
        let mut lines = vec!["## Contents".to_owned()];
        lines.extend(book.chapters.iter().map(|c| format!("- {}", c.label)));
        blocks.push(lines.join("\n"));
    }
    let heading = |title: &str| {
        if title.is_empty() {
            String::new()
        } else {
            format!("## {title}")
        }
    };
    for page in &book.front_matter {
        blocks.push(matter_text(page, heading(&page.title)));
    }
    for (c, chapter) in book.chapters.iter().enumerate() {
        if options.include_chapter_titles {
            blocks.push(format!("## {}", chapter.label));
        }
        let mut scenes = Vec::new();
        for (s, scene) in chapter.scenes.iter().enumerate() {
            let mut parts = Vec::new();
            if options.include_scene_titles {
                parts.push(format!("### {}", scene.title));
            }
            let mut nodes = parse_fragment(&scene.html);
            if let Some(map) = notes
                .by_scene
                .get(&(c, s))
                .filter(|_| scene.html.contains(FOOTNOTE_REF_CLASS))
            {
                renumber(&mut nodes, &|id| {
                    map.get(id).map(|n| label(n, restart, chapters))
                });
            }
            parts.push(normalize(&turndown_nodes(nodes)));
            scenes.push(
                parts
                    .into_iter()
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            );
        }
        let separator = separator(options.scene_separator);
        let glue = if separator.is_empty() {
            "\n\n".to_owned()
        } else {
            format!("\n\n{separator}\n\n")
        };
        blocks.push(scenes.join(&glue));
    }
    for page in &book.back_matter {
        blocks.push(matter_text(page, heading(&page.title)));
    }
    if let Some(bibliography) = bibliography.filter(|b| !b.entries.is_empty()) {
        let items: Vec<String> = bibliography
            .entries
            .iter()
            .map(|e| format!("- {}", crate::markdown::html_to_inline_markdown(&e.html)))
            .collect();
        blocks.push(format!(
            "## {}\n\n{}",
            bibliography.heading,
            items.join("\n")
        ));
    }
    if !notes.all.is_empty() {
        let definitions: Vec<String> = notes
            .all
            .iter()
            .map(|n| footnote_definition(&label(n, restart, chapters), &n.html))
            .collect();
        blocks.push(definitions.join("\n"));
    }
    let markdown = format!(
        "{}\n",
        blocks
            .into_iter()
            .filter(|b| !b.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    );
    Export {
        file_name: format!("{}.md", slugify_filename(&book.title)),
        bytes: markdown.into_bytes(),
    }
}
