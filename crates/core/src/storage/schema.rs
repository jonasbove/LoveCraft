//! The stored project envelope and its checks.
//!
//! Ported from ProseCraft `src/lib/storage/schema.ts` and `types.ts` (upstream `7dae544`). Values
//! stay [`JsValue`]s so every field, including ones a newer release added, survives a round
//! trip; these functions only decide whether a value has the shape ProseCraft accepts.

use crate::js::JsValue;

/// `SCHEMA_VERSION` in `types.ts`: bumped whenever the stored envelope changes shape.
pub const SCHEMA_VERSION: f64 = 2.0;

/// `ENTITY_KINDS`: the record lists that carry per-entity sync metadata.
pub const ENTITY_KINDS: [&str; 4] = ["documents", "chapters", "storyboardRows", "storyboardCards"];

fn is_number(value: Option<&JsValue>) -> bool {
    matches!(value, Some(JsValue::Number(_)))
}

fn is_finite_number(value: Option<&JsValue>) -> bool {
    matches!(value, Some(JsValue::Number(n)) if n.is_finite())
}

fn is_string(value: Option<&JsValue>) -> bool {
    matches!(value, Some(JsValue::String(_)))
}

fn is_null_or_number(value: Option<&JsValue>) -> bool {
    matches!(value, Some(JsValue::Null | JsValue::Number(_)))
}

/// `typeof value === "object" && value !== null`: an object or an array.
fn is_object_like(value: Option<&JsValue>) -> bool {
    matches!(value, Some(JsValue::Object(_) | JsValue::Array(_)))
}

/// `isEntityMeta`.
pub fn is_entity_meta(value: &JsValue) -> bool {
    value.as_object().is_some_and(|meta| {
        is_finite_number(meta.get("updatedAt"))
            && is_finite_number(meta.get("revision"))
            && is_string(meta.get("updatedBy"))
            && is_null_or_number(meta.get("deletedAt"))
    })
}

/// `isEntityMetaMap`: a plain object whose every value is entity metadata.
pub fn is_entity_meta_map(value: Option<&JsValue>) -> bool {
    match value {
        Some(JsValue::Object(map)) => map.iter().all(|(_, meta)| is_entity_meta(meta)),
        _ => false,
    }
}

/// `isLegacyProjectData`: the unversioned `ProjectData` shape, also the `project` field of a
/// stored project.
pub fn is_project_data(value: Option<&JsValue>) -> bool {
    let Some(project) = value.and_then(JsValue::as_object) else {
        return false;
    };
    is_string(project.get("id"))
        && is_string(project.get("name"))
        && is_string(project.get("author"))
        && matches!(project.get("documents"), Some(JsValue::Array(_)))
        && matches!(project.get("chapters"), Some(JsValue::Array(_)))
        && is_string(project.get("activeDocumentId"))
        && is_number(project.get("projectGoal"))
        && is_object_like(project.get("chapterGoals"))
        && is_object_like(project.get("activity"))
}

/// `isStoredProject`.
pub fn is_stored_project(value: &JsValue) -> bool {
    let Some(project) = value.as_object() else {
        return false;
    };
    let shape = is_number(project.get("schemaVersion"))
        && is_string(project.get("id"))
        && is_number(project.get("createdAt"))
        && is_number(project.get("updatedAt"))
        && is_number(project.get("revision"))
        && is_string(project.get("updatedBy"))
        && is_null_or_number(project.get("deletedAt"))
        && is_project_data(project.get("project"));
    let Some(entities) = project.get("entities").filter(|e| is_object_like(Some(e))) else {
        return false;
    };
    shape
        && ENTITY_KINDS
            .iter()
            .all(|kind| is_entity_meta_map(entities.get(kind)))
}

/// A stored project's `schemaVersion`, when it is newer than this build understands. Such a
/// project is read and shown, but never written back: a newer release may have changed what
/// its fields mean.
pub fn newer_schema(value: &JsValue) -> Option<f64> {
    value
        .get("schemaVersion")
        .and_then(JsValue::as_number)
        .filter(|v| *v > SCHEMA_VERSION)
}

#[cfg(test)]
mod tests {
    use super::{is_stored_project, newer_schema};
    use crate::js::parse;

    const MINIMAL: &str = r#"{"schemaVersion":2,"id":"p","createdAt":1,"updatedAt":1,"revision":1,
        "updatedBy":"d","deletedAt":null,
        "project":{"id":"p","name":"N","author":"","documents":[],"chapters":[],
                   "activeDocumentId":"","projectGoal":0,"chapterGoals":{},"activity":{}},
        "entities":{"documents":{},"chapters":{},"storyboardRows":{},"storyboardCards":{}}}"#;

    #[test]
    fn accepts_the_minimal_stored_project() {
        let value = parse(MINIMAL).ok();
        assert!(value.as_ref().is_some_and(is_stored_project));
        assert_eq!(value.as_ref().and_then(newer_schema), None);
    }

    #[test]
    fn rejects_missing_entity_kinds_and_bad_metadata() {
        for (from, to) in [
            (r#""storyboardCards":{}"#, r#""other":{}"#),
            (
                r#""documents":{}"#,
                r#""documents":{"x":{"updatedAt":1,"revision":1,"updatedBy":"d"}}"#,
            ),
            (r#""deletedAt":null"#, r#""deletedAt":"never""#),
            (r#""projectGoal":0"#, r#""projectGoal":"0""#),
        ] {
            let text = MINIMAL.replace(from, to);
            assert!(
                !parse(&text).ok().as_ref().is_some_and(is_stored_project),
                "accepted {to}"
            );
        }
    }

    #[test]
    fn notices_a_newer_schema() {
        let value = parse(&MINIMAL.replace(r#""schemaVersion":2"#, r#""schemaVersion":3"#)).ok();
        assert_eq!(value.as_ref().and_then(newer_schema), Some(3.0));
    }
}
