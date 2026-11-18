//! prosecraft-rs core: the project model, the storage formats and the editing logic.
//!
//! Nothing here depends on a UI toolkit, and everything builds for `wasm32-unknown-unknown`.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod about;
pub mod calc;
pub mod chapter;
pub mod citations;
pub mod cite;
pub mod comments;
pub mod csl;
pub mod edit;
pub mod footnotes;
pub mod html;
pub mod import;
pub mod js;
pub mod labels;
pub mod library;
pub mod markdown;
pub mod picture;
pub mod project;
pub mod proofread;
pub mod publish;
pub mod research;
pub mod rich;
pub mod richtext;
pub mod screenplay;
pub mod search;
pub mod shortcuts;
pub mod snapshots;
pub mod split;
pub mod starters;
pub mod storage;
pub mod storyboard;
pub mod sync;
pub mod timeline;
pub mod undo;
pub mod wiki;
