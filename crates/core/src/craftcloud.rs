//! CraftCloud (`jonasbove/craftcloud`, the Craft apps' self-hosted sync) through its integration
//! contract (its `docs/CRAFT-INTEGRATION.md`, section 3): an app never talks to the server and
//! never sees the sign-in. It runs the `craftcloud` program with `--json` and reads one JSON value
//! from standard output; a failure is a JSON error on standard error and an exit code.
//!
//! Everything here goes through a [`Runner`], so it is tested with recorded answers and never
//! needs a real CraftCloud; the desktop app's runner starts the program (`prosecraft-native`).
//!
//! - **Open from CraftCloud:** [`list_backups`], `craftcloud files --app prosecraft-rs --json`.
//!   A CraftCloud that doesn't know the app id yet refuses it (exit 2); the listing is then asked
//!   for unfiltered and filtered here by extension.
//! - **Save to CraftCloud:** [`save_backup`], `craftcloud save <file> --to ProseCraft-rs --wait
//!   --json`, `--force` once the person agreed to replace the file that is there.
//! - **The sync folder:** [`sync_folder`], `local_folder` of `craftcloud status --json`.

use crate::js::{self, JsValue};
use crate::sync::SyncMode;

/// ProseCraft-rs' app id in CraftCloud's `apps.toml`.
pub const APP_ID: &str = "prosecraft-rs";
/// The extension of the files ProseCraft-rs opens from CraftCloud: its backups.
pub const EXTENSION: &str = "prosecraft";
/// The cloud folder backups are saved into, and device sync's folder is made in.
pub const FOLDER: &str = "ProseCraft-rs";
/// The longest answer read from the program: a listing of every file in a large cloud is a few
/// megabytes; anything past this is not read.
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

/// What one run of the program gave back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Output {
    /// The exit code; `None` when the program was stopped by a signal.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    /// A run that worked and printed `json`.
    pub fn ok(json: &str) -> Self {
        Self {
            code: Some(0),
            stdout: json.to_owned(),
            stderr: String::new(),
        }
    }

    /// A run that failed with `code` and CraftCloud's JSON error saying `message`.
    pub fn failed(code: i32, kind: &str, message: &str) -> Self {
        let error = js::stringify(&JsValue::Object({
            let mut error = js::JsObject::new();
            error.insert("kind", JsValue::str(kind));
            error.insert("message", JsValue::str(message));
            error.insert("exit_code", JsValue::Number(f64::from(code)));
            let mut outer = js::JsObject::new();
            outer.insert("error", JsValue::Object(error));
            outer
        }));
        Self {
            code: Some(code),
            stdout: String::new(),
            stderr: error,
        }
    }
}

/// Runs the `craftcloud` program. Shared with a background thread, so it is `Send + Sync`.
pub trait Runner: Send + Sync {
    /// Runs `craftcloud` with these arguments and waits for it. `Err` when it couldn't be
    /// started at all.
    fn run(&self, args: &[&str]) -> Result<Output, String>;
    /// Writes a file named `name` with these bytes in a new temporary folder, for `save` to
    /// copy into CraftCloud (it keeps the file's name). Its path.
    fn stage(&self, name: &str, bytes: &[u8]) -> Result<String, String>;
    /// Removes a file [`Self::stage`] wrote, and its folder. Best effort.
    fn discard(&self, path: &str);
}

/// Why a CraftCloud command didn't work, by its exit code (CraftCloud's `docs/CLI.md`, "Exit
/// codes"), with CraftCloud's own message where it says more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The program couldn't be started.
    NotStarted(String),
    /// 2: a bad option or value (an app id it doesn't know).
    Usage(String),
    /// 3: not signed in, or the sign-in ran out.
    SignedOut,
    /// 4: no such file or folder.
    NotFound(String),
    /// 5: a file is already there (or it changed in the cloud meanwhile): `--force` replaces it.
    Exists(String),
    /// 6: no desktop client (or daemon) is running.
    NoClient(String),
    /// 7: the server refused.
    Forbidden(String),
    /// 8: the server can't be reached.
    Unreachable,
    /// 9: the sync is held and waits for the person.
    Held(String),
    /// Any other failure, or an answer that couldn't be read.
    Other(String),
}

impl Failure {
    /// The failure a run reports, from its exit code and JSON error.
    fn of(output: &Output) -> Self {
        let message = error_message(&output.stderr);
        match output.code {
            Some(2) => Self::Usage(message),
            Some(3) => Self::SignedOut,
            Some(4) => Self::NotFound(message),
            Some(5) => Self::Exists(message),
            Some(6) => Self::NoClient(message),
            Some(7) => Self::Forbidden(message),
            Some(8) => Self::Unreachable,
            Some(9) => Self::Held(message),
            Some(_) => Self::Other(message),
            None => Self::Other("CraftCloud was stopped before it answered.".into()),
        }
    }

