//! CraftCloud (self-hosted file sync, `jonasbove/craftcloud`) through its documented
//! integration contract: apps work on local files and run the `craftcloud` program, never its
//! server API (`docs/CRAFT-INTEGRATION.md`). What to ask and how to read the answers is
//! `prosecraft_core::craftcloud`; this is the [`Runner`] that starts the program.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use prosecraft_core::craftcloud::{MAX_OUTPUT_BYTES, Output, Runner};

/// The `craftcloud` program on `PATH`, if CraftCloud is installed ("without it, show nothing").
/// On Windows the window program, `craftcloud.exe`: the `craftcloud.com` beside it is for
/// terminals.
pub fn find_on_path() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "craftcloud.exe"
    } else {
        "craftcloud"
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// The prefix of the temporary folders a save goes through.
const STAGE_PREFIX: &str = "prosecraft-rs-craftcloud-";

/// Starts the `craftcloud` program at `program`.
pub struct ProcessRunner {
    program: PathBuf,
}

impl ProcessRunner {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }
}

/// Up to `cap` bytes of a stream as text; the rest is read and dropped, so the program never
/// waits on a full pipe.
fn read_capped(mut from: impl Read, cap: usize) -> String {
    let mut kept = Vec::new();
    let _ = (&mut from)
        .take(u64::try_from(cap).unwrap_or(u64::MAX))
        .read_to_end(&mut kept);
    let _ = std::io::copy(&mut from, &mut std::io::sink());
    String::from_utf8_lossy(&kept).into_owned()
}

impl Runner for ProcessRunner {
    fn run(&self, args: &[&str]) -> Result<Output, String> {
        let mut command = Command::new(&self.program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // No console window flashing up beside the app for each command.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        let stderr = child.stderr.take();
        let errors = std::thread::Builder::new()
            .name("craftcloud-stderr".into())
            .spawn(move || {
                stderr
                    .map(|s| read_capped(s, 1024 * 1024))
                    .unwrap_or_default()
            });
        let stdout = child
            .stdout
            .take()
            .map(|s| read_capped(s, MAX_OUTPUT_BYTES))
            .unwrap_or_default();
        let status = child.wait().map_err(|e| e.to_string())?;
        let stderr = errors
            .ok()
            .and_then(|thread| thread.join().ok())
            .unwrap_or_default();
        Ok(Output {
            code: status.code(),
            stdout,
            stderr,
        })
    }

    fn stage(&self, name: &str, bytes: &[u8]) -> Result<String, String> {
        let plain =
            Path::new(name).file_name().is_some_and(|n| n == name) && !name.contains(['/', '\\']);
        if !plain {
            return Err(format!("{name:?} is not a file name"));
        }
        let dir = tempfile::Builder::new()
            .prefix(STAGE_PREFIX)
            .tempdir()
            .map_err(|e| e.to_string())?
            .keep();
        let file = dir.join(name);
        std::fs::write(&file, bytes).map_err(|e| {
            let _ = std::fs::remove_dir_all(&dir);
            e.to_string()
        })?;
        Ok(file.to_string_lossy().into_owned())
    }

    fn discard(&self, path: &str) {
        let file = Path::new(path);
        // Only in a folder this runner made: a temporary one named for it.
        let Some(dir) = file.parent() else { return };
        let ours = dir
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(STAGE_PREFIX));
        if ours {
            let _ = std::fs::remove_file(file);
            let _ = std::fs::remove_dir(dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use prosecraft_core::craftcloud::Runner;

    use super::ProcessRunner;

    #[test]
    fn a_staged_file_keeps_its_name_and_goes_again() {
        let runner = ProcessRunner::new("craftcloud");
        let path = runner
            .stage("The Glass Orchard.prosecraft", b"{}")
            .expect("staged");
        let file = std::path::Path::new(&path);
        assert_eq!(
            file.file_name().and_then(|n| n.to_str()),
            Some("The Glass Orchard.prosecraft")
        );
        assert_eq!(std::fs::read(file).expect("read"), b"{}");
        let dir = file.parent().expect("dir").to_owned();
        runner.discard(&path);
        assert!(!file.exists() && !dir.exists());
        assert!(runner.stage("../x.prosecraft", b"").is_err());
        assert!(runner.stage("", b"").is_err());
    }

    #[test]
    fn a_folder_it_did_not_make_is_never_emptied() {
        let dir = tempfile::tempdir().expect("dir");
        let file = dir.path().join("keep.prosecraft");
        std::fs::write(&file, "k").expect("write");
        ProcessRunner::new("craftcloud").discard(&file.to_string_lossy());
        assert!(file.exists());
    }

    #[test]
    fn a_missing_program_is_an_error_not_a_crash() {
        let runner = ProcessRunner::new("/nonexistent/craftcloud-for-a-test");
        assert!(runner.run(&["status", "--json"]).is_err());
    }

    /// The exit code, standard output and standard error of a real program.
    #[cfg(unix)]
    #[test]
    fn reads_what_the_program_says_and_its_exit_code() {
        let runner = ProcessRunner::new("/bin/sh");
        let output = runner
            .run(&["-c", "printf '[]'; printf 'oops' >&2; exit 5"])
            .expect("ran");
        assert_eq!(
            (output.code, output.stdout.as_str(), output.stderr.as_str()),
            (Some(5), "[]", "oops")
        );
    }
}
