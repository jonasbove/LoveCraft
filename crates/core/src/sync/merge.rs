//! `mergeProjects` (`merge.ts`): the library's projects and the folder's, reconciled against
//! the stamps remembered from the last sync.
//!
//! - only one side changed: take it
//! - both changed: last write wins, and the loser is kept as a conflicted copy
//! - on one side only and never synced: a new project
//! - on one side only and synced before: a deletion (a deletion racing an edit keeps the
//!   edited side as a conflicted copy)

use crate::js::{JsObject, JsString, JsValue};
use crate::storage::stamp::{create_stored_project, projects_have_same_content};

/// A project's version when it was last synced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SyncStamp {
    pub updated_at: f64,
    pub revision: f64,
}

impl SyncStamp {
    pub fn of(project: &JsValue) -> Self {
        Self {
            updated_at: number(project, "updatedAt"),
            revision: number(project, "revision"),
        }
    }

    pub fn read(value: &JsValue) -> Option<Self> {
        Some(Self {
            updated_at: value.get("updatedAt")?.as_number()?,
            revision: value.get("revision")?.as_number()?,
        })
    }

    pub fn to_js(self) -> JsValue {
        let mut o = JsObject::new();
        o.insert("updatedAt", JsValue::Number(self.updated_at));
        o.insert("revision", JsValue::Number(self.revision));
        JsValue::Object(o)
    }
}

fn number(project: &JsValue, key: &str) -> f64 {
    project.get(key).and_then(JsValue::as_number).unwrap_or(0.0)
}

fn text<'a>(project: &'a JsValue, key: &str) -> Option<&'a JsString> {
    project.get(key).and_then(JsValue::as_js_string)
}

fn newer_than(project: &JsValue, stamp: SyncStamp) -> bool {
    let (updated, revision) = (number(project, "updatedAt"), number(project, "revision"));
    updated > stamp.updated_at || (updated == stamp.updated_at && revision > stamp.revision)
}

/// `compareRecords`: newer time, then higher revision, then the device id by UTF-16 code
/// units, so every device picks the same winner.
fn compare_records(a: &JsValue, b: &JsValue) -> std::cmp::Ordering {
    let by_number = |key| {
        number(a, key)
            .partial_cmp(&number(b, key))
            .unwrap_or(std::cmp::Ordering::Equal)
    };
    by_number("updatedAt")
        .then_with(|| by_number("revision"))
        .then_with(|| {
            text(a, "updatedBy")
                .map(JsString::units)
                .cmp(&text(b, "updatedBy").map(JsString::units))
        })
}

