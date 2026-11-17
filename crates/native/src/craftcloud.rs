//! CraftCloud (self-hosted file sync, `jonasbove/craftcloud`) through its documented
//! integration contract: apps work on local files and run the `craftcloud` program, never its
//! server API (`docs/CRAFT-INTEGRATION.md`). prosecraft-rs only asks where the sync folder is,
//! so it can offer it for device sync in one click.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The `craftcloud` program on `PATH`, if CraftCloud is installed ("without it, show nothing").
pub fn find_on_path() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["craftcloud.exe", "craftcloud.com"]
    } else {
        &["craftcloud"]
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|candidate| candidate.is_file())
}

/// The folder CraftCloud keeps in sync on this computer (`craftcloud status --json`'s
/// `local_folder`). An error says why there is none (not signed in, or the program failed).
pub fn sync_folder(program: &Path) -> Result<PathBuf, String> {
    let output = Command::new(program)
        .args(["status", "--json"])
        .output()
        .map_err(|e| format!("CraftCloud couldn't be asked: {e}"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let status = prosecraft_core::js::parse(text.trim())
        .map_err(|_| "CraftCloud's answer couldn't be read.".to_owned())?;
    let folder = status
        .get("local_folder")
        .and_then(|v| v.as_js_string())
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    if folder.is_empty() {
        return Err(
            "CraftCloud isn't signed in on this computer. Sign in to CraftCloud first.".into(),
        );
    }
    Ok(PathBuf::from(folder))
}

/// The folder ProseCraft syncs through inside CraftCloud's: `<sync folder>/ProseCraft`, made
/// if missing.
pub fn prosecraft_folder(program: &Path) -> Result<PathBuf, String> {
    let folder = sync_folder(program)?.join("ProseCraft");
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    Ok(folder)
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::{prosecraft_folder, sync_folder};

    /// Runs `ask` again while the fake client is "busy": a test writing its script can race a
    /// fork in another test that briefly holds the file open (ETXTBSY), which is the test's
    /// problem, not the client's.
    fn retry_busy<T>(mut ask: impl FnMut() -> Result<T, String>) -> Result<T, String> {
        for _ in 0..50 {
            match ask() {
                Err(e) if e.contains("Text file busy") => {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                other => return other,
            }
        }
        ask()
    }

    fn fake(dir: &std::path::Path, json: &str) -> std::io::Result<std::path::PathBuf> {
        let program = dir.join("craftcloud");
        std::fs::write(&program, format!("#!/bin/sh\nprintf '%s' '{json}'\n"))?;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))?;
        Ok(program)
    }

    #[test]
    fn uses_the_signed_in_clients_folder() {
        let dir = tempfile::tempdir().expect("dir");
        let cloud = dir.path().join("CraftCloud");
        let program = fake(
            dir.path(),
            &format!(r#"{{"state":"idle","local_folder":"{}"}}"#, cloud.display()),
        )
        .expect("fake client");
        assert_eq!(
            retry_busy(|| prosecraft_folder(&program)).expect("folder"),
            cloud.join("ProseCraft")
        );
        assert!(cloud.join("ProseCraft").is_dir());
    }

    #[test]
    fn a_signed_out_client_says_so() {
        let dir = tempfile::tempdir().expect("dir");
        let program = fake(
            dir.path(),
            r#"{"syncing":false,"local_folder":"","state":"signed_out"}"#,
        )
        .expect("fake client");
        assert!(
            retry_busy(|| sync_folder(&program))
                .expect_err("signed out")
                .contains("isn't signed in")
        );
    }
}
