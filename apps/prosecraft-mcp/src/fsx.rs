//! Files as the TypeScript server touches them through `node:fs` and `node:path`: paths joined and
//! normalized like `path.join`, errors worded like Node's (`ENOENT: no such file or directory,
//! open '…'`), atomic replacement, and the `.prosecraft-mcp.lock` advisory lock that keeps two
//! servers, TypeScript or Rust, from writing at once.

use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::path::Path;

use crate::env::Env;

pub const LOCK_FILE: &str = ".prosecraft-mcp.lock";
/// A lock older than this was left behind by an interrupted run.
pub const LOCK_STALE_MS: f64 = 30_000.0;

/// `path.join(...parts)`.
pub fn join(parts: &[&str]) -> String {
    let joined = parts
        .iter()
        .filter(|p| !p.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(SEPARATOR);
    if joined.is_empty() {
        return ".".into();
    }
    normalize(&joined)
}

#[cfg(windows)]
const SEPARATOR: &str = "\\";
#[cfg(not(windows))]
const SEPARATOR: &str = "/";

/// `path.normalize` (POSIX): `.` dropped, `..` resolved, repeated separators collapsed, a
/// trailing separator kept.
#[cfg(not(windows))]
pub fn normalize(path: &str) -> String {
    let absolute = path.starts_with('/');
    let trailing = path.ends_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.last().is_some_and(|last| *last != "..") {
                    segments.pop();
                } else if !absolute {
                    segments.push("..");
                }
            }
            other => segments.push(other),
        }
    }
    let mut out = segments.join("/");
    if out.is_empty() && !absolute {
        out.push('.');
    }
    if trailing && !out.is_empty() && out != "." {
        out.push('/');
    }
    if absolute {
        out.insert(0, '/');
    }
    out
}

/// On Windows the path is left to the platform; `path.join` there resolves the same parts.
#[cfg(windows)]
pub fn normalize(path: &str) -> String {
    path.to_owned()
}

/// libuv's code and text for an I/O error, as Node puts them in an error message.
fn code(error: &io::Error) -> Option<(&'static str, &'static str)> {
    #[cfg(unix)]
    if let Some(errno) = error.raw_os_error() {
        // EPERM and EACCES share `PermissionDenied`.
        if errno == 1 {
            return Some(("EPERM", "operation not permitted"));
        }
    }
    Some(match error.kind() {
        ErrorKind::NotFound => ("ENOENT", "no such file or directory"),
        ErrorKind::PermissionDenied => ("EACCES", "permission denied"),
        ErrorKind::AlreadyExists => ("EEXIST", "file already exists"),
        ErrorKind::IsADirectory => ("EISDIR", "illegal operation on a directory"),
        ErrorKind::NotADirectory => ("ENOTDIR", "not a directory"),
        ErrorKind::StorageFull => ("ENOSPC", "no space left on device"),
        ErrorKind::ReadOnlyFilesystem => ("EROFS", "read-only file system"),
        ErrorKind::ResourceBusy => ("EBUSY", "resource busy or locked"),
        ErrorKind::InvalidFilename => ("ENAMETOOLONG", "name too long"),
        ErrorKind::DirectoryNotEmpty => ("ENOTEMPTY", "directory not empty"),
        ErrorKind::CrossesDevices => ("EXDEV", "cross-device link not permitted"),
        ErrorKind::InvalidInput => ("EINVAL", "invalid argument"),
        _ => return None,
    })
}

/// A Node `fs` error message: `CODE: text, syscall 'path'`.
pub fn node_error(error: &io::Error, syscall: &str, path: Option<&str>) -> String {
    let Some((code, text)) = code(error) else {
        return error.to_string();
    };
    match path {
        Some(path) => format!("{code}: {text}, {syscall} '{path}'"),
        None => format!("{code}: {text}, {syscall}"),
    }
}

