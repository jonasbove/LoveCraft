//! The app inside a Craft host: CraftCloud's file browser frames the web build with
//! `?host=parent` and talks to it through `craft-web-host`'s messages (`ready`, `open`, `save`,
//! `saved`, `dirty`, `failed`). This is the session's logic, without the browser, so it is tested
//! natively; `main.rs` passes the host's messages in and posts what comes out.
//!
//! The file the host sends is the project library the app edits: its projects are shown and
//! edited as the app's own library is, and each save sends the whole file back, under the same
//! name, as a ProseCraft backup. The browser's own library (IndexedDB) is left alone: a file from
//! the host is never merged into it, so a stale copy there can never win over the host's file.
//!
//! Opening never writes. A file the app must not write is shown read-only and never sent back:
//! one with a project from a newer ProseCraft (as "Open backup…" treats it) and an encrypted one
//! (saving would send its projects back unencrypted).

use std::cell::RefCell;
use std::rc::Rc;

use prosecraft_ui_egui::{Library, LibraryStore, OpenError, ProjectLibrary, ProseApp, SaveState};

/// The app's id in the host's messages (`prosecraft-rs:open`), CraftCloud's id for the app (the
/// table name in its `apps.toml`) and the folder the build is served from (`/apps/prosecraft-rs/`).
/// It keeps the app's former name, ProseCraft-rs: a host page speaks to the app by it. The web
/// build's release file is named for the app (`lovecraft-web-<version>.zip`) since 0.2.0.
pub const APP_ID: &str = prosecraft_core::about::ids::HOST_MESSAGE_PREFIX;

/// Least time, in seconds, between two saves sent while the person writes. The app saves after
/// every pause in typing; each save the host stores is a new version of the file, so they are
/// gathered. Ctrl+S sends at once.
pub const SAVE_EVERY: f64 = 15.0;

/// A save the host hasn't confirmed (`saved`) by then is reported as failed; a larger file gets
/// [`SAVE_TIMEOUT_PER_MB`] more per megabyte. No other save is sent until it is confirmed: the
/// host must not get two writes of the file at once.
pub const SAVE_TIMEOUT: f64 = 30.0;

/// Extra time a save may take per megabyte of the file.
pub const SAVE_TIMEOUT_PER_MB: f64 = 10.0;

/// What the session asks the host page for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// The file as it is now, to store under `name`; the host answers `saved` with `id`.
    Save {
        name: String,
        bytes: Vec<u8>,
        id: String,
    },
    /// Unsaved changes appeared or went away.
    Dirty(bool),
    /// A file couldn't be taken, or a save (`id`) went unconfirmed.
    Failed { error: String, id: Option<String> },
}

/// The library's newest text, waiting to go to the host.
type Outbox = Rc<RefCell<Option<String>>>;

/// The hosted file's store: the app's saves wait for the host; time, ids and device settings
/// come from the browser's store, whose library is never written.
pub struct HostStore {
    outbox: Outbox,
    device: Box<dyn LibraryStore>,
}

impl LibraryStore for HostStore {
    fn save(&mut self, text: &str) -> Result<(), String> {
        *self.outbox.borrow_mut() = Some(text.to_owned());
        Ok(())
    }

    fn now_ms(&self) -> f64 {
        self.device.now_ms()
    }

    fn new_id(&mut self) -> String {
        self.device.new_id()
    }

    fn today(&self) -> String {
        self.device.today()
    }

    fn load_setting(&mut self, key: &str) -> Option<String> {
        self.device.load_setting(key)
    }

    fn save_setting(&mut self, key: &str, value: Option<&str>) -> Result<(), String> {
        self.device.save_setting(key, value)
    }
}

/// The file the host opened.
struct File {
    name: String,
    /// Saves go back to the host (not read-only, not encrypted).
    writable: bool,
    /// The projects as opened: sent back exactly so while they are not edited.
    opened: Vec<prosecraft_core::js::JsValue>,
}

/// A save posted to the host.
struct Flight {
    id: String,
    sent_at: f64,
    /// When it counts as unanswered.
    timeout: f64,
    /// Already reported as unanswered.
    reported: bool,
}

/// One hosted file's session: what was opened, what has been sent, what the host confirmed.
pub struct Bridge {
    /// Names this page's saves apart from an earlier page's (a reload of the same file).
    session: String,
    file: Option<File>,
    outbox: Outbox,
    /// The save waiting for `saved`. One at a time: the host writes each over the last, and
    /// one that went unanswered still holds the next back.
    flight: Option<Flight>,
    last_sent: Option<f64>,
    /// Ctrl+S or the page going away: send the next save without waiting for [`SAVE_EVERY`].
    save_now: bool,
    next_id: u64,
    /// The unsaved state last reported to the host.
    dirty: bool,
    effects: Vec<Effect>,
}