/// `Number.prototype.toString(36)` for a non-negative whole number.
fn base36(mut n: u64) -> String {
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(digits[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// `createConflictCopy`: the losing project under a new id, so no edit is lost. `suffix` is
/// ProseCraft's five random base-36 characters.
pub fn create_conflict_copy(loser: &JsValue, now: f64, device_id: &str, suffix: &str) -> JsValue {
    let id = format!(
        "{}-conflict-{}-{suffix}",
        text(loser, "id")
            .map(JsString::to_string_lossy)
            .unwrap_or_default(),
        base36(now.max(0.0) as u64)
    );
    let mut project = loser
        .get("project")
        .cloned()
        .unwrap_or(JsValue::Object(JsObject::new()));
    let name = text(&project, "name")
        .map(JsString::to_string_lossy)
        .unwrap_or_default();
    if let Some(o) = project.as_object_mut() {
        o.insert("id", JsValue::str(&id));
        o.insert("name", JsValue::str(&format!("{name} (conflicted copy)")));
    }
    create_stored_project(project, device_id, now)
}

/// What a merge decided (`MergeOutcome`).
#[derive(Clone, Debug, PartialEq)]
pub struct MergeOutcome {
    pub projects: Vec<JsValue>,
    /// id → stamp, for the next sync.
    pub last_synced: JsValue,
    pub imported: usize,
    pub exported: usize,
    pub conflicts: usize,
    /// Projects to delete from the folder.
    pub removed_ids: Vec<String>,
}

/// `mergeProjects`. `last_synced` is the remembered id → stamp object; `suffix` makes each
/// conflicted copy's random suffix.
pub fn merge_projects(
    local: &[JsValue],
    remote: &[JsValue],
    last_synced: &JsValue,
    now: f64,
    device_id: &str,
    suffix: &mut dyn FnMut() -> String,
) -> MergeOutcome {
    let id_of = |p: &JsValue| {
        text(p, "id")
            .map(JsString::to_string_lossy)
            .unwrap_or_default()
    };
    let find = |list: &[JsValue], id: &str| list.iter().rev().find(|p| id_of(p) == id).cloned();
    // `new Set([...local ids, ...remote ids, ...Object.keys(lastSynced)])`.
    let mut ids: Vec<String> = Vec::new();
    let synced: Vec<(String, Option<SyncStamp>)> = last_synced
        .as_object()
        .map(|o| {
            o.iter()
                .map(|(k, v)| (k.to_string_lossy(), SyncStamp::read(v)))
                .collect()
        })
        .unwrap_or_default();
    for id in local
        .iter()
        .map(id_of)
        .chain(remote.iter().map(id_of))
        .chain(synced.iter().map(|(k, _)| k.clone()))
    {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let mut projects = Vec::new();
    let mut removed_ids = Vec::new();
    let (mut imported, mut exported, mut conflicts) = (0, 0, 0);
    for id in ids {
        let local_project = find(local, &id);
        let remote_project = find(remote, &id);
        let base = synced.iter().find(|(k, _)| *k == id).and_then(|(_, s)| *s);
        match (local_project, remote_project) {
            (Some(l), Some(r)) => {
                let same = match (l.get("project"), r.get("project")) {
                    (Some(a), Some(b)) => projects_have_same_content(a, b),
                    _ => false,
                };
                if same {
                    projects.push(if compare_records(&l, &r).is_ge() {
                        l
                    } else {
                        r
                    });
                    continue;
                }
                let local_changed = base.is_none_or(|b| newer_than(&l, b));
                let remote_changed = base.is_none_or(|b| newer_than(&r, b));
                if local_changed && remote_changed {
                    let local_wins = compare_records(&l, &r).is_ge();
                    let (winner, loser) = if local_wins { (l, r) } else { (r, l) };
                    projects.push(winner);
                    projects.push(create_conflict_copy(&loser, now, device_id, &suffix()));
                    conflicts += 1;
                } else if remote_changed {
                    projects.push(r);
                    imported += 1;
                } else {
                    projects.push(l);
                    exported += 1;
                }
            }
            (Some(l), None) => match base {
                // Deleted in the folder: an edit made since the last sync is kept as a copy.
                Some(b) => {
                    if newer_than(&l, b) {
                        projects.push(create_conflict_copy(&l, now, device_id, &suffix()));
                        conflicts += 1;
                    }
                }
                None => {
                    projects.push(l);
                    exported += 1;
                }
            },
            (None, Some(r)) => match base {
                // Deleted here: a change made in the folder since is kept as a copy.
                Some(b) => {
                    if newer_than(&r, b) {
                        projects.push(create_conflict_copy(&r, now, device_id, &suffix()));
                        conflicts += 1;
                    }
                    removed_ids.push(id);
                }
                None => {
                    projects.push(r);
                    imported += 1;
                }
            },
            (None, None) => {}
        }
    }
    let mut next = JsObject::new();
    for project in &projects {
        next.insert(id_of(project).as_str(), SyncStamp::of(project).to_js());
    }
    MergeOutcome {
        projects,
        last_synced: JsValue::Object(next),
        imported,
        exported,
        conflicts,
        removed_ids,
    }
}

#[cfg(test)]
mod tests {
    use super::base36;

    #[test]
    fn writes_base36_as_javascript() {
        assert_eq!(base36(5_000_000), "2z60w");
        assert_eq!(base36(0), "0");
        assert_eq!(base36(1_763_000_000_000), "mhwsoi68");
    }
}
