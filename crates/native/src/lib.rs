//! LoveCraft on the desktop: where the library lives, and saving it without ever losing it.
//!
//! The library is one file, `library.prosecraft` (a plain ProseCraft backup, see
//! `prosecraft_core::library`), in the platform's data folder (`~/.local/share/prosecraft-rs` on
//! Linux). Every save goes through `craft_atomic`: the new text is written beside the file,
//! flushed and renamed over it, so a crash or a full disk leaves the previous library intact. The
//! version before each save is kept as `library.prosecraft.bak`.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod craftcloud;
pub mod sync_fs;
pub mod wiki_files;

use std::io;
use std::path::{Path, PathBuf};

/// The app's folder name under the platform's data folder. It keeps the app's former name
/// (ProseCraft-rs), so an updated app finds the library where 0.1 kept it.
pub const APP_DIR: &str = prosecraft_core::about::ids::DATA_DIR;
/// The library file in it.
pub const LIBRARY_FILE: &str = "library.prosecraft";
/// Set to a folder to keep the library somewhere else (tests, portable use).
pub const DATA_DIR_ENV: &str = prosecraft_core::about::ids::DATA_DIR_ENV;
/// A library far larger than this isn't one; it is never read into memory.
pub const MAX_LIBRARY_BYTES: u64 = 1024 * 1024 * 1024;

/// The folder the library lives in.
pub fn data_dir() -> Option<PathBuf> {
    match std::env::var_os(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir)),
        None => dirs::data_dir().map(|d| d.join(APP_DIR)),
    }
}

/// The device's settings, beside the library: one JSON object of ProseCraft's `localStorage`
/// keys (`prosecraft-theme`, `prosecraft-sync-state`…) to their string values.
pub const SETTINGS_FILE: &str = "settings.json";

fn read_settings(dir: &Path) -> prosecraft_core::js::JsObject {
    let path = dir.join(SETTINGS_FILE);
    // Settings are a few kilobytes; anything far larger isn't them.
    let too_big = std::fs::metadata(&path)
        .map(|m| m.len() > 16 * 1024 * 1024)
        .unwrap_or(true);
    if too_big {
        return prosecraft_core::js::JsObject::new();
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| prosecraft_core::js::parse(&text).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

/// One setting's value.
pub fn load_setting(dir: &Path, key: &str) -> Option<String> {
    read_settings(dir)
        .get(key)
        .and_then(|v| v.as_js_string())
        .map(|s| s.to_string_lossy())
}

/// Sets (or, with `None`, removes) one setting, writing the file atomically.
pub fn save_setting(dir: &Path, key: &str, value: Option<&str>) -> io::Result<()> {
    let mut settings = read_settings(dir);
    match value {
        Some(v) => settings.insert(key, prosecraft_core::js::JsValue::str(v)),
        None => {
            settings.remove(key);
        }
    }
    std::fs::create_dir_all(dir)?;
    let text =
        prosecraft_core::js::stringify_pretty(&prosecraft_core::js::JsValue::Object(settings));
    craft_atomic::atomic_write(&dir.join(SETTINGS_FILE), text.as_bytes())
}

/// The library file on disk.
#[derive(Clone, Debug)]
pub struct LibraryFile {
    path: PathBuf,
}

impl LibraryFile {
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            path: dir.join(LIBRARY_FILE),
        }
    }

    /// The library in the platform's data folder.
    pub fn default_location() -> Option<Self> {
        data_dir().map(|d| Self::in_dir(&d))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn backup_path(&self) -> PathBuf {
        let mut name = self
            .path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();
        name.push(".bak");
        self.path.with_file_name(name)
    }

    /// The library's text, or `None` when there is none yet.
    pub fn load(&self) -> io::Result<Option<String>> {
        match std::fs::metadata(&self.path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
            Ok(m) if m.len() > MAX_LIBRARY_BYTES => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "the library file is far too large",
                ));
            }
            Ok(_) => {}
        }
        let bytes = std::fs::read(&self.path)?;
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "the library file isn't text"))
    }

    /// Saves `text` atomically, keeping the current file as the backup first.
    pub fn save(&self, text: &str) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if self.path.is_file() {
            let current = std::fs::read(&self.path)?;
            craft_atomic::atomic_write(&self.backup_path(), &current)?;
        }
        craft_atomic::atomic_write(&self.path, text.as_bytes())
    }
}

/// The operating system's random numbers, for new device ids and encryption.
pub use prosecraft_core::storage::crypto::OsRandom;

/// A backup's bytes for opening (Open backup…, a dropped file, one CraftCloud synced here),
/// or why it can't be opened, in words for the status line. The file is only read.
pub fn read_backup_file(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let meta = std::fs::metadata(path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => format!(
            "Couldn't open {name}: it isn't on this computer (any more). If CraftCloud is still downloading it, try again in a moment."
        ),
        _ => format!("Couldn't open {name}: {e}"),
    })?;
    if !meta.is_file() {
        return Err(format!("Couldn't open {name}: it isn't a file."));
    }
    if meta.len() > max {
        return Err(format!(
            "Couldn't open {name}: it is far too large to be a ProseCraft backup."
        ));
    }
    std::fs::read(path).map_err(|e| format!("Couldn't open {name}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::LibraryFile;

    /// LoveCraft was ProseCraft-rs until 0.2.0: an updated app must find the library where 0.1
    /// kept it, so the folder keeps the former name.
    #[test]
    fn the_library_folder_keeps_the_former_name() {
        assert_eq!(super::APP_DIR, "prosecraft-rs");
        assert_eq!(super::DATA_DIR_ENV, "PROSECRAFT_RS_DATA_DIR");
        if std::env::var_os(super::DATA_DIR_ENV).is_none()
            && let Some(dir) = super::data_dir()
        {
            assert_eq!(
                dir.file_name().and_then(|n| n.to_str()),
                Some("prosecraft-rs")
            );
        }
    }

    #[test]
    fn a_backup_that_cannot_be_read_says_why() {
        let dir = tempfile::tempdir().expect("dir");
        let gone = dir.path().join("Book.prosecraft");
        let err = super::read_backup_file(&gone, 100).unwrap_err();
        assert!(
            err.starts_with("Couldn't open Book.prosecraft: it isn't on this computer"),
            "{err}"
        );
        std::fs::write(&gone, "0123456789").expect("write");
        assert_eq!(
            super::read_backup_file(&gone, 100).expect("read"),
            b"0123456789"
        );
        let err = super::read_backup_file(&gone, 5).unwrap_err();
        assert!(err.contains("far too large"), "{err}");
        let err = super::read_backup_file(dir.path(), 100).unwrap_err();
        assert!(err.contains("isn't a file"), "{err}");
    }

    #[test]
    fn saves_atomically_and_keeps_the_previous_version() {
        let dir = tempfile::tempdir().unwrap();
        let file = LibraryFile::in_dir(&dir.path().join("nested"));
        assert_eq!(file.load().unwrap(), None);
        file.save("first").unwrap();
        file.save("second").unwrap();
        assert_eq!(file.load().unwrap().as_deref(), Some("second"));
        let backup =
            std::fs::read_to_string(dir.path().join("nested/library.prosecraft.bak")).unwrap();
        assert_eq!(backup, "first");
        // No staging files are left behind.
        let names: Vec<_> = std::fs::read_dir(dir.path().join("nested"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
    }

    #[test]
    fn a_library_that_isnt_text_is_an_error_not_a_crash() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("library.prosecraft"), [0xff, 0xfe, 0x00]).unwrap();
        assert!(LibraryFile::in_dir(dir.path()).load().is_err());
    }
}
