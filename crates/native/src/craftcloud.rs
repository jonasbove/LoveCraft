//! CraftCloud (self-hosted file sync, `jonasbove/craftcloud`) through its documented
//! integration contract: apps work on local files and run the `craftcloud` program, never its
//! server API (`docs/CRAFT-INTEGRATION.md`). What to ask and how to read the answers is
//! `prosecraft_core::craftcloud`; this is the [`Runner`] that starts the program.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

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
const STAGE_PREFIX: &str = "lovecraft-craftcloud-";

/// Removes what earlier saves left in the temporary folder (a crash, a power cut): folders
/// this runner made, untouched for more than `age`, in `temp` (the system's temporary
/// folder). Best effort; how many went.
pub fn clean_stale_stages(temp: &Path, age: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(temp) else {
        return 0;
    };
    let now = SystemTime::now();
    let mut removed = 0;
    for entry in entries.flatten() {
        let ours = entry
            .file_name()
            .to_string_lossy()
            .starts_with(STAGE_PREFIX)
            && entry.file_type().is_ok_and(|t| t.is_dir());
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|d| d >= age);
        if ours && old && std::fs::remove_dir_all(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

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
    fn run(&self, args: &[&str], timeout: Duration) -> Result<Output, String> {
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
        // Both streams are read on threads of their own, so the wait below can give up.
        let reader = |stream: Option<Box<dyn Read + Send>>, cap: usize, name: &str| {
            let (send, receive) = mpsc::channel();
            let started = std::thread::Builder::new()
                .name(name.to_owned())
                .spawn(move || {
                    let _ = send.send(stream.map(|s| read_capped(s, cap)).unwrap_or_default());
                });
            started.ok().map(|_| receive)
        };
        let stdout = reader(
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            MAX_OUTPUT_BYTES,
            "craftcloud-stdout",
        );
        let stderr = reader(
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            1024 * 1024,
            "craftcloud-stderr",
        );
        let deadline = Instant::now() + timeout;
        let mut timed_out = false;
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                break Some(status);
            }
            if Instant::now() >= deadline {
                timed_out = true;
                let _ = child.kill();
                break child.wait().ok();
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        // A child of the program may still hold the pipes; their text is not waited on long.
        let take = |receive: Option<mpsc::Receiver<String>>| {
            receive
                .and_then(|r| r.recv_timeout(Duration::from_secs(2)).ok())
                .unwrap_or_default()
        };
        Ok(Output {
            code: if timed_out {
                None
            } else {
                status.and_then(|s| s.code())
            },
            stdout: take(stdout),
            stderr: take(stderr),
            timed_out,
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

    use std::time::Duration;

    use super::{ProcessRunner, STAGE_PREFIX, clean_stale_stages};

    /// A program that doesn't answer is stopped at the deadline: the window never waits on it.
    #[cfg(unix)]
    #[test]
    fn a_program_that_hangs_is_stopped_at_the_deadline() {
        let started = std::time::Instant::now();
        let output = ProcessRunner::new("/bin/sh")
            .run(
                &["-c", "printf 'half'; exec sleep 30"],
                Duration::from_millis(300),
            )
            .expect("ran");
        assert!(output.timed_out && output.code.is_none(), "{output:?}");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn stale_temporary_folders_are_cleaned_up_and_nothing_else() {
        let temp = tempfile::tempdir().expect("dir");
        let stale = temp.path().join(format!("{STAGE_PREFIX}old"));
        std::fs::create_dir_all(&stale).expect("dir");
        std::fs::write(stale.join("Book.prosecraft"), "x").expect("write");
        let other = temp.path().join("someone-elses");
        std::fs::create_dir_all(&other).expect("dir");
        assert_eq!(
            clean_stale_stages(temp.path(), Duration::from_secs(3600)),
            0,
            "too new"
        );
        assert_eq!(clean_stale_stages(temp.path(), Duration::ZERO), 1);
        assert!(!stale.exists() && other.exists());
    }

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
        assert!(
            runner
                .run(&["status", "--json"], Duration::from_secs(5))
                .is_err()
        );
    }

    /// The exit code, standard output and standard error of a real program.
    #[cfg(unix)]
    #[test]
    fn reads_what_the_program_says_and_its_exit_code() {
        let runner = ProcessRunner::new("/bin/sh");
        let output = runner
            .run(
                &["-c", "printf '[]'; printf 'oops' >&2; exit 5"],
                Duration::from_secs(30),
            )
            .expect("ran");
        assert_eq!(
            (output.code, output.stdout.as_str(), output.stderr.as_str()),
            (Some(5), "[]", "oops")
        );
    }
}
