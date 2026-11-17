//! Citations as the exporters write them: `resolveBookCitations` (in
//! [`crate::citations::bibliography`]) for the book's style, language and bibliography setting.

use crate::citations::bibliography::{self, BookBibliography};

use super::{CompiledBook, PublishOptions};

/// The book with every citation resolved, and its bibliography when it has one.
pub fn resolve_book_citations(
    book: &CompiledBook,
    options: &PublishOptions,
) -> (CompiledBook, Option<BookBibliography>) {
    let (book, bibliography, _) = bibliography::resolve_book_citations(book, options);
    (book, bibliography)
}