impl Bridge {
    /// A session whose save ids start with `session` (random in the browser).
    pub fn new(session: &str) -> Self {
        Self {
            session: session.to_owned(),
            file: None,
            outbox: Rc::default(),
            flight: None,
            last_sent: None,
            save_now: false,
            next_id: 1,
            dirty: false,
            effects: Vec::new(),
        }
    }

    /// What to post to the host, in order.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    /// Why a file of `size` bytes isn't read at all, if it is too large to be a backup.
    pub fn too_large(name: &str, size: f64) -> Option<String> {
        (size > ProseApp::MAX_BACKUP_BYTES as f64).then(|| {
            format!("Couldn't open {name}: it is far too large to be a ProseCraft backup.")
        })
    }

    /// A file the host sent that couldn't be taken (too large, or unreadable).
    pub fn refuse(&mut self, error: String) {
        self.effects.push(Effect::Failed { error, id: None });
    }

    /// `open`: the app for the file the host sent, as "Open backup…" reads it (an encrypted one
    /// asks for its passphrase), or why it couldn't be opened (the host is told too).
    /// `device_id` names this browser in the changes it makes; `device` gives time, ids and
    /// settings.
    pub fn open(
        &mut self,
        name: &str,
        bytes: &[u8],
        device_id: String,
        device: Box<dyn LibraryStore>,
    ) -> Result<ProseApp, String> {
        let opened = self.read(name, bytes, device_id, device);
        if let Err(error) = &opened {
            self.refuse(error.clone());
        }
        opened
    }

    fn read(
        &mut self,
        name: &str,
        bytes: &[u8],
        device_id: String,
        device: Box<dyn LibraryStore>,
    ) -> Result<ProseApp, String> {
        if self.file.is_some() {
            // Its projects would be merged into the open file, and saved under its name.
            return Err(format!(
                "Couldn't open {name}: a file is already open here. Open it in a new window."
            ));
        }
        if let Some(why) = Self::too_large(name, bytes.len() as f64) {
            return Err(why);
        }
        let not_a_backup = || format!("Couldn't open {name}: it isn't a ProseCraft backup.");
        let text = std::str::from_utf8(bytes).map_err(|_| not_a_backup())?;
        match Library::read(text) {
            // Written back, it would lose what the app doesn't know about the file itself.
            Ok(_) if !rewritable(text) => Ok(self.read_only(name, text)),
            Ok(_) => {
                // Read as the library file is, so records this build can't read are kept and
                // written back as they are. Projects are sanitized as an opened backup's are,
                // but only once edited ([`Self::outgoing`]): untouched ones go back as they came.
                let mut library = ProjectLibrary::from_text(text).map_err(|_| not_a_backup())?;
                let opened = library.projects().cloned().collect();
                library.device_id = device_id;
                let writable = !library.is_read_only();
                let store = HostStore {
                    outbox: Rc::clone(&self.outbox),
                    device,
                };
                self.file = Some(File {
                    name: name.to_owned(),
                    writable,
                    opened,
                });
                Ok(ProseApp::with_library(library, Box::new(store)))
            }
            Err(OpenError::Encrypted(_)) => Ok(self.read_only(name, text)),
            Err(OpenError::Unreadable(error)) => Err(format!("Couldn't open {name}: {error}")),
        }
    }

    /// The file shown and never written back: without a library of its own, the app shows the
    /// backup's projects read-only, as "Open backup…" does there (an encrypted one once unlocked).
    fn read_only(&mut self, name: &str, text: &str) -> ProseApp {
        let mut app = ProseApp::new();
        app.open_backup(name, text);
        self.file = Some(File {
            name: name.to_owned(),
            writable: false,
            opened: Vec::new(),
        });
        app
    }

    /// Ctrl+S: the typing so far committed, and the file sent without waiting.
    pub fn save_now(&mut self, app: &mut ProseApp) {
        if self.file.as_ref().is_some_and(|f| f.writable) {
            app.flush();
            self.save_now = true;
        }
    }

