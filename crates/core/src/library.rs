//! The app's own library: every project, kept as a plain ProseCraft backup.
//!
//! The library file is a `prosecraft.bundle`, so ProseCraft can import it directly and nothing is
//! ever locked into this app. Projects stay [`JsValue`]s, kept whole, including ones this build
//! can't read. Importing follows ProseCraft's `mergeIncomingProjects`
//! (`src/lib/storage/repository.ts`, upstream `7dae544`): per project, the newer `updatedAt` wins,
//! then the higher `revision`.

use crate::js::{self, JsValue};
use crate::storage::bundle::{BundleError, PLAIN_FORMAT, write_plain_bundle};
use crate::storage::crypto::{CryptoError, RandomSource};
use crate::storage::schema::{is_stored_project, newer_schema};

/// What an import did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportResult {
    /// Projects added or replaced by a newer copy.
    pub imported: usize,
    /// Projects in the library afterwards.
    pub total: usize,
}

/// The library.
#[derive(Clone, Debug, PartialEq)]
pub struct Library {
    /// This installation's id: who made a change, for last-write-wins merges.
    pub device_id: String,
    /// Every project, as stored.
    projects: Vec<JsValue>,
    /// A project from a newer release was found: show everything, write nothing back.
    read_only: bool,
}

impl Library {
    pub fn new(device_id: String) -> Self {
        Self {
            device_id,
            projects: Vec::new(),
            read_only: false,
        }
    }

    /// Reads the library file. Unlike an import, an empty library is fine, and projects this
    /// build can't read are kept (and written back) as they are.
    pub fn from_text(text: &str) -> Result<Self, BundleError> {
        let value = js::parse(text).map_err(|_| BundleError::NotJson)?;
        let format_ok = value
            .get("format")
            .and_then(JsValue::as_js_string)
            .is_some_and(|f| f.eq_str(PLAIN_FORMAT));
        if !format_ok {
            return Err(BundleError::NotABackup);
        }
        let projects = value
            .get("projects")
            .and_then(JsValue::as_array)
            .ok_or(BundleError::NoProjects)?
            .to_vec();
        let device_id = value
            .get("deviceId")
            .and_then(JsValue::as_js_string)
            .map(|d| d.to_string_lossy())
            .unwrap_or_default();
        let read_only = projects.iter().any(|p| newer_schema(p).is_some());
        Ok(Self {
            device_id,
            projects,
            read_only,
        })
    }

    /// The library file's text.
    pub fn to_text(&self, saved_at: f64) -> String {
        write_plain_bundle(&self.projects, saved_at, &self.device_id)
    }

    /// Whether this build must not write the library (a newer release's project is in it).
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// The projects ProseCraft would show (valid stored projects), in library order.
    pub fn projects(&self) -> impl Iterator<Item = &JsValue> {
        self.projects.iter().filter(|p| is_stored_project(p))
    }

    pub fn project(&self, id: &str) -> Option<&JsValue> {
        self.projects().find(|p| {
            p.get("id")
                .and_then(JsValue::as_js_string)
                .is_some_and(|i| i.eq_str(id))
        })
    }

    /// Replaces a project with a new stored version (after an edit), or adds it.
    pub fn put(&mut self, stored: JsValue) {
        let id = stored.get("id").and_then(JsValue::as_js_string).cloned();
        match self
            .projects
            .iter_mut()
            .find(|p| p.get("id").and_then(JsValue::as_js_string) == id.as_ref())
        {
            Some(slot) => *slot = stored,
            None => self.projects.push(stored),
        }
    }

