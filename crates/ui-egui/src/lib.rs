//! prosecraft-rs user interface, drawn with egui. The desktop and web apps both host [`ProseApp`].
//!
//! The page composition follows ProseCraft's (`src/routes/+page.svelte`, upstream `7dae544`): the
//! binder on the left, the workspace views in the middle (Outline, Write, Storyboard, Timeline,
//! Publish), and the sidebar on the right (Progress, Comments, Snapshots, Footnotes). The panes
//! are craft-ui's, so they size, collapse and take F6 focus like the other Craft apps'.
//!
//! This stage is read-only: it opens ProseCraft backups and shows them. Nothing is written.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod binder;
mod device_sync;
mod editing;
pub mod find;
pub mod fonts;
mod footnote_editor;
mod fountain_import;
mod labels;
mod library;
mod markdown_import;
mod project_settings;
mod publish;
mod screenplay_editor;
mod scrivener_import;
mod settings;
mod sidebar;
mod starter;
mod storyboard;
pub mod theme;
mod views;
mod widgets;

use std::collections::HashMap;

use craft_ui::panes::{Edge, PaneContent, PaneHost, PaneRequest, PaneState};
use egui::{Id, Key, Ui};
use prosecraft_core::about;
use prosecraft_core::js::JsString;
use prosecraft_core::storage::bundle::EncryptedBundle;

pub use library::{Library, OpenError};
pub use prosecraft_core::library::Library as ProjectLibrary;

/// Where the app keeps its library: the host saves it (the desktop app to a file, atomically).
pub trait LibraryStore {
    /// Saves the library's text. An error leaves the previous library in place.
    fn save(&mut self, text: &str) -> Result<(), String>;
    /// Now, in milliseconds since 1970 (ProseCraft's timestamps).
    fn now_ms(&self) -> f64;
    /// A new random id for a chapter or scene (a UUID, as ProseCraft's `crypto.randomUUID()`).
    fn new_id(&mut self) -> String;
    /// Today's date as `YYYY-MM-DD` in local time, for writing activity (ProseCraft's
    /// `dateKey`). Without a local clock, the UTC date of [`Self::now_ms`].
    fn today(&self) -> String {
        chrono::DateTime::from_timestamp_millis(self.now_ms() as i64)
            .map(|t| t.format("%Y-%m-%d").to_string())
            .unwrap_or_default()
    }
    /// Whether this host can sync through a folder (the desktop app).
    fn supports_sync(&self) -> bool {
        false
    }
    /// The sync folder's files, for a sync on a background thread.
    fn sync_fs(
        &mut self,
        _folder: &str,
    ) -> Option<Box<dyn prosecraft_core::sync::SyncFileSystem + Send>> {
        None
    }
    /// A device setting, under ProseCraft's `localStorage` key (`prosecraft-theme`,
    /// `prosecraft-sync-state`…).
    fn load_setting(&mut self, _key: &str) -> Option<String> {
        None
    }
    /// Whether CraftCloud's `craftcloud` program is installed (then its folder is offered).
    fn has_craftcloud(&self) -> bool {
        false
    }
    /// The folder to sync through inside CraftCloud's sync folder, or why there is none.
    fn craftcloud_folder(&mut self) -> Result<String, String> {
        Err("CraftCloud isn't installed.".into())
    }
    /// Saves (or, with `None`, forgets) a device setting.
    fn save_setting(&mut self, _key: &str, _value: Option<&str>) -> Result<(), String> {
        Ok(())
    }
}

/// Where the library's changes are.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SaveState {
    #[default]
    Saved,
    Pending,
    Failed(String),
}

/// Something only the host can do: native file dialogs, writing files the person chose.
#[derive(Clone, Debug, PartialEq)]
pub enum HostRequest {
    /// Ask for a backup to open, then call [`ProseApp::open_backup_bytes`].
    OpenBackup,
    /// Ask where to save this backup, then write it there.
    ExportBackup { file_name: String, text: String },
    /// Ask for an EPUB to import, then call [`ProseApp::import_epub`].
    ImportEpub,
    /// Ask for a zipped Scrivener project, then call [`ProseApp::import_scrivener_zip`].
    ImportScrivenerZip,
    /// Ask for a Scrivener project folder, then call [`ProseApp::import_scrivener_folder`].
    ImportScrivenerFolder,
    /// Ask for a folder of Markdown files, then call [`ProseApp::import_markdown_folder`] with
    /// them (or [`ProseApp::import_markdown_folder_failed`]).
    ImportMarkdownFolder,
    /// Ask for a Fountain screenplay, then call [`ProseApp::import_fountain`].
    ImportFountain,
    /// Ask for a sync folder, then call [`ProseApp::set_sync_folder`].
    ChooseSyncFolder,
    /// Ask where to save an exported manuscript, then write it there. `kind` names the format
    /// ("Plain text") and `extension` its file extension, for the dialog's filter.
    ExportFile {
        file_name: String,
        bytes: Vec<u8>,
        kind: String,
        extension: String,
    },
}

