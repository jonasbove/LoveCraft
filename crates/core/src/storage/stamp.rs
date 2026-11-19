//! Change detection and revision stamping of stored projects.
//!
//! Ported from ProseCraft `src/lib/storage/schema.ts` (`projectContentKey`, `createStoredProject`,
//! `stampProject`, `stampEntities`) at upstream `7dae544`. A project's revision moves only when
//! its synced content changes; each record keeps its own revision, and a removed record becomes
//! a tombstone so the deletion syncs.

use crate::js::{self, JsObject, JsValue};

use super::schema::{ENTITY_KINDS, SCHEMA_VERSION};

fn or_default(value: Option<&JsValue>, default: JsValue) -> JsValue {
    match value {
        None | Some(JsValue::Null) => default,
        Some(v) => v.clone(),
    }
}

/// `projectKindOf`: `"screenplay"`, else `"novel"`.
fn kind_of(project: &JsValue) -> &'static str {
    match project.get("kind").and_then(JsValue::as_js_string) {
        Some(kind) if kind.eq_str("screenplay") => "screenplay",
        _ => "novel",
    }
}

/// `projectContentKey`: the fields that take part in sync, so that opening a scene (the active
/// document) is not a change.
///
/// As in ProseCraft since #40 (`c033cca`), the key holds `trashed`, so trashing or restoring a
/// whole project counts as a change and syncs; since #31 (`bcb884d`) it holds the project's
/// sheet templates and dismissed relation suggestions too.
pub fn project_content_key(project: &JsValue) -> String {
    let get = |key: &str| project.get(key);
    let mut key = JsObject::new();
    // Plain reads in ProseCraft: an absent field is `undefined`, which stableStringify drops.
    let plain = |key: &mut JsObject, field: &str| {
        if let Some(value) = get(field) {
            key.insert(field, value.clone());
        }
    };
    plain(&mut key, "id");
    key.insert("kind", JsValue::str(kind_of(project)));
    plain(&mut key, "name");
    plain(&mut key, "author");
    key.insert(
        "trashed",
        JsValue::Bool(matches!(get("trashed"), Some(JsValue::Bool(true)))),
    );
    for (field, default) in [
        ("documents", JsValue::Array(Vec::new())),
        ("chapters", JsValue::Array(Vec::new())),
        ("projectGoal", JsValue::Null),
        ("chapterGoals", JsValue::Null),
        ("activity", JsValue::Null),
        ("storyboardCards", JsValue::Array(Vec::new())),
        ("storyboardRows", JsValue::Array(Vec::new())),
        ("storyboardDrawings", JsValue::Array(Vec::new())),
        ("timelineNodes", JsValue::Array(Vec::new())),
        ("noteFolders", JsValue::Array(Vec::new())),
        ("researchFolderName", JsValue::str("")),
        ("researchFolders", JsValue::Array(Vec::new())),
        ("researchItems", JsValue::Array(Vec::new())),
        ("publishOptions", JsValue::Null),
        ("comments", JsValue::Array(Vec::new())),
        ("snapshots", JsValue::Array(Vec::new())),
        ("labels", JsValue::Array(Vec::new())),
        ("sheetTemplates", JsValue::Array(Vec::new())),
        ("dismissedSuggestions", JsValue::Array(Vec::new())),
        ("citations", JsValue::Array(Vec::new())),
    ] {
        // The first five are plain reads in ProseCraft (`project.documents`); undefined there
        // drops out of stableStringify, which an absent key here does too.
        let value = match field {
            "documents" | "chapters" | "projectGoal" | "chapterGoals" | "activity" => {
                match get(field) {
                    Some(v) => v.clone(),
                    None => continue,
                }
            }
            _ => or_default(get(field), default),
        };
        key.insert(field, value);
    }
    js::stable_stringify(&JsValue::Object(key))
}

pub fn projects_have_same_content(a: &JsValue, b: &JsValue) -> bool {
    project_content_key(a) == project_content_key(b)
}

/// The records of one kind (`entityList`): documents, chapters, storyboard rows or cards.
fn entity_list<'a>(project: &'a JsValue, kind: &str) -> &'a [JsValue] {
    project
        .get(kind)
        .and_then(JsValue::as_array)
        .unwrap_or_default()
}

fn entity_id(entity: &JsValue) -> Option<&crate::js::JsString> {
    entity.get("id").and_then(JsValue::as_js_string)
}

fn meta(
    updated_at: JsValue,
    revision: JsValue,
    updated_by: JsValue,
    deleted_at: JsValue,
) -> JsValue {
    let mut meta = JsObject::new();
    meta.insert("updatedAt", updated_at);
    meta.insert("revision", revision);
    meta.insert("updatedBy", updated_by);
    meta.insert("deletedAt", deleted_at);
    JsValue::Object(meta)
}

/// `createEntityMaps`: every record new at `now`.
fn create_entity_maps(project: &JsValue, device_id: &str, now: f64) -> JsValue {
    let mut maps = JsObject::new();
    for kind in ENTITY_KINDS {
        let mut map = JsObject::new();
        for entity in entity_list(project, kind) {
            if let Some(id) = entity_id(entity) {
                map.insert(
                    id.clone(),
                    meta(
                        JsValue::Number(now),
                        JsValue::Number(1.0),
                        JsValue::str(device_id),
                        JsValue::Null,
                    ),
                );
            }
        }
        maps.insert(kind, JsValue::Object(map));
    }
    JsValue::Object(maps)
}

