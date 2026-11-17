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
