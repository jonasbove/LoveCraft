//! Wiki files on the desktop: the research commands of ProseCraft's Tauri shell
//! (`research_*` in `src-tauri/src/lib.rs`). Each project's files live under
//! `<root>/<project folder>/research/<id>/<file name>`; every path is checked before use.

use std::path::{Component, Path, PathBuf};
use std::process::Command;

use prosecraft_core::research::{MAX_FILE_BYTES, MAX_MARKDOWN_PREVIEW_BYTES};

/// `research_root`: an existing absolute folder.
fn root(root: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(root);
    if !path.is_absolute() || !path.is_dir() {
        return Err("The research files folder is unavailable.".to_owned());
    }
    Ok(path)
}

/// `validate_research_component`.
fn component(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value == "." || value == ".." || value.contains(['/', '\\', '\0']) {
        return Err(format!("Invalid {label}."));
    }
    Ok(())
}

/// `research_destination`: `research/<id>/<name>` inside the project folder, nothing else.
pub fn destination(
    root_dir: &str,
    project_folder: &str,
    storage_key: &str,
) -> Result<PathBuf, String> {
    component(project_folder, "project folder")?;
    let invalid = || "Invalid research file path.".to_owned();
    let relative = Path::new(storage_key);
    let parts: Vec<Component> = relative.components().collect();
    if relative.is_absolute()
        || !storage_key.starts_with("research/")
        || storage_key.contains('\\')
        || parts.len() != 3
        || parts[0].as_os_str() != "research"
        || !parts.iter().all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(invalid());
    }
    Ok(root(root_dir)?.join(project_folder).join(relative))
}

/// `research_prepare_project_folder`: the project's folder, made if missing.
pub fn prepare_project_folder(root_dir: &str, project_folder: &str) -> Result<PathBuf, String> {
    component(project_folder, "project folder")?;
    let folder = root(root_dir)?.join(project_folder);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    Ok(folder)
}

/// `research_import_file`: copies the file in; its size.
pub fn import(
    root_dir: &str,
    project_folder: &str,
    storage_key: &str,
    source: &Path,
) -> Result<u64, String> {
    if !source.is_file() {
        return Err("The selected file could not be read.".to_owned());
    }
    let size = std::fs::metadata(source).map_err(|e| e.to_string())?.len();
    if size > MAX_FILE_BYTES {
        return Err("Research files must be 200 MB or smaller.".to_owned());
    }
    let to = destination(root_dir, project_folder, storage_key)?;
    if to.exists() {
        return Err("That research file already exists.".to_owned());
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::copy(source, &to).map_err(|e| e.to_string())?;
    Ok(size)
}

/// `research_file_path`: where a stored file is, if it is still there.
pub fn file_path(
    root_dir: &str,
    project_folder: &str,
    storage_key: &str,
) -> Result<PathBuf, String> {
    let path = destination(root_dir, project_folder, storage_key)?;
    if !path.is_file() {
        return Err("That research file is no longer in the project folder.".to_owned());
    }
    Ok(path)
}

/// `research_read_markdown`: a Markdown file's text, at most 2 MB.
pub fn read_markdown(
    root_dir: &str,
    project_folder: &str,
    storage_key: &str,
) -> Result<String, String> {
    let path = destination(root_dir, project_folder, storage_key)?;
    if !path
        .extension()
        .and_then(|e| e.to_str())
        // `.md` and `.markdown`, as `openResearchFile` sends both (jennraeross/ProseCraft#64).
        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
    {
        return Err("Only Markdown research files can be previewed.".to_owned());
    }
    if !path.is_file() {
        return Err("That research file is no longer in the project folder.".to_owned());
    }
    let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    if size > MAX_MARKDOWN_PREVIEW_BYTES {
        return Err("Markdown files must be 2 MB or smaller to preview.".to_owned());
    }
    std::fs::read_to_string(&path)
        .map_err(|e| format!("Could not read that Markdown file as UTF-8: {e}"))
}

/// `research_delete_file`: the file, and its id folder once empty.
pub fn delete(root_dir: &str, project_folder: &str, storage_key: &str) -> Result<(), String> {
    let path = destination(root_dir, project_folder, storage_key)?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    if let Some(parent) = path.parent() {
        let empty = std::fs::read_dir(parent).is_ok_and(|mut entries| entries.next().is_none());
        if empty && parent.file_name().and_then(|n| n.to_str()) != Some("research") {
            let _ = std::fs::remove_dir(parent);
        }
    }
    Ok(())
}

fn spawn(program: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    Command::new(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not start {program}: {e}"))
}

/// Opens a file or a web address with the system's default app.
pub fn open(target: &std::ffi::OsStr) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        spawn("open", &[target])
    } else if cfg!(target_os = "windows") {
        spawn("explorer", &[target])
    } else {
        spawn("xdg-open", &[target])
    }
}

