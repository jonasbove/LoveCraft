//! The sync folder on disk: a [`SyncFileSystem`] rooted at the folder the person chose,
//! as ProseCraft's desktop `sync_*` commands and `TauriFileSystem` behave. Paths are
//! checked first: no empty path and no `.` or `..` segment, so a malformed path can never
//! reach a delete outside the folder.

use std::path::{Path, PathBuf};

use prosecraft_core::sync::{DirEntry, SyncError, SyncFileSystem};

pub struct FolderFs {
    root: PathBuf,
}

fn io(error: std::io::Error) -> SyncError {
    SyncError::Io(error.to_string())
}

impl FolderFs {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn resolve(&self, path: &str) -> Result<PathBuf, SyncError> {
        let segments: Vec<&str> = path.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
        if segments.is_empty() || segments.iter().any(|s| *s == "." || *s == "..") {
            return Err(SyncError::Io("Invalid sync path.".into()));
        }
        Ok(segments.iter().fold(self.root.clone(), |p, s| p.join(s)))
    }
}

impl SyncFileSystem for FolderFs {
    fn list_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, SyncError> {
        let dir = self.resolve(path)?;
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(io)? {
            let entry = entry.map_err(io)?;
            entries.push(DirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_directory: entry.file_type().map_err(io)?.is_dir(),
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    fn read_text(&mut self, path: &str) -> Result<String, SyncError> {
        std::fs::read_to_string(self.resolve(path)?).map_err(io)
    }

    fn write_text(&mut self, path: &str, content: &str) -> Result<(), SyncError> {
        let target = self.resolve(path)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::write(target, content).map_err(io)
    }

    fn create_dir(&mut self, path: &str) -> Result<(), SyncError> {
        std::fs::create_dir_all(self.resolve(path)?).map_err(io)
    }

    fn remove(&mut self, path: &str) -> Result<(), SyncError> {
        let target = self.resolve(path)?;
        // The link itself, never what it points to.
        match std::fs::symlink_metadata(&target) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io(e)),
            Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(&target).map_err(io),
            Ok(_) => std::fs::remove_file(&target).map_err(io),
        }
    }

    fn exists(&mut self, path: &str) -> Result<bool, SyncError> {
        Ok(self.resolve(path)?.exists())
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), SyncError> {
        let (source, target) = (self.resolve(from)?, self.resolve(to)?);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        if !source.exists() {
            return Err(SyncError::Io(format!("Missing file: {from}")));
        }
        // rename replaces the target atomically on Unix; Windows refuses, so remove it first.
        #[cfg(windows)]
        if target.exists() {
            std::fs::remove_file(&target).map_err(io)?;
        }
        std::fs::rename(source, target).map_err(io)
    }
}

#[cfg(test)]
mod tests {
    use prosecraft_core::sync::SyncFileSystem;

    use super::FolderFs;

    #[test]
    fn round_trips_and_tolerates_missing_paths() {
        let dir = tempfile::tempdir().expect("dir");
        let mut fs = FolderFs::new(dir.path());
        assert!(fs.list_dir("projects").expect("list").is_empty());
        fs.remove("nothing/here").expect("missing is fine");
        fs.write_text("projects/a/project.enc", "x").expect("write");
        assert_eq!(fs.read_text("projects/a/project.enc").expect("read"), "x");
        let entries = fs.list_dir("projects").expect("list");
        assert_eq!(
            (entries[0].name.as_str(), entries[0].is_directory),
            ("a", true)
        );
        fs.write_text("b.tmp", "new").expect("write");
        fs.write_text("b", "old").expect("write");
        fs.rename("b.tmp", "b").expect("rename");
        assert_eq!(fs.read_text("b").expect("read"), "new");
        assert!(!fs.exists("b.tmp").expect("exists"));
        fs.remove("projects").expect("remove");
        assert!(!fs.exists("projects/a/project.enc").expect("exists"));
    }

    #[test]
    fn refuses_paths_outside_the_folder() {
        let dir = tempfile::tempdir().expect("dir");
        let mut fs = FolderFs::new(dir.path().join("sync"));
        for path in ["", "/", "..", "projects/../..", "a/./b", "..\\x"] {
            assert!(fs.remove(path).is_err(), "{path:?}");
            assert!(fs.write_text(path, "x").is_err(), "{path:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn removing_a_symlink_leaves_its_target() {
        let dir = tempfile::tempdir().expect("dir");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).expect("dir");
        std::fs::write(outside.join("keep"), "k").expect("write");
        let root = dir.path().join("sync");
        std::fs::create_dir_all(root.join("projects")).expect("dir");
        std::os::unix::fs::symlink(&outside, root.join("projects/link")).expect("link");
        FolderFs::new(&root)
            .remove("projects/link")
            .expect("remove");
        assert!(outside.join("keep").exists());
    }
}