    /// One sentence or two for the person.
    pub fn message(&self) -> String {
        let said = |intro: &str, detail: &str| {
            if detail.is_empty() {
                intro.to_owned()
            } else {
                format!("{intro} {}", sentence(detail))
            }
        };
        match self {
            Self::NotStarted(why) => said("CraftCloud couldn't be started.", why),
            Self::Usage(why) => said("CraftCloud didn't accept the request.", why),
            Self::SignedOut => "CraftCloud isn't signed in on this computer. Sign in to CraftCloud, then try again.".into(),
            Self::NotFound(why) => said("CraftCloud couldn't find it.", why),
            Self::Exists(why) => said("There is already a file of this name in CraftCloud.", why),
            Self::NoClient(_) => "The CraftCloud desktop client isn't running. Start CraftCloud, then try again.".into(),
            Self::Forbidden(why) => said("The CraftCloud server refused.", why),
            Self::Unreachable => "The CraftCloud server can't be reached. Check the connection, then try again.".into(),
            Self::Held(why) => said("CraftCloud's sync is waiting for you; open CraftCloud to see why.", why),
            Self::Other(why) if why.is_empty() => "CraftCloud didn't manage it.".into(),
            Self::Other(why) => said("CraftCloud didn't manage it.", why),
        }
    }
}

/// `text` as a sentence: a capital first letter and a full stop.
fn sentence(text: &str) -> String {
    let text = text.trim();
    let mut chars = text.chars();
    let mut out: String = chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default();
    if !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

/// The message of CraftCloud's JSON error (`{"error": {"kind", "message", "exit_code"}}`), or
/// the text itself when it isn't one.
fn error_message(stderr: &str) -> String {
    let text = stderr.trim();
    js::parse(text)
        .ok()
        .and_then(|v| {
            v.get("error")?
                .get("message")?
                .as_js_string()
                .map(|s| s.to_string_lossy())
        })
        .unwrap_or_else(|| {
            let line = text.lines().last().unwrap_or_default();
            line.strip_prefix("craftcloud: ").unwrap_or(line).to_owned()
        })
}

/// Runs a command and reads its JSON answer. With `held_ok`, a held sync (exit 9) still
/// answers.
fn ask(runner: &dyn Runner, args: &[&str], held_ok: bool) -> Result<JsValue, Failure> {
    let output = runner.run(args).map_err(Failure::NotStarted)?;
    let answered = output.code == Some(0) || (held_ok && output.code == Some(9));
    if !answered {
        return Err(Failure::of(&output));
    }
    js::parse(output.stdout.trim())
        .map_err(|_| Failure::Other("CraftCloud's answer couldn't be read.".into()))
}

/// Where a file stands on this computer: the `sync` field of CraftCloud's listings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncState {
    Synced,
    Pending,
    Syncing,
    Error,
    Conflict,
    Excluded,
    /// No sync folder here, or a state this build doesn't know.
    Unknown,
}

impl SyncState {
    fn read(word: Option<&str>) -> Self {
        match word {
            Some("synced") => Self::Synced,
            Some("pending") => Self::Pending,
            Some("syncing") => Self::Syncing,
            Some("error") => Self::Error,
            Some("conflict") => Self::Conflict,
            Some("excluded") => Self::Excluded,
            _ => Self::Unknown,
        }
    }

    /// The badge beside a file.
    pub fn badge(self) -> &'static str {
        match self {
            Self::Synced => "Synced",
            Self::Pending => "Waiting to sync",
            Self::Syncing => "Syncing",
            Self::Error => "Sync error",
            Self::Conflict => "Conflict",
            Self::Excluded => "Not synced here",
            Self::Unknown => "",
        }
    }
}

/// One file CraftCloud has.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudFile {
    /// The path in the cloud (`ProseCraft-rs/The Glass Orchard.prosecraft`).
    pub path: String,
    pub name: String,
    pub size: Option<u64>,
    /// When it last changed, RFC 3339.
    pub modified: String,
    /// The synced copy on this computer; `None` when it isn't here.
    pub local_path: Option<String>,
    pub sync: SyncState,
}

impl CloudFile {
    fn read(item: &JsValue) -> Option<Self> {
        let text = |key: &str| {
            item.get(key)
                .and_then(JsValue::as_js_string)
                .map(|s| s.to_string_lossy())
        };
        if text("type").is_some_and(|t| t != "file") {
            return None;
        }
        let path = text("path")?;
        let name = text("name")
            .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path.as_str()).to_owned());
        let size = item
            .get("size")
            .and_then(JsValue::as_number)
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|n| n as u64);
        Some(Self {
            path,
            name,
            size,
            modified: text("modified").unwrap_or_default(),
            local_path: text("local_path").filter(|p| !p.is_empty()),
            sync: SyncState::read(text("sync").as_deref()),
        })
    }

    /// Whether the file can be opened on this computer.
    pub fn is_here(&self) -> bool {
        self.local_path.is_some()
    }

    /// The cloud folder it is in (`""` at the top).
    pub fn folder(&self) -> &str {
        self.path.rsplit_once('/').map_or("", |(folder, _)| folder)
    }

    fn modified_at(&self) -> Option<chrono::DateTime<chrono::FixedOffset>> {
        chrono::DateTime::parse_from_rfc3339(&self.modified).ok()
    }

    /// When it last changed, in this computer's time as the caller formats it (`None` when
    /// CraftCloud gave no time).
    pub fn modified_utc(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.modified_at().map(|t| t.with_timezone(&chrono::Utc))
    }
}

