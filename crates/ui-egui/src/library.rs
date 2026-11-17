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

impl Library {
    fn from_stored(stored: Vec<JsValue>) -> Self {
        let mut projects: Vec<OpenProject> = stored
            .into_iter()
            .filter_map(|stored| {
                Project::from_stored(&stored).map(|view| OpenProject { stored, view })
            })
            .collect();
        projects.sort_by_key(|p| p.view.trashed);
        Self { projects }
    }

    /// Opens a backup's text: plain opens at once, encrypted asks for a passphrase.
    pub fn open(text: &str) -> Result<Self, OpenError> {
        match PlainBundle::parse(text) {
            Ok(bundle) => Ok(Self::from_stored(
                bundle.projects().into_iter().cloned().collect(),
            )),
            Err(plain_error) => match EncryptedBundle::parse(text) {
                Ok(bundle) => Err(OpenError::Encrypted(Box::new(bundle))),
                // Report the plain reading's reason unless the file says it's encrypted.
                Err(_) => Err(OpenError::Unreadable(plain_error)),
            },
        }
    }

    pub fn unlock(bundle: &EncryptedBundle, passphrase: &JsString) -> Result<Self, BundleError> {
        let (_key, projects) = bundle.open(passphrase)?;
        Ok(Self::from_stored(projects))
    }

    pub fn projects(&self) -> &[OpenProject] {
        &self.projects
    }
}
