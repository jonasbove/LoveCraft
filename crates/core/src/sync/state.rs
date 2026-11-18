//! The device's sync settings (`state.ts`): which folder, which layout, and the stamps from
//! the last sync. No project content and no keys. ProseCraft keeps this in `localStorage`
//! under `prosecraft-sync-state`; the desktop app keeps the same JSON in a file.
//!
//! Only folder sync is ported, so state for ProseCraft's Google Drive provider reads as not
//! set up.

use super::SyncMode;

/// ProseCraft's `localStorage` key for these settings.
pub const SYNC_STATE_KEY: &str = "prosecraft-sync-state";
use crate::js::{self, JsObject, JsValue};

#[derive(Clone, Debug, PartialEq)]
pub struct SyncState {
    /// The sync folder (a path on this device).
    pub folder: String,
    pub mode: SyncMode,
    /// id → `{ updatedAt, revision }` from the last sync.
    pub last_synced: JsValue,
}

impl SyncState {
    pub fn new(folder: String, mode: SyncMode) -> Self {
        Self {
            folder,
            mode,
            last_synced: JsValue::Object(JsObject::new()),
        }
    }

    /// `loadSyncState`: `None` when nothing usable is set up.
    pub fn read(text: &str) -> Option<Self> {
        let parsed = js::parse(text).ok()?;
        if parsed
            .get("provider")
            .and_then(JsValue::as_js_string)
            .is_some_and(|p| p.eq_str("googledrive"))
        {
            return None;
        }
        let folder = parsed
            .get("folder")
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .filter(|f| !f.is_empty())?;
        // State from before sync modes existed always used the encrypted folder.
        let mode = parsed
            .get("mode")
            .and_then(JsValue::as_js_string)
            .and_then(|m| SyncMode::from_id(&m.to_string_lossy()))
            .unwrap_or(SyncMode::EncryptedFolder);
        let mut last_synced = JsObject::new();
        if let Some(stamps) = parsed.get("lastSynced").and_then(JsValue::as_object) {
            for (id, stamp) in stamps.iter() {
                if let (Some(u), Some(r)) = (
                    stamp.get("updatedAt").and_then(JsValue::as_number),
                    stamp.get("revision").and_then(JsValue::as_number),
                ) {
                    let mut s = JsObject::new();
                    s.insert("updatedAt", JsValue::Number(u));
                    s.insert("revision", JsValue::Number(r));
                    last_synced.insert(id.clone(), JsValue::Object(s));
                }
            }
        }
        Some(Self {
            folder,
            mode,
            last_synced: JsValue::Object(last_synced),
        })
    }

    /// `saveSyncState`, with ProseCraft's fields (the Google Drive ones empty).
    pub fn to_text(&self) -> String {
        let mut o = JsObject::new();
        o.insert("provider", JsValue::str("folder"));
        o.insert("folder", JsValue::str(&self.folder));
        o.insert("mode", JsValue::str(self.mode.id()));
        for key in ["googleDriveAccessToken", "googleDriveRefreshToken"] {
            o.insert(key, JsValue::str(""));
        }
        o.insert("googleDriveTokenExpiry", JsValue::Number(0.0));
        o.insert("googleDriveEmail", JsValue::str(""));
        o.insert("googleDriveClientKind", JsValue::str("desktop"));
        o.insert("lastSynced", self.last_synced.clone());
        js::stringify(&JsValue::Object(o))
    }
}

#[cfg(test)]
mod tests {
    use super::{SyncMode, SyncState};

    #[test]
    fn reads_prosecraft_state() {
        let old = SyncState::read(r#"{"folder":"/x","lastSynced":{"p":{"updatedAt":5,"revision":2},"bad":{"updatedAt":"5"}}}"#).expect("state");
        assert_eq!(old.mode, SyncMode::EncryptedFolder);
        assert_eq!(
            crate::js::stringify(&old.last_synced),
            r#"{"p":{"updatedAt":5,"revision":2}}"#
        );
        assert!(
            SyncState::read(r#"{"provider":"googledrive","googleDriveRefreshToken":"t"}"#)
                .is_none()
        );
        assert!(SyncState::read(r#"{"folder":""}"#).is_none());
        let state = SyncState::new("/f".into(), SyncMode::EncryptedFile);
        assert_eq!(SyncState::read(&state.to_text()), Some(state));
    }
}
