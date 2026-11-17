//! ProseCraft backups (`.prosecraft` files), plain and encrypted.
//!
//! Ported from ProseCraft `src/lib/storage/repository.ts` (`createBundleText`,
//! `importBundleText`) and `src/lib/storage/encrypted-bundle.ts` (upstream `7dae544`). Both are
//! JSON written with an indent of two; the encrypted one doubles as the single-file sync
//! container. Projects stay [`JsValue`]s, so nothing in them is lost on the way through.

use crate::js::{self, JsObject, JsString, JsValue};

use super::crypto::{
    self, CryptoError, ENVELOPE_VERSION, EntityEnvelope, EntityMeta, RandomSource, VaultHeader,
    VaultKey,
};
use super::schema::{SCHEMA_VERSION, is_stored_project};

/// `format` of a plain backup.
pub const PLAIN_FORMAT: &str = "prosecraft.bundle";
/// `format` of an encrypted backup (`ENCRYPTED_BUNDLE_FORMAT`).
pub const ENCRYPTED_FORMAT: &str = "prosecraft.encrypted";

/// What can go wrong reading a backup. The messages are ProseCraft's.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BundleError {
    #[error("That file is not valid ProseCraft backup data.")]
    NotJson,
    #[error("That file is not a ProseCraft backup.")]
    NotABackup,
    #[error("The backup did not contain any projects.")]
    NoProjects,
    #[error("The backup did not contain any valid projects.")]
    NoValidProjects,
    #[error("That file is not an encrypted ProseCraft backup.")]
    NotEncrypted,
    #[error("The backup did not contain any readable projects.")]
    NoReadableProjects,
    #[error(transparent)]
    Crypto(#[from] CryptoError),
}

fn parse_object(text: &str) -> Result<JsValue, BundleError> {
    let value = js::parse(text).map_err(|_| BundleError::NotJson)?;
    match value {
        JsValue::Object(_) => Ok(value),
        // `typeof parsed !== "object" || parsed === null`; an array passes that check in
        // ProseCraft and then fails on `format`, with the same message.
        _ => Err(BundleError::NotABackup),
    }
}

fn format_is(value: &JsValue, format: &str) -> bool {
    value
        .get("format")
        .and_then(JsValue::as_js_string)
        .is_some_and(|f| f.eq_str(format))
}

/// A plain backup as read: the whole file, and which of its projects are valid.
#[derive(Clone, Debug, PartialEq)]
pub struct PlainBundle {
    /// The file as parsed, every field kept.
    pub raw: JsValue,
}

impl PlainBundle {
    /// `importBundleText`'s checks: a `prosecraft.bundle` with at least one valid project.
    pub fn parse(text: &str) -> Result<Self, BundleError> {
        let raw = parse_object(text)?;
        if !format_is(&raw, PLAIN_FORMAT) {
            return Err(BundleError::NotABackup);
        }
        let projects = raw
            .get("projects")
            .and_then(JsValue::as_array)
            .ok_or(BundleError::NoProjects)?;
        if !projects.iter().any(is_stored_project) {
            return Err(BundleError::NoValidProjects);
        }
        Ok(Self { raw })
    }

    /// The projects ProseCraft would import (`isStoredProject`); others are skipped, as there.
    pub fn projects(&self) -> Vec<&JsValue> {
        self.raw
            .get("projects")
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .filter(|project| is_stored_project(project))
            .collect()
    }

    /// The file's text as ProseCraft would write this backup.
    pub fn to_text(&self) -> String {
        js::stringify_pretty(&self.raw)
    }
}

/// `createBundleText`: a plain backup of `projects`.
pub fn write_plain_bundle(projects: &[JsValue], exported_at: f64, device_id: &str) -> String {
    let mut bundle = JsObject::new();
    bundle.insert("format", JsValue::str(PLAIN_FORMAT));
    bundle.insert("schemaVersion", JsValue::Number(SCHEMA_VERSION));
    bundle.insert("exportedAt", JsValue::Number(exported_at));
    bundle.insert("deviceId", JsValue::str(device_id));
    bundle.insert("projects", JsValue::Array(projects.to_vec()));
    js::stringify_pretty(&JsValue::Object(bundle))
}

/// An encrypted backup as read, before it is unlocked (`parseEncryptedBundle`).
#[derive(Clone, Debug, PartialEq)]
pub struct EncryptedBundle {
    pub raw: JsValue,
    pub header: VaultHeader,
}

impl EncryptedBundle {
    pub fn parse(text: &str) -> Result<Self, BundleError> {
        let raw = parse_object(text)?;
        let version_ok = raw.get("version").and_then(JsValue::as_number) == Some(ENVELOPE_VERSION);
        let header = raw.get("header").and_then(VaultHeader::read);
        let entities_ok = matches!(raw.get("entities"), Some(JsValue::Array(_)));
        match header {
            Some(header) if format_is(&raw, ENCRYPTED_FORMAT) && version_ok && entities_ok => {
                Ok(Self { raw, header })
            }
            _ => Err(BundleError::NotEncrypted),
        }
    }

