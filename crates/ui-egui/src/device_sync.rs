//! Running device sync (ProseCraft's `runSync`, `setSyncMode`, `chooseSyncFolder`,
//! `forgetSyncSettings`): the Settings dialog's actions, and the sync itself on a background
//! thread (unlocking a vault is deliberately slow work). Editing pauses while a sync runs, as
//! its result replaces the library.

use prosecraft_core::js::{JsObject, JsString, JsValue};
use prosecraft_core::sync::state::{SYNC_STATE_KEY, SyncState};
use prosecraft_core::sync::{SyncMode, SyncResult};

use crate::settings::{SyncAction, created_message, folder_saved_message, synced_message};
use crate::{HostRequest, Library, ProseApp};

/// A sync running on a background thread.
pub(crate) struct SyncJob {
    receiver: std::sync::mpsc::Receiver<Result<SyncResult, String>>,
    mode: SyncMode,
    folder: String,
}

fn empty() -> JsValue {
    JsValue::Object(JsObject::new())
}

impl ProseApp {
    /// Reads the saved sync settings into the dialog (when the app has a store that syncs).
    pub(crate) fn init_sync(&mut self) {
        let Some(home) = self.home.as_mut() else {
            return;
        };
        self.state.sync.available = home.store.supports_sync();
        self.state.sync.craftcloud = self.state.cloud.runner.is_some();
        self.state.comment_author = home
            .store
            .load_setting(crate::COMMENT_AUTHOR_KEY)
            .unwrap_or_default();
        self.state.custom_starters = prosecraft_core::starters::custom::load_custom_starters(
            home.store
                .load_setting(prosecraft_core::starters::custom::CUSTOM_STARTERS_KEY)
                .as_deref(),
        );
        self.state.writing = crate::WritingPrefs::load(home.store.as_mut());
        self.state.proofread = crate::proofreading::ProofreadPrefs {
            enabled: home
                .store
                .load_setting(crate::proofreading::ENABLED_KEY)
                .as_deref()
                == Some("true"),
            dialect: home
                .store
                .load_setting(crate::proofreading::DIALECT_KEY)
                .filter(|d| {
                    prosecraft_core::proofread::DIALECTS
                        .iter()
                        .any(|(id, _)| id == d)
                })
                .unwrap_or_else(|| "american".to_owned()),
        };
        self.state.markdown_enabled = home
            .store
            .load_setting(crate::settings::MARKDOWN_EDITOR_KEY)
            .as_deref()
            == Some("true");
        self.state.theme = home
            .store
            .load_setting(crate::settings::THEME_KEY)
            .as_deref()
            .and_then(crate::settings::ThemePreference::from_id)
            .unwrap_or_default();
        self.state.font_prefs =
            crate::editor_fonts::FontPrefs::read(&mut |key| home.store.load_setting(key));
        self.state.fonts_applied = None;
        // `isColorScheme`: an unknown value is ProseCraft's own.
        self.state.color_scheme = home
            .store
            .load_setting(crate::theme::SCHEME_KEY)
            .filter(|s| crate::theme::SCHEMES.iter().any(|(v, _)| v == s))
            .unwrap_or_else(|| "prosecraft".to_owned());
        if let Some(state) = home
            .store
            .load_setting(SYNC_STATE_KEY)
            .as_deref()
            .and_then(SyncState::read)
        {
            self.state.sync.folder = state.folder;
            self.state.sync.mode = state.mode;
        }
    }

    fn saved_sync_state(&mut self) -> Option<SyncState> {
        self.home
            .as_mut()?
            .store
            .load_setting(SYNC_STATE_KEY)
            .as_deref()
            .and_then(SyncState::read)
    }

    /// `persistSyncSettings`: the folder and method, with `last_synced` or (for the same
    /// folder and method) the remembered stamps.
    fn persist_sync(&mut self, last_synced: Option<JsValue>) {
        if self.state.sync.folder.is_empty() {
            return;
        }
        let previous = self.saved_sync_state();
        let same = previous
            .as_ref()
            .is_some_and(|p| p.folder == self.state.sync.folder && p.mode == self.state.sync.mode);
        let mut state = SyncState::new(self.state.sync.folder.clone(), self.state.sync.mode);
        state.last_synced = last_synced.unwrap_or_else(|| {
            previous
                .filter(|_| same)
                .map_or_else(empty, |p| p.last_synced)
        });
        if let Some(home) = self.home.as_mut()
            && let Err(error) = home
                .store
                .save_setting(SYNC_STATE_KEY, Some(&state.to_text()))
        {
            self.state.sync.status = format!("Could not save the sync settings: {error}");
        }
    }

