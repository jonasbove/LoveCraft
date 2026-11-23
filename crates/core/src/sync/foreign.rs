//! Files that another sync program puts in a sync folder, which are never one of ProseCraft's
//! records: its conflict copies, its own folder and its files in progress.
//!
//! CraftCloud (`jonasbove/craftcloud`) is the sync program prosecraft-rs is made for. When a
//! file changed on two computers, it keeps the cloud's version under the original name and this
//! computer's under a conflict name, `doc-1 (conflict 2026-11-13 14.32 Studio).md` (a second
//! one on the same minute counts on: `… Studio 2).md`), and uploads both. It keeps its own
//! state in `.craftcloud/`, downloads into `.<name>.craftcloud-part`, and a save into the folder
//! goes through such a part file too. Syncthing (`doc-1.sync-conflict-20261113-143200-ABC.md`),
//! Nextcloud (`doc-1 (conflicted copy 2026-11-13 143200).md`) and Dropbox (`doc-1 (Jane's
//! conflicted copy 2026-11-13).md`) name theirs differently.
//!
//! The names prosecraft-rs writes are `safe_name`s: letters, digits, `.`, `_`, `-` and `~`,
//! never a space or a parenthesis, so a CraftCloud, Nextcloud or Dropbox conflict mark is never
//! one of its own. A Syncthing-like stamp or a `.craft_tmp` ending can be part of an id, so the
//! names of the records a project knows always count as its own ([`foreign`]).

use super::{SyncError, SyncFileSystem};

/// The name a CraftCloud conflict copy was made from: `doc-1 (conflict 2026-11-13 14.32
/// Studio).md` → `doc-1.md`. `None` for any other name.
pub fn craftcloud_original(name: &str) -> Option<String> {
    let start = name.find(" (conflict ")?;
    // The last `)`: a device name may hold one of its own.
    let end = start + name[start..].rfind(')')?;
    let original = format!("{}{}", &name[..start], &name[end + 1..]);
    (!original.is_empty()).then_some(original)
}

/// Whether a file or folder name is another sync program's conflict copy (CraftCloud's,
/// Syncthing's, Nextcloud's or Dropbox's).
pub fn is_conflict_copy(name: &str) -> bool {
    craftcloud_original(name).is_some() || name.contains("conflicted copy") || syncthing(name)
}

/// Syncthing's `.sync-conflict-<yyyymmdd>-<hhmmss>-<device>`: the full stamp, so that a record
/// id that merely contains the words isn't taken for one.
fn syncthing(name: &str) -> bool {
    let digits = |s: Option<&[u8]>| s.is_some_and(|d| d.iter().all(u8::is_ascii_digit));
    name.match_indices(".sync-conflict-").any(|(at, mark)| {
        let stamp = name.get(at + mark.len()..).unwrap_or_default().as_bytes();
        digits(stamp.get(..8))
            && stamp.get(8) == Some(&b'-')
            && digits(stamp.get(9..15))
            && stamp.get(15) == Some(&b'-')
            && stamp.get(16).is_some_and(u8::is_ascii_alphanumeric)
    })
}

/// Whether a name is a sync program's own and never synced content: CraftCloud's folder, a
/// download or save in progress (`*.craftcloud-part`), a Craft app's save in progress
/// (`*.craft_tmp`).
pub fn is_sync_program_file(name: &str) -> bool {
    name == ".craftcloud" || name.ends_with(".craftcloud-part") || name.ends_with(".craft_tmp")
}

/// Whether the open folder leaves this name alone: neither read as a record nor ever removed.
/// A name this app gave a record it knows is never left alone, whatever it looks like (an id
/// can end in `.craft_tmp` or hold a Syncthing-like stamp): see [`foreign`].
pub fn left_alone(name: &str) -> bool {
    is_conflict_copy(name) || is_sync_program_file(name)
}

/// [`left_alone`], unless `ours` says this app wrote the name for a record it knows.
pub fn foreign(name: &str, ours: impl Fn(&str) -> bool) -> bool {
    left_alone(name) && !ours(name)
}

/// How deep a project folder goes: `projects/<id>/<kind>/<file>`.
const MAX_DEPTH: usize = 4;

/// Removes what a layout wrote in `dir` (a project's folder) and keeps every name another sync
/// program owns (its conflict copies can be the only copy of an edit). Folders are removed only
/// once empty. The paths it kept.
pub(crate) fn remove_keeping_foreign(
    fs: &mut dyn SyncFileSystem,
    dir: &str,
) -> Result<Vec<String>, SyncError> {
    let mut kept = Vec::new();
    remove_in(fs, dir, 0, &mut kept)?;
    Ok(kept)
}

fn remove_in(
    fs: &mut dyn SyncFileSystem,
    dir: &str,
    depth: usize,
    kept: &mut Vec<String>,
) -> Result<(), SyncError> {
    for entry in fs.list_dir(dir)? {
        let path = format!("{dir}/{}", entry.name);
        if left_alone(&entry.name) || (entry.is_directory && depth + 1 >= MAX_DEPTH) {
            kept.push(path);
        } else if entry.is_directory {
            remove_in(fs, &path, depth + 1, kept)?;
        } else {
            fs.remove(&path)?;
        }
    }
    fs.remove_empty_dir(dir)
}

