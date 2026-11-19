//! The projects opened from a backup.

use std::fmt;

use prosecraft_core::js::{JsString, JsValue};
use prosecraft_core::project::Project;
use prosecraft_core::storage::bundle::{BundleError, EncryptedBundle, PlainBundle};

/// A project as stored (kept whole) and its view.
pub struct OpenProject {
    pub stored: JsValue,
    pub view: Project,
}

/// The projects of an opened backup, in its order; projects in the trash come last, as in
/// ProseCraft's switcher, which lists only the others.
pub struct Library {
    projects: Vec<OpenProject>,
}

/// Why a backup didn't open.
pub enum OpenError {
    /// Encrypted: ask for the passphrase, then [`Library::unlock`].
    Encrypted(Box<EncryptedBundle>),
    Unreadable(BundleError),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encrypted(_) => f.write_str("it is encrypted"),
            Self::Unreadable(error) => write!(f, "{error}"),
        }
    }
}

/// A stored project with its data migrated to the Wiki, when it needs it.
fn migrated(mut stored: JsValue) -> JsValue {
    let data = stored
        .get("project")
        .and_then(prosecraft_core::wiki_migration::migrate_project_to_wiki);
    if let (Some(data), Some(object)) = (data, stored.as_object_mut()) {
        object.insert("project", data);
    }
    stored
}

impl Library {
    /// Views of stored projects (for example the app's own library).
    pub fn from_stored(stored: Vec<JsValue>) -> Self {
        let mut projects: Vec<OpenProject> = stored
            .into_iter()
            .filter_map(|stored| {
                // As ProseCraft loads it: the Wiki migration done in memory
                // (`migrateProjectToWiki`); it is stored with the project's next change.
                let stored = migrated(stored);
                Project::from_stored(&stored).map(|view| OpenProject { stored, view })
            })
            .collect();
        projects.sort_by_key(|p| p.view.trashed);
        Self { projects }
    }

    /// Reads a backup's projects: plain at once, encrypted after its passphrase.
    pub fn read(text: &str) -> Result<Vec<JsValue>, OpenError> {
        match PlainBundle::parse(text) {
            Ok(bundle) => Ok(bundle.projects().into_iter().cloned().collect()),
            Err(plain_error) => match EncryptedBundle::parse(text) {
                Ok(bundle) => Err(OpenError::Encrypted(Box::new(bundle))),
                // Report the plain reading's reason unless the file says it's encrypted.
                Err(_) => Err(OpenError::Unreadable(plain_error)),
            },
        }
    }

    pub fn unlock(
        bundle: &EncryptedBundle,
        passphrase: &JsString,
    ) -> Result<Vec<JsValue>, BundleError> {
        let (_key, projects) = bundle.open(passphrase)?;
        Ok(projects)
    }

    pub fn projects(&self) -> &[OpenProject] {
        &self.projects
    }
}