/// The app's own library and where it is saved.
struct Home {
    library: ProjectLibrary,
    store: Box<dyn LibraryStore>,
}

/// The views of the main panel, as ProseCraft names them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WorkspaceView {
    #[default]
    Outline,
    Write,
    Storyboard,
    Timeline,
    Publish,
}

impl WorkspaceView {
    pub const ALL: [Self; 5] = [
        Self::Outline,
        Self::Write,
        Self::Storyboard,
        Self::Timeline,
        Self::Publish,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Outline => "Outline",
            Self::Write => "Write",
            Self::Storyboard => "Storyboard",
            Self::Timeline => "Timeline",
            Self::Publish => "Publish",
        }
    }
}

/// The tabs of the right sidebar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SideTab {
    #[default]
    Progress,
    Comments,
    Snapshots,
    Footnotes,
}

impl SideTab {
    pub const ALL: [Self; 4] = [
        Self::Progress,
        Self::Comments,
        Self::Snapshots,
        Self::Footnotes,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Progress => "Progress",
            Self::Comments => "Comments",
            Self::Snapshots => "Snapshots",
            Self::Footnotes => "Footnotes",
        }
    }
}

/// What the Write view shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    /// One scene or Wiki page.
    Document(String),
    /// A whole chapter, its scenes one after another (ProseCraft's chapter mode).
    Chapter(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Pane {
    Binder,
    Sidebar,
}

/// A backup waiting for its passphrase.
struct Locked {
    name: String,
    bundle: EncryptedBundle,
    passphrase: String,
    error: Option<String>,
    /// Put keyboard focus in the passphrase field on the next frame: when the dialog opens and
    /// after a wrong passphrase, so the next attempt can be typed straight away.
    focus: bool,
}

/// Everything the panes and views draw from.
#[derive(Default)]
pub struct AppState {
    library: Option<Library>,
    active: usize,
    view: WorkspaceView,
    side_tab: SideTab,
    selection: Option<Selection>,
    /// The last message for the person: what was opened, or why it wasn't.
    status: Option<String>,
    palette: theme::Palette,
    /// The month the activity calendar shows, once the person has moved it.
    activity_month: Option<(i32, u32)>,
    /// Whether there is a library to add backups to (else backups are only shown).
    has_home: bool,
    /// Requests for the host, taken with [`ProseApp::take_requests`].
    requests: Vec<HostRequest>,
    /// The binder's Export button was pressed; the app builds the backup after the frame.
    export_wanted: bool,
    /// Whether projects can be changed (a library that isn't read-only).
    editable: bool,
    /// Changes the panes asked for, applied after the frame.
    intents: Vec<editing::Intent>,
    /// The scene or page being typed into.
    draft: Option<editing::Draft>,
    save_state: SaveState,
    /// egui's time this frame, for the draft's pause.
    now: f64,
    /// The store's clock this frame, in Unix milliseconds (0 without a library store), for
    /// dates written into exports.
    now_ms: f64,
    /// A fresh id from the store for the next EPUB (its book identifier); an export takes it.
    book_id: Option<String>,
    /// The Comments tab: the passage about to be commented on, the comment and author being
    /// written, the list's filters, and a passage to select in the editor ("Go to").
    pending_comment: Option<PendingComment>,
    comment_draft: String,
    comment_author: String,
    show_all_comments: bool,
    show_resolved_comments: bool,
    comment_goto: Option<(String, usize, usize)>,
    /// The Snapshots tab's note for the next snapshot, and the one being previewed.
    snapshot_note: String,
    snapshot_preview: Option<String>,
    /// The "Start from" dialog for a new project, while open, and a project to create.
    starter: Option<starter::StarterDraft>,
    create_project: Option<(bool, prosecraft_core::starters::Blueprint)>,
    /// A Wiki folder being renamed: its id and the name being typed.
    renaming_folder: Option<(String, String)>,
    /// The Find and replace dialog, while open.
    find: Option<find::FindState>,
    /// A footnote to open in the footnote editor (document, id), and the one open.
    footnote_open: Option<(String, String)>,
    footnote_edit: Option<footnote_editor::FootnoteEdit>,
    /// A Scrivener project read and shown before it is imported, and one to import now.
    scrivener: Option<scrivener_import::ScrivenerPreview>,
    scrivener_done: Option<scrivener_import::ScrivenerPreview>,
    /// A Markdown folder read and shown in the import dialog, and one to import now.
    markdown_import: Option<markdown_import::MarkdownImportDraft>,
    markdown_import_done: Option<markdown_import::MarkdownImportDraft>,
    /// ProseCraft's Markdown editor: the preference, the mode (on for every scene until turned
    /// off), a switch asked for this frame, and the preference changed in Settings.
    markdown_enabled: bool,
    markdown_mode: bool,
    markdown_switch: Option<bool>,
    markdown_enabled_choice: Option<bool>,
    /// The Markdown editor was opened from a scene the rich editor can't hold ("Edit as
    /// Markdown"), so it is on though the preference is off.
    markdown_forced: bool,
    /// The user's own wiki templates (a device setting), and a new list to save (a new one has
    /// no id yet).
    custom_starters: Vec<prosecraft_core::starters::custom::CustomStarter>,
    custom_starters_save: Option<Vec<prosecraft_core::starters::custom::CustomStarter>>,
    /// The Labels dialog, while open.
    labels_dialog: Option<labels::LabelsDraft>,
    /// The Project Settings dialog, while open.
    project_settings: Option<project_settings::ProjectSettingsDraft>,
    /// The Settings dialog is open.
    settings_open: bool,
    /// The Settings dialog's category.
    settings_tab: settings::Category,
    /// The Theme preference, and one chosen in Settings this frame.
    theme: settings::ThemePreference,
    theme_choice: Option<settings::ThemePreference>,
    /// The Settings dialog's Device sync group.
    sync: settings::SyncPanel,
    /// What the Device sync group asked for this frame.
    sync_actions: Vec<settings::SyncAction>,
}

impl AppState {
    pub fn view(&self) -> WorkspaceView {
        self.view
    }

    pub fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn save_state(&self) -> &SaveState {
        &self.save_state
    }

    pub fn is_editable(&self) -> bool {
        self.editable
    }

    pub fn library(&self) -> Option<&Library> {
        self.library.as_ref()
    }

    fn select(&mut self, selection: Selection) {
        self.selection = Some(selection);
        self.view = WorkspaceView::Write;
    }

    fn set_library(&mut self, library: Library, status: String) {
        self.status = Some(status);
        self.library = Some(library);
        self.active = 0;
        self.selection = None;
        self.view = WorkspaceView::Outline;
    }
}

impl PaneContent<Pane> for AppState {
    fn title(&self, pane: &Pane) -> String {
        match pane {
            Pane::Binder => "Binder".into(),
            Pane::Sidebar => "Sidebar".into(),
        }
    }

    fn body(&mut self, ui: &mut Ui, pane: &Pane) {
        match pane {
            Pane::Binder => binder::show(ui, self),
            Pane::Sidebar => sidebar::show(ui, self),
        }
    }

    fn default_state(&self, pane: &Pane) -> PaneState {
        match pane {
            Pane::Binder => PaneState::docked(Edge::Start, 280.0),
            Pane::Sidebar => PaneState::docked(Edge::End, 300.0),
        }
    }

    /// ProseCraft's sidebars stay where they are.
    fn movable(&self, _pane: &Pane) -> bool {
        false
    }

    /// ProseCraft's sidebars have no title bars; the contents start at the top.
    fn title_bar(&self, _pane: &Pane) -> bool {
        false
    }

    fn closable(&self, _pane: &Pane) -> bool {
        false
    }
}

/// Prepares an egui context for the app. Call once before the first frame.
///
/// People's projects hold any character there is (emoji in a title, a script no bundled font
/// covers). egui can panic on a character it has no glyph for; here it draws a box instead, so
/// text never crashes the app.
pub fn setup(ctx: &egui::Context) {
    setup_with(ctx, true);
}

/// [`setup`] without the system's fonts, for tests: text draws the same everywhere, and no
/// font arriving late from the background index repaints in the middle of a test.
pub fn setup_for_tests(ctx: &egui::Context) {
    setup_with(ctx, false);
}

fn setup_with(ctx: &egui::Context, system_fonts: bool) {
    ctx.set_missing_glyph_policy(egui::MissingGlyphPolicy::Tofu);
    fonts::install(ctx, system_fonts);
    theme::Palette::default().apply(ctx);
}

/// ProseCraft's `localStorage` key for the comment author's name.
const COMMENT_AUTHOR_KEY: &str = "prosecraft-comment-author";

/// A passage chosen for a new comment.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PendingComment {
    pub document: String,
    pub anchor: prosecraft_core::comments::Anchor,
}

