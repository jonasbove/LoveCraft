//! Publishing: the book every exporter renders, and the exporters.
//!
//! Ported from ProseCraft `src/lib/publish/` (upstream `7dae544`): `types.ts`
//! (`PublishOptions`, `normalizePublishOptions`, `slugifyFilename`), `matter.ts`,
//! `compile.ts` (`compileBook`), `footnotes.ts` (`resolveBookFootnotes`), `text.ts`
//! (`exportText`) and `markdown.ts` (`exportMarkdown`). Citation formatting isn't ported
//! yet: citation markers are written as ProseCraft writes sources it can't find, `[@key]`.

mod citations;
mod compile;
mod footnotes;
mod markdown;
mod options;
mod text;

pub use compile::{
    CompiledBook, CompiledChapter, CompiledFootnote, CompiledScene, MatterPage, compile_book,
};
pub use markdown::export_markdown;
pub use options::{FORMATS, PublishOptions, SceneSeparator, slugify_filename};
pub use text::export_text;

/// A finished export: the file's name and its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    pub file_name: String,
    pub bytes: Vec<u8>,
}