    /// `permanentlyDeleteProject`: the project gone from the library (ProseCraft drops it from
    /// storage, with no tombstone).
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.projects.len();
        self.projects.retain(|p| {
            !p.get("id")
                .and_then(JsValue::as_js_string)
                .is_some_and(|i| i.eq_str(id))
        });
        self.projects.len() != before
    }

    /// After a sync: the library holds the merged working set. Records this build can't read
    /// (not valid stored projects) aren't part of a sync and are kept as they are.
    pub fn replace_synced(&mut self, synced: Vec<JsValue>) {
        self.projects.retain(|p| !is_stored_project(p));
        self.projects.extend(synced);
    }

    /// `mergeIncomingProjects`: each incoming project replaces the library's copy when it is
    /// newer (`updatedAt`, then `revision`); new projects are added.
    pub fn merge(&mut self, incoming: Vec<JsValue>) -> ImportResult {
        let mut imported = 0;
        for project in incoming.into_iter().filter(is_stored_project) {
            let id = project.get("id").and_then(JsValue::as_js_string).cloned();
            let number = |p: &JsValue, key: &str| {
                p.get(key)
                    .and_then(JsValue::as_number)
                    .unwrap_or(f64::NEG_INFINITY)
            };
            let existing = self
                .projects
                .iter_mut()
                .find(|p| p.get("id").and_then(JsValue::as_js_string) == id.as_ref());
            match existing {
                None => {
                    self.projects.push(project);
                    imported += 1;
                }
                Some(previous) => {
                    let (new_at, old_at) =
                        (number(&project, "updatedAt"), number(previous, "updatedAt"));
                    let wins = new_at > old_at
                        || (new_at == old_at
                            && number(&project, "revision") > number(previous, "revision"));
                    if wins {
                        *previous = project;
                        imported += 1;
                    }
                }
            }
        }
        self.read_only |= self.projects.iter().any(|p| newer_schema(p).is_some());
        ImportResult {
            imported,
            total: self.projects().count(),
        }
    }
}

/// A new installation id: a random UUID (version 4), as ProseCraft's `crypto.randomUUID()`.
pub fn new_device_id(random: &mut dyn RandomSource) -> Result<String, CryptoError> {
    let mut b = [0u8; 16];
    random.fill(&mut b)?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: Vec<String> = b.iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        hex[0..4].concat(),
        hex[4..6].concat(),
        hex[6..8].concat(),
        hex[8..10].concat(),
        hex[10..16].concat()
    ))
}

#[cfg(test)]
mod tests {
    use super::{Library, new_device_id};
    use crate::js::{JsValue, parse};
    use crate::storage::crypto::{CryptoError, RandomSource};

    fn project(id: &str, updated_at: f64, revision: f64, name: &str) -> JsValue {
        let text = format!(
            r#"{{"schemaVersion":2,"id":"{id}","createdAt":1,"updatedAt":{updated_at},"revision":{revision},"updatedBy":"d","deletedAt":null,
            "project":{{"id":"{id}","name":"{name}","author":"","documents":[],"chapters":[],"activeDocumentId":"","projectGoal":0,"chapterGoals":{{}},"activity":{{}}}},
            "entities":{{"documents":{{}},"chapters":{{}},"storyboardRows":{{}},"storyboardCards":{{}}}}}}"#
        );
        parse(&text).unwrap_or(JsValue::Null)
    }

    fn name(library: &Library, id: &str) -> String {
        library
            .project(id)
            .and_then(|p| p.get("project"))
            .and_then(|p| p.get("name"))
            .and_then(JsValue::as_js_string)
            .map(|n| n.to_string_lossy())
            .unwrap_or_default()
    }

    #[test]
    fn newer_copies_win_then_higher_revisions() {
        let mut library = Library::new("me".into());
        let first = library.merge(vec![
            project("a", 10.0, 1.0, "A1"),
            project("b", 10.0, 1.0, "B1"),
        ]);
        assert_eq!((first.imported, first.total), (2, 2));
        let second = library.merge(vec![
            project("a", 9.0, 9.0, "older"),
            project("b", 10.0, 2.0, "B2"),
            project("c", 1.0, 1.0, "C"),
        ]);
        assert_eq!((second.imported, second.total), (2, 3));
        assert_eq!(name(&library, "a"), "A1");
        assert_eq!(name(&library, "b"), "B2");
    }

    #[test]
    fn an_empty_library_round_trips_and_a_newer_schema_makes_it_read_only() {
        let library = Library::new("me".into());
        let text = library.to_text(5.0);
        let reread = Library::from_text(&text).ok();
        assert_eq!(reread.as_ref().map(|l| l.device_id.as_str()), Some("me"));
        let mut newer = Library::new("me".into());
        let mut future = project("f", 1.0, 1.0, "F");
        if let Some(object) = future.as_object_mut() {
            object.insert("schemaVersion", JsValue::Number(3.0));
        }
        newer.merge(vec![future]);
        assert!(newer.is_read_only());
    }

    struct Fixed;
    impl RandomSource for Fixed {
        fn fill(&mut self, bytes: &mut [u8]) -> Result<(), CryptoError> {
            bytes.fill(0xab);
            Ok(())
        }
    }

    #[test]
    fn device_ids_are_version_4_uuids() {
        let id = new_device_id(&mut Fixed).unwrap_or_default();
        assert_eq!(id, "abababab-abab-4bab-abab-abababababab");
    }
}
