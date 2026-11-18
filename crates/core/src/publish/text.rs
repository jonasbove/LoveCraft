//! `exportText`: the book as plain text.

use std::collections::HashMap;

use super::compile::citation_fallback;
use super::{CompiledBook, Export, PublishOptions, SceneSeparator, slugify_filename};
use crate::html::{PlainTextOptions, html_to_plain_text};

fn plain(html: &str) -> String {
    html_to_plain_text(html, &PlainTextOptions::default())
}

fn separator_text(separator: SceneSeparator) -> &'static str {
    match separator {
        SceneSeparator::Hash => "#",
        SceneSeparator::Asterisks => "* * *",
        SceneSeparator::Line => "--------",
        SceneSeparator::Blank | SceneSeparator::None => "",
    }
}

/// A "Notes" block: `[n] text` per note.
fn notes_block(notes: &[(usize, String)], uppercase: bool) -> String {
    let mut lines = vec![if uppercase { "NOTES" } else { "Notes" }.to_owned()];
    for (number, html) in notes {
        let text = plain(html);
        lines.push(if text.is_empty() {
            format!("[{number}]")
        } else {
            format!("[{number}] {text}")
        });
    }
    lines.join("\n")
}

/// `exportText`, byte for byte.
pub fn export_text(book: &CompiledBook, options: &PublishOptions) -> Export {
    let upper = options.chapter_heading_uppercase;
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
    let matter = |page: &super::MatterPage| {
        [page.title.as_str(), page.body.as_str()]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    for page in &book.front_matter {
        blocks.push(matter(page));
    }

    // Footnotes are numbered in compile order (`resolveBookFootnotes`).
    let mut all_notes: Vec<(usize, String)> = Vec::new();
    let mut index = 0;
    for chapter in &book.chapters {
        if options.include_chapter_titles {
            blocks.push(if upper {
                chapter.label.to_uppercase()
            } else {
                chapter.label.clone()
            });
        }
        let mut chapter_notes: Vec<(usize, String)> = Vec::new();
        let mut scenes: Vec<String> = Vec::new();
        for scene in &chapter.scenes {
            let mut numbers = HashMap::new();
            for note in &scene.footnotes {
                if numbers.contains_key(&note.id) {
                    continue;
                }
                index += 1;
                let number = if options.footnotes_restart_per_chapter {
                    chapter_notes.len() + 1
                } else {
                    index
                };
                numbers.insert(note.id.clone(), number);
                chapter_notes.push((number, note.html.clone()));
                all_notes.push((number, note.html.clone()));
            }
            let text_options = PlainTextOptions {
                footnote_numbers: (!numbers.is_empty()).then_some(numbers),
                citations: Some(citation_fallback),
                ..Default::default()
            };
            let text = html_to_plain_text(&scene.html, &text_options);
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
        if options.footnotes_restart_per_chapter && !chapter_notes.is_empty() {
            blocks.push(notes_block(&chapter_notes, upper));
        }
    }
    for page in &book.back_matter {
        blocks.push(matter(page));
    }
    if !options.footnotes_restart_per_chapter && !all_notes.is_empty() {
        blocks.push(notes_block(&all_notes, upper));
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
