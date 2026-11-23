//! Sync folders (ProseCraft `src/lib/storage/sync/`, upstream `7dae544`): the working set of
//! projects kept in step with a folder another program syncs (Syncthing, Nextcloud, Dropbox…).
//!
//! All three layouts are ported: one encrypted file (`prosecraft-sync.prosecraft`, an encrypted
//! backup), an encrypted folder (`vault.json` and one encrypted file per project, chapter, scene
//! and storyboard record), and the open folder of Markdown and JSON files.
//!
//! Every layout runs the same flow (`syncViaProvider`): connect (unlock or create the vault),
//! merge the folder's projects with the library's ([`merge::merge_projects`]), write the result
//! back, and delete projects deleted on this side.

pub mod foreign;
pub mod layout;
pub mod memory;
pub mod merge;
pub mod open_layout;
pub mod providers;
pub mod state;

use crate::js::JsValue;
use crate::storage::bundle::BundleError;
use crate::storage::crypto::CryptoError;

pub use merge::{MergeOutcome, SyncStamp, merge_projects};
pub use open_layout::OpenFolderProvider;
pub use providers::{EncryptedFileProvider, FolderProvider, SyncProvider, sync_via};

/// Root-level names that identify each layout (`files.ts`).
pub const VAULT_FILE: &str = "vault.json";
pub const PROJECTS_DIR: &str = "projects";
pub const BUNDLE_FILE: &str = "prosecraft-sync.prosecraft";
pub const OPEN_MARKER_FILE: &str = "prosecraft-folder.json";

/// How a sync folder is laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SyncMode {
    #[default]
    EncryptedFile,
    OpenFolder,
    EncryptedFolder,
}

impl SyncMode {
    pub fn id(self) -> &'static str {
        match self {
            Self::EncryptedFile => "encrypted-file",
            Self::OpenFolder => "open-folder",
            Self::EncryptedFolder => "encrypted-folder",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [Self::EncryptedFile, Self::OpenFolder, Self::EncryptedFolder]
            .into_iter()
            .find(|m| m.id() == id)
    }
}

/// Why a sync failed. The messages are ProseCraft's.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("{0}")]
    Folder(String),
    #[error("{0}")]
    Io(String),
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    #[error(transparent)]
    Bundle(#[from] BundleError),
}

/// One entry of a directory listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub is_directory: bool,
}

/// The filesystem surface the layouts need (`SyncFileSystem`). Paths are relative to the
/// sync folder and use `/`; each implementation resolves them against its own root.
pub trait SyncFileSystem {
    /// The entries of a directory; a missing directory lists as empty.
    fn list_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, SyncError>;
    fn read_text(&mut self, path: &str) -> Result<String, SyncError>;
    fn write_text(&mut self, path: &str, content: &str) -> Result<(), SyncError>;
    fn create_dir(&mut self, path: &str) -> Result<(), SyncError>;
    /// Removes a file or a directory with everything in it; a missing path is not an error.
    fn remove(&mut self, path: &str) -> Result<(), SyncError>;
    fn exists(&mut self, path: &str) -> Result<bool, SyncError>;
    /// Moves a file, replacing the target (used to replace the single sync file atomically).
    fn rename(&mut self, from: &str, to: &str) -> Result<(), SyncError>;
}

/// What a sync did (`SyncResult`).
#[derive(Clone, Debug, PartialEq)]
pub struct SyncResult {
    /// The merged working set the library should now hold.
    pub projects: Vec<JsValue>,
    /// The folder was set up by this sync.
    pub created: bool,
    pub imported: usize,
    pub exported: usize,
    pub removed: usize,
    pub conflicts: usize,
    /// The stamps to remember for the next sync, as a JSON object (id → stamp).
    pub last_synced: JsValue,
    /// Another sync tool's conflict copies found in the folder (left as they are).
    pub conflict_files: Vec<String>,
}