/// The whole app.
pub struct ProseApp {
    state: AppState,
    open: Vec<Pane>,
    pane_states: HashMap<Pane, PaneState>,
    locked: Option<Locked>,
    ready: bool,
    home: Option<Home>,
    /// The library has changes not yet saved, last made at this egui time.
    unsaved_since: Option<f64>,
    /// A device sync running in the background.
    sync_job: Option<device_sync::SyncJob>,
    /// A Markdown folder import being built in the background.
    markdown_job: Option<
        std::sync::mpsc::Receiver<(
            prosecraft_core::js::JsValue,
            prosecraft_core::import::markdown_folder::MarkdownImportReport,
        )>,
    >,
}

impl Default for ProseApp {
    fn default() -> Self {
        Self::new()
    }
}

impl ProseApp {
    pub fn new() -> Self {
        Self {
            state: AppState::default(),
            open: vec![Pane::Binder, Pane::Sidebar],
            pane_states: HashMap::new(),
            locked: None,
            ready: false,
            home: None,
            unsaved_since: None,
            sync_job: None,
            markdown_job: None,
        }
    }

    /// The app with its own library, saved through `store`. Backups opened later are added to it.
    pub fn with_library(library: ProjectLibrary, store: Box<dyn LibraryStore>) -> Self {
        let mut app = Self::new();
        let views = Library::from_stored(library.projects().cloned().collect());
        let count = views.projects().len();
        let status = if library.is_read_only() {
            "This library has a project from a newer version of ProseCraft. It is shown, but nothing is saved.".to_owned()
        } else if count == 0 {
            "Your library is empty. Create a project, or open a ProseCraft backup to bring your projects in.".to_owned()
        } else {
            format!(
                "Your library: {count} project{}.",
                if count == 1 { "" } else { "s" }
            )
        };
        app.state.set_library(views, status);
        app.state.has_home = true;
        app.state.editable = !library.is_read_only();
        app.state.save_state = SaveState::Saved;
        app.home = Some(Home { library, store });
        app.init_sync();
        app
    }

