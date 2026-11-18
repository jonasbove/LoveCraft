//! `resolveBookFootnotes`: footnote numbers across the book, or restarting each chapter.

use std::collections::HashMap;

use super::CompiledBook;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedFootnote {
    /// 1-based, unique across the book.
    pub index: usize,
    /// The number shown (continuous, or per chapter).
    pub number: usize,
    /// 0-based chapter index.
    pub chapter: usize,
    pub id: String,
    pub html: String,
}

pub struct BookFootnotes {
    /// Per scene (by chapter and scene index): footnote id → its resolved note.
    pub by_scene: HashMap<(usize, usize), HashMap<String, ResolvedFootnote>>,
    pub by_chapter: Vec<Vec<ResolvedFootnote>>,
    pub all: Vec<ResolvedFootnote>,
}

pub fn resolve_book_footnotes(book: &CompiledBook, restart_per_chapter: bool) -> BookFootnotes {
    let mut by_scene = HashMap::new();
    let mut by_chapter = Vec::new();
    let mut all = Vec::new();
    let mut index = 0;
    for (c, chapter) in book.chapters.iter().enumerate() {
        let mut chapter_notes: Vec<ResolvedFootnote> = Vec::new();
        for (s, scene) in chapter.scenes.iter().enumerate() {
            let mut map: HashMap<String, ResolvedFootnote> = HashMap::new();
            for note in &scene.footnotes {
                if map.contains_key(&note.id) {
                    continue;
                }
                index += 1;
                let number = if restart_per_chapter {
                    chapter_notes.len() + 1
                } else {
                    index
                };
                let resolved = ResolvedFootnote {
                    index,
                    number,
                    chapter: c,
                    id: note.id.clone(),
                    html: note.html.clone(),
                };
                map.insert(note.id.clone(), resolved.clone());
                chapter_notes.push(resolved.clone());
                all.push(resolved);
            }
            if !map.is_empty() {
                by_scene.insert((c, s), map);
            }
        }
        by_chapter.push(chapter_notes);
    }
    BookFootnotes {
        by_scene,
        by_chapter,
        all,
    }
}
