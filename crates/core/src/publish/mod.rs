//! Publishing: the book every exporter renders, and the exporters.
//!
//! Ported from ProseCraft `src/lib/publish/` (upstream `7dae544`): `types.ts`
//! (`PublishOptions`, `normalizePublishOptions`, `slugifyFilename`), `matter.ts`,
//! `compile.ts` (`compileBook`), `footnotes.ts` (`resolveBookFootnotes`) and `text.ts`
//! (`exportText`). Citation formatting isn't ported yet: citation markers are written as
//! ProseCraft writes sources it can't find, `[@key]`.

mod compile;
mod options;
mod text;

pub use compile::{
    CompiledBook, CompiledChapter, CompiledFootnote, CompiledScene, MatterPage, compile_book,
};
pub use options::{FORMATS, PublishOptions, SceneSeparator, slugify_filename};
pub use text::export_text;

/// A finished export: the file's name and its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    pub file_name: String,
    pub bytes: Vec<u8>,
}