    /// What the host should do for the app (file dialogs, writing exports). Take them after
    /// each frame.
    pub fn take_requests(&mut self) -> Vec<HostRequest> {
        std::mem::take(&mut self.state.requests)
    }

    /// Projects read from a backup: added to the library (newer copies win, as in ProseCraft),
    /// or, without one, only shown.
    fn receive(&mut self, name: &str, projects: Vec<prosecraft_core::js::JsValue>) {
        let Some(home) = self.home.as_mut() else {
            let count = projects.len();
            let status = format!(
                "Opened {name}: {count} project{}. Read-only: nothing is written back.",
                if count == 1 { "" } else { "s" }
            );
            self.state
                .set_library(Library::from_stored(projects), status);
            return;
        };
        let result = home.library.merge(projects);
        let saved = if home.library.is_read_only() {
            Err(
                "it has a project from a newer version of ProseCraft, so nothing is saved"
                    .to_owned(),
            )
        } else {
            let text = home.library.to_text(home.store.now_ms());
            home.store.save(&text)
        };
        let plural = if result.imported == 1 { "" } else { "s" };
        let status = match saved {
            Ok(()) => format!(
                "Added {} project{plural} from {name} to your library ({} in all).",
                result.imported, result.total
            ),
            Err(error) => format!("Read {name}, but couldn't save your library: {error}"),
        };
        let views = Library::from_stored(home.library.projects().cloned().collect());
        self.state.set_library(views, status);
    }

    /// The open project's id.
    fn active_project_id(&self) -> Option<String> {
        self.state
            .library
            .as_ref()
            .and_then(|l| l.projects().get(self.state.active))
            .map(|p| p.view.id.clone())
    }

    /// Rebuilds the views after a change, keeping the open project and selection.
    fn refresh_views(&mut self) {
        let Some(home) = self.home.as_ref() else {
            return;
        };
        let active = self.active_project_id();
        let views = Library::from_stored(home.library.projects().cloned().collect());
        self.state.active = active
            .and_then(|id| views.projects().iter().position(|p| p.view.id == id))
            .unwrap_or(0);
        self.state.library = Some(views);
    }

    /// Applies one change to the open project, stamped, and marks the library unsaved.
    fn change(
        &mut self,
        change: impl FnOnce(
            &mut prosecraft_core::js::JsValue,
            &mut dyn FnMut() -> String,
        ) -> Result<(), prosecraft_core::edit::EditError>,
    ) {
        let Some(project_id) = self.active_project_id() else {
            return;
        };
        let Some(home) = self.home.as_mut() else {
            return;
        };
        if home.library.is_read_only() {
            return;
        }
        let now_ms = home.store.now_ms();
        let store = &mut home.store;
        let mut new_id = || store.new_id();
        let result = editing::apply(&mut home.library, &project_id, now_ms, |data| {
            change(data, &mut new_id)
        });
        match result {
            Ok(()) => {
                self.unsaved_since = Some(self.state.now);
                self.state.save_state = SaveState::Pending;
            }
            Err(error) => self.state.status = Some(format!("That change wasn't made: {error}")),
        }
        self.refresh_views();
    }

