//! Snapshots (ProseCraft `src/lib/snapshots.ts`, upstream `7dae544`): a scene's title and HTML
//! kept as they were, up to ten per scene. Restoring first snapshots the current text, so a
//! restore can itself be undone. These work on the stored project, so entries this version
//! can't read are left as they are.

use crate::edit::EditError;
use crate::html::{is_js_whitespace, sanitize_html};
use crate::js::{JsObject, JsValue};

/// Oldest snapshots of a scene are dropped beyond this many.
pub const MAX_SNAPSHOTS_PER_DOCUMENT: usize = 10;
const MAX_NOTE_LENGTH: usize = 120;

fn text<'a>(v: &'a JsValue, key: &str) -> Option<&'a crate::js::JsString> {
    v.get(key).and_then(JsValue::as_js_string)
}

fn is_snapshot_of(entry: &JsValue, document_id: &str) -> bool {
    text(entry, "documentId").is_some_and(|d| d.eq_str(document_id))
}

fn snapshots_mut(project: &mut JsValue) -> Result<&mut Vec<JsValue>, EditError> {
    let object = project.as_object_mut().ok_or(EditError::Shape)?;
    if !matches!(object.get("snapshots"), Some(JsValue::Array(_))) {
        object.insert("snapshots", JsValue::Array(Vec::new()));
    }
    match object.get_mut("snapshots") {
        Some(JsValue::Array(list)) => Ok(list),
        _ => Err(EditError::Shape),
    }
}

fn document<'a>(project: &'a JsValue, id: &str) -> Option<&'a JsValue> {
    project
        .get("documents")?
        .as_array()?
        .iter()
        .find(|d| text(d, "id").is_some_and(|i| i.eq_str(id)))
}

/// `createSnapshot`: `{ id, documentId, createdAt, title, content, reason, note? }`.
fn create(document: &JsValue, id: &str, now: f64, reason: &str, note: Option<&str>) -> JsValue {
    let mut s = JsObject::new();
    s.insert("id", JsValue::str(id));
    s.insert(
        "documentId",
        document.get("id").cloned().unwrap_or(JsValue::str("")),
    );
    s.insert("createdAt", JsValue::Number(now));
    s.insert(
        "title",
        document.get("title").cloned().unwrap_or(JsValue::str("")),
    );
    s.insert(
        "content",
        document.get("content").cloned().unwrap_or(JsValue::str("")),
    );
    s.insert("reason", JsValue::str(reason));
    // `note?.trim().slice(0, 120)`, in UTF-16 code units.
    let trimmed = note
        .map(|n| n.trim_matches(is_js_whitespace))
        .unwrap_or_default();
    let units: Vec<u16> = trimmed.encode_utf16().take(MAX_NOTE_LENGTH).collect();
    let note = String::from_utf16_lossy(&units);
    if !note.is_empty() {
        s.insert("note", JsValue::str(&note));
    }
    JsValue::Object(s)
}

/// `addSnapshot`: adds `snapshot` and drops its scene's oldest beyond the cap.
fn add(list: &mut Vec<JsValue>, snapshot: JsValue) {
    let document_id = text(&snapshot, "documentId")
        .map(|d| d.to_string_lossy())
        .unwrap_or_default();
    list.push(snapshot);
    let mut own: Vec<(f64, usize)> = list
        .iter()
        .enumerate()
        .filter(|(_, e)| is_snapshot_of(e, &document_id))
        .map(|(i, e)| {
            (
                e.get("createdAt")
                    .and_then(JsValue::as_number)
                    .unwrap_or(0.0),
                i,
            )
        })
        .collect();
    // Stable by time, as `sort((a, b) => a.createdAt - b.createdAt)`.
    own.sort_by(|a, b| a.0.total_cmp(&b.0));
    let drop: Vec<usize> = own
        .iter()
        .take(own.len().saturating_sub(MAX_SNAPSHOTS_PER_DOCUMENT))
        .map(|(_, i)| *i)
        .collect();
    let mut index = 0;
    list.retain(|_| {
        let keep = !drop.contains(&index);
        index += 1;
        keep
    });
}

/// Takes a snapshot of each of `documents` (a scene, or a chapter's scenes) with `note`.
pub fn take_snapshots(
    project: &mut JsValue,
    documents: &[String],
    now: f64,
    note: &str,
    new_id: &mut dyn FnMut() -> String,
) -> Result<(), EditError> {
    let found: Vec<JsValue> = documents
        .iter()
        .filter_map(|id| document(project, id).cloned())
        .collect();
    let list = snapshots_mut(project)?;
    for doc in found {
        add(list, create(&doc, &new_id(), now, "manual", Some(note)));
    }
    Ok(())
}

/// One scene's snapshot taken for `reason` ("before-restore"), with `note`: what ProseCraft
/// saves before changing a scene for you (a restore, a screenplay conversion).
pub fn snapshot_before(
    project: &mut JsValue,
    document_id: &str,
    id: &str,
    now: f64,
    reason: &str,
    note: &str,
) -> Result<(), EditError> {
    let found = document(project, document_id)
        .cloned()
        .ok_or(EditError::Shape)?;
    let list = snapshots_mut(project)?;
    add(list, create(&found, id, now, reason, Some(note)));
    Ok(())
}

