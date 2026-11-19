//! The relation network, character sheets and story data (ProseCraft#31, upstream `bcb884d`):
//! typed relations between Wiki pages, where pages appear in the story, sheet templates, the
//! graph's view model and its deterministic layout.

pub mod edit;
pub mod graph;
pub mod layout;
pub mod meta;
pub mod relations;
pub mod templates;
pub mod tile;

/// `localeCompare` for ordering suggestion keys. craft-i18n's root collation compares runs of
/// digits by value, which plain `localeCompare` doesn't; for page ids this only changes the
/// order in which equally strong suggestions are drawn.
pub(crate) fn locale_compare(a: &str, b: &str) -> std::cmp::Ordering {
    crate::import::markdown_folder::natural_compare(a, b)
}