    /// Commits the draft to the project if it has unsaved typing.
    fn commit_draft(&mut self) {
        let Some(draft) = self.state.draft.clone().filter(editing::Draft::is_dirty) else {
            return;
        };
        let today = self
            .home
            .as_ref()
            .map(|h| h.store.today())
            .unwrap_or_default();
        self.change(|data, _| editing::commit_draft(&draft, data, &today));
        if let Some(current) = self.state.draft.as_mut() {
            current.title_dirty = false;
            current.body_dirty = false;
            current.edits = 0;
            current.words_added = 0.0;
        }
    }

    /// After a Wiki page or folder was added: open the page, or start renaming the folder (the
    /// newest one, as each is added last).
    fn after_wiki_add(&mut self, page: bool) {
        let Some(project) = self
            .state
            .library
            .as_ref()
            .and_then(|l| l.projects().get(self.state.active))
        else {
            return;
        };
        let data = project.stored.get("project");
        if page {
            let last = data
                .and_then(|d| d.get("documents"))
                .and_then(|d| d.as_array())
                .and_then(|d| d.last())
                .and_then(|d| d.get("id"))
                .and_then(|i| i.as_js_string())
                .map(|s| s.to_string_lossy());
            if let Some(id) = last {
                self.state.draft = None;
                self.state.select(Selection::Document(id));
            }
        } else {
            let last = data
                .and_then(|d| d.get("researchFolders"))
                .and_then(|d| d.as_array())
                .and_then(|d| d.last())
                .and_then(|f| f.get("id"))
                .and_then(|i| i.as_js_string())
                .map(|s| s.to_string_lossy());
            if let Some(id) = last {
                self.state.renaming_folder =
                    Some((id, prosecraft_core::wiki::FOLDER_TITLE.to_owned()));
            }
        }
    }

    /// `insertFootnoteAtCaret`: a marker at character `at` of the open scene, an empty
    /// definition, and the footnote editor open on it.
    fn insert_footnote(&mut self, document: &str, at: usize) {
        let Some(home) = self.home.as_mut() else {
            return;
        };
        let existing: Vec<String> = self
            .state
            .library
            .as_ref()
            .and_then(|l| l.projects().get(self.state.active))
            .and_then(|p| p.view.document(document))
            .map(|d| d.footnotes.iter().map(|f| f.id.clone()).collect())
            .unwrap_or_default();
        // `createFootnoteId`, from the store's randomness; a new id in this scene.
        let id = (0..16)
            .map(|_| {
                let seed = home.store.new_id();
                let hex: Vec<u8> = seed
                    .bytes()
                    .filter_map(|b| (b as char).to_digit(16).map(|d| d as u8))
                    .collect();
                let mut pairs = hex
                    .chunks(2)
                    .map(|p| f64::from(p.iter().fold(0u32, |a, d| a * 16 + u32::from(*d))) / 256.0);
                prosecraft_core::footnotes::create_footnote_id(|| pairs.next().unwrap_or(0.0))
            })
            .find(|id| !existing.contains(id));
        let Some(id) = id else { return };
        let now = self.state.now;
        let Some(draft) = self.state.draft.as_mut().filter(|d| d.document == document) else {
            return;
        };
        draft.body = draft
            .doc
            .insert_atom(at, &prosecraft_core::footnotes::marker_html(&id));
        draft.body_dirty = true;
        draft.changed_at = now;
        // The caret goes after the marker.
        self.state.comment_goto = Some((document.to_owned(), at + 1, at + 1));
        self.commit_draft();
        self.change(|data, _| prosecraft_core::footnotes::add_definition(data, document, &id));
        self.state.footnote_open = Some((document.to_owned(), id));
    }

    /// The markers of footnote `id` leave the open scene's draft (the caret where the first
    /// was); the definition goes with the intent.
    fn remove_footnote_marker(&mut self, document: &str, id: &str) {
        let now = self.state.now;
        if let Some(draft) = self.state.draft.as_mut().filter(|d| d.document == document) {
            let (body, at) = draft.doc.remove_atoms(|html| {
                prosecraft_core::footnotes::extract_ids(html)
                    .first()
                    .is_some_and(|i| i == id)
            });
            if let Some(at) = at {
                draft.body = body;
                draft.body_dirty = true;
                draft.changed_at = now;
                self.state.comment_goto = Some((document.to_owned(), at, at));
            }
        }
        if self
            .state
            .footnote_edit
            .as_ref()
            .is_some_and(|e| e.id == id)
        {
            self.state.footnote_edit = None;
        }
    }