/// Shows a file in the system's file manager (on Linux, its folder).
pub fn reveal(path: &Path) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        spawn("open", &["-R".as_ref(), path.as_os_str()])
    } else if cfg!(target_os = "windows") {
        let select = format!("/select,{}", path.display());
        spawn("explorer", &[select.as_ref()])
    } else {
        let folder = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        spawn("xdg-open", &[folder.as_os_str()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_copied_read_and_deleted_inside_the_project_folder() {
        let dir = tempfile::tempdir().expect("dir");
        let root_dir = dir.path().to_str().expect("utf-8");
        let source = dir.path().join("notes.md");
        std::fs::write(&source, "# Notes\n\nSome *text*.").expect("write");
        let key = "research/abc-1/notes.md";
        assert_eq!(import(root_dir, "Novel (abc)", key, &source), Ok(21));
        assert_eq!(
            import(root_dir, "Novel (abc)", key, &source).unwrap_err(),
            "That research file already exists."
        );
        let stored = file_path(root_dir, "Novel (abc)", key).expect("there");
        assert!(stored.ends_with("Novel (abc)/research/abc-1/notes.md"));
        assert_eq!(
            read_markdown(root_dir, "Novel (abc)", key).as_deref(),
            Ok("# Notes\n\nSome *text*.")
        );
        // A .markdown file previews too (jennraeross/ProseCraft#64).
        let long = "research/abc-2/notes.markdown";
        assert_eq!(import(root_dir, "Novel (abc)", long, &source), Ok(21));
        assert_eq!(
            read_markdown(root_dir, "Novel (abc)", long).as_deref(),
            Ok("# Notes\n\nSome *text*.")
        );
        delete(root_dir, "Novel (abc)", long).expect("deleted");
        delete(root_dir, "Novel (abc)", key).expect("deleted");
        assert!(!stored.exists() && !stored.parent().expect("id folder").exists());
        assert!(
            dir.path().join("Novel (abc)/research").is_dir(),
            "the research folder stays"
        );
        assert_eq!(
            file_path(root_dir, "Novel (abc)", key).unwrap_err(),
            "That research file is no longer in the project folder."
        );
    }

    #[test]
    fn paths_out_of_the_project_folder_are_refused() {
        let dir = tempfile::tempdir().expect("dir");
        let root_dir = dir.path().to_str().expect("utf-8");
        for key in [
            "research/../x",
            "research/a/../../x",
            "/etc/passwd",
            "other/a/x",
            "research/a",
            "research/a/b/c",
            "research\\a\\x",
        ] {
            assert_eq!(
                destination(root_dir, "P", key).unwrap_err(),
                "Invalid research file path.",
                "{key}"
            );
        }
        for folder in ["", ".", "..", "a/b", "a\\b"] {
            assert_eq!(
                destination(root_dir, folder, "research/a/x").unwrap_err(),
                "Invalid project folder."
            );
        }
        assert_eq!(
            destination("relative", "P", "research/a/x").unwrap_err(),
            "The research files folder is unavailable."
        );
        let text = dir.path().join("a.txt");
        std::fs::write(&text, "x").expect("write");
        import(root_dir, "P", "research/a/a.txt", &text).expect("copied");
        assert_eq!(
            read_markdown(root_dir, "P", "research/a/a.txt").unwrap_err(),
            "Only Markdown research files can be previewed."
        );
    }
}