    /// The folder the person chose for syncing (from the host's folder dialog).
    pub fn set_sync_folder(&mut self, folder: String) {
        if self.state.sync.busy || folder.is_empty() {
            return;
        }
        let same = self
            .saved_sync_state()
            .is_some_and(|p| p.folder == folder && p.mode == self.state.sync.mode);
        if !same {
            self.state.sync.passphrase.clear();
        }
        self.state.sync.folder = folder;
        self.persist_sync(None);
        self.state.sync.status = folder_saved_message(self.state.sync.mode, same).to_owned();
    }

    pub(crate) fn handle_sync_actions(&mut self, ctx: &egui::Context) {
        self.poll_craftcloud_folder();
        for action in std::mem::take(&mut self.state.sync_actions) {
            if self.state.sync.busy {
                continue;
            }
            match action {
                SyncAction::ChooseFolder => {
                    self.state.sync.status.clear();
                    self.state.requests.push(HostRequest::ChooseSyncFolder);
                }
                SyncAction::SetMode(mode) if mode != self.state.sync.mode => {
                    self.state.sync.mode = mode;
                    self.state.sync.passphrase.clear();
                    self.persist_sync(Some(empty()));
                    self.state.sync.status = if self.state.sync.folder.is_empty() {
                        String::new()
                    } else {
                        "Sync method changed. Use a folder already set up for this method, or an empty folder.".to_owned()
                    };
                    // A CraftCloud folder has one folder for each method: move to this one's.
                    let in_craftcloud =
                        prosecraft_core::craftcloud::craftcloud_root_of(&self.state.sync.folder)
                            .is_some();
                    if in_craftcloud && self.state.cloud.runner.is_some() {
                        self.start_craftcloud_folder(ctx);
                    }
                }
                SyncAction::SetMode(_) => {}
                SyncAction::UseCraftCloud => self.start_craftcloud_folder(ctx),
                SyncAction::Forget => {
                    if let Some(home) = self.home.as_mut() {
                        let _ = home.store.save_setting(SYNC_STATE_KEY, None);
                    }
                    self.state.sync.folder.clear();
                    self.state.sync.passphrase.clear();
                    self.state.sync.status.clear();
                }
                SyncAction::SyncNow => self.start_sync(),
            }
        }
    }

    /// "In CraftCloud": asks CraftCloud where its sync folder is, on a background thread (the
    /// program can take a while, or not answer), and then picks device sync's folder there
    /// for the chosen method ([`Self::poll_craftcloud_folder`]).
    fn start_craftcloud_folder(&mut self, ctx: &egui::Context) {
        use prosecraft_core::craftcloud;
        let Some(runner) = self.state.cloud.runner.clone() else {
            self.state.sync.status = "CraftCloud isn't installed.".into();
            return;
        };
        if self.state.cloud.folder_job.is_some() {
            return;
        }
        let mode = self.state.sync.mode;
        let receiver = crate::craftcloud::spawn(ctx, "craftcloud-status", move || {
            let root = craftcloud::sync_folder(runner.as_ref())?;
            // 0.1's sync file in the cloud, not necessarily downloaded here yet.
            let legacy_in_cloud = mode == SyncMode::EncryptedFile
                && craftcloud::legacy_sync_file_in_cloud(runner.as_ref());
            Ok((root, legacy_in_cloud))
        });
        self.state.cloud.folder_job = Some(crate::craftcloud::FolderJob { receiver, mode });
        self.state.sync.craftcloud_pending = true;
        self.state.sync.status = "Asking CraftCloud where its folder is…".into();
    }