/// Largest file read. The open folder holds manuscripts as text; nothing in it comes near this.
const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// `fs.readFile(path, "utf8")`: invalid UTF-8 becomes U+FFFD, a byte order mark is kept.
pub fn read_text(path: &str) -> Result<String, String> {
    let file = File::open(path).map_err(|e| node_error(&e, "open", Some(path)))?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| node_error(&e, "read", None))?;
    // Refused rather than cut: a write tool would put the cut text back on disk.
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(format!("File is larger than 512 MiB: {path}"));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Writes through a sibling temp file renamed over the target, so a crash or a full disk leaves
/// the previous content in place (`writeFileAtomic`).
pub fn write_atomic(path: &str, content: &str) -> Result<(), String> {
    craft_atomic::atomic_write(Path::new(path), content.as_bytes())
        .map_err(|e| node_error(&e, "rename", Some(path)))
}

/// `fs.mkdir(path, { recursive: true })`.
pub fn mkdir_all(path: &str) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| node_error(&e, "mkdir", Some(path)))
}

/// `fs.rm(path, { force: true })` for a file: a missing file is not an error.
pub fn remove_file(path: &str) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(node_error(&e, "rm", Some(path))),
    }
}

/// One directory entry: its name and whether it is a directory (not following links, as a
/// `Dirent` reports it).
pub fn read_dir(path: &str) -> Result<Vec<(String, bool)>, io::Error> {
    let mut out = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        out.push((entry.file_name().to_string_lossy().into_owned(), is_dir));
    }
    Ok(out)
}

/// The write lock held for one tool call.
pub struct WriteLock {
    path: String,
    token: String,
}

fn lock_busy(lock_path: &str) -> String {
    format!(
        "Another ProseCraft MCP write is in progress ({lock_path}). Retry in a moment; a lock older than {}s is treated as stale.",
        LOCK_STALE_MS / 1000.0
    )
}

/// Milliseconds since the epoch the file was last modified, with the fraction Node keeps.
fn modified_ms(path: &str) -> Option<f64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let since = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(since.as_secs_f64() * 1000.0)
}

/// `acquireWriteLock`: creates the lock file exclusively with `pid:uuid` in it. A fresh lock
/// fails the call; one older than 30 s is removed and the creation tried once more.
pub fn acquire_lock(root: &str, env: &mut dyn Env) -> Result<WriteLock, String> {
    let path = join(&[root, LOCK_FILE]);
    let token = format!("{}:{}", std::process::id(), env.uuid()?);
    for _ in 0..2 {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(token.as_bytes())
                    .map_err(|e| node_error(&e, "write", None))?;
                return Ok(WriteLock { path, token });
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(node_error(&e, "open", Some(&path))),
        }
        if let Some(mtime) = modified_ms(&path)
            && env.now() - mtime <= LOCK_STALE_MS
        {
            return Err(lock_busy(&path));
        }
        remove_file(&path)?;
    }
    Err(lock_busy(&path))
}

/// `releaseWriteLock`: removes the lock only while it still holds this call's token.
pub fn release_lock(lock: WriteLock) {
    if read_text(&lock.path).is_ok_and(|text| text == lock.token) {
        // Already removed, or taken over as stale: nothing to release.
        let _ = fs::remove_file(&lock.path);
    }
}

/// `withWriteLock`.
pub fn with_lock<T>(
    root: &str,
    env: &mut dyn Env,
    run: impl FnOnce(&mut dyn Env) -> Result<T, String>,
) -> Result<T, String> {
    let lock = acquire_lock(root, env)?;
    let result = run(env);
    release_lock(lock);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn joins_like_node() {
        assert_eq!(join(&["/a/b/", "projects", "x"]), "/a/b/projects/x");
        assert_eq!(join(&["./root", "projects"]), "root/projects");
        assert_eq!(join(&["a", "..", "..", "b"]), "../b");
        assert_eq!(join(&["/", "..", "b"]), "/b");
    }

    #[test]
    fn errors_read_like_node() {
        let missing = io::Error::from(ErrorKind::NotFound);
        assert_eq!(
            node_error(&missing, "open", Some("/x/y.md")),
            "ENOENT: no such file or directory, open '/x/y.md'"
        );
        let dir = io::Error::from(ErrorKind::IsADirectory);
        assert_eq!(
            node_error(&dir, "read", None),
            "EISDIR: illegal operation on a directory, read"
        );
    }
}
