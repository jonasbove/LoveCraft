//! The encrypted folder's files (`layout.ts`):
//!
//! ```text
//! projects/<id>/project.enc                 project fields, entity metadata and order
//! projects/<id>/chapters/<chapterId>.enc
//! projects/<id>/documents/<documentId>.enc
//! projects/<id>/storyboard/rows/<rowId>.enc
//! projects/<id>/storyboard/cards/<cardId>.enc
//! ```
//!
//! Deleted records live only as tombstones in `project.enc`, so a deletion needs no file.

use base64::Engine;

use super::{PROJECTS_DIR, SyncError, SyncFileSystem};
use crate::js::{self, JsObject, JsString, JsValue};
use crate::storage::crypto::{
    EntityEnvelope, EntityMeta, RandomSource, VaultKey, decrypt_entity, encrypt_entity,
};
use crate::storage::schema::{ENTITY_KINDS, SCHEMA_VERSION, is_entity_meta_map, is_project_data};

/// `safeName`: a record id as one safe path segment. Safe ids stay as they are; anything
/// else is its UTF-8 in base64url behind `~` (which safe ids can't contain), so the mapping
/// is one-to-one and never `.` or `..`.
pub fn safe_name(id: &str) -> String {
    let mut chars = id.chars();
    let safe = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if safe {
        id.to_owned()
    } else {
        format!(
            "~{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(id.as_bytes())
        )
    }
}

pub fn project_dir_name(id: &str) -> String {
    format!("{PROJECTS_DIR}/{}", safe_name(id))
}

pub fn entity_dir(kind: &str) -> &'static str {
    match kind {
        "documents" => "documents",
        "chapters" => "chapters",
        "storyboardRows" => "storyboard/rows",
        _ => "storyboard/cards",
    }
}

fn entity_kind_name(kind: &str) -> &'static str {
    match kind {
        "documents" => "document",
        "chapters" => "chapter",
        "storyboardRows" => "storyboard-row",
        _ => "storyboard-card",
    }
}

pub fn entity_file_name(id: &str) -> String {
    format!("{}.enc", safe_name(id))
}

/// A file to write: its path in the folder and its content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedFile {
    pub path: String,
    pub content: String,
}

fn get_str(value: &JsValue, key: &str) -> JsString {
    value
        .get(key)
        .and_then(JsValue::as_js_string)
        .cloned()
        .unwrap_or_default()
}

fn get_num(value: &JsValue, key: &str) -> f64 {
    value.get(key).and_then(JsValue::as_number).unwrap_or(0.0)
}

fn envelope_file(
    key: &VaultKey,
    meta: EntityMeta,
    payload: &JsValue,
    random: &mut dyn RandomSource,
) -> Result<String, SyncError> {
    Ok(js::stringify_pretty(
        &encrypt_entity(key, meta, payload, random)?.to_js(),
    ))
}