    /// CraftCloud's answer for "In CraftCloud": the folder for the method, an existing one
    /// first. A 0.1 library's sync file that CraftCloud has but hasn't downloaded here is
    /// waited for, rather than starting a second library beside it.
    fn poll_craftcloud_folder(&mut self) {
        use prosecraft_core::craftcloud;
        use prosecraft_core::sync::BUNDLE_FILE;
        let Some(job) = self.state.cloud.folder_job.as_ref() else {
            return;
        };
        let answer = match job.receiver.try_recv() {
            Ok(answer) => answer,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err(craftcloud::Failure::Other(String::new()))
            }
        };
        let mode = job.mode;
        self.state.cloud.folder_job = None;
        self.state.sync.craftcloud_pending = false;
        let (root, legacy_in_cloud) = match answer {
            Ok(found) => found,
            Err(failure) => {
                self.state.sync.status = failure.message();
                return;
            }
        };
        if mode != self.state.sync.mode {
            // The method changed meanwhile; the next answer is for it.
            return;
        }
        let mut has_bundle = |folder: &str| {
            self.home.as_mut().is_some_and(|home| {
                home.store
                    .sync_fs(folder)
                    .is_some_and(|mut fs| fs.exists(BUNDLE_FILE).unwrap_or(false))
            })
        };
        let legacy = craftcloud::device_sync_folder(&root, mode, true);
        let folder = craftcloud::device_sync_folder(&root, mode, false);
        let folder = if mode != SyncMode::EncryptedFile || has_bundle(&folder) {
            folder
        } else if has_bundle(&legacy) {
            legacy
        } else if legacy_in_cloud {
            self.state.sync.status = format!(
                "CraftCloud has a sync file from ProseCraft-rs 0.1 in its {} folder that isn't on this computer yet. Let CraftCloud download it (the folder must be synced here), then choose In CraftCloud again, so this computer joins that library instead of starting another.",
                craftcloud::LEGACY_SYNC_FOLDER
            );
            return;
        } else {
            folder
        };
        self.set_sync_folder(folder);
    }

    fn start_sync(&mut self) {
        if self.state.sync.folder.is_empty() {
            return;
        }
        if self.state.sync.mode != SyncMode::OpenFolder
            && self.state.sync.passphrase.encode_utf16().count() < 8
        {
            self.state.sync.status = "Use a sync passphrase of at least 8 characters.".into();
            return;
        }
        // Everything typed so far is in the library before it syncs.
        self.commit_draft();
        self.save_now();
        let previous = self.saved_sync_state();
        let Some(home) = self.home.as_mut() else {
            return;
        };
        if home.library.is_read_only() {
            self.state.sync.status =
                "Sync failed: this library has a project from a newer version of ProseCraft."
                    .into();
            return;
        }
        let Some(fs) = home.store.sync_fs(&self.state.sync.folder) else {
            self.state.sync.status = "Sync failed: Sync configuration not found.".into();
            return;
        };
        let same = previous
            .as_ref()
            .is_some_and(|p| p.folder == self.state.sync.folder && p.mode == self.state.sync.mode);
        let last_synced = previous
            .filter(|_| same)
            .map_or_else(empty, |p| p.last_synced);
        let local: Vec<JsValue> = home.library.projects().cloned().collect();
        let job = Job {
            fs,
            mode: self.state.sync.mode,
            passphrase: JsString::from(self.state.sync.passphrase.as_str()),
            local,
            last_synced,
            now: home.store.now_ms(),
            device_id: home.library.device_id.clone(),
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        if std::thread::Builder::new()
            .name("prosecraft-sync".into())
            .spawn(move || {
                let _ = sender.send(job.run());
            })
            .is_err()
        {
            self.state.sync.status = "Sync failed: could not start.".into();
            return;
        }
        self.sync_job = Some(SyncJob {
            receiver,
            mode: self.state.sync.mode,
            folder: self.state.sync.folder.clone(),
        });
        self.state.sync.busy = true;
        self.state.sync.status = "Syncing…".into();
        self.state.editable = false;
    }

    /// Takes a finished sync's result. Returns whether one is still running.
    pub(crate) fn poll_sync(&mut self) -> bool {
        let Some(job) = self.sync_job.as_ref() else {
            return false;
        };
        let result = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("Sync failed.".to_owned()),
        };
        let (mode, folder) = (job.mode, job.folder.clone());
        self.sync_job = None;
        self.state.sync.busy = false;
        self.state.editable = self
            .home
            .as_ref()
            .is_some_and(|h| !h.library.is_read_only());
        match result {
            Ok(result) => {
                // Settings for this folder and method, with the new stamps.
                let mut state = SyncState::new(folder, mode);
                state.last_synced = result.last_synced.clone();
                let Some(home) = self.home.as_mut() else {
                    return false;
                };
                let _ = home
                    .store
                    .save_setting(SYNC_STATE_KEY, Some(&state.to_text()));
                home.library.replace_synced(result.projects.clone());
                let text = home.library.to_text(home.store.now_ms());
                let saved = home.store.save(&text);
                let active = self.active_project_id();
                let Some(home) = self.home.as_ref() else {
                    return false;
                };
                let views = Library::from_stored(home.library.projects().cloned().collect());
                self.state.active = active
                    .and_then(|id| views.projects().iter().position(|p| p.view.id == id))
                    .unwrap_or(0);
                self.state.library = Some(views);
                self.state.draft = None;
                self.state.sync.status = match saved {
                    Err(error) => format!("Synced, but couldn't save your library: {error}"),
                    Ok(()) if result.created => created_message(mode, result.projects.len()),
                    Ok(()) => synced_message(&result),
                };
                if let Some(first) = result.conflict_files.first() {
                    let more = result.conflict_files.len() - 1;
                    let others = if more == 0 {
                        String::new()
                    } else {
                        format!(" and {more} more")
                    };
                    let advice = match mode {
                        SyncMode::EncryptedFile => {
                            " Open it with Open backup… to bring its changes in."
                        }
                        // A copy holds the other computer's version of a scene, chapter or
                        // storyboard: plain text, read beside the original.
                        SyncMode::OpenFolder => {
                            " A conflict copy is one computer's version of a file that changed on two; the original beside it is what syncs. Open the copy in a text editor, bring what you need into LoveCraft, then delete the copy."
                        }
                        SyncMode::EncryptedFolder => "",
                    };
                    self.state.sync.status.push_str(&format!(" Another sync tool left a conflict copy in the folder: {first}{others}. It was left as it is.{advice}"));
                }
            }
            Err(message) if message == "Incorrect passphrase." => self.state.sync.status = message,
            Err(message) => self.state.sync.status = format!("Sync failed: {message}"),
        }
        false
    }
}

