//! The encrypted single file (`bundle-provider.ts`) and the encrypted folder
//! (`folder-provider.ts`), and the flow both run (`orchestrate.ts`).

use super::layout::{encode_project_files, entity_dir, project_dir_name, read_project_files};
use super::merge::merge_projects;
use super::{
    BUNDLE_FILE, OPEN_MARKER_FILE, PROJECTS_DIR, SyncError, SyncFileSystem, SyncResult, VAULT_FILE,
};
use crate::js::{self, JsString, JsValue};
use crate::storage::bundle::{EncryptedBundle, write_encrypted_bundle};
use crate::storage::crypto::{
    RandomSource, VaultHeader, VaultKey, VaultOptions, create_vault, unlock_vault,
};
use crate::storage::schema::ENTITY_KINDS;

/// The folder's projects once unlocked or set up (`SyncConnection`).
pub struct Connection {
    pub key: VaultKey,
    pub projects: Vec<JsValue>,
    pub created: bool,
}

/// One layout (`SyncProvider`).
pub trait SyncProvider {
    fn connect(
        &mut self,
        passphrase: &JsString,
        random: &mut dyn RandomSource,
    ) -> Result<Connection, SyncError>;
    fn push(
        &mut self,
        projects: &[JsValue],
        key: &VaultKey,
        random: &mut dyn RandomSource,
    ) -> Result<(), SyncError>;
    fn remove(&mut self, project_ids: &[String]) -> Result<(), SyncError>;
}

/// Refuses a folder that holds encrypted-folder projects but no vault file: a second vault
/// would make them unreadable.
fn refuse_orphaned_projects(fs: &mut dyn SyncFileSystem, message: &str) -> Result<(), SyncError> {
    for entry in fs.list_dir(PROJECTS_DIR)? {
        if entry.is_directory
            && (fs.exists(&format!("{PROJECTS_DIR}/{}/project.enc", entry.name))?
                || fs.exists(&format!("{PROJECTS_DIR}/{}/project.json", entry.name))?)
        {
            return Err(SyncError::Folder(message.to_owned()));
        }
    }
    Ok(())
}

/// All projects in one encrypted file at the folder's root: one download and one upload per
/// sync, the simplest layout for cloud drives.
pub struct EncryptedFileProvider<'a> {
    fs: &'a mut dyn SyncFileSystem,
    header: Option<VaultHeader>,
    created_at: f64,
    hint: Option<String>,
    /// The time, in Unix milliseconds, for a newly created file.
    now: f64,
    vault_options: VaultOptions,
}

impl<'a> EncryptedFileProvider<'a> {
    pub fn new(fs: &'a mut dyn SyncFileSystem, now: f64) -> Self {
        Self {
            fs,
            header: None,
            created_at: 0.0,
            hint: None,
            now,
            vault_options: VaultOptions::default(),
        }
    }

    pub fn with_vault_options(mut self, options: VaultOptions) -> Self {
        self.vault_options = options;
        self
    }
}

impl SyncProvider for EncryptedFileProvider<'_> {
    fn connect(
        &mut self,
        passphrase: &JsString,
        random: &mut dyn RandomSource,
    ) -> Result<Connection, SyncError> {
        if self.fs.exists(BUNDLE_FILE)? {
            let bundle = EncryptedBundle::parse(&self.fs.read_text(BUNDLE_FILE)?)?;
            let key = bundle.unlock(passphrase)?;
            let projects = bundle.decrypt_projects(&key)?;
            self.created_at = bundle
                .raw
                .get("createdAt")
                .and_then(JsValue::as_number)
                .unwrap_or(0.0);
            self.hint = bundle.hint();
            self.header = Some(bundle.header);
            return Ok(Connection {
                key,
                projects,
                created: false,
            });
        }
        if self.fs.exists(VAULT_FILE)? {
            return Err(SyncError::Folder(
                "This folder is set up for encrypted-folder sync, not single-file sync.".into(),
            ));
        }
        if self.fs.exists(OPEN_MARKER_FILE)? {
            return Err(SyncError::Folder(
                "This folder is set up for open-folder sync, not single-file sync.".into(),
            ));
        }
        refuse_orphaned_projects(
            self.fs,
            "This folder contains project files but no sync vault file.",
        )?;
        let (header, key) = create_vault(passphrase, &self.vault_options, random)?;
        self.header = Some(header);
        self.created_at = self.now;
        self.hint = None;
        Ok(Connection {
            key,
            projects: Vec::new(),
            created: true,
        })
    }

    fn push(
        &mut self,
        projects: &[JsValue],
        key: &VaultKey,
        random: &mut dyn RandomSource,
    ) -> Result<(), SyncError> {
        let header = self
            .header
            .as_ref()
            .ok_or_else(|| SyncError::Folder("The sync file is locked.".into()))?;
        let created_at = if self.created_at != 0.0 {
            self.created_at
        } else {
            self.now
        };
        let content = write_encrypted_bundle(
            projects,
            header,
            key,
            "sync",
            created_at,
            self.hint.as_deref(),
            random,
        )?;
        let temporary = format!("{BUNDLE_FILE}.tmp");
        let written = self
            .fs
            .write_text(&temporary, &content)
            .and_then(|()| self.fs.rename(&temporary, BUNDLE_FILE));
        if let Err(error) = written {
            // Best effort: don't leave a stray or half-written file in the folder.
            if self.fs.exists(&temporary).unwrap_or(false) {
                let _ = self.fs.remove(&temporary);
            }
            return Err(error);
        }
        Ok(())
    }

    fn remove(&mut self, _project_ids: &[String]) -> Result<(), SyncError> {
        // Deleted projects are simply left out of the next file.
        Ok(())
    }
}