/// `createStoredProject`.
pub fn create_stored_project(project: JsValue, device_id: &str, now: f64) -> JsValue {
    let mut stored = JsObject::new();
    stored.insert("schemaVersion", JsValue::Number(SCHEMA_VERSION));
    stored.insert("id", or_default(project.get("id"), JsValue::Null));
    stored.insert("createdAt", JsValue::Number(now));
    stored.insert("updatedAt", JsValue::Number(now));
    stored.insert("revision", JsValue::Number(1.0));
    stored.insert("updatedBy", JsValue::str(device_id));
    stored.insert("deletedAt", JsValue::Null);
    let entities = create_entity_maps(&project, device_id, now);
    stored.insert("project", project);
    stored.insert("entities", entities);
    JsValue::Object(stored)
}

/// `stampEntities`.
fn stamp_entities(
    previous: Option<&JsValue>,
    project: &JsValue,
    device_id: &str,
    now: f64,
) -> JsValue {
    let mut result = JsObject::new();
    for kind in ENTITY_KINDS {
        let previous_map = previous
            .and_then(|p| p.get("entities"))
            .and_then(|e| e.get(kind))
            .and_then(JsValue::as_object);
        let previous_list = previous
            .and_then(|p| p.get("project"))
            .map(|p| entity_list(p, kind))
            .unwrap_or_default();
        let mut map = JsObject::new();
        let mut seen = Vec::new();
        for entity in entity_list(project, kind) {
            let Some(id) = entity_id(entity) else {
                continue;
            };
            seen.push(id.clone());
            let prev_meta =
                previous_map.and_then(|m| m.iter().find(|(k, _)| *k == id).map(|(_, v)| v));
            let prev_entity = previous_list.iter().find(|e| entity_id(e) == Some(id));
            let unchanged = matches!((prev_meta, prev_entity), (Some(_), Some(prev)) if js::stable_stringify(prev) == js::stable_stringify(entity));
            let stamped = match (unchanged, prev_meta) {
                (true, Some(prev)) => meta(
                    or_default(prev.get("updatedAt"), JsValue::Null),
                    or_default(prev.get("revision"), JsValue::Null),
                    or_default(prev.get("updatedBy"), JsValue::Null),
                    JsValue::Null,
                ),
                _ => {
                    let revision = prev_meta
                        .and_then(|m| m.get("revision"))
                        .and_then(JsValue::as_number)
                        .unwrap_or(0.0)
                        + 1.0;
                    meta(
                        JsValue::Number(now),
                        JsValue::Number(revision),
                        JsValue::str(device_id),
                        JsValue::Null,
                    )
                }
            };
            map.insert(id.clone(), stamped);
        }
        // Records that disappeared become tombstones; an existing tombstone is kept as it is.
        if let Some(previous_map) = previous_map {
            for (id, prev) in previous_map.iter() {
                if seen.contains(id) {
                    continue;
                }
                let tombstoned = !matches!(prev.get("deletedAt"), Some(JsValue::Null) | None);
                let stamped = if tombstoned {
                    prev.clone()
                } else {
                    let revision = prev
                        .get("revision")
                        .and_then(JsValue::as_number)
                        .unwrap_or(0.0)
                        + 1.0;
                    meta(
                        JsValue::Number(now),
                        JsValue::Number(revision),
                        JsValue::str(device_id),
                        JsValue::Number(now),
                    )
                };
                map.insert(id.clone(), stamped);
            }
        }
        result.insert(kind, JsValue::Object(map));
    }
    JsValue::Object(result)
}

/// `stampProject`: the stored project after a save of `project`.
pub fn stamp_project(
    previous: Option<&JsValue>,
    project: JsValue,
    device_id: &str,
    now: f64,
) -> JsValue {
    let previous_project = previous.and_then(|p| p.get("project"));
    let changed = previous_project.is_none_or(|p| !projects_have_same_content(p, &project));
    let prev = |key: &str| previous.and_then(|p| p.get(key)).cloned();
    let mut stored = JsObject::new();
    stored.insert("schemaVersion", JsValue::Number(SCHEMA_VERSION));
    stored.insert("id", or_default(project.get("id"), JsValue::Null));
    stored.insert(
        "createdAt",
        prev("createdAt").unwrap_or(JsValue::Number(now)),
    );
    let updated_at = if changed {
        JsValue::Number(now)
    } else {
        prev("updatedAt").unwrap_or(JsValue::Number(now))
    };
    stored.insert("updatedAt", updated_at);
    let previous_revision = prev("revision").and_then(|r| r.as_number());
    let revision = if changed {
        previous_revision.unwrap_or(0.0) + 1.0
    } else {
        previous_revision.unwrap_or(1.0)
    };
    stored.insert("revision", JsValue::Number(revision));
    let updated_by = if changed {
        JsValue::str(device_id)
    } else {
        prev("updatedBy").unwrap_or_else(|| JsValue::str(device_id))
    };
    stored.insert("updatedBy", updated_by);
    stored.insert("deletedAt", prev("deletedAt").unwrap_or(JsValue::Null));
    let entities = stamp_entities(previous, &project, device_id, now);
    stored.insert("project", project);
    stored.insert("entities", entities);
    JsValue::Object(stored)
}
