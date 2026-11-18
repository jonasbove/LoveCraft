//! Screenplays out as Fountain (ProseCraft `src/lib/screenplay/export-fountain.ts`, upstream
//! `7dae544`).

use super::fountain::{ScreenplayElement, TitlePage, serialize_fountain};
use super::layout::html_to_elements;
use crate::html::is_js_whitespace;
use crate::publish::{CompiledBook, Export, slugify_filename};

/// The project's title page: its title, and "Written by" its author when there is one.
fn project_title_page(book: &CompiledBook) -> TitlePage {
    let mut page = vec![("Title".to_owned(), book.title.clone())];
    if !book.author.trim_matches(is_js_whitespace).is_empty() {
        page.push(("Credit".into(), "Written by".into()));
        page.push(("Author".into(), book.author.clone()));
    }
    page
}

/// `bookToElements`: every scene's elements, in chapter order.
pub fn book_to_elements(book: &CompiledBook) -> Vec<ScreenplayElement> {
    book.chapters
        .iter()
        .flat_map(|c| c.scenes.iter())
        .flat_map(|s| html_to_elements(&s.html))
        .collect()
}

/// `documentToFountain`: one scene as Fountain, without a title page.
pub fn document_to_fountain(html: &str) -> String {
    serialize_fountain(&TitlePage::new(), &html_to_elements(html))
}

/// `bookToFountain`: the whole book as one Fountain file.
pub fn book_to_fountain(book: &CompiledBook) -> String {
    serialize_fountain(&project_title_page(book), &book_to_elements(book))
}

/// The Fountain export (`renderExport` for "fountain").
pub fn export_fountain(book: &CompiledBook) -> Export {
    Export {
        file_name: format!("{}.fountain", slugify_filename(&book.title)),
        bytes: book_to_fountain(book).into_bytes(),
    }
}