    /// The templates the Start-from dialog changed, saved as the device setting
    /// (`saveCustomStarters`), with ids for new ones, then read back as saved.
    fn save_custom_starters(&mut self) {
        use prosecraft_core::starters::custom::{
            CUSTOM_STARTERS_KEY, custom_starters_json, load_custom_starters,
        };
        let Some(home) = self.home.as_mut() else {
            return;
        };
        let Some(mut next) = self.state.custom_starters_save.take() else {
            return;
        };
        for template in next.iter_mut().filter(|t| t.id.is_empty()) {
            template.id = home.store.new_id();
        }
        let json = custom_starters_json(&next);
        match home.store.save_setting(CUSTOM_STARTERS_KEY, Some(&json)) {
            Ok(()) => self.state.custom_starters = load_custom_starters(Some(&json)),
            Err(_) => {
                if let Some(draft) = self.state.starter.as_mut() {
                    draft.failed("Could not save the template on this device.");
                }
            }
        }
    }

    /// Writes the library now if it has changes.
    fn save_now(&mut self) {
        let Some(home) = self.home.as_mut() else {
            return;
        };
        if self.unsaved_since.is_none() {
            return;
        }
        let text = home.library.to_text(home.store.now_ms());
        match home.store.save(&text) {
            Ok(()) => {
                self.unsaved_since = None;
                self.state.save_state = SaveState::Saved;
            }
            Err(error) => {
                // Kept unsaved: the next pause tries again.
                self.unsaved_since = Some(self.state.now);
                self.state.save_state = SaveState::Failed(error);
            }
        }
    }

    /// Commits any typing and saves. The host calls it before the app closes.
    pub fn flush(&mut self) {
        self.commit_draft();
        self.save_now();
    }

    /// After each frame: the panes' changes, the draft after a pause, the library after a pause.
    fn after_frame(&mut self) {
        // `switchToMarkdownEditor` / `switchToRichTextEditor`: the typing so far saved, then
        // the editor reopened in the other mode.
        if let Some(on) = self.state.markdown_switch.take() {
            self.commit_draft();
            if !on {
                self.state.markdown_forced = false;
            }
            self.state.markdown_mode =
                on && (self.state.markdown_enabled || self.state.markdown_forced);
            self.state.draft = None;
        }
        for intent in std::mem::take(&mut self.state.intents) {
            match &intent {
                editing::Intent::InsertFootnote { document, at } => {
                    self.insert_footnote(document, *at);
                    continue;
                }
                editing::Intent::RemoveFootnote { document, id } => {
                    self.remove_footnote_marker(document, id)
                }
                _ => {}
            }
            self.commit_draft();
            self.change(|data, new_id| editing::run(&intent, data, new_id));
            if let editing::Intent::AddComment { author, .. } = &intent
                && let Some(home) = self.home.as_mut()
            {
                // The author is remembered for the next comment, as ProseCraft does.
                let _ = home.store.save_setting(COMMENT_AUTHOR_KEY, Some(author));
            }
            // A new page opens; a new folder starts its rename (as ProseCraft does).
            if matches!(
                intent,
                editing::Intent::AddWikiPage { .. } | editing::Intent::AddWikiFolder { .. }
            ) {
                self.after_wiki_add(matches!(intent, editing::Intent::AddWikiPage { .. }));
            }
            if matches!(
                intent,
                editing::Intent::RestoreSnapshot { .. }
                    | editing::Intent::ConvertToScreenplay { .. }
            ) {
                // The editor reloads the restored text (its draft held the replaced one).
                self.state.draft = None;
            }
        }
        let now = self.state.now;
        if self
            .state
            .draft
            .as_ref()
            .is_some_and(|d| d.is_dirty() && now - d.changed_at >= editing::COMMIT_AFTER)
        {
            self.commit_draft();
        }
        if self
            .unsaved_since
            .is_some_and(|since| now - since >= editing::SAVE_AFTER)
        {
            self.save_now();
        }
    }