    /// The cleartext passphrase hint (`readEncryptedBundleHint`).
    pub fn hint(&self) -> Option<String> {
        self.raw
            .get("hint")
            .and_then(JsValue::as_js_string)
            .filter(|hint| !hint.is_empty())
            .map(JsString::to_string_lossy)
    }

    /// `unlockVault` with this backup's header.
    pub fn unlock(&self, passphrase: &JsString) -> Result<VaultKey, BundleError> {
        Ok(crypto::unlock_vault(&self.header, passphrase)?)
    }

    /// `decryptBundleProjects`: every project envelope, decrypted. Other kinds are skipped (a
    /// different or future layout), and so is a payload whose id differs from its authenticated
    /// envelope id (tampering). A record that fails to decrypt fails the whole read, as there.
    pub fn decrypt_projects(&self, key: &VaultKey) -> Result<Vec<JsValue>, BundleError> {
        let mut projects = Vec::new();
        for value in self
            .raw
            .get("entities")
            .and_then(JsValue::as_array)
            .unwrap_or_default()
        {
            let Some(envelope) = EntityEnvelope::read(value) else {
                continue;
            };
            if !envelope.meta.kind.eq_str("project") {
                continue;
            }
            let payload = crypto::decrypt_entity(key, &envelope)?;
            let id_matches =
                payload.get("id").and_then(JsValue::as_js_string) == Some(&envelope.meta.id);
            if is_stored_project(&payload) && id_matches {
                projects.push(payload);
            }
        }
        Ok(projects)
    }

    /// `decryptEncryptedBundle`: unlock and decrypt, failing when nothing was readable.
    pub fn open(&self, passphrase: &JsString) -> Result<(VaultKey, Vec<JsValue>), BundleError> {
        let key = self.unlock(passphrase)?;
        let projects = self.decrypt_projects(&key)?;
        if projects.is_empty() {
            return Err(BundleError::NoReadableProjects);
        }
        Ok((key, projects))
    }
}

/// The envelope metadata ProseCraft uses for a project in an encrypted backup.
fn project_meta(project: &JsValue, device_id: &str) -> EntityMeta {
    let text = |key: &str| {
        project
            .get(key)
            .and_then(JsValue::as_js_string)
            .cloned()
            .unwrap_or_default()
    };
    let updated_by = text("updatedBy");
    EntityMeta {
        kind: JsString::from("project"),
        id: text("id"),
        project_id: text("id"),
        // `project.updatedBy || deviceId`
        device_id: if updated_by.is_empty() {
            JsString::from(device_id)
        } else {
            updated_by
        },
        updated_at: project
            .get("updatedAt")
            .and_then(JsValue::as_number)
            .unwrap_or(0.0),
        revision: project
            .get("revision")
            .and_then(JsValue::as_number)
            .unwrap_or(0.0),
    }
}

/// `serializeEncryptedBundle`: `projects` sealed under an existing vault.
pub fn write_encrypted_bundle(
    projects: &[JsValue],
    header: &VaultHeader,
    key: &VaultKey,
    device_id: &str,
    created_at: f64,
    hint: Option<&str>,
    random: &mut dyn RandomSource,
) -> Result<String, BundleError> {
    let envelopes = projects
        .iter()
        .map(|project| {
            crypto::encrypt_entity(key, project_meta(project, device_id), project, random)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(assemble_encrypted(&envelopes, header, created_at, hint))
}

/// Builds the file around already-sealed records.
pub fn assemble_encrypted(
    envelopes: &[EntityEnvelope],
    header: &VaultHeader,
    created_at: f64,
    hint: Option<&str>,
) -> String {
    let mut bundle = JsObject::new();
    bundle.insert("format", JsValue::str(ENCRYPTED_FORMAT));
    bundle.insert("version", JsValue::Number(ENVELOPE_VERSION));
    bundle.insert("createdAt", JsValue::Number(created_at));
    if let Some(hint) = hint {
        bundle.insert("hint", JsValue::str(hint));
    }
    bundle.insert("header", header.raw.clone());
    bundle.insert(
        "entities",
        JsValue::Array(envelopes.iter().map(EntityEnvelope::to_js).collect()),
    );
    js::stringify_pretty(&JsValue::Object(bundle))
}

/// Re-seals `projects` with the nonces of `previous`, in order. Only for tests that compare
/// with ProseCraft's own output byte for byte; real writes use [`write_encrypted_bundle`].
#[doc(hidden)]
pub fn reseal_with_nonces(
    projects: &[JsValue],
    previous: &[EntityEnvelope],
    key: &VaultKey,
    device_id: &str,
) -> Result<Vec<EntityEnvelope>, BundleError> {
    projects
        .iter()
        .zip(previous)
        .map(|(project, old)| {
            crypto::encrypt_entity_with_nonce(
                key,
                project_meta(project, device_id),
                project,
                old.nonce,
            )
            .map_err(BundleError::from)
        })
        .collect()
}
