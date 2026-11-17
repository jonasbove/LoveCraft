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
        self.state.sync.craftcloud = home.store.has_craftcloud();
        self.state.comment_author = home
            .store
            .load_setting(crate::COMMENT_AUTHOR_KEY)
            .unwrap_or_default();
        self.state.theme = home
            .store
            .load_setting(crate::settings::THEME_KEY)
            .as_deref()
            .and_then(crate::settings::ThemePreference::from_id)
            .unwrap_or_default();
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

    pub(crate) fn handle_sync_actions(&mut self) {
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
                }
                SyncAction::SetMode(_) => {}
                SyncAction::UseCraftCloud => {
                    let Some(home) = self.home.as_mut() else {
                        continue;
                    };
                    match home.store.craftcloud_folder() {
                        Ok(folder) => {
                            // One encrypted file: a CraftCloud clash is then one whole-file
                            // conflict copy, never a half-synced layout.
                            if self.state.sync.mode != SyncMode::EncryptedFile {
                                self.state.sync.mode = SyncMode::EncryptedFile;
                                self.state.sync.passphrase.clear();
                            }
                            self.set_sync_folder(folder);
                        }
                        Err(why) => self.state.sync.status = why,
                    }
                }
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

    fn start_sync(&mut self) {
        if self.state.sync.folder.is_empty() {
            return;
        }
        if self.state.sync.mode == SyncMode::OpenFolder {
            self.state.sync.status =
                "Sync failed: open-folder sync isn't available in prosecraft-rs yet.".into();
            return;
        }
        if self.state.sync.passphrase.encode_utf16().count() < 8 {
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
                    let advice = if mode == SyncMode::EncryptedFile {
                        " Open it with Open backup… to bring its changes in."
                    } else {
                        ""
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
        use prosecraft_core::sync::{EncryptedFileProvider, FolderProvider, sync_via};
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
            _ => sync_via(
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