/// Whether a folder holds nothing but other sync programs' files (what is left of a project
/// deleted while a conflict copy was in it).
pub(crate) fn only_foreign(fs: &mut dyn SyncFileSystem, dir: &str) -> Result<bool, SyncError> {
    only_foreign_in(fs, dir, 0)
}

fn only_foreign_in(
    fs: &mut dyn SyncFileSystem,
    dir: &str,
    depth: usize,
) -> Result<bool, SyncError> {
    for entry in fs.list_dir(dir)? {
        if left_alone(&entry.name) {
            continue;
        }
        if !entry.is_directory || depth + 1 >= MAX_DEPTH {
            return Ok(false);
        }
        if !only_foreign_in(fs, &format!("{dir}/{}", entry.name), depth + 1)? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{craftcloud_original, is_conflict_copy, is_sync_program_file, left_alone};
    use crate::sync::layout::safe_name;

    #[test]
    fn craftcloud_conflict_copies_lead_back_to_their_original() {
        for (copy, original) in [
            ("doc-1 (conflict 2026-11-13 14.32 Studio).md", "doc-1.md"),
            ("doc-1 (conflict 2026-11-13 14.32 Studio 2).md", "doc-1.md"),
            (
                "chapter-1 (conflict 2026-11-13 14.32 Jo's Laptop).json",
                "chapter-1.json",
            ),
            (
                "rows (conflict 2026-11-13 14.32 Box (old)).json",
                "rows.json",
            ),
            ("project-1 (conflict 2026-11-13 14.32 Studio)", "project-1"),
            ("README (conflict 2026-11-13 14.32 Laptop)", "README"),
        ] {
            assert_eq!(
                craftcloud_original(copy).as_deref(),
                Some(original),
                "{copy}"
            );
            assert!(is_conflict_copy(copy) && left_alone(copy), "{copy}");
        }
        for plain in [
            "doc-1.md",
            "conflict.md",
            "a (conflicted).md",
            " (conflict x)",
        ] {
            assert_eq!(craftcloud_original(plain), None, "{plain}");
        }
    }

    #[test]
    fn other_programs_conflict_copies_and_craftclouds_own_files_are_known() {
        for name in [
            "doc-1.sync-conflict-20261113-143200-ABCDEFG.md",
            "doc-1 (conflicted copy 2026-11-13 143200).md",
            "doc-1 (Jane's conflicted copy 2026-11-13).md",
        ] {
            assert!(is_conflict_copy(name), "{name}");
        }
        for name in [
            ".craftcloud",
            ".doc-1.md.craftcloud-part",
            ".doc-1.md.d7fc8c633e524686-1794615066.craftcloud-part",
            ".ccpart-0123456789abcdef01234567.craftcloud-part",
            ".doc-1.1234-1.craft_tmp",
        ] {
            assert!(is_sync_program_file(name) && left_alone(name), "{name}");
        }
        assert!(!left_alone("doc-1.md") && !left_alone("project.json"));
    }

    #[test]
    fn no_name_this_app_writes_looks_like_a_conflict_copy() {
        for id in [
            "doc-1",
            "a b (conflict 2026-11-13 14.32 Studio)",
            "x.sync-conflict-1",
            "x.sync-conflict-2026-11",
            "conflicted copy",
            "proj/../x é",
        ] {
            let name = format!("{}.md", safe_name(id));
            assert!(!left_alone(&name), "{id} → {name}");
        }
        // Safe ids that do look like another program's names: a known record's name is its own.
        for id in [
            "doc-2.sync-conflict-20261113-143200-A",
            "project.craft_tmp",
            "p.craftcloud-part",
        ] {
            let name = safe_name(id);
            assert_eq!(name, id, "kept as it is");
            assert!(left_alone(&name), "{id} looks foreign by name alone");
            assert!(!super::foreign(&name, |n| n == id), "{id} is known: ours");
            assert!(super::foreign(&name, |_| false), "{id} unknown: foreign");
        }
    }

    #[test]
    fn removing_a_project_keeps_conflict_copies_and_its_folder_for_them() {
        use crate::sync::SyncFileSystem;
        use crate::sync::memory::MemoryFs;
        let copy = "projects/p/documents/d (conflict 2026-11-13 14.32 Studio).md";
        let mut fs = MemoryFs::with_files([
            ("projects/p/project.json".to_owned(), "{}".to_owned()),
            ("projects/p/documents/d.md".to_owned(), "d".to_owned()),
            (copy.to_owned(), "edit".to_owned()),
            (
                "projects/p/storyboard/rows.json".to_owned(),
                "[]".to_owned(),
            ),
            ("projects/q/project.json".to_owned(), "{}".to_owned()),
        ]);
        let kept = super::remove_keeping_foreign(&mut fs, "projects/p").unwrap();
        assert_eq!(kept, [copy]);
        let left: Vec<&String> = fs.files().keys().collect();
        assert_eq!(left, [copy, "projects/q/project.json"]);
        assert!(super::only_foreign(&mut fs, "projects/p").unwrap());
        assert!(!super::only_foreign(&mut fs, "projects/q").unwrap());
        super::remove_keeping_foreign(&mut fs, "projects/q").unwrap();
        assert!(!fs.exists("projects/q").unwrap(), "an empty folder goes");
    }
}