/// `encodeProjectFiles`: a stored project as the folder's files, in ProseCraft's order.
pub fn encode_project_files(
    stored: &JsValue,
    key: &VaultKey,
    random: &mut dyn RandomSource,
) -> Result<Vec<EncodedFile>, SyncError> {
    let id = get_str(stored, "id");
    let dir = project_dir_name(&id.to_string_lossy());
    let project = stored
        .get("project")
        .cloned()
        .unwrap_or(JsValue::Object(JsObject::new()));
    let list = |kind: &str| {
        project
            .get(kind)
            .and_then(JsValue::as_array)
            .map(<[JsValue]>::to_vec)
            .unwrap_or_default()
    };
    let mut rest = project.as_object().cloned().unwrap_or_default();
    let mut order = JsObject::new();
    for kind in ENTITY_KINDS {
        rest.remove(kind);
        order.insert(
            kind,
            JsValue::Array(
                list(kind)
                    .iter()
                    .map(|e| JsValue::String(get_str(e, "id")))
                    .collect(),
            ),
        );
    }
    for kind in ENTITY_KINDS {
        rest.insert(kind, JsValue::Array(Vec::new()));
    }
    let entities = stored
        .get("entities")
        .cloned()
        .unwrap_or(JsValue::Object(JsObject::new()));
    let mut payload = JsObject::new();
    payload.insert(
        "createdAt",
        stored
            .get("createdAt")
            .cloned()
            .unwrap_or(JsValue::Number(0.0)),
    );
    payload.insert(
        "deletedAt",
        stored.get("deletedAt").cloned().unwrap_or(JsValue::Null),
    );
    payload.insert("entities", entities.clone());
    payload.insert("order", JsValue::Object(order));
    payload.insert("project", JsValue::Object(rest));
    let mut files = vec![EncodedFile {
        path: format!("{dir}/project.enc"),
        content: envelope_file(
            key,
            EntityMeta {
                kind: "project".into(),
                id: id.clone(),
                project_id: id.clone(),
                device_id: get_str(stored, "updatedBy"),
                updated_at: get_num(stored, "updatedAt"),
                revision: get_num(stored, "revision"),
            },
            &JsValue::Object(payload),
            random,
        )?,
    }];
    for kind in ENTITY_KINDS {
        for entity in list(kind) {
            let entity_id = get_str(&entity, "id");
            let Some(meta) = entities
                .get(kind)
                .and_then(|m| m.as_object())
                .and_then(|m| {
                    m.iter()
                        .find(|(k, _)| **k == entity_id)
                        .map(|(_, v)| v.clone())
                })
            else {
                continue;
            };
            if !matches!(meta.get("deletedAt"), Some(JsValue::Null)) {
                continue;
            }
            files.push(EncodedFile {
                path: format!(
                    "{dir}/{}/{}",
                    entity_dir(kind),
                    entity_file_name(&entity_id.to_string_lossy())
                ),
                content: envelope_file(
                    key,
                    EntityMeta {
                        kind: entity_kind_name(kind).into(),
                        id: entity_id,
                        project_id: id.clone(),
                        device_id: get_str(&meta, "updatedBy"),
                        updated_at: get_num(&meta, "updatedAt"),
                        revision: get_num(&meta, "revision"),
                    },
                    &entity,
                    random,
                )?,
            });
        }
    }
    Ok(files)
}

fn is_project_file_payload(value: &JsValue) -> bool {
    let Some(o) = value.as_object() else {
        return false;
    };
    let created = o.get("createdAt").and_then(JsValue::as_number).is_some();
    let deleted = matches!(o.get("deletedAt"), Some(JsValue::Null | JsValue::Number(_)));
    let (Some(entities), Some(order)) = (
        o.get("entities").filter(|e| e.as_object().is_some()),
        o.get("order").filter(|e| e.as_object().is_some()),
    ) else {
        return false;
    };
    created
        && deleted
        && is_project_data(o.get("project"))
        && ENTITY_KINDS.iter().all(|kind| {
            is_entity_meta_map(entities.get(kind))
                && order
                    .get(kind)
                    .and_then(JsValue::as_array)
                    .is_some_and(|ids| ids.iter().all(|id| id.as_js_string().is_some()))
        })
}

fn read_json(
    fs: &mut dyn SyncFileSystem,
    path: &str,
    failure: String,
) -> Result<JsValue, SyncError> {
    let text = fs
        .read_text(path)
        .map_err(|_| SyncError::Folder(failure.clone()))?;
    js::parse(&text).map_err(|_| SyncError::Folder(failure))
}

