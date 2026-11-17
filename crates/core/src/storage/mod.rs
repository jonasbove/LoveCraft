//! ProseCraft's storage formats: the stored project envelope, encryption, and backups.
//!
//! Sync folders (open, encrypted and single-file) build on these in later modules.

pub mod bundle;
pub mod crypto;
pub mod schema;
