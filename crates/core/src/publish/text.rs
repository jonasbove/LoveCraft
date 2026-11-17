//! `exportText`: the book as plain text.

use std::collections::HashMap;

use super::citations::resolve_book_citations;
use super::footnotes::{ResolvedFootnote, resolve_book_footnotes};
use super::{CompiledBook, Export, MatterPage, PublishOptions, SceneSeparator, slugify_filename};
use crate::html::{PlainTextOptions, html_to_plain_text};

fn separator_text(separator: SceneSeparator) -> &'static str {
    match separator {
        SceneSeparator::Hash => "#",
        SceneSeparator::Asterisks => "* * *",
        SceneSeparator::Line => "--------",
        SceneSeparator::Blank | SceneSeparator::None => "",
    }
}

/// A "Notes" block: `[n] text` per note.
fn notes_block(notes: &[ResolvedFootnote], uppercase: bool) -> String {
    let mut lines = vec![if uppercase { "NOTES" } else { "Notes" }.to_owned()];
    for note in notes {
        let text = html_to_plain_text(&note.html, &PlainTextOptions::default());
        lines.push(if text.is_empty() {
            format!("[{}]", note.number)
        } else {
            format!("[{}] {text}", note.number)
        });
    }
    lines.join("\n")
}

/// A matter page: its title and body, blank parts left out.
pub(super) fn matter_text(page: &MatterPage, title: String) -> String {
    [title, page.body.clone()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// `exportText`, byte for byte.
pub fn export_text(book: &CompiledBook, options: &PublishOptions) -> Export {
    let book = &resolve_book_citations(book);
    let upper = options.chapter_heading_uppercase;
    let restart = options.footnotes_restart_per_chapter;
    let notes = resolve_book_footnotes(book, restart);
    let mut blocks: Vec<String> = Vec::new();
    if options.include_title_page {
        blocks.push(book.title.clone());
        if !book.subtitle.is_empty() {
            blocks.push(book.subtitle.clone());
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
        blocks.push(book.dedication.clone());
    }
    if options.include_table_of_contents && !book.chapters.is_empty() {
        let mut lines = vec![if upper { "CONTENTS" } else { "Contents" }.to_owned()];
        lines.extend(
            book.chapters
                .iter()
                .enumerate()
                .map(|(i, c)| format!("{}. {}", i + 1, c.label)),
        );
        blocks.push(lines.join("\n"));
    }
    for page in &book.front_matter {
        blocks.push(matter_text(page, page.title.clone()));
    }
    for (c, chapter) in book.chapters.iter().enumerate() {
        if options.include_chapter_titles {
            blocks.push(if upper {
                chapter.label.to_uppercase()
            } else {
                chapter.label.clone()
            });
        }
        let mut scenes: Vec<String> = Vec::new();
        for (s, scene) in chapter.scenes.iter().enumerate() {
            let numbers: Option<HashMap<String, usize>> = notes
                .by_scene
                .get(&(c, s))
                .map(|m| m.iter().map(|(id, n)| (id.clone(), n.number)).collect());
            let text = html_to_plain_text(
                &scene.html,
                &PlainTextOptions {
                    footnote_numbers: numbers,
                    ..Default::default()
                },
            );
            let mut parts = Vec::new();
            if options.include_scene_titles {
                parts.push(scene.title.clone());
            }
            parts.push(text);
            scenes.push(
                parts
                    .into_iter()
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            );
        }
        let separator = separator_text(options.scene_separator);
        let glue = if separator.is_empty() {
            "\n\n".to_owned()
        } else {
            format!("\n\n{separator}\n\n")
        };
        blocks.push(scenes.join(&glue));
        if restart && !notes.by_chapter[c].is_empty() {
            blocks.push(notes_block(&notes.by_chapter[c], upper));
        }
    }
    for page in &book.back_matter {
        blocks.push(matter_text(page, page.title.clone()));
    }
    if !restart && !notes.all.is_empty() {
        blocks.push(notes_block(&notes.all, upper));
    }
    let text = format!(
        "{}\n",
        blocks
            .into_iter()
            .filter(|b| !b.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    );
    Export {
        file_name: format!("{}.txt", slugify_filename(&book.title)),
        bytes: text.into_bytes(),
    }
}