fn read_entities(
    fs: &mut dyn SyncFileSystem,
    dir: &str,
    kind: &str,
    payload: &JsValue,
    key: &VaultKey,
    project_id: &JsString,
) -> Result<Vec<JsValue>, SyncError> {
    let metas = payload
        .get("entities")
        .and_then(|e| e.get(kind))
        .and_then(JsValue::as_object)
        .cloned()
        .unwrap_or_default();
    let mut ids: Vec<JsString> = Vec::new();
    let ordered = payload
        .get("order")
        .and_then(|o| o.get(kind))
        .and_then(JsValue::as_array)
        .unwrap_or_default();
    for id in ordered
        .iter()
        .filter_map(JsValue::as_js_string)
        .chain(metas.iter().map(|(k, _)| k))
    {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    let mut result = Vec::new();
    for id in ids {
        let Some(meta) = metas.iter().find(|(k, _)| **k == id).map(|(_, v)| v) else {
            continue;
        };
        if !matches!(meta.get("deletedAt"), Some(JsValue::Null)) {
            continue;
        }
        let path = format!(
            "{dir}/{}/{}",
            entity_dir(kind),
            entity_file_name(&id.to_string_lossy())
        );
        // A live record missing on disk is an interrupted upload or an outside deletion:
        // corruption, not a deletion to sync back.
        if !fs.exists(&path)? {
            return Err(SyncError::Folder(format!(
                "The entity file {path} is missing."
            )));
        }
        let value = read_json(
            fs,
            &path,
            format!("The entity file {path} could not be read."),
        )?;
        // Routing fields are checked against the expected place, so a valid record copied
        // from another project can't be slipped in.
        let envelope = EntityEnvelope::read(&value).filter(|e| {
            e.meta.id == id
                && e.meta.kind.eq_str(entity_kind_name(kind))
                && e.meta.project_id == *project_id
        });
        let Some(envelope) = envelope else {
            return Err(SyncError::Folder(format!(
                "The entity file {path} is not a valid ProseCraft record for this project."
            )));
        };
        let entity = decrypt_entity(key, &envelope)?;
        if entity.as_object().is_none() {
            return Err(SyncError::Folder(format!(
                "The entity file {path} has an unsupported format."
            )));
        }
        result.push(entity);
    }
    Ok(result)
}

/// `readProjectFiles`: one project directory, decrypted. `None` for a directory without a
/// header (an incomplete upload).
pub fn read_project_files(
    fs: &mut dyn SyncFileSystem,
    dir_name: &str,
    key: &VaultKey,
) -> Result<Option<JsValue>, SyncError> {
    let dir = format!("{PROJECTS_DIR}/{dir_name}");
    let header_path = format!("{dir}/project.enc");
    if !fs.exists(&header_path)? {
        return Ok(None);
    }
    let value = read_json(
        fs,
        &header_path,
        format!("The project file in {dir_name} could not be read."),
    )?;
    let Some(envelope) = EntityEnvelope::read(&value).filter(|e| e.meta.kind.eq_str("project"))
    else {
        return Err(SyncError::Folder(format!(
            "The project file in {dir_name} is not a valid ProseCraft record."
        )));
    };
    if envelope.meta.project_id != envelope.meta.id {
        return Err(SyncError::Folder(format!(
            "The project file in {dir_name} has inconsistent routing metadata."
        )));
    }
    let project_id = envelope.meta.id.clone();
    let payload = decrypt_entity(key, &envelope)?;
    if !is_project_file_payload(&payload) {
        return Err(SyncError::Folder(format!(
            "The project in {dir_name} has an unsupported format."
        )));
    }
    if payload
        .get("project")
        .and_then(|p| p.get("id"))
        .and_then(JsValue::as_js_string)
        != Some(&project_id)
    {
        return Err(SyncError::Folder(format!(
            "The project in {dir_name} has inconsistent identity metadata."
        )));
    }
    let mut project = payload
        .get("project")
        .and_then(JsValue::as_object)
        .cloned()
        .unwrap_or_default();
    project.insert("id", JsValue::String(project_id.clone()));
    for kind in ["documents", "chapters", "storyboardRows", "storyboardCards"] {
        let entities = read_entities(fs, &dir, kind, &payload, key, &project_id)?;
        project.insert(kind, JsValue::Array(entities));
    }
    let mut stored = JsObject::new();
    stored.insert("schemaVersion", JsValue::Number(SCHEMA_VERSION));
    stored.insert("id", JsValue::String(project_id));
    stored.insert(
        "createdAt",
        payload
            .get("createdAt")
            .cloned()
            .unwrap_or(JsValue::Number(0.0)),
    );
    stored.insert("updatedAt", JsValue::Number(envelope.meta.updated_at));
    stored.insert("revision", JsValue::Number(envelope.meta.revision));
    stored.insert(
        "updatedBy",
        JsValue::String(envelope.meta.device_id.clone()),
    );
    stored.insert(
        "deletedAt",
        payload.get("deletedAt").cloned().unwrap_or(JsValue::Null),
    );
    stored.insert("project", JsValue::Object(project));
    stored.insert(
        "entities",
        payload
            .get("entities")
            .cloned()
            .unwrap_or(JsValue::Object(JsObject::new())),
    );
    Ok(Some(JsValue::Object(stored)))
}
