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
//! never a space or a parenthesis, so a name with a conflict mark is never one of its own.

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
pub fn left_alone(name: &str) -> bool {
    is_conflict_copy(name) || is_sync_program_file(name)
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
    }
}