    /// Ctrl+S (⌘S), taken before the app sees the frame's keys.
    pub fn keys(&mut self, ctx: &egui::Context, app: &mut ProseApp) {
        let shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);
        if self.file.is_some() && ctx.input_mut(|i| i.consume_shortcut(&shortcut)) {
            self.save_now(app);
        }
    }

    /// The page is hidden or going away (`visibilitychange`, `pagehide`): what was typed goes to
    /// the host now, without waiting for [`SAVE_EVERY`] (but never beside an unconfirmed save).
    pub fn leaving(&mut self, now: f64, app: &mut ProseApp) {
        self.save_now(app);
        self.tick(now, app);
    }

    /// `saved`: the host stored the save with this `id`. Other ids are ignored.
    pub fn saved(&mut self, id: &str) {
        if self.flight.as_ref().is_some_and(|f| f.id == id) {
            self.flight = None;
        }
    }

    /// A save that couldn't be posted: it waits for the next turn, unless something newer does.
    pub fn not_sent(&mut self, id: &str, text: String) {
        if self.flight.take_if(|f| f.id == id).is_some() {
            self.outbox.borrow_mut().get_or_insert(text);
        }
    }

    /// The file to send for the library's `text`: projects not edited since the file was opened
    /// exactly as they came, edited ones sanitized as an opened backup's are.
    fn outgoing(&self, text: String) -> String {
        let Some(file) = self.file.as_ref() else {
            return text;
        };
        let Ok(mut library) = ProjectLibrary::from_text(&text) else {
            return text;
        };
        let mut changed = false;
        let projects: Vec<_> = library.projects().cloned().collect();
        for project in projects {
            if file.opened.contains(&project) {
                continue;
            }
            let clean = prosecraft_core::library::sanitize_stored_project(project.clone());
            if clean != project {
                library.put(clean);
                changed = true;
            }
        }
        if !changed {
            return text;
        }
        let saved_at = prosecraft_core::js::parse(&text)
            .ok()
            .and_then(|b| b.get("exportedAt").and_then(|t| t.as_number()))
            .unwrap_or_default();
        library.to_text(saved_at)
    }

    /// After each frame (`now` in seconds): a save that went unanswered, the next save, and
    /// the unsaved state.
    pub fn tick(&mut self, now: f64, app: &mut ProseApp) {
        let Some(file) = self.file.as_ref() else {
            return;
        };
        // Read-only from the start, or since a backup from a newer ProseCraft was merged in.
        if !file.writable || !app.state().is_editable() {
            // Nothing is ever sent back; the app shows why.
            self.outbox.borrow_mut().take();
            self.save_now = false;
            return;
        }
        let name = file.name.clone();
        if let Some(flight) = self
            .flight
            .as_mut()
            .filter(|f| !f.reported && now - f.sent_at >= f.timeout)
        {
            flight.reported = true;
            let error = format!(
                "{name} isn't saved yet: the host hasn't confirmed the save. Your changes are kept here."
            );
            app.report(&error);
            let id = Some(flight.id.clone());
            self.effects.push(Effect::Failed { error, id });
        }
        let due = self.save_now || self.last_sent.is_none_or(|at| now - at >= SAVE_EVERY);
        if self.flight.is_none() && due {
            let next = self.outbox.borrow_mut().take();
            if let Some(text) = next {
                let text = self.outgoing(text);
                let id = format!("save-{}-{}", self.session, self.next_id);
                self.next_id += 1;
                let megabytes = text.len() as f64 / 1_000_000.0;
                self.flight = Some(Flight {
                    id: id.clone(),
                    sent_at: now,
                    timeout: SAVE_TIMEOUT + SAVE_TIMEOUT_PER_MB * megabytes,
                    reported: false,
                });
                self.effects.push(Effect::Save {
                    name,
                    bytes: text.into_bytes(),
                    id,
                });
                self.last_sent = Some(now);
            }
            self.save_now = false;
        }
        let dirty = app.state().save_state() != &SaveState::Saved
            || app.has_unsaved_draft()
            || self.outbox.borrow().is_some()
            || self.flight.is_some();
        if dirty != self.dirty {
            self.dirty = dirty;
            self.effects.push(Effect::Dirty(dirty));
        }
    }

    /// When, in seconds from `now`, the session next has something to do on its own.
    pub fn wake_in(&self, now: f64) -> Option<f64> {
        if let Some(flight) = &self.flight {
            return (!flight.reported).then(|| (flight.sent_at + flight.timeout - now).max(0.0));
        }
        if self.outbox.borrow().is_some() {
            let at = self.last_sent.map_or(now, |t| t + SAVE_EVERY);
            return Some((at - now).max(0.0));
        }
        None
    }
}

/// Whether a plain backup can be written back as the app writes its library: the file is no
/// newer than this build's format, and holds nothing but what that format has.
fn rewritable(text: &str) -> bool {
    const KNOWN: [&str; 5] = [
        "format",
        "schemaVersion",
        "exportedAt",
        "deviceId",
        "projects",
    ];
    prosecraft_core::js::parse(text).is_ok_and(|bundle| {
        prosecraft_core::storage::schema::newer_schema(&bundle).is_none()
            && bundle.as_object().is_some_and(|fields| {
                fields
                    .iter()
                    .all(|(key, _)| KNOWN.iter().any(|known| key.eq_str(known)))
            })
    })
}

#[cfg(test)]
mod tests;
