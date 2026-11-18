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
pub mod fonts;
mod library;
mod sidebar;
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
}

/// Something only the host can do: native file dialogs, writing files the person chose.
#[derive(Clone, Debug, PartialEq)]
pub enum HostRequest {
    /// Ask for a backup to open, then call [`ProseApp::open_backup_bytes`].
    OpenBackup,
    /// Ask where to save this backup, then write it there.
    ExportBackup { file_name: String, text: String },
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
    ctx.set_missing_glyph_policy(egui::MissingGlyphPolicy::Tofu);
    fonts::install(ctx);
    theme::Palette::default().apply(ctx);
}

/// The whole app.
pub struct ProseApp {
    state: AppState,
    open: Vec<Pane>,
    pane_states: HashMap<Pane, PaneState>,
    locked: Option<Locked>,
    ready: bool,
    home: Option<Home>,
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
            "Your library is empty. Open a ProseCraft backup to bring your projects in.".to_owned()
        } else {
            format!(
                "Your library: {count} project{}.",
                if count == 1 { "" } else { "s" }
            )
        };
        app.state.set_library(views, status);
        app.state.has_home = true;
        app.home = Some(Home { library, store });
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
        egui::CentralPanel::default().show(ui, |ui| views::show(ui, &mut self.state));
        self.passphrase_dialog(ui);
        if std::mem::take(&mut self.state.export_wanted) {
            self.export();
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