    /// The library as a ProseCraft backup, for the host to save where the person chooses.
    fn export(&mut self) {
        let Some(home) = self.home.as_ref() else {
            return;
        };
        let text = home.library.to_text(home.store.now_ms());
        self.state.requests.push(HostRequest::ExportBackup {
            file_name: "prosecraft-backup.prosecraft".into(),
            text,
        });
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// Whether a passphrase is being asked for.
    pub fn is_locked(&self) -> bool {
        self.locked.is_some()
    }

    /// Opens a ProseCraft backup (`.prosecraft`, plain or encrypted) from its text. An encrypted
    /// one asks for its passphrase first. Nothing is ever written back to the file.
    pub fn open_backup(&mut self, name: &str, text: &str) {
        match Library::read(text) {
            Ok(projects) => self.receive(name, projects),
            Err(OpenError::Encrypted(bundle)) => {
                self.locked = Some(Locked {
                    name: name.to_owned(),
                    bundle: *bundle,
                    passphrase: String::new(),
                    error: None,
                    focus: true,
                });
            }
            Err(error) => self.state.status = Some(format!("Couldn't open {name}: {error}")),
        }
    }

    /// Opens a dropped or chosen file's bytes.
    pub fn open_backup_bytes(&mut self, name: &str, bytes: &[u8]) {
        match std::str::from_utf8(bytes) {
            Ok(text) => self.open_backup(name, text),
            Err(_) => {
                self.state.status = Some(format!(
                    "Couldn't open {name}: it isn't a ProseCraft backup."
                ))
            }
        }
    }

    /// Backups are a few megabytes; anything far larger isn't one and isn't read into memory.
    pub const MAX_BACKUP_BYTES: u64 = 512 * 1024 * 1024;

    #[cfg(not(target_arch = "wasm32"))]
    fn open_dropped(&mut self, file: &dyn egui::DroppedFile) {
        let path = file.path();
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        if std::fs::metadata(path).is_ok_and(|m| m.len() > Self::MAX_BACKUP_BYTES) {
            self.state.status = Some(format!(
                "Couldn't open {name}: it is far too large to be a ProseCraft backup."
            ));
            return;
        }
        match file.bytes() {
            // A dropped EPUB is imported (`importEpubData`); anything else is a backup.
            Ok(bytes) if name.to_lowercase().ends_with(".epub") => self.import_epub(&name, &bytes),
            Ok(bytes) => self.open_backup_bytes(&name, &bytes),
            Err(error) => self.state.status = Some(format!("Couldn't open {name}: {error}")),
        }
    }

    fn unlock(&mut self) {
        let Some(locked) = self.locked.as_mut() else {
            return;
        };
        match Library::unlock(&locked.bundle, &JsString::from(locked.passphrase.as_str())) {
            Ok(projects) => {
                let name = locked.name.clone();
                self.locked = None;
                self.receive(&name, projects);
            }
            Err(error) => {
                locked.passphrase.clear();
                locked.error = Some(error.to_string());
                locked.focus = true;
            }
        }
    }

    /// Draws the app into `ui`.
    pub fn show(&mut self, ui: &mut Ui) {
        if !self.ready {
            setup(ui.ctx());
            self.ready = true;
        }
        self.state.now = ui.ctx().input(|i| i.time);
        self.state.now_ms = self.home.as_ref().map_or(0.0, |h| h.store.now_ms());
        // Drawn only while publishing, so other views use no ids.
        if self.state.book_id.is_none()
            && self.state.view == WorkspaceView::Publish
            && let Some(home) = self.home.as_mut()
        {
            self.state.book_id = Some(home.store.new_id());
        }
        // Files dropped on the window. (The web build reads them asynchronously; that comes
        // with the web build's file handling.)
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dropped = ui.ctx().input(|input| input.raw.dropped_files.clone());
            for file in dropped {
                self.open_dropped(file.as_ref());
            }
        }

        let states = self.pane_states.clone();
        let palette = self.state.palette;
        let frame = egui::Frame::new()
            .fill(palette.sidebar_bg)
            .stroke(egui::Stroke::new(1.0, palette.border));
        let output = PaneHost::new(Id::unique("prosecraft-panes"))
            .frame(frame)
            .body_margin(egui::Margin::symmetric(18, 16))
            .show(
                ui,
                &self.open,
                |pane| states.get(pane).copied(),
                &mut self.state,
            );
        for request in output.requests {
            let pane = *request.pane();
            let state = self
                .pane_states
                .entry(pane)
                .or_insert_with(|| self.state.default_state(&pane));
            match request {
                PaneRequest::Resize(_, size) => state.size = Some(size),
                PaneRequest::Place(_, place) => state.place = place,
                PaneRequest::FloatRect(_, rect) => state.float_rect = Some(rect),
                PaneRequest::Close(_) | PaneRequest::Focus(_) | PaneRequest::Before(..) => {}
            }
        }
        self.apply_theme(ui.ctx());
        egui::CentralPanel::default().show(ui, |ui| views::show(ui, &mut self.state));
        settings::show(ui, &mut self.state);
        starter::show(ui, &mut self.state);
        project_settings::show(ui, &mut self.state);
        labels::show(ui, &mut self.state);
        footnote_editor::show(ui, &mut self.state);
        scrivener_import::show(ui, &mut self.state);
        if let Some(preview) = self.state.scrivener_done.take() {
            self.finish_scrivener_import(preview);
        }
        markdown_import::show(ui, &mut self.state);
        if let Some(draft) = self.state.markdown_import_done.take() {
            self.finish_markdown_import(draft);
        }
        // Ctrl+F opens Find and replace, as in ProseCraft.
        if self.state.find.is_none()
            && self.state.library.is_some()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::F))
        {
            self.state.find = Some(find::FindState::new());
        }
        find::show(ui, &mut self.state);
        if let Some((screenplay, blueprint)) = self.state.create_project.take() {
            self.create_project(screenplay, &blueprint);
        }
        self.passphrase_dialog(ui);
        self.handle_sync_actions();
        self.save_custom_starters();
        if let Some(enabled) = self.state.markdown_enabled_choice.take() {
            self.state.markdown_enabled = enabled;
            if !enabled && self.state.markdown_mode {
                self.state.markdown_switch = Some(false);
            }
            if let Some(home) = self.home.as_mut()
                && let Err(error) = home.store.save_setting(
                    settings::MARKDOWN_EDITOR_KEY,
                    Some(if enabled { "true" } else { "false" }),
                )
            {
                self.state.status = Some(format!("Could not save the setting: {error}"));
            }
        }
        if let Some(theme) = self.state.theme_choice.take() {
            self.state.theme = theme;
            if let Some(home) = self.home.as_mut()
                && let Err(error) = home
                    .store
                    .save_setting(settings::THEME_KEY, Some(theme.id()))
            {
                self.state.status = Some(format!("Could not save the theme: {error}"));
            }
        }
        if self.poll_sync() | self.poll_markdown_import() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        if std::mem::take(&mut self.state.export_wanted) {
            self.flush();
            self.export();
        }
        self.after_frame();
        if self
            .state
            .draft
            .as_ref()
            .is_some_and(editing::Draft::is_dirty)
            || self.unsaved_since.is_some()
        {
            // Wake up again to commit and save after the pause, even with no input.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(300));
        }
    }

    /// The palette for the Theme preference: Follow system uses the system's light or dark
    /// theme (dark when the system doesn't say, as ProseCraft opens).
    fn apply_theme(&mut self, ctx: &egui::Context) {
        let light = match self.state.theme {
            settings::ThemePreference::Light => true,
            settings::ThemePreference::Dark => false,
            settings::ThemePreference::System => {
                ctx.input(|i| i.raw.system_theme) == Some(egui::Theme::Light)
            }
        };
        let palette = if light {
            theme::Palette::LIGHT
        } else {
            theme::Palette::DARK
        };
        // Compared with the style egui holds, so a style set elsewhere is put right too.
        if palette != self.state.palette || ctx.global_style().visuals != palette.visuals() {
            palette.apply(ctx);
            self.state.palette = palette;
        }
    }

    fn passphrase_dialog(&mut self, ui: &mut Ui) {
        let Some(locked) = self.locked.as_mut() else {
            return;
        };
        let mut submit = false;
        let mut cancel = false;
        egui::Window::new("Passphrase")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.label(format!(
                    "{} is encrypted. Enter its passphrase to open it.",
                    locked.name
                ));
                if let Some(hint) = locked.bundle.hint() {
                    ui.label(format!("Hint: {hint}"));
                }
                let field = ui.add(
                    egui::TextEdit::singleline(&mut locked.passphrase)
                        .password(true)
                        .hint_text("Passphrase"),
                );
                let field = field.labelled_by(ui.label("Passphrase").id);
                if locked.focus {
                    field.request_focus();
                    locked.focus = false;
                }
                if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    submit = true;
                }
                if let Some(error) = &locked.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.horizontal(|ui| {
                    submit |= ui.button("Open").clicked();
                    cancel |= ui.button("Cancel").clicked();
                });
            });
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            cancel = true;
        }
        if cancel {
            self.locked = None;
            self.state.status = Some("Opening the encrypted backup was cancelled.".into());
        } else if submit {
            self.unlock();
        }
    }
}

/// The window title: the app, and the open project if there is one.
pub fn window_title(app: &ProseApp) -> String {
    match app
        .state
        .library
        .as_ref()
        .and_then(|l| l.projects().get(app.state.active))
    {
        Some(project) => format!("{} — {}", project.view.name, about::APP_NAME),
        None => about::APP_NAME.to_owned(),
    }
}