/// What a sync needs, moved to its thread. (The web build has no sync folder, so it never
/// runs one.)
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
struct Job {
    fs: Box<dyn prosecraft_core::sync::SyncFileSystem + Send>,
    mode: SyncMode,
    passphrase: JsString,
    local: Vec<JsValue>,
    last_synced: JsValue,
    now: f64,
    device_id: String,
}

impl Job {
    #[cfg(not(target_arch = "wasm32"))]
    fn run(mut self) -> Result<SyncResult, String> {
        use prosecraft_core::storage::crypto::{OsRandom, RandomSource};
        use prosecraft_core::sync::{
            EncryptedFileProvider, FolderProvider, OpenFolderProvider, sync_via,
        };
        // ProseCraft's conflicted-copy suffix: five random base-36 characters.
        let mut suffix = || {
            let mut bytes = [0u8; 5];
            let _ = OsRandom.fill(&mut bytes);
            bytes
                .iter()
                .map(|b| char::from(b"0123456789abcdefghijklmnopqrstuvwxyz"[usize::from(*b) % 36]))
                .collect()
        };
        let fs = self.fs.as_mut();
        let result = match self.mode {
            SyncMode::EncryptedFile => sync_via(
                &mut EncryptedFileProvider::new(fs, self.now),
                &self.passphrase,
                &self.local,
                &self.last_synced,
                self.now,
                &self.device_id,
                &mut OsRandom,
                &mut suffix,
            ),
            SyncMode::OpenFolder => sync_via(
                &mut OpenFolderProvider::new(fs, self.now),
                &self.passphrase,
                &self.local,
                &self.last_synced,
                self.now,
                &self.device_id,
                &mut OsRandom,
                &mut suffix,
            ),
            SyncMode::EncryptedFolder => sync_via(
                &mut FolderProvider::new(fs),
                &self.passphrase,
                &self.local,
                &self.last_synced,
                self.now,
                &self.device_id,
                &mut OsRandom,
                &mut suffix,
            ),
        };
        result.map_err(|e| e.to_string())
    }

    #[cfg(target_arch = "wasm32")]
    fn run(self) -> Result<SyncResult, String> {
        Err("Device sync isn't available in the web build.".into())
    }
}