/// Whether a name ends in `.prosecraft`, in any case.
pub fn is_backup_name(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && ext.eq_ignore_ascii_case(EXTENSION))
}

/// The ProseCraft backups CraftCloud has, newest first: `files --app prosecraft-rs`, or, from a
/// CraftCloud that doesn't know that app yet, every file it lists, kept here by extension.
pub fn list_backups(runner: &dyn Runner) -> Result<Vec<CloudFile>, Failure> {
    let answer = match ask(runner, &["files", "--app", APP_ID, "--json"], false) {
        Err(Failure::Usage(_)) => ask(runner, &["files", "--json"], false)?,
        other => other?,
    };
    let items = answer
        .as_array()
        .ok_or_else(|| Failure::Other("CraftCloud's answer couldn't be read.".into()))?;
    // Device sync's single file is a backup too, but device sync's own: it isn't offered.
    let mut files: Vec<CloudFile> = items
        .iter()
        .filter_map(CloudFile::read)
        .filter(|f| is_backup_name(&f.name) && f.name != crate::sync::BUNDLE_FILE)
        .collect();
    files.sort_by(|a, b| {
        b.modified_at()
            .cmp(&a.modified_at())
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(files)
}

/// What a save did (`save --json`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    /// Where it is in the cloud.
    pub path: String,
    /// The copy in the sync folder (saved through the desktop client), if any.
    pub local_path: Option<String>,
    /// It replaced a file that was there.
    pub replaced: bool,
    /// `synced` (uploaded through the desktop client), `uploaded` (straight to the server) or
    /// `queued`.
    pub state: String,
}

/// Saves a backup into CraftCloud's `folder` as `file_name`: written to a temporary file of
/// that name, then `craftcloud save <file> --to <folder> --wait --json` (and `--force` to
/// replace what is there). The temporary file is removed after.
pub fn save_backup(
    runner: &dyn Runner,
    file_name: &str,
    bytes: &[u8],
    folder: &str,
    force: bool,
) -> Result<Saved, Failure> {
    let staged = runner.stage(file_name, bytes).map_err(|why| {
        Failure::Other(format!(
            "The backup couldn't be written for CraftCloud: {why}"
        ))
    })?;
    let mut args = vec!["save", staged.as_str(), "--to", folder, "--wait", "--json"];
    if force {
        args.push("--force");
    }
    let answer = ask(runner, &args, false);
    runner.discard(&staged);
    let answer = answer?;
    let text = |key: &str| {
        answer
            .get(key)
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
    };
    Ok(Saved {
        path: text("path").unwrap_or_else(|| format!("{folder}/{file_name}")),
        local_path: text("local_path").filter(|p| !p.is_empty()),
        replaced: matches!(answer.get("replaced"), Some(JsValue::Bool(true))),
        state: text("state").unwrap_or_default(),
    })
}

/// The folder CraftCloud keeps in step on this computer (`status --json`'s `local_folder`).
pub fn sync_folder(runner: &dyn Runner) -> Result<String, Failure> {
    let status = ask(runner, &["status", "--json"], true)?;
    let folder = status
        .get("local_folder")
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    let signed_out = status
        .get("state")
        .and_then(JsValue::as_js_string)
        .is_some_and(|s| s.eq_str("signed_out"));
    if folder.is_empty() || signed_out {
        return Err(Failure::SignedOut);
    }
    Ok(folder)
}

/// The folder 0.1 synced through, inside CraftCloud's: one encrypted sync file.
pub const LEGACY_SYNC_FOLDER: &str = "ProseCraft";

/// Device sync's folder inside CraftCloud's sync folder `root`: `ProseCraft-rs/<method>`, one
/// folder for each sync method, so changing the method never meets a folder set up for another,
/// and none of them is where backups are saved. A library that synced as one encrypted file
/// through 0.1's `ProseCraft` folder keeps it (`legacy_in_use`: that folder has the sync file),
/// so a second computer finds the first one's.
pub fn device_sync_folder(root: &str, mode: SyncMode, legacy_in_use: bool) -> String {
    let root = std::path::Path::new(root);
    let folder = match mode {
        SyncMode::EncryptedFile if legacy_in_use => root.join(LEGACY_SYNC_FOLDER),
        SyncMode::EncryptedFile => root.join(FOLDER).join("Encrypted sync file"),
        SyncMode::OpenFolder => root.join(FOLDER).join("Open sync folder"),
        SyncMode::EncryptedFolder => root.join(FOLDER).join("Encrypted sync folder"),
    };
    folder.to_string_lossy().into_owned()
}

/// A backup's file name for a project: its name with the characters file systems refuse
/// replaced, `.prosecraft` after it.
pub fn backup_file_name(project_name: &str) -> String {
    let cleaned: String = project_name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => ' ',
            c => c,
        })
        .take(120)
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').trim();
    let stem = if cleaned.is_empty() {
        "Untitled project"
    } else {
        cleaned
    };
    format!("{stem}.{EXTENSION}")
}

#[cfg(test)]
mod tests;
