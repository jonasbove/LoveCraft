//! An in-memory [`SyncFileSystem`] (`memory-fs.ts`), for tests and previews.

use std::collections::{BTreeMap, BTreeSet};

use super::{DirEntry, SyncError, SyncFileSystem};

fn normalize(path: &str) -> String {
    path.split('/')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryFs {
    files: BTreeMap<String, String>,
    dirs: BTreeSet<String>,
}

impl MemoryFs {
    pub fn new() -> Self {
        Self::default()
    }

    /// A folder holding these files (path → content).
    pub fn with_files(files: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut fs = Self::new();
        for (path, content) in files {
            let path = normalize(&path);
            fs.create_ancestors(&path);
            fs.files.insert(path, content);
        }
        fs
    }

    /// Every file, path → content.
    pub fn files(&self) -> &BTreeMap<String, String> {
        &self.files
    }

    fn create_ancestors(&mut self, path: &str) {
        let parts: Vec<&str> = path.split('/').collect();
        for end in 1..parts.len() {
            self.dirs.insert(parts[..end].join("/"));
        }
    }
}

impl SyncFileSystem for MemoryFs {
    fn list_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, SyncError> {
        let base = normalize(path);
        let prefix = if base.is_empty() {
            String::new()
        } else {
            format!("{base}/")
        };
        let mut entries: BTreeMap<String, bool> = BTreeMap::new();
        for full in self.files.keys().chain(self.dirs.iter()) {
            let Some(rest) = full.strip_prefix(&prefix) else {
                continue;
            };
            if rest.is_empty() {
                continue;
            }
            match rest.split_once('/') {
                Some((first, _)) => {
                    entries.insert(first.to_owned(), true);
                }
                None => {
                    let is_dir = self.dirs.contains(&format!("{prefix}{rest}"));
                    entries.entry(rest.to_owned()).or_insert(is_dir);
                }
            }
        }
        Ok(entries
            .into_iter()
            .map(|(name, is_directory)| DirEntry { name, is_directory })
            .collect())
    }

    fn read_text(&mut self, path: &str) -> Result<String, SyncError> {
        self.files
            .get(&normalize(path))
            .cloned()
            .ok_or_else(|| SyncError::Io(format!("Missing file: {path}")))
    }

    fn write_text(&mut self, path: &str, content: &str) -> Result<(), SyncError> {
        let target = normalize(path);
        self.create_ancestors(&target);
        self.files.insert(target, content.to_owned());
        Ok(())
    }

    fn create_dir(&mut self, path: &str) -> Result<(), SyncError> {
        let target = normalize(path);
        self.create_ancestors(&target);
        if !target.is_empty() {
            self.dirs.insert(target);
        }
        Ok(())
    }

    fn remove(&mut self, path: &str) -> Result<(), SyncError> {
        let target = normalize(path);
        let prefix = format!("{target}/");
        self.files
            .retain(|k, _| *k != target && !k.starts_with(&prefix));
        self.dirs
            .retain(|k| *k != target && !k.starts_with(&prefix));
        Ok(())
    }

    fn exists(&mut self, path: &str) -> Result<bool, SyncError> {
        let target = normalize(path);
        let prefix = format!("{target}/");
        Ok(self.files.contains_key(&target)
            || self.dirs.contains(&target)
            || self.files.keys().any(|k| k.starts_with(&prefix)))
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), SyncError> {
        let value = self
            .files
            .remove(&normalize(from))
            .ok_or_else(|| SyncError::Io(format!("Missing file: {from}")))?;
        let target = normalize(to);
        self.create_ancestors(&target);
        self.files.insert(target, value);
        Ok(())
    }
}