/// Restores snapshot `id`: the scene's current text is snapshotted first ("before a
/// restore"), then the snapshot's HTML, sanitized, replaces it (and any Markdown source).
/// Returns the scene's id.
pub fn restore_snapshot(
    project: &mut JsValue,
    id: &str,
    now: f64,
    new_id: &mut dyn FnMut() -> String,
) -> Result<String, EditError> {
    let snapshot = project
        .get("snapshots")
        .and_then(JsValue::as_array)
        .and_then(|l| {
            l.iter()
                .find(|s| text(s, "id").is_some_and(|i| i.eq_str(id)))
        })
        .cloned()
        .ok_or(EditError::Shape)?;
    let document_id = text(&snapshot, "documentId")
        .map(|d| d.to_string_lossy())
        .ok_or(EditError::Shape)?;
    let content = text(&snapshot, "content")
        .map(|c| c.to_string_lossy())
        .ok_or(EditError::Shape)?;
    let current = document(project, &document_id)
        .cloned()
        .ok_or(EditError::Shape)?;
    add(
        snapshots_mut(project)?,
        create(&current, &new_id(), now, "before-restore", None),
    );
    crate::edit::set_document_content(project, &document_id, &sanitize_html(&content))?;
    Ok(document_id)
}

/// `removeSnapshot`.
pub fn delete_snapshot(project: &mut JsValue, id: &str) -> Result<(), EditError> {
    snapshots_mut(project)?.retain(|s| !text(s, "id").is_some_and(|i| i.eq_str(id)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MAX_SNAPSHOTS_PER_DOCUMENT, delete_snapshot, restore_snapshot, take_snapshots};
    use crate::js::{JsValue, parse};

    fn project() -> JsValue {
        parse(r#"{"documents":[{"id":"d","title":"T","content":"<p>v1</p>","markdownSource":"v1"},{"id":"e","title":"E","content":"<p>e</p>"}],"snapshots":[{"odd":"kept"}]}"#).expect("json")
    }

    fn count(p: &JsValue, doc: &str) -> usize {
        p.get("snapshots")
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .filter(|s| {
                s.get("documentId")
                    .and_then(JsValue::as_js_string)
                    .is_some_and(|d| d.eq_str(doc))
            })
            .count()
    }

    #[test]
    fn keeps_ten_per_scene_and_leaves_others() {
        let mut p = project();
        let mut n = 0;
        let mut id = || {
            n += 1;
            format!("s{n}")
        };
        for t in 0..12 {
            take_snapshots(&mut p, &["d".into()], f64::from(t), "  a note  ", &mut id)
                .expect("take");
        }
        take_snapshots(&mut p, &["e".into()], 0.0, "", &mut id).expect("take");
        assert_eq!(
            (count(&p, "d"), count(&p, "e")),
            (MAX_SNAPSHOTS_PER_DOCUMENT, 1)
        );
        let list = p
            .get("snapshots")
            .and_then(JsValue::as_array)
            .expect("list");
        assert_eq!(
            list[0]
                .get("odd")
                .and_then(JsValue::as_js_string)
                .map(|s| s.to_string_lossy())
                .as_deref(),
            Some("kept")
        );
        assert_eq!(
            crate::js::stringify(&list[1]),
            r#"{"id":"s3","documentId":"d","createdAt":2,"title":"T","content":"<p>v1</p>","reason":"manual","note":"a note"}"#
        );
    }

    #[test]
    fn restoring_keeps_what_it_replaces() {
        let mut p = project();
        let mut id = || "first".to_owned();
        take_snapshots(&mut p, &["d".into()], 1.0, "", &mut id).expect("take");
        crate::edit::set_document_content(&mut p, "d", "<p>v2</p>").expect("edit");
        let doc =
            restore_snapshot(&mut p, "first", 2.0, &mut || "before".to_owned()).expect("restore");
        assert_eq!(doc, "d");
        let d = &p
            .get("documents")
            .and_then(JsValue::as_array)
            .expect("docs")[0];
        assert_eq!(
            crate::js::stringify(d),
            r#"{"id":"d","title":"T","content":"<p>v1</p>"}"#
        );
        let before = p
            .get("snapshots")
            .and_then(JsValue::as_array)
            .expect("list")
            .iter()
            .find(|s| {
                s.get("id")
                    .and_then(JsValue::as_js_string)
                    .is_some_and(|i| i.eq_str("before"))
            })
            .cloned()
            .expect("before-restore");
        assert_eq!(
            before
                .get("content")
                .and_then(JsValue::as_js_string)
                .map(|s| s.to_string_lossy())
                .as_deref(),
            Some("<p>v2</p>")
        );
        assert_eq!(
            before
                .get("reason")
                .and_then(JsValue::as_js_string)
                .map(|s| s.to_string_lossy())
                .as_deref(),
            Some("before-restore")
        );
        delete_snapshot(&mut p, "first").expect("delete");
        assert_eq!(count(&p, "d"), 1);
    }
}