/// One encrypted file per record, discoverable from directory listings alone (no index).
pub struct FolderProvider<'a> {
    fs: &'a mut dyn SyncFileSystem,
    vault_options: VaultOptions,
}

impl<'a> FolderProvider<'a> {
    pub fn new(fs: &'a mut dyn SyncFileSystem) -> Self {
        Self {
            fs,
            vault_options: VaultOptions::default(),
        }
    }

    pub fn with_vault_options(mut self, options: VaultOptions) -> Self {
        self.vault_options = options;
        self
    }

    /// `pull`: every project directory, decrypted, sorted by id (UTF-16 code units).
    pub fn pull(&mut self, key: &VaultKey) -> Result<Vec<JsValue>, SyncError> {
        let mut projects = Vec::new();
        for entry in self.fs.list_dir(PROJECTS_DIR)? {
            if entry.is_directory
                && let Some(project) = read_project_files(self.fs, &entry.name, key)?
            {
                projects.push(project);
            }
        }
        let id = |p: &JsValue| {
            p.get("id")
                .and_then(JsValue::as_js_string)
                .map(|s| s.units().to_vec())
                .unwrap_or_default()
        };
        projects.sort_by_key(id);
        Ok(projects)
    }

    /// Removes the `.enc` files of records that are no longer live.
    fn prune(&mut self, dir: &str, keep: &[String]) -> Result<(), SyncError> {
        for kind in ENTITY_KINDS {
            let sub = format!("{dir}/{}", entity_dir(kind));
            for entry in self.fs.list_dir(&sub)? {
                let path = format!("{sub}/{}", entry.name);
                if !entry.is_directory && entry.name.ends_with(".enc") && !keep.contains(&path) {
                    self.fs.remove(&path)?;
                }
            }
        }
        Ok(())
    }
}

impl SyncProvider for FolderProvider<'_> {
    fn connect(
        &mut self,
        passphrase: &JsString,
        random: &mut dyn RandomSource,
    ) -> Result<Connection, SyncError> {
        if !self.fs.exists(VAULT_FILE)? {
            if self.fs.exists(BUNDLE_FILE)? {
                return Err(SyncError::Folder(
                    "This folder is set up for single-file sync, not encrypted-folder sync.".into(),
                ));
            }
            if self.fs.exists(OPEN_MARKER_FILE)? {
                return Err(SyncError::Folder(
                    "This folder is set up for open-folder sync, not encrypted-folder sync.".into(),
                ));
            }
            refuse_orphaned_projects(
                self.fs,
                "This folder contains project files but no vault file.",
            )?;
            let (header, key) = create_vault(passphrase, &self.vault_options, random)?;
            self.fs.create_dir(PROJECTS_DIR)?;
            self.fs
                .write_text(VAULT_FILE, &js::stringify_pretty(&header.raw))?;
            return Ok(Connection {
                key,
                projects: Vec::new(),
                created: true,
            });
        }
        let raw = js::parse(&self.fs.read_text(VAULT_FILE)?).ok();
        let header = raw.as_ref().and_then(VaultHeader::read).ok_or_else(|| {
            SyncError::Folder("The sync folder's vault file is not valid.".into())
        })?;
        let key = unlock_vault(&header, passphrase)?;
        let projects = self.pull(&key)?;
        Ok(Connection {
            key,
            projects,
            created: false,
        })
    }

    fn push(
        &mut self,
        projects: &[JsValue],
        key: &VaultKey,
        random: &mut dyn RandomSource,
    ) -> Result<(), SyncError> {
        self.fs.create_dir(PROJECTS_DIR)?;
        for project in projects {
            let id = project
                .get("id")
                .and_then(JsValue::as_js_string)
                .map(JsString::to_string_lossy)
                .unwrap_or_default();
            let dir = project_dir_name(&id);
            self.fs.create_dir(&dir)?;
            for kind in ENTITY_KINDS {
                self.fs.create_dir(&format!("{dir}/{}", entity_dir(kind)))?;
            }
            let files = encode_project_files(project, key, random)?;
            for file in &files {
                self.fs.write_text(&file.path, &file.content)?;
            }
            let keep: Vec<String> = files.into_iter().map(|f| f.path).collect();
            self.prune(&dir, &keep)?;
        }
        Ok(())
    }

    fn remove(&mut self, project_ids: &[String]) -> Result<(), SyncError> {
        for id in project_ids {
            self.fs.remove(&project_dir_name(id))?;
        }
        Ok(())
    }
}

/// `syncViaProvider`: connect, merge, write back, delete. The same for every layout, so
/// conflict handling can't drift between them.
#[allow(clippy::too_many_arguments)]
pub fn sync_via(
    provider: &mut dyn SyncProvider,
    passphrase: &JsString,
    local: &[JsValue],
    last_synced: &JsValue,
    now: f64,
    device_id: &str,
    random: &mut dyn RandomSource,
    suffix: &mut dyn FnMut() -> String,
) -> Result<SyncResult, SyncError> {
    let connection = provider.connect(passphrase, random)?;
    let merged = merge_projects(
        local,
        &connection.projects,
        last_synced,
        now,
        device_id,
        suffix,
    );
    provider.push(&merged.projects, &connection.key, random)?;
    if !merged.removed_ids.is_empty() {
        provider.remove(&merged.removed_ids)?;
    }
    Ok(SyncResult {
        projects: merged.projects,
        created: connection.created,
        imported: merged.imported,
        exported: merged.exported,
        removed: merged.removed_ids.len(),
        conflicts: merged.conflicts,
        last_synced: merged.last_synced,
    })
}
