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

mod about_window;
mod app_menu;
mod binder;
mod character_sheet;
mod citation_picker;
mod clipboard_keys;
mod cover_art;
mod device_sync;
mod editing;
pub mod editor_fonts;
pub mod find;
pub mod fonts;
mod footnote_editor;
mod fountain_import;
mod graph_tile;
mod labels;
mod library;
mod markdown_import;
mod network_view;
mod picture_flow;
mod picture_tools;
mod pictures;
mod preview;
mod project_settings;
mod proofreading;
mod publish;
mod relation_graph;
mod rich_dialog;
mod rich_view;
mod screenplay_editor;
mod scrivener_import;
mod settings;
mod sidebar;
mod sources;
mod split_view;
mod starter;
mod story_data;
mod storyboard;
mod text_marks;
pub mod theme;
mod views;
mod widgets;
mod zotero;

pub use zotero::Transport as ZoteroTransport;
mod wiki_files;
pub use about_window::Platform;
pub use wiki_files::WikiFileStore;

/// A source of random bytes for keys and nonces, made where it is used.
pub type RandomFactory = fn() -> Box<dyn prosecraft_core::storage::crypto::RandomSource + Send>;

/// Puts HTML and its plain text on the clipboard.
pub type ClipboardWriter = std::sync::Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Reads the clipboard's HTML, if it holds some.
pub type ClipboardHtml = std::sync::Arc<dyn Fn() -> Option<String> + Send + Sync>;

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
    /// Ask for a BibTeX, RIS or CSL-JSON file, then call [`ProseApp::import_citation_file`].
    ImportCitations,
    /// Ask for a Fountain screenplay, then call [`ProseApp::import_fountain`].
    ImportFountain,
    /// Ask for a picture (a PNG, JPEG, WebP, GIF, BMP or AVIF file; see
    /// [`prosecraft_core::picture::EXTENSIONS`]), then call [`ProseApp::insert_picture`].
    InsertPicture,
    /// Ask for a cover image (PNG, JPEG or WebP; see
    /// [`prosecraft_core::publish::cover::EXTENSIONS`]), then call [`ProseApp::set_cover_image`].
    ChooseCover,
    /// Ask for a folder for wiki files, then call [`ProseApp::set_wiki_root`]. `add_to`: files
    /// were being added (to that Wiki folder, or the top level), so ask for them next.
    ChooseWikiRoot { add_to: Option<Option<String>> },
    /// Ask for files to add to the Wiki, then call [`ProseApp::add_wiki_files`].
    PickWikiFiles { folder: Option<String> },
    /// Open this web address in the browser.
    OpenUrl(String),
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

/// The passphrase dialog for an encrypted backup (`passphraseMode = "export"`), and the backup
/// being sealed in the background (`passphraseBusy`).
#[derive(Default)]
pub(crate) struct EncryptExport {
    passphrase: String,
    confirm: String,
    hint: String,
    error: Option<String>,
    working: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
}

/// The views of the main panel, as ProseCraft names them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WorkspaceView {
    #[default]
    Outline,
    Write,
    Storyboard,
    Timeline,
    Network,
    Publish,
}

impl WorkspaceView {
    pub const ALL: [Self; 6] = [
        Self::Outline,
        Self::Write,
        Self::Storyboard,
        Self::Timeline,
        Self::Network,
        Self::Publish,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Outline => "Outline",
            Self::Write => "Write",
            Self::Storyboard => "Storyboard",
            Self::Timeline => "Timeline",
            Self::Network => "Network",
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

/// A status message as it was first shown: the view and selection then, and the time.
#[derive(Clone, Debug)]
struct StatusSeen {
    message: String,
    view: WorkspaceView,
    selection: Option<Selection>,
    at: f64,
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
    /// When and where the status message was first shown, to let it go again.
    status_seen: Option<StatusSeen>,
    palette: theme::Palette,
    /// The month the activity calendar shows, once the person has moved it.
    activity_month: Option<(i32, u32)>,
    /// Whether there is a library to add backups to (else backups are only shown).
    has_home: bool,
    /// Requests for the host, taken with [`ProseApp::take_requests`].
    requests: Vec<HostRequest>,
    /// Where a picture asked for goes: the scene and a character.
    picture_target: Option<(String, usize)>,
    /// Why the last cover image couldn't be used (`coverError`).
    cover_error: Option<String>,
    /// Wiki files (`researchStatus`, `researchSettingsStatus`, the link dialog, the Markdown
    /// preview); `wiki_root` is read from the device's settings once.
    wiki_available: bool,
    wiki_root: Option<String>,
    wiki_status: Option<String>,
    wiki_settings_status: Option<String>,
    wiki_link: Option<wiki_files::LinkDraft>,
    wiki_markdown: Option<(String, String)>,
    /// Proofreading: the settings, a change asked for in Settings, and the open scene's check.
    proofread: proofreading::ProofreadPrefs,
    proofread_choice: Option<proofreading::ProofreadPrefs>,
    proofing: proofreading::Proofing,
    rich_dialog: Option<rich_dialog::RichDialog>,
    /// The split view's panes (none when it is closed), and the device's wiki file store for
    /// what they show.
    split: Vec<prosecraft_core::split::Pane>,
    /// The host's clipboard HTML, for rich paste.
    clipboard_html: Option<ClipboardHtml>,
    /// The host's clipboard text, for Edit > Paste.
    clipboard_text: Option<ClipboardHtml>,
    /// How the host puts HTML on the clipboard, for a rich copy.
    clipboard_write: Option<ClipboardWriter>,
    /// The desktop host's clipboard HTML when its text is empty, when the host leaves the
    /// clipboard keys to the app ([`clipboard_keys`]).
    clipboard_keys: Option<ClipboardHtml>,
    /// The web host's HTML of a paste that had no text, taken once.
    html_paste_take: Option<ClipboardHtml>,
    /// This frame's paste with HTML and no text, for the rich editor.
    html_paste: clipboard_keys::HtmlPaste,
    /// Edit > Paste asked for one, given with the menu's input next frame.
    menu_html_paste: bool,
    /// The input an Edit menu item stands for, for the field that had the keys, given next
    /// frame; and the field that last had them.
    menu_events: Vec<egui::Event>,
    /// The text field (and its selection) that had the keys when a menu opened: an Edit item
    /// is for it, or for the page when there was none. The one that had them last frame, and
    /// whether a menu was open then.
    last_focus: Option<(egui::Id, Option<egui::text_edit::TextEditState>)>,
    previous_focus: Option<(egui::Id, Option<egui::text_edit::TextEditState>)>,
    menu_was_open: bool,
    /// ProseCraft's Chapters and Progress toggles: the binder and the progress sidebar hidden.
    binder_hidden: bool,
    sidebar_hidden: bool,
    /// The split view panes' editors.
    tiles: split_view::Tiles,
    /// The Network view's filters, selection, layout and open character card.
    network: network_view::NetworkState,
    /// Fresh ids for what the views create themselves (relations, template fields), topped up
    /// from the store each frame.
    fresh_ids: Vec<String>,
    /// The story data panel open (shared by every scene for the session).
    story_data_open: bool,
    wiki_store: Option<std::rc::Rc<dyn WikiFileStore>>,
    /// Undo and redo (ProseCraft's `UndoManager`), by text field and by page; selections are
    /// character ranges.
    undo: prosecraft_core::undo::UndoManager<(usize, usize)>,
    /// The page the history is kept for (`currentUndoPage`), worked out each frame.
    undo_page: String,
    /// An undo or redo being applied: the project change it makes isn't a new step.
    applying_undo: bool,
    /// Something being deleted for good: no undo step, and every step forgets it
    /// (`forgetPermanentlyDeleted`).
    deleting_forever: bool,
    /// The Writing settings: text size, typewriter, fade and Zen modes, the Timeline view.
    writing: WritingPrefs,
    writing_choice: Option<WritingPrefs>,
    /// Zen mode has hidden the interface (typing), or focus mode has.
    zen_hidden: bool,
    focus_mode: bool,
    /// Until when (egui seconds) the focus-mode hint shows.
    focus_hint_until: f64,
    /// Focus mode asked for from the Settings dialog.
    focus_wanted: bool,
    /// The binder's label filter: the project it is for, and the label (`labelFilter`).
    label_filter: Option<(String, String)>,
    /// The keyboard shortcuts sheet, and which platform's keys it shows.
    shortcuts: Option<bool>,
    /// Where the editor's text was drawn last frame (the pointer leaving it brings Zen mode's
    /// interface back).
    editor_rect: Option<egui::Rect>,
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
    /// A scene being renamed in the binder: its id and the title typed so far.
    renaming_scene: Option<(String, String)>,
    /// Chapters shown or hidden with their chevron in the binder this session, by id.
    chapter_open: std::collections::HashMap<String, bool>,
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
    /// The Sources dialog (the citation library), while open.
    sources: Option<sources::SourcesDialog>,
    /// How to reach Zotero on this computer, when the host can (the desktop app).
    zotero: Option<zotero::Transport>,
    /// The citation picker, while open.
    citation_picker: Option<citation_picker::CitationPicker>,
    /// The editor's citation labels as last worked out.
    cite_labels: std::cell::RefCell<prosecraft_core::citations::labels::LabelCache>,
    /// The Project Settings dialog, while open.
    project_settings: Option<project_settings::ProjectSettingsDraft>,
    /// The Settings dialog is open.
    settings_open: bool,
    /// The Settings dialog's category.
    settings_tab: settings::Category,
    /// The Theme preference, and one chosen in Settings this frame.
    theme: settings::ThemePreference,
    theme_choice: Option<settings::ThemePreference>,
    /// The Color scheme preference (`prosecraft-color-scheme`), and a new choice to save.
    color_scheme: String,
    scheme_choice: Option<String>,
    /// The writing fonts, a new choice to save, the host's installed fonts, and the choice the
    /// app's fonts were last built for.
    /// The About window, while it is open, and where the app runs.
    about: Option<about_window::About>,
    about_wanted: bool,
    platform: about_window::Platform,
    /// Settings > Data > Export encrypted, while its passphrase dialog is open.
    encrypt_export: Option<EncryptExport>,
    /// Where new keys and nonces come from (the web host's; the system's on the desktop).
    random: Option<RandomFactory>,
    font_prefs: editor_fonts::FontPrefs,
    font_choice: Option<editor_fonts::FontPrefs>,
    installed_fonts: Option<std::sync::Arc<dyn editor_fonts::InstalledFonts>>,
    fonts_applied: Option<editor_fonts::FontPrefs>,
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
    theme::STEADY_CURSOR.store(true, std::sync::atomic::Ordering::Relaxed);
    setup_with(ctx, false);
}

/// egui's own line breaking, with a row ending after a line separator: the editor's newline in
/// the text (U+2028) and `<br>` (U+2029) end their row as they do in ProseCraft; and a row may
/// end before and after a picture (`views::PICTURE_CHAR`).
struct RowBreaks;

impl egui::epaint::text::LineBreaker for RowBreaks {
    fn line_breaks(
        &self,
        text: &str,
        language: Option<&str>,
        out: &mut Vec<egui::epaint::text::LineBreak>,
    ) {
        use egui::epaint::text::{LineBreak, SimpleLineBreaker};
        SimpleLineBreaker.line_breaks(text, language, out);
        for (at, c) in text.char_indices() {
            if matches!(c, '\u{2028}' | '\u{2029}') {
                out.push(LineBreak::hard(at + c.len_utf8()));
            }
            if c == views::PICTURE_CHAR {
                out.push(LineBreak::soft(at));
                out.push(LineBreak::soft(at + c.len_utf8()));
            }
        }
    }
}

fn setup_with(ctx: &egui::Context, system_fonts: bool) {
    ctx.set_line_breaker(std::sync::Arc::new(RowBreaks));
    ctx.set_missing_glyph_policy(egui::MissingGlyphPolicy::Tofu);
    ctx.add_glyph_rasterizer(views::atom_glyph());
    fonts::install(ctx, system_fonts);
    // Diagram labels are set in the interface's face (Inter, else egui's own).
    let faces: Vec<&[u8]> = ["Inter-Regular", "Inter-SemiBold"]
        .into_iter()
        .filter_map(fonts::app_face)
        .chain(cover_art::egui_face("Ubuntu-Light"))
        .collect();
    prosecraft_core::rich::set_fonts(&faces);
    theme::Palette::default().apply(ctx);
}

/// ProseCraft's writing preferences (`localStorage`, read and written by the store).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WritingPrefs {
    /// `editorScale`, in percent: 80 to 140 in steps of 10.
    pub scale: u32,
    pub typewriter: bool,
    pub fade: bool,
    pub zen: bool,
    /// "Show Timeline view" (off by default; the timeline's data is kept either way).
    pub timeline: bool,
}

impl Default for WritingPrefs {
    fn default() -> Self {
        Self {
            scale: 100,
            typewriter: false,
            fade: false,
            zen: false,
            timeline: false,
        }
    }
}

impl WritingPrefs {
    pub const SCALES: [u32; 7] = [80, 90, 100, 110, 120, 130, 140];
    const SCALE_KEY: &str = "prosecraft-editor-scale";
    const TYPEWRITER_KEY: &str = "prosecraft-typewriter-mode";
    const FADE_KEY: &str = "prosecraft-fade-other-paragraphs";
    const ZEN_KEY: &str = "prosecraft-zen-mode";
    const TIMELINE_KEY: &str = "prosecraft-timeline-enabled";

    pub(crate) fn load(store: &mut dyn LibraryStore) -> Self {
        let mut flag = |key: &str| store.load_setting(key).as_deref() == Some("true");
        let typewriter = flag(Self::TYPEWRITER_KEY);
        let fade = flag(Self::FADE_KEY);
        let zen = flag(Self::ZEN_KEY);
        let timeline = flag(Self::TIMELINE_KEY);
        let scale = store
            .load_setting(Self::SCALE_KEY)
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|s| Self::SCALES.contains(s))
            .unwrap_or(100);
        Self {
            scale,
            typewriter,
            fade,
            zen,
            timeline,
        }
    }

    fn save(&self, store: &mut dyn LibraryStore) -> Result<(), String> {
        let flag = |on: bool| if on { "true" } else { "false" };
        store.save_setting(Self::SCALE_KEY, Some(&self.scale.to_string()))?;
        store.save_setting(Self::TYPEWRITER_KEY, Some(flag(self.typewriter)))?;
        store.save_setting(Self::FADE_KEY, Some(flag(self.fade)))?;
        store.save_setting(Self::ZEN_KEY, Some(flag(self.zen)))?;
        store.save_setting(Self::TIMELINE_KEY, Some(flag(self.timeline)))
    }
}

/// `capturePageState`: the parts of a project a page's undo history covers.
const UNDO_COLLECTIONS: [&str; 8] = [
    "storyboardCards",
    "storyboardDrawings",
    "storyboardRows",
    "timelineNodes",
    "publishOptions",
    "citations",
    "chapters",
    "documents",
];

/// `undoIgnoredProps`: what typing changes (the editor's own history covers it), and what
/// only shows or hides.
const UNDO_IGNORED: &[(&str, &[&str])] = &[
    (
        "documents",
        &[
            "content",
            "title",
            "markdownSource",
            "folderId",
            "footnotes",
        ],
    ),
    ("chapters", &["expanded"]),
];

fn undo_state(data: &prosecraft_core::js::JsValue) -> prosecraft_core::js::JsObject {
    let mut state = prosecraft_core::js::JsObject::new();
    for name in UNDO_COLLECTIONS {
        if let Some(value) = data.get(name) {
            state.insert(name, value.clone());
        }
    }
    state
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
    /// Where wiki files live on this device (the desktop host's).
    wiki_files: Option<std::rc::Rc<dyn WikiFileStore>>,
    /// The macOS menu bar, which replaces the in-window one there.
    native_menu: Option<craft_menu::NativeMenu>,
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
            wiki_files: None,
            native_menu: None,
        }
    }

    /// On macOS, the app's menus as the system menu bar (ProseCraft's Tauri `run` menu), from
    /// eframe's creator closure on the main thread. Elsewhere, and when AppKit refuses them, the
    /// in-window bar stays.
    #[cfg(target_os = "macos")]
    pub fn install_native_menu(&mut self, ctx: &egui::Context) {
        let menu = craft_menu::mac::install(ctx, &app_menu::mac_layout(&self.state).bar);
        // With the clipboard keys the app's (`set_clipboard_keys`), ⌘C, ⌘X and ⌘V reach it as
        // keys too.
        self.native_menu = match self.state.clipboard_keys {
            Some(_) => menu.map(|m| m.clipboard_keys(craft_menu::ClipboardKeys::Raw)),
            None => menu,
        };
    }

    /// Before egui reads a frame's input (`eframe::App::raw_input_hook`): the native menu's key
    /// equivalents become the key presses they were.
    pub fn raw_input_hook(&mut self, raw: &mut egui::RawInput) {
        if let Some(menu) = self.native_menu.as_mut() {
            menu.raw_input(raw);
        }
    }

    /// Lets the Sources dialog search Zotero through `transport` (a path in, the body or the
    /// error message out): `prosecraft_core::citations::zotero::request` on the desktop.
    pub fn set_zotero(&mut self, transport: ZoteroTransport) {
        self.state.zotero = Some(transport);
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
    /// Says `message` in the status line (for what the host can't do, such as a folder on the
    /// web).
    pub fn report(&mut self, message: &str) {
        self.state.status = Some(message.to_owned());
    }

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

    /// A project's `trashed` flag, stamped, without an undo step (projects aren't on a page).
    fn set_project_trashed(&mut self, id: &str, trashed: bool) {
        let Some(home) = self.home.as_mut().filter(|h| !h.library.is_read_only()) else {
            return;
        };
        let now_ms = home.store.now_ms();
        let result = editing::apply(&mut home.library, id, now_ms, |data| {
            data.as_object_mut()
                .ok_or(prosecraft_core::edit::EditError::Shape)?
                .insert("trashed", prosecraft_core::js::JsValue::Bool(trashed));
            Ok(())
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

    /// `trashActiveProject`: the open project to the Trash and another one opened; the last
    /// project can't go.
    fn trash_project(&mut self) {
        let Some(library) = self.state.library.as_ref() else {
            return;
        };
        let live: Vec<String> = library
            .projects()
            .iter()
            .filter(|p| !p.view.trashed)
            .map(|p| p.view.id.clone())
            .collect();
        let Some(current) = self.active_project_id().filter(|_| live.len() > 1) else {
            return;
        };
        let Some(next) = live.iter().find(|id| **id != current).cloned() else {
            return;
        };
        self.commit_draft();
        self.state.draft = None;
        self.set_project_trashed(&current, true);
        if let Some(index) = self
            .state
            .library
            .as_ref()
            .and_then(|l| l.projects().iter().position(|p| p.view.id == next))
        {
            self.state.active = index;
            self.state.selection = None;
        }
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
        // `commitPageState`: what the change did to the page's state, as an undo step.
        let mut step = None;
        let mut removed = Vec::new();
        let result = editing::apply(&mut home.library, &project_id, now_ms, |data| {
            // `migrateProjectToWiki`, kept with this first change as ProseCraft keeps it with its
            // next save (the views show it from loading on); not part of the undo step.
            if let Some(migrated) = prosecraft_core::wiki_migration::migrate_project_to_wiki(data) {
                *data = migrated;
            }
            let before = undo_state(data);
            change(data, &mut new_id)?;
            let after = undo_state(data);
            removed = prosecraft_core::undo::removed_ids(&before, &after);
            step = prosecraft_core::undo::diff_state(&before, &after, UNDO_IGNORED, now_ms);
            Ok(())
        });
        if self.state.deleting_forever {
            for (collection, id) in &removed {
                self.state.undo.forget(collection, id);
            }
            step = None;
        }
        match result {
            Ok(()) => {
                self.unsaved_since = Some(self.state.now);
                self.state.save_state = SaveState::Pending;
                if let Some(step) = step.filter(|_| !self.state.applying_undo) {
                    let page = self.state.undo_page.clone();
                    self.state.undo.push_state(&page, step);
                }
            }
            Err(error) => self.state.status = Some(format!("That change wasn't made: {error}")),
        }
        self.refresh_views();
    }

    /// The menu bar, the commands chosen in it, and an Edit item's input given to the text
    /// field that had the keys (with its selection, which clicking the menu collapsed).
    fn menu_bar(&mut self, ui: &mut Ui) {
        // The text field with the keys now, with its selection (clicking the menu takes the
        // focus and collapses it).
        let ctx = ui.ctx().clone();
        let focus_now = ctx
            .memory(|m| m.focused())
            .filter(|_| ctx.text_edit_focused())
            .map(|id| (id, egui::TextEdit::load_state(&ctx, id)));
        let chosen = match self.native_menu.as_mut() {
            Some(menu) => {
                menu.set_gate(app_menu::gate(&ctx, &self.state));
                let clicks = menu.take_clicks(&ctx);
                let state = &self.state;
                menu.sync(app_menu::mac_layout(state));
                clicks
            }
            None => {
                egui::Panel::top("menu-bar")
                    .show(ui, |ui| app_menu::show(ui, &self.state))
                    .inner
            }
        };
        // A native menu leaves the focus where it was: its Edit items go to that field.
        if self.native_menu.is_some() {
            self.state.last_focus = focus_now.clone();
        }
        let open = craft_menu::bar::is_open(&ctx);
        if open && !self.state.menu_was_open {
            // The click that opened it may already have taken the focus: last frame's field.
            self.state.last_focus = focus_now
                .clone()
                .or_else(|| self.state.previous_focus.clone());
        }
        self.state.menu_was_open = open;
        if !open {
            self.state.previous_focus = focus_now;
        }
        // An Edit menu item's input, to the field that had the keys before the menu took them.
        let events = std::mem::take(&mut self.state.menu_events);
        let menu_html = std::mem::take(&mut self.state.menu_html_paste);
        if menu_html {
            self.state.html_paste = clipboard_keys::HtmlPaste::Asked;
        }
        if !events.is_empty() || menu_html {
            if let Some((focus, text)) = self.state.last_focus.clone() {
                ui.ctx().memory_mut(|m| m.request_focus(focus));
                if let Some(text) = text {
                    text.store(ui.ctx(), focus);
                }
            }
            ui.ctx().input_mut(|i| {
                // As the keys would be held: a text field reads the modifiers too.
                if let Some(egui::Event::Key { modifiers, .. }) = events.first() {
                    i.modifiers = *modifiers;
                }
                i.events.extend(events);
            });
        }
        for command in chosen {
            self.run_command(ui.ctx(), &command.id);
        }
    }

    /// This frame's clipboard keys (on the desktop) and paste with HTML but no text
    /// ([`clipboard_keys`]).
    fn clipboard_keys(&mut self, ctx: &egui::Context) {
        self.state.html_paste = clipboard_keys::HtmlPaste::None;
        if self.state.clipboard_keys.is_some()
            && ctx.input_mut(|i| clipboard_keys::translate(&mut i.events))
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            self.state.html_paste = clipboard_keys::HtmlPaste::Asked;
        }
        if let Some(html) = self.state.html_paste_take.as_ref().and_then(|take| take()) {
            self.state.html_paste = clipboard_keys::HtmlPaste::Held(html);
        }
    }

    /// ProseCraft's app shortcuts (its menu accelerators and `handleKeydown`): Ctrl+N a new
    /// scene, Ctrl+, Settings, Ctrl+Shift+E the scene as text, Ctrl+Shift+P Publish, and Ctrl+/
    /// (or "?" outside text) the shortcuts sheet, which Escape closes.
    fn app_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::Modifiers as M;
        let typing = ctx.memory(|m| m.focused().is_some());
        if ctx.input_mut(|i| i.consume_key(M::COMMAND, Key::Q)) {
            self.run_command(ctx, "quit");
        }
        let (sheet, question, escape, export, publish, settings, new) = ctx.input_mut(|i| {
            (
                i.consume_key(M::COMMAND, Key::Slash),
                !typing && i.consume_key(M::SHIFT, Key::Questionmark)
                    || !typing && i.consume_key(M::NONE, Key::Questionmark),
                self.state.shortcuts.is_some() && i.consume_key(M::NONE, Key::Escape),
                i.consume_key(M::COMMAND | M::SHIFT, Key::E),
                i.consume_key(M::COMMAND | M::SHIFT, Key::P),
                i.consume_key(M::COMMAND, Key::Comma),
                i.consume_key(M::COMMAND, Key::N),
            )
        });
        if sheet || question {
            self.run_command(ctx, "keyboard-shortcuts");
        }
        if escape {
            self.state.shortcuts = None;
        }
        for (asked, command) in [
            (publish, "publish-manuscript"),
            (settings, "settings"),
            (export, "export-document"),
            (new, "new-scene"),
        ] {
            if asked {
                self.run_command(ctx, command);
            }
        }
    }

    /// A menu command (ProseCraft's menu events, `prosecraft:…`), also run by its shortcut.
    fn run_command(&mut self, ctx: &egui::Context, id: &str) {
        match id {
            "keyboard-shortcuts" => {
                self.state.shortcuts = match self.state.shortcuts {
                    Some(_) => None,
                    None => Some(cfg!(target_os = "macos")),
                };
            }
            "publish-manuscript" if self.state.library.is_some() => {
                self.state.view = WorkspaceView::Publish;
            }
            "settings" => self.state.settings_open = true,
            "export-document" => self.export_scene(),
            "new-scene" => self.new_scene(),
            "new-project" if self.state.has_home => {
                self.state.starter = Some(starter::StarterDraft::new());
            }
            "import-epub" if self.state.has_home => {
                self.state.requests.push(HostRequest::ImportEpub);
            }
            "project-settings" => {
                if let Some(project) = self
                    .state
                    .library
                    .as_ref()
                    .and_then(|l| l.projects().get(self.state.active))
                {
                    self.state.project_settings = Some(
                        project_settings::ProjectSettingsDraft::for_project(&project.view),
                    );
                }
            }
            "typewriter-mode" | "fade-other-paragraphs" | "zen-mode" => {
                let mut writing = self.state.writing;
                match id {
                    "typewriter-mode" => writing.typewriter = !writing.typewriter,
                    "fade-other-paragraphs" => writing.fade = !writing.fade,
                    _ => writing.zen = !writing.zen,
                }
                self.state.writing_choice = Some(writing);
            }
            "quit" => {
                self.flush();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            // With no text field: the page's history (`handleHistoryShortcut`).
            "undo" | "redo" if self.state.last_focus.is_none() => {
                self.state
                    .intents
                    .push(editing::Intent::PageHistory { undo: id == "undo" });
            }
            "undo" | "redo" | "cut" | "copy" | "paste" | "select-all" => {
                let read = self.state.clipboard_text.clone();
                self.state.menu_events = app_menu::edit_events(id, || read.and_then(|r| r()));
                // With no text, the clipboard's HTML (a footnote marker copied alone).
                self.state.menu_html_paste = id == "paste" && self.state.clipboard_keys.is_some();
            }
            _ => {
                if let Some(project) = id.strip_prefix(app_menu::SWITCH_PROJECT)
                    && let Some(index) = self
                        .state
                        .library
                        .as_ref()
                        .and_then(|l| l.projects().iter().position(|p| p.view.id == project))
                    && index != self.state.active
                {
                    self.commit_draft();
                    self.state.active = index;
                    self.state.selection = None;
                }
            }
        }
    }

    /// `createDocument` (New Scene, Ctrl+N).
    fn new_scene(&mut self) {
        if self.state.editable {
            // `createDocument`: a scene in the first chapter, or a first chapter.
            let first = self
                .state
                .library
                .as_ref()
                .and_then(|l| l.projects().get(self.state.active))
                .and_then(|p| p.view.chapters.iter().find(|c| !c.trashed))
                .map(|c| c.id.clone());
            self.state.intents.push(match first {
                Some(chapter) => editing::Intent::AddScene { chapter },
                None => editing::Intent::AddChapter,
            });
        }
    }

    /// `exportDocument`: the open scene's text (`innerText`, trimmed) as a `.txt` named after
    /// its title.
    fn export_scene(&mut self) {
        self.commit_draft();
        let Some(Selection::Document(id)) = self.state.selection.clone() else {
            return;
        };
        let Some(document) = self
            .state
            .library
            .as_ref()
            .and_then(|l| l.projects().get(self.state.active))
            .and_then(|p| p.view.document(&id))
        else {
            return;
        };
        let text = prosecraft_core::html::inner_text(&document.content);
        let text = text.trim_matches(prosecraft_core::html::is_js_whitespace);
        let title = if document.title.is_empty() {
            "untitled"
        } else {
            &document.title
        };
        // `title.replace(/[^\w-]+/g, "-").toLowerCase()`.
        let mut name = String::new();
        for c in title.chars() {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                name.push(c);
            } else if !name.ends_with('\u{1}') {
                name.push('\u{1}');
            }
        }
        let name = name.replace('\u{1}', "-").to_lowercase();
        self.state.requests.push(HostRequest::ExportFile {
            file_name: format!("{name}.txt"),
            bytes: text.as_bytes().to_vec(),
            kind: "Plain text".to_owned(),
            extension: "txt".to_owned(),
        });
    }

    /// `ShortcutsSheet`: the shortcuts by group, as macOS or Windows and Linux show them, and
    /// Save as PDF.
    fn shortcuts_sheet(&mut self, ctx: &egui::Context) {
        use prosecraft_core::shortcuts::{GROUPS, format_key};
        let Some(mut mac) = self.state.shortcuts else {
            return;
        };
        let palette = self.state.palette;
        let mut open = true;
        let mut save = false;
        egui::Window::new("Keyboard shortcuts")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("PROSECRAFT")
                        .size(10.0)
                        .color(palette.text_muted),
                );
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut mac, true, "macOS");
                    ui.selectable_value(&mut mac, false, "Windows / Linux");
                    ui.separator();
                    save = ui.button("Save as PDF").clicked();
                });
                ui.add_space(8.0);
                ui.columns(2, |columns| {
                    for (index, group) in GROUPS.iter().enumerate() {
                        let ui = &mut columns[index % 2];
                        ui.label(
                            egui::RichText::new(group.title)
                                .strong()
                                .color(palette.text_heading),
                        );
                        for entry in group.entries {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(entry.action).size(12.0));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        for key in entry.keys.iter().rev() {
                                            egui::Frame::new()
                                                .stroke(egui::Stroke::new(
                                                    1.0,
                                                    palette.border_control,
                                                ))
                                                .fill(palette.control_bg)
                                                .corner_radius(4.0)
                                                .inner_margin(egui::Margin::symmetric(5, 2))
                                                .show(ui, |ui| {
                                                    ui.label(
                                                        egui::RichText::new(format_key(key, mac))
                                                            .size(10.0),
                                                    );
                                                });
                                        }
                                    },
                                );
                            });
                        }
                        ui.add_space(10.0);
                    }
                });
            });
        self.state.shortcuts = open.then_some(mac);
        if save {
            match prosecraft_core::shortcuts::pdf(mac) {
                Ok(bytes) => self.state.requests.push(HostRequest::ExportFile {
                    file_name: "prosecraft-shortcuts.pdf".to_owned(),
                    bytes,
                    kind: "PDF document".to_owned(),
                    extension: "pdf".to_owned(),
                }),
                Err(error) => {
                    self.state.status = Some(format!("Could not save the PDF: {error}"));
                }
            }
        }
    }

    /// Focus mode (Ctrl+Shift+Enter, Escape), Zen mode's interface coming back, the focus
    /// hint, and the Timeline view hidden when it is off.
    fn writing_modes(&mut self, ctx: &egui::Context) {
        let state = &mut self.state;
        if !state.writing.timeline && state.view == WorkspaceView::Timeline {
            state.view = WorkspaceView::Outline;
        }
        let toggle = std::mem::take(&mut state.focus_wanted)
            || ctx.input_mut(|i| {
                i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::Enter,
                )
            });
        if toggle {
            // `toggleFocusMode`.
            if state.focus_mode {
                state.focus_mode = false;
                state.zen_hidden = false;
                state.focus_hint_until = 0.0;
            } else if state.view == WorkspaceView::Write {
                state.focus_mode = true;
                state.zen_hidden = true;
                state.focus_hint_until = state.now + 3.0;
            }
        }
        if (state.focus_mode || state.zen_hidden)
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            state.focus_mode = false;
            state.zen_hidden = false;
            state.focus_hint_until = 0.0;
        }
        // `handleZenPointerMove`: the pointer moving outside the text.
        if state.zen_hidden && !state.focus_mode {
            let moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
            let outside = ctx
                .input(|i| i.pointer.hover_pos())
                .is_some_and(|p| state.editor_rect.is_none_or(|r| !r.contains(p)));
            if moved && outside {
                state.zen_hidden = false;
            }
        }
        if !state.writing.zen && !state.focus_mode {
            state.zen_hidden = false;
        }
        if state.now < state.focus_hint_until {
            let palette = state.palette;
            let text = format!(
                "Focus mode · {}+Shift+Enter or Escape to leave",
                if cfg!(target_os = "macos") {
                    "Cmd"
                } else {
                    "Ctrl"
                }
            );
            egui::Area::new(Id::unique("focus-hint"))
                .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -22.0])
                .interactable(false)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(palette.surface)
                        .stroke(egui::Stroke::new(1.0, palette.border_control))
                        .corner_radius(999.0)
                        .inner_margin(egui::Margin::symmetric(14, 7))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(text)
                                    .size(10.0)
                                    .color(palette.text_muted),
                            );
                        });
                });
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    /// `currentUndoPage`: the project and what is open in it (a scene, a chapter, or a view).
    fn undo_page_key(&self) -> String {
        let project = self.active_project_id().unwrap_or_default();
        let page = match self.state.view {
            WorkspaceView::Write => {
                let scope = match &self.state.selection {
                    Some(Selection::Chapter(id)) => format!("chapter:{id}"),
                    Some(Selection::Document(id)) => {
                        let chapter = self
                            .state
                            .library
                            .as_ref()
                            .and_then(|l| l.projects().get(self.state.active))
                            .and_then(|p| p.view.document(id))
                            .and_then(|d| d.chapter_id.clone())
                            .filter(|c| !c.is_empty());
                        chapter.map_or_else(|| id.clone(), |c| format!("chapter:{c}"))
                    }
                    None => String::new(),
                };
                format!("writing:{scope}")
            }
            WorkspaceView::Outline => "outline".to_owned(),
            WorkspaceView::Storyboard => "storyboard".to_owned(),
            WorkspaceView::Timeline => "timeline".to_owned(),
            WorkspaceView::Network => "network".to_owned(),
            WorkspaceView::Publish => "publish".to_owned(),
        };
        format!("{project}/{page}")
    }

    /// `runFieldHistory` for the editor, or `runPageHistory`.
    fn run_history(&mut self, document: Option<&str>, undo: bool) {
        use prosecraft_core::undo::PageResult;
        let project = self.active_project_id().unwrap_or_default();
        let open = self
            .state
            .draft
            .as_ref()
            .filter(|d| d.markdown.is_none())
            .map(|d| d.document.clone());
        let body_key = open.as_ref().map(|d| format!("{project}/body:{d}"));
        let result = match document {
            Some(document) => {
                let key = format!("{project}/body:{document}");
                if body_key.as_deref() != Some(key.as_str()) {
                    return;
                }
                let found = if undo {
                    self.state.undo.undo_field(&key)
                } else {
                    self.state.undo.redo_field(&key)
                };
                found.map(PageResult::Field)
            }
            None => {
                self.commit_draft();
                let page = self.state.undo_page.clone();
                let can = |key: &str| body_key.as_deref() == Some(key);
                if undo {
                    self.state.undo.undo_page(&page, can)
                } else {
                    self.state.undo.redo_page(&page, can)
                }
            }
        };
        match result {
            Some(PageResult::Field(field)) => {
                if let Some(document) = open {
                    self.apply_body_history(&document, &field.value, field.selection);
                }
            }
            Some(PageResult::State(step)) => self.apply_state_history(step, undo),
            Some(PageResult::External { .. }) | None => {}
        }
    }

    /// `applyFieldValue` for the editor: the scene and its footnotes as they were, and the
    /// selection.
    fn apply_body_history(&mut self, document: &str, value: &str, selection: (usize, usize)) {
        let (html, scopes) = prosecraft_core::undo::decode_body(value);
        let Some(doc) = prosecraft_core::richtext::RichDoc::from_html(&html) else {
            return;
        };
        let now = self.state.now;
        let Some(draft) = self.state.draft.as_mut().filter(|d| d.document == document) else {
            return;
        };
        draft.body = doc.text.clone();
        draft.doc = doc;
        draft.body_dirty = true;
        draft.changed_at = now;
        draft.undo_caret = Some(selection);
        self.state.comment_goto = Some((document.to_owned(), selection.0, selection.1));
        let notes = scopes
            .into_iter()
            .find(|(d, _)| d == document)
            .map(|(_, n)| n)
            .unwrap_or_default();
        let stored: Vec<(String, String)> = self
            .state
            .library
            .as_ref()
            .and_then(|l| l.projects().get(self.state.active))
            .and_then(|p| p.view.document(document))
            .map(|d| {
                d.footnotes
                    .iter()
                    .map(|f| (f.id.clone(), f.html.clone()))
                    .collect()
            })
            .unwrap_or_default();
        self.commit_draft();
        if stored != notes {
            self.change(|data, _| {
                prosecraft_core::footnotes::restore_definitions(data, document, &notes)
            });
        }
    }

    /// `applyPageStateStep`: the collections the step changed, as they were (or are again).
    fn apply_state_history(&mut self, step: prosecraft_core::undo::StateStep, undo: bool) {
        let page = self.state.undo_page.clone();
        let original = step.clone();
        let mut applied = step;
        self.state.applying_undo = true;
        self.change(|data, _| {
            let current = undo_state(data);
            let next = prosecraft_core::undo::apply_state_step(&current, &mut applied, undo);
            let object = data
                .as_object_mut()
                .ok_or(prosecraft_core::edit::EditError::Shape)?;
            for (name, _) in &applied.changes {
                if let Some(value) = next.get(name) {
                    object.insert(name.as_str(), value.clone());
                }
            }
            Ok(())
        });
        self.state.applying_undo = false;
        self.state.undo.replace_state(&original, applied.clone());
        // The open scene may be gone (an added scene undone): open another.
        let gone = match &self.state.selection {
            Some(Selection::Document(id)) => self
                .state
                .library
                .as_ref()
                .and_then(|l| l.projects().get(self.state.active))
                .and_then(|p| p.view.document(id))
                .is_none_or(|d| d.trashed),
            _ => false,
        };
        if gone {
            self.state.draft = None;
            let fallback = self
                .state
                .library
                .as_ref()
                .and_then(|l| l.projects().get(self.state.active))
                .and_then(|p| p.view.documents.iter().find(|d| !d.trashed))
                .map(|d| d.id.clone());
            self.state.selection = fallback.map(Selection::Document);
        }
        let now_page = self.undo_page_key();
        self.state.undo_page = now_page.clone();
        self.state.undo.move_state(&page, &now_page, &applied);
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
            // What the scene now holds, when its text was written.
            if draft.body_dirty && draft.markdown.is_none() {
                current.source = draft.doc.to_html();
            }
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

    /// A new scene from an empty paragraph of `document`; it opens with the caret at its start.
    fn split_scene(&mut self, document: &str, before: &str, after: &str) {
        self.commit_draft();
        let mut created = None;
        self.change(|data, new_id| {
            let id = new_id();
            created = Some(id.clone());
            prosecraft_core::chapter::split_scene(data, document, before, after, id)
        });
        if let Some(id) = created {
            self.open_at(id, 0);
        }
    }

    /// `document` joined to the scene before it, which opens with the caret at the join.
    fn join_scene(&mut self, document: &str, caret: usize) {
        self.commit_draft();
        let mut previous = None;
        self.change(|data, _| {
            previous = Some(prosecraft_core::chapter::join_scene_to_previous(
                data, document,
            )?);
            Ok(())
        });
        if let Some(id) = previous {
            self.open_at(id, caret);
        }
    }

    /// Opens `document` in the editor with the caret at character `at`.
    fn open_at(&mut self, document: String, at: usize) {
        self.state.draft = None;
        self.state.comment_goto = Some((document.clone(), at, at));
        self.state.select(Selection::Document(document));
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

    /// A picture from the host (`insertImageFiles`): made into an embedded image as ProseCraft
    /// makes it and put into the open scene where it was asked for, or at the caret; ProseCraft's
    /// message in the status line when it can't be.
    /// The "Add web link" dialog and a Markdown file's preview.
    fn wiki_windows(&mut self, ctx: &egui::Context) {
        let palette = self.state.palette;
        if let Some(mut draft) = self.state.wiki_link.take() {
            match wiki_files::link_dialog(ctx, &palette, &mut draft) {
                wiki_files::LinkAction::Save => {
                    // `saveResearchLink`.
                    if prosecraft_core::research::normalize_url(&draft.url).is_none() {
                        draft.error = "Enter a valid http or https address.".to_owned();
                        self.state.wiki_link = Some(draft);
                    } else {
                        let now = self.state.now_ms;
                        self.change(move |data, new_id| {
                            let link = prosecraft_core::research::new_link(
                                &new_id(),
                                draft.folder.as_deref(),
                                &draft.url,
                                &draft.title,
                                &draft.description,
                                now,
                            )
                            .map_err(|_| prosecraft_core::edit::EditError::Shape)?;
                            prosecraft_core::research::add_link(data, link)
                        });
                        self.state.wiki_status = Some("Link added.".to_owned());
                    }
                }
                wiki_files::LinkAction::Close => {}
                wiki_files::LinkAction::None => self.state.wiki_link = Some(draft),
            }
        }
        if let Some((title, html)) = &self.state.wiki_markdown
            && !wiki_files::markdown_window(ctx, &palette, title, html)
        {
            self.state.wiki_markdown = None;
        }
    }

    /// Gives the app the device's wiki file store (the desktop host's).
    /// Where the app runs, for the About window's diagnostics (the web host's browser and
    /// storage; the desktop's otherwise).
    pub fn set_platform(&mut self, platform: about_window::Platform) {
        self.state.platform = platform;
    }

    /// Where keys and nonces come from (the browser's `crypto.getRandomValues` on the web; the
    /// system's random numbers on the desktop without this).
    pub fn set_random(&mut self, random: RandomFactory) {
        self.state.random = Some(random);
    }

    /// The device's installed fonts, for the writing font pickers (the desktop host's).
    pub fn set_installed_fonts(&mut self, fonts: std::sync::Arc<dyn editor_fonts::InstalledFonts>) {
        self.state.installed_fonts = Some(fonts);
        self.state.fonts_applied = None;
    }

    /// How the host puts HTML (and its text) on the clipboard, so a copy in the editor keeps
    /// its formatting (egui copies plain text).
    pub fn set_clipboard_writer(&mut self, write: ClipboardWriter) {
        self.state.clipboard_write = Some(write);
    }

    /// How the host reads the clipboard's text, for Edit > Paste in the menu bar.
    pub fn set_clipboard_text(&mut self, read: ClipboardHtml) {
        self.state.clipboard_text = Some(read);
    }

    /// How the host reads the clipboard's HTML, for rich paste (egui's paste is its text).
    pub fn set_clipboard_html(&mut self, read: ClipboardHtml) {
        self.state.clipboard_html = Some(read);
    }

    /// The desktop host leaves the clipboard keys to the app (`NativeOptions::clipboard_shortcuts`
    /// off): Cmd/Ctrl+X and C become cut and copy, and Cmd/Ctrl+V asks the host to paste
    /// (`ViewportCommand::RequestPaste`). `read` is the clipboard's HTML when its text is
    /// empty, as after a copy of a footnote marker alone, which egui-winit sends no paste for.
    pub fn set_clipboard_keys(&mut self, read: ClipboardHtml) {
        self.state.clipboard_keys = Some(read);
    }

    /// How the web host hands over the HTML of a paste that had no text (eframe sends no paste
    /// event for one): taken once, each frame.
    pub fn set_html_paste(&mut self, take: ClipboardHtml) {
        self.state.html_paste_take = Some(take);
    }

    pub fn set_wiki_files(&mut self, store: Box<dyn WikiFileStore>) {
        let store: std::rc::Rc<dyn WikiFileStore> = std::rc::Rc::from(store);
        self.state.wiki_store = Some(store.clone());
        self.wiki_files = Some(store);
        self.state.wiki_available = true;
        let _ = self.wiki_root();
    }

    /// The wiki files folder for this device (`researchRoot`), read once from the settings.
    fn wiki_root(&mut self) -> Option<String> {
        if self.state.wiki_root.is_none() {
            let saved = self
                .home
                .as_mut()
                .and_then(|h| h.store.load_setting(wiki_files::ROOT_KEY))
                .unwrap_or_default();
            self.state.wiki_root = Some(saved);
        }
        self.state.wiki_root.clone().filter(|r| !r.is_empty())
    }

    /// The project's wiki folder name (`ensureResearchFolderName`), stored with the project the
    /// first time it is needed.
    fn wiki_folder_name(&mut self) -> Option<String> {
        let id = self.active_project_id()?;
        let stored = self
            .home
            .as_ref()
            .and_then(|h| h.library.project(&id))
            .cloned()?;
        let project = stored.get("project")?;
        let current =
            prosecraft_core::research::normalize_folder_name(project.get("researchFolderName"));
        if !current.is_empty() {
            return Some(current);
        }
        let name = project
            .get("name")
            .and_then(prosecraft_core::js::JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();
        let folder = prosecraft_core::research::create_folder_name(&name, &id);
        let stored_name = folder.clone();
        self.change(move |data, _| {
            data.as_object_mut()
                .ok_or(prosecraft_core::edit::EditError::Shape)?
                .insert(
                    "researchFolderName",
                    prosecraft_core::js::JsValue::str(&stored_name),
                );
            Ok(())
        });
        Some(folder)
    }

    /// `chooseResearchRoot` / `ensureResearchRoot`: the folder the host's picker gave.
    pub fn set_wiki_root(&mut self, path: &str, add_to: Option<Option<String>>) {
        self.state.wiki_root = Some(path.to_owned());
        let saved = self
            .home
            .as_mut()
            .map(|h| h.store.save_setting(wiki_files::ROOT_KEY, Some(path)));
        if let Some(Err(e)) = saved {
            self.state.wiki_settings_status = Some(e);
            return;
        }
        match add_to {
            Some(folder) => {
                self.state.wiki_status =
                    Some("Wiki files folder saved for this device.".to_owned());
                self.state
                    .requests
                    .push(HostRequest::PickWikiFiles { folder });
            }
            None => {
                self.state.wiki_settings_status = Some(
                    "Wiki files folder saved. Existing files remain in the previous folder."
                        .to_owned(),
                );
            }
        }
    }

    /// `addResearchFiles`: asks for the folder first if there is none.
    fn start_adding_wiki_files(&mut self, folder: Option<String>) {
        self.state.wiki_status = None;
        if self.wiki_files.is_none() {
            self.state.wiki_status = Some("Wiki files can be added in the desktop app.".to_owned());
            return;
        }
        let request = match self.wiki_root() {
            Some(_) => HostRequest::PickWikiFiles { folder },
            None => HostRequest::ChooseWikiRoot {
                add_to: Some(folder),
            },
        };
        self.state.requests.push(request);
    }

    /// `addResearchFiles` after the picker: each file copied into the project's folder and
    /// added to the Wiki, and what happened said (`researchImportStatus`).
    pub fn add_wiki_files(&mut self, folder: Option<String>, paths: &[String]) {
        if paths.is_empty() || !self.state.editable {
            return;
        }
        let (Some(root), Some(project_folder)) = (self.wiki_root(), self.wiki_folder_name()) else {
            return;
        };
        let Some(store) = self.wiki_files.as_ref() else {
            return;
        };
        let Some(home) = self.home.as_mut() else {
            return;
        };
        let mut added = Vec::new();
        let mut failures = Vec::new();
        for path in paths {
            let id = home.store.new_id();
            let now = home.store.now_ms();
            let file_name = prosecraft_core::research::file_name_from_path(path, now);
            let file = prosecraft_core::research::NewFile {
                id,
                folder_id: folder.clone(),
                file_name,
                size: 0,
                added_at: now,
            };
            match store.import(&root, &project_folder, &file.storage_key(), path) {
                Ok(size) => added.push(prosecraft_core::research::NewFile { size, ..file }),
                Err(e) => failures.push(e),
            }
        }
        let count = added.len();
        if count > 0 {
            self.change(move |data, _| prosecraft_core::research::add_files(data, &added));
        }
        let noun = if count == 1 { "file" } else { "files" };
        self.state.wiki_status = Some(match failures.first() {
            Some(first) if count == 0 => format!("Could not add files. {first}"),
            Some(first) => format!("Added {count} {noun}. {first}"),
            None => format!("Added {count} {noun}."),
        });
    }

    fn wiki_item(&self, id: &str) -> Option<prosecraft_core::research::Item> {
        let index = self.state.active;
        let project = self.state.library.as_ref()?.projects().get(index)?;
        project
            .view
            .research_items
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.item.clone())
    }

    /// `openResearchFile` and `openResearchLink`.
    fn open_wiki_item(&mut self, id: &str) {
        use prosecraft_core::research::Item;
        self.state.wiki_status = None;
        match self.wiki_item(id) {
            Some(Item::Link { url, .. }) => match prosecraft_core::research::normalize_url(&url) {
                Some(url) => self.state.requests.push(HostRequest::OpenUrl(url)),
                None => {
                    self.state.wiki_status =
                        Some("That wiki link is not a valid web address.".to_owned());
                }
            },
            Some(item @ Item::File { .. }) => self.open_wiki_file(&item),
            None => {}
        }
    }

    fn open_wiki_file(&mut self, item: &prosecraft_core::research::Item) {
        let prosecraft_core::research::Item::File {
            file_name,
            storage_key,
            ..
        } = item
        else {
            return;
        };
        if self.wiki_files.is_none() {
            self.state.wiki_status = Some("Wiki files open in the desktop app.".to_owned());
            return;
        }
        let Some(root) = self.wiki_root() else {
            self.state
                .requests
                .push(HostRequest::ChooseWikiRoot { add_to: None });
            return;
        };
        let Some(folder) = self.wiki_folder_name() else {
            return;
        };
        let Some(store) = self.wiki_files.as_ref() else {
            return;
        };
        let lower = file_name.to_lowercase();
        // `/\.(?:md|markdown)$/i`: previewed here (the host reads only `.md`, as ProseCraft's does).
        let result = if lower.ends_with(".md") || lower.ends_with(".markdown") {
            store
                .read_markdown(&root, &folder, storage_key)
                .map(|text| {
                    let html = prosecraft_core::markdown::markdown_to_html(&text);
                    self.state.wiki_markdown = Some((item.title().to_owned(), html));
                })
        } else {
            store
                .file_path(&root, &folder, storage_key)
                .and_then(|path| store.open(&path))
        };
        if let Err(e) = result {
            self.state.wiki_status = Some(e);
        }
    }

    /// `openResearchFile(file, { external: true })`: in its app, even a Markdown file.
    fn open_wiki_file_externally(&mut self, item: &prosecraft_core::research::Item) {
        let prosecraft_core::research::Item::File { storage_key, .. } = item else {
            return;
        };
        let (Some(root), Some(folder)) = (self.wiki_root(), self.wiki_folder_name()) else {
            return;
        };
        let Some(store) = self.wiki_files.as_ref() else {
            return;
        };
        if let Err(e) = store
            .file_path(&root, &folder, storage_key)
            .and_then(|path| store.open(&path))
        {
            self.state.wiki_status = Some(e);
        }
    }

    /// `revealResearchFile`.
    fn reveal_wiki_file(&mut self, id: &str) {
        let Some(prosecraft_core::research::Item::File { storage_key, .. }) = self.wiki_item(id)
        else {
            return;
        };
        self.state.wiki_status = None;
        let (Some(root), Some(folder)) = (self.wiki_root(), self.wiki_folder_name()) else {
            return;
        };
        let Some(store) = self.wiki_files.as_ref() else {
            self.state.wiki_status = Some(
                "Revealing files in a folder is only available in the desktop app.".to_owned(),
            );
            return;
        };
        if let Err(e) = store
            .file_path(&root, &folder, &storage_key)
            .and_then(|path| store.reveal(&path))
        {
            self.state.wiki_status = Some(e);
        }
    }

    /// `revealResearchProjectFolder`.
    fn reveal_wiki_project_folder(&mut self) {
        self.state.wiki_settings_status = None;
        let Some(root) = self.wiki_root() else {
            self.state
                .requests
                .push(HostRequest::ChooseWikiRoot { add_to: None });
            return;
        };
        let Some(folder) = self.wiki_folder_name() else {
            return;
        };
        let Some(store) = self.wiki_files.as_ref() else {
            return;
        };
        if let Err(e) = store
            .prepare_folder(&root, &folder)
            .and_then(|path| store.reveal(&path))
        {
            self.state.wiki_settings_status = Some(e);
        }
    }

    /// `removeResearchItem` (asked first in the binder): gone from the Wiki, and a file deleted
    /// from the project folder.
    fn remove_wiki_item(&mut self, id: &str) {
        let item = self.wiki_item(id);
        let owned = id.to_owned();
        self.change(move |data, _| prosecraft_core::research::remove_item(data, &owned));
        self.state.wiki_status = None;
        let Some(prosecraft_core::research::Item::File { storage_key, .. }) = item else {
            return;
        };
        let root = self.wiki_root();
        let folder = self
            .active_project_id()
            .and_then(|pid| self.home.as_ref()?.library.project(&pid).cloned())
            .map(|stored| {
                prosecraft_core::research::normalize_folder_name(
                    stored
                        .get("project")
                        .and_then(|p| p.get("researchFolderName")),
                )
            })
            .filter(|f| !f.is_empty());
        if let (Some(root), Some(folder), Some(store)) = (root, folder, self.wiki_files.as_ref())
            && store.delete(&root, &folder, &storage_key).is_err()
        {
            self.state.wiki_status =
                Some("The item was removed, but its file could not be deleted.".to_owned());
        }
    }

    /// `handleCoverFile`: the chosen image made into the project's cover, or why it can't be.
    pub fn set_cover_image(&mut self, name: &str, bytes: &[u8]) {
        if !self.state.editable {
            return;
        }
        match prosecraft_core::publish::cover::prepare_image(name, bytes) {
            Ok(image) => {
                self.state.cover_error = None;
                self.change(|data, new_id| {
                    let mut cover = data
                        .get("publishOptions")
                        .and_then(|o| o.get("cover"))
                        .and_then(prosecraft_core::js::JsValue::as_object)
                        .cloned()
                        .unwrap_or_default();
                    cover.insert("mode", prosecraft_core::js::JsValue::str("image"));
                    cover.insert("image", prosecraft_core::js::JsValue::str(&image));
                    editing::run(
                        &editing::Intent::SetPublishOption {
                            key: "cover".to_owned(),
                            value: prosecraft_core::js::JsValue::Object(cover),
                        },
                        data,
                        new_id,
                    )
                });
            }
            Err(message) => self.state.cover_error = Some(message),
        }
    }

    pub fn insert_picture(&mut self, name: &str, bytes: &[u8]) {
        let target = self.state.picture_target.take();
        let Some(draft) = self
            .state
            .draft
            .as_mut()
            .filter(|d| d.markdown.is_none())
            .filter(|d| target.as_ref().is_none_or(|(doc, _)| *doc == d.document))
        else {
            return;
        };
        let url = match prosecraft_core::picture::prepare(name, bytes) {
            Ok(url) => url,
            Err(message) => {
                self.state.status = Some(message);
                return;
            }
        };
        let html =
            prosecraft_core::picture::image_html(&url, &prosecraft_core::picture::alt_text(name));
        let at = target
            .map(|(_, at)| at)
            .or(draft.caret)
            .unwrap_or_else(|| draft.doc.text.chars().count());
        let (body, after) = draft.doc.insert_picture(at, &html);
        draft.body = body;
        draft.body_dirty = true;
        draft.changed_at = self.state.now;
        draft.caret = Some(after);
        let document = draft.document.clone();
        self.state.comment_goto = Some((document, after, after));
        self.commit_draft();
    }

    /// `commitCitationPicker`: the citation goes into the open scene's draft, new at a
    /// character or in place of an atom, and the caret after it.
    fn place_citation(
        &mut self,
        document: &str,
        target: citation_picker::Target,
        html: &str,
        body: &str,
    ) {
        let now = self.state.now;
        let Some(draft) = self
            .state
            .draft
            .as_mut()
            .filter(|d| d.document == document && d.body == body)
        else {
            return;
        };
        let after = match target {
            citation_picker::Target::Insert { at } => {
                draft.body = draft.doc.insert_atom(at, html);
                at.min(draft.body.chars().count().saturating_sub(1)) + 1
            }
            citation_picker::Target::Edit { atom } => {
                let Some(slot) = draft
                    .doc
                    .atoms
                    .get_mut(atom)
                    .filter(|h| citation_picker::is_cite_marker(h))
                else {
                    return;
                };
                html.clone_into(slot);
                draft
                    .body
                    .chars()
                    .enumerate()
                    .filter(|(_, c)| *c == prosecraft_core::richtext::ATOM)
                    .nth(atom)
                    .map_or(0, |(i, _)| i + 1)
            }
        };
        draft.body_dirty = true;
        draft.changed_at = now;
        self.state.comment_goto = Some((document.to_owned(), after, after));
        self.commit_draft();
    }

    /// `removeCitationFromPicker`: the open scene's `atom`th atom leaves its draft.
    fn remove_citation(&mut self, document: &str, atom: usize, body: &str) {
        let now = self.state.now;
        let Some(draft) = self
            .state
            .draft
            .as_mut()
            .filter(|d| d.document == document && d.body == body)
        else {
            return;
        };
        let seen = std::cell::Cell::new(0);
        let (body, at) = draft.doc.remove_atoms(|html| {
            let index = seen.get();
            seen.set(index + 1);
            index == atom && citation_picker::is_cite_marker(html)
        });
        if let Some(at) = at {
            draft.body = body;
            draft.body_dirty = true;
            draft.changed_at = now;
            self.state.comment_goto = Some((document.to_owned(), at, at));
            self.commit_draft();
        }
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
                editing::Intent::BodyHistory { document, undo } => {
                    self.run_history(Some(document), *undo);
                    continue;
                }
                editing::Intent::TrashProject => {
                    self.trash_project();
                    continue;
                }
                editing::Intent::RestoreProject(id) => {
                    self.set_project_trashed(id, false);
                    continue;
                }
                editing::Intent::DeleteProject(id) => {
                    if let Some(home) = self.home.as_mut()
                        && !home.library.is_read_only()
                        && home.library.remove(id)
                    {
                        self.unsaved_since = Some(self.state.now);
                        self.state.save_state = SaveState::Pending;
                        self.refresh_views();
                    }
                    continue;
                }
                editing::Intent::DeleteDocument(_) | editing::Intent::DeleteChapter(_) => {
                    self.commit_draft();
                    self.state.deleting_forever = true;
                    self.change(|data, new_id| editing::run(&intent, data, new_id));
                    self.state.deleting_forever = false;
                    continue;
                }
                editing::Intent::HideInterface => {
                    // `hideZenInterface`: once typing starts in the editor.
                    if self.state.writing.zen && self.state.view == WorkspaceView::Write {
                        self.state.zen_hidden = true;
                    }
                    continue;
                }
                editing::Intent::PageHistory { undo } => {
                    self.run_history(None, *undo);
                    continue;
                }
                editing::Intent::InsertFootnote { document, at } => {
                    self.insert_footnote(document, *at);
                    continue;
                }
                editing::Intent::SetPaneTarget { pane, target } => {
                    self.state.split =
                        prosecraft_core::split::set_target(&self.state.split, pane, target);
                    continue;
                }
                editing::Intent::OpenCharacterCard(id) => {
                    self.state.network.open_card(id);
                    self.state.view = WorkspaceView::Network;
                    continue;
                }
                editing::Intent::OpenBeside(target) => {
                    self.state.split = prosecraft_core::split::open_beside(
                        &self.state.split,
                        target,
                        &split_view::pane_id(),
                    );
                    self.state.view = WorkspaceView::Write;
                    continue;
                }
                editing::Intent::OpenWikiFileExternally(id) => {
                    if let Some(item) = self.wiki_item(id) {
                        self.open_wiki_file_externally(&item);
                    }
                    continue;
                }
                editing::Intent::AddWikiFiles(folder) => {
                    self.start_adding_wiki_files(folder.clone());
                    continue;
                }
                editing::Intent::AddWikiLink(folder) => {
                    self.state.wiki_link = Some(wiki_files::LinkDraft {
                        folder: folder.clone(),
                        ..Default::default()
                    });
                    continue;
                }
                editing::Intent::OpenWikiItem(id) => {
                    self.open_wiki_item(id);
                    continue;
                }
                editing::Intent::RevealWikiFile(id) => {
                    self.reveal_wiki_file(id);
                    continue;
                }
                editing::Intent::RemoveWikiItem(id) => {
                    self.remove_wiki_item(id);
                    continue;
                }
                editing::Intent::ChooseWikiRoot => {
                    self.state.wiki_settings_status = None;
                    self.state
                        .requests
                        .push(HostRequest::ChooseWikiRoot { add_to: None });
                    continue;
                }
                editing::Intent::RevealWikiProjectFolder => {
                    self.reveal_wiki_project_folder();
                    continue;
                }
                editing::Intent::ChooseCover => {
                    self.state.requests.push(HostRequest::ChooseCover);
                    continue;
                }
                editing::Intent::PastePicture { document, at, png } => {
                    // The name the browser gives a pasted image.
                    self.state.picture_target = Some((document.clone(), *at));
                    let png = png.clone();
                    self.insert_picture("image.png", &png);
                    continue;
                }
                editing::Intent::InsertPicture { document, at } => {
                    self.state.picture_target = Some((document.clone(), *at));
                    self.state.requests.push(HostRequest::InsertPicture);
                    continue;
                }
                editing::Intent::PlaceCitation {
                    document,
                    target,
                    html,
                    body,
                } => {
                    self.place_citation(document, *target, html, body);
                    continue;
                }
                editing::Intent::RemoveCitation {
                    document,
                    atom,
                    body,
                } => {
                    self.remove_citation(document, *atom, body);
                    continue;
                }
                editing::Intent::SplitScene {
                    document,
                    before,
                    after,
                } => {
                    self.split_scene(document, before, after);
                    continue;
                }
                editing::Intent::JoinScene { document, caret } => {
                    self.join_scene(document, *caret);
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

    /// Whether a dropped picture has a scene to go into: one open in the rich editor.
    pub fn picture_wanted(&self) -> bool {
        self.state.editable
            && self
                .state
                .draft
                .as_ref()
                .is_some_and(|d| d.markdown.is_none())
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
            // A picture dropped while a scene is open goes into it (`handleImageDrop`).
            Ok(bytes) if prosecraft_core::picture::is_supported(&name) && self.picture_wanted() => {
                self.insert_picture(&name, &bytes);
            }
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
        self.clipboard_keys(ui.ctx());
        // Drawn only while publishing, so other views use no ids.
        if self.state.book_id.is_none()
            && self.state.view == WorkspaceView::Publish
            && let Some(home) = self.home.as_mut()
        {
            self.state.book_id = Some(home.store.new_id());
        }
        // Ids the Network view and the character card draw for what they create.
        if self.state.view == WorkspaceView::Network
            && let Some(home) = self.home.as_mut()
        {
            while self.state.fresh_ids.len() < 8 {
                self.state.fresh_ids.push(home.store.new_id());
            }
        }
        self.state.undo_page = self.undo_page_key();
        // Undo and redo with no text field focused take back the page's last change
        // (`handleHistoryShortcut`); a focused field keeps them for its own text.
        if ui.ctx().memory(|m| m.focused().is_none()) {
            let (undo, redo) = ui.ctx().input_mut(|i| {
                // Ctrl+Shift+Z first: a Ctrl+Z pattern also matches it.
                let redo = i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::Z,
                ) || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y);
                let undo = i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z);
                (undo, redo)
            });
            if undo || redo {
                self.state
                    .intents
                    .push(editing::Intent::PageHistory { undo });
            }
        }
        // Files dropped on the window. (In the browser they are read asynchronously, so the web
        // app reads them and hands them in as it does a chosen file.)
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
        self.menu_bar(ui);
        self.app_shortcuts(ui.ctx());
        self.writing_modes(ui.ctx());
        // Zen and focus mode hide the panes around the text.
        // Zen and focus mode hide both; the toggles hide each (not on the Storyboard, which
        // has no toggles).
        let storyboard = self.state.view == WorkspaceView::Storyboard;
        let shown: Vec<Pane> = self
            .open
            .iter()
            .copied()
            .filter(|_| !self.state.zen_hidden)
            .filter(|p| storyboard || !(*p == Pane::Binder && self.state.binder_hidden))
            .filter(|p| storyboard || !(*p == Pane::Sidebar && self.state.sidebar_hidden))
            .collect();
        let open = &shown;
        let output = PaneHost::new(Id::unique("prosecraft-panes"))
            .frame(frame)
            .body_margin(egui::Margin::symmetric(18, 16))
            .show(ui, open, |pane| states.get(pane).copied(), &mut self.state);
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
        self.wiki_windows(ui.ctx());
        starter::show(ui, &mut self.state);
        project_settings::show(ui, &mut self.state);
        labels::show(ui, &mut self.state);
        // The footnote editor before the picker it opens, so the picker is drawn over it.
        footnote_editor::show(ui, &mut self.state);
        citation_picker::show(ui, &mut self.state);
        sources::show(ui, &mut self.state);
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
        self.shortcuts_sheet(ui.ctx());
        if let Some((screenplay, blueprint)) = self.state.create_project.take() {
            self.create_project(screenplay, &blueprint);
        }
        self.passphrase_dialog(ui);
        self.encrypt_export_dialog(ui);
        if std::mem::take(&mut self.state.about_wanted) {
            let now = self.home.as_ref().map_or(0.0, |h| h.store.now_ms());
            about_window::open(&mut self.state, now);
        }
        about_window::show(ui.ctx(), &mut self.state);
        self.handle_sync_actions();
        self.save_custom_starters();
        if let Some(prefs) = self.state.proofread_choice.take() {
            // `setHarperEnabled`, `setHarperDialect`.
            if prefs.dialect != self.state.proofread.dialect || !prefs.enabled {
                self.state.proofing.clear();
            }
            self.state.proofread = prefs.clone();
            if let Some(home) = self.home.as_mut() {
                let saved = home
                    .store
                    .save_setting(
                        proofreading::ENABLED_KEY,
                        Some(if prefs.enabled { "true" } else { "false" }),
                    )
                    .and_then(|()| {
                        home.store
                            .save_setting(proofreading::DIALECT_KEY, Some(&prefs.dialect))
                    });
                if let Err(error) = saved {
                    self.state.status = Some(format!("Could not save the setting: {error}"));
                }
            }
        }
        let draft = self
            .state
            .draft
            .as_ref()
            .filter(|_| self.state.view == WorkspaceView::Write);
        if self
            .state
            .proofing
            .tick(&self.state.proofread, draft, self.state.now)
        {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
        if let Some(writing) = self.state.writing_choice.take() {
            self.state.writing = writing;
            if let Some(home) = self.home.as_mut()
                && let Err(error) = writing.save(home.store.as_mut())
            {
                self.state.status = Some(format!("Could not save the setting: {error}"));
            }
        }
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
        self.save_appearance();
        self.apply_fonts(ui.ctx());
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

    /// The writing fonts: a new choice saved, and the app's fonts built again for the faces it
    /// comes to once the installed fonts are listed.
    fn apply_fonts(&mut self, ctx: &egui::Context) {
        if let Some(prefs) = self.state.font_choice.take() {
            self.state.font_prefs = prefs;
            if let Some(home) = self.home.as_mut() {
                for (key, value) in self.state.font_prefs.entries() {
                    if let Err(error) = home.store.save_setting(key, Some(value)) {
                        self.state.status = Some(format!("Could not save the font: {error}"));
                    }
                }
            }
        }
        if self.state.fonts_applied.as_ref() == Some(&self.state.font_prefs) {
            return;
        }
        let installed = self.state.installed_fonts.clone();
        if installed.as_ref().is_some_and(|i| i.families().is_none()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            return;
        }
        let faces = self.state.font_prefs.faces(installed.as_deref());
        if faces != fonts::WritingFaces::default() || self.state.fonts_applied.is_some() {
            fonts::reinstall(ctx, &faces);
        }
        self.state.fonts_applied = Some(self.state.font_prefs.clone());
    }

    /// A new Theme or Color scheme choice, applied and saved on this device.
    fn save_appearance(&mut self) {
        if let Some(scheme) = self.state.scheme_choice.take() {
            self.state.color_scheme = scheme;
            if let Some(home) = self.home.as_mut()
                && let Err(error) = home
                    .store
                    .save_setting(theme::SCHEME_KEY, Some(&self.state.color_scheme))
            {
                self.state.status = Some(format!("Could not save the color scheme: {error}"));
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
        let palette = theme::Palette::for_scheme(&self.state.color_scheme, !light);
        // Compared with the style egui holds, so a style set elsewhere is put right too.
        if palette != self.state.palette || ctx.global_style().visuals != palette.visuals() {
            palette.apply(ctx);
            self.state.palette = palette;
        }
    }

    /// `openEncryptedExport` and `submitPassphrase` for an export: at least 8 characters,
    /// confirmed, an optional hint kept in clear text; the library sealed (Argon2id,
    /// XChaCha20-Poly1305) off the interface thread, then saved where the person chooses.
    fn encrypt_export_dialog(&mut self, ui: &mut Ui) {
        let Some(dialog) = self.state.encrypt_export.as_mut() else {
            return;
        };
        if let Some(done) = dialog.working.as_ref().and_then(|r| r.try_recv().ok()) {
            dialog.working = None;
            match done {
                Ok(text) => {
                    self.state.requests.push(HostRequest::ExportBackup {
                        file_name: "prosecraft-encrypted.prosecraft".into(),
                        text,
                    });
                    self.state.status = Some("Encrypted backup exported.".into());
                    self.state.encrypt_export = None;
                    return;
                }
                Err(error) => dialog.error = Some(error),
            }
        }
        let busy = dialog.working.is_some();
        if busy {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        let palette = self.state.palette;
        let mut submit = false;
        let mut close = false;
        egui::Window::new("Encrypt backup")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.set_max_width(380.0);
                ui.label(egui::RichText::new("Passphrase").strong().color(palette.text_heading));
                ui.label(
                    egui::RichText::new(
                        "You will need this passphrase to open the backup on another device. It cannot be recovered.",
                    )
                    .size(12.0)
                    .color(palette.text_muted),
                );
                ui.add_enabled_ui(!busy, |ui| {
                    for (text, name, password, hint) in [
                        (&mut dialog.passphrase, "Passphrase", true, "Passphrase"),
                        (&mut dialog.confirm, "Confirm passphrase", true, "Confirm passphrase"),
                        (&mut dialog.hint, "Passphrase hint", false, "Hint (optional, stored unencrypted)"),
                    ] {
                        let field = ui.add(
                            egui::TextEdit::singleline(text)
                                .password(password)
                                .hint_text(hint)
                                .desired_width(f32::INFINITY),
                        );
                        views::name_field(ui, &field, name);
                        if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            submit = true;
                        }
                    }
                });
                if let Some(error) = &dialog.error {
                    let label = ui.colored_label(palette.danger, error);
                    let text = error.clone();
                    ui.ctx().accesskit_node_builder(label.id, move |node| {
                        node.set_role(egui::accesskit::Role::Alert);
                        node.set_label(text);
                    });
                }
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Argon2id · XChaCha20-Poly1305").size(11.0).color(palette.text_muted));
                    let label = if busy { "Working…" } else { "Encrypt & export" };
                    submit |= ui.add_enabled(!busy, egui::Button::new(label)).clicked();
                    let button = ui.add_enabled(!busy, egui::Button::new("Cancel"));
                    views::name_field(ui, &button, "Close passphrase dialog");
                    close |= button.clicked();
                });
            });
        if !busy && ui.input(|i| i.key_pressed(Key::Escape)) {
            close = true;
        }
        if close {
            self.state.encrypt_export = None;
            return;
        }
        if !submit || busy {
            return;
        }
        if dialog.passphrase.chars().count() < 8 {
            dialog.error = Some("Use at least 8 characters.".into());
            return;
        }
        if dialog.passphrase != dialog.confirm {
            dialog.error = Some("The passphrases do not match.".into());
            return;
        }
        dialog.error = None;
        let passphrase = dialog.passphrase.clone();
        let hint = Some(dialog.hint.trim().to_owned()).filter(|h| !h.is_empty());
        let (send, receive) = std::sync::mpsc::channel();
        dialog.working = Some(receive);
        // `persistNow`: the library as it is now.
        self.flush();
        let Some(home) = self.home.as_ref() else {
            self.state.encrypt_export = None;
            return;
        };
        let library = home.library.clone();
        let now = home.store.now_ms();
        let ctx = ui.ctx().clone();
        let random = self.state.random;
        let work = move || {
            #[cfg(not(target_arch = "wasm32"))]
            let mut source: Box<
                dyn prosecraft_core::storage::crypto::RandomSource + Send,
            > = random.map_or_else(
                || Box::new(prosecraft_core::storage::crypto::OsRandom) as _,
                |make| make(),
            );
            #[cfg(target_arch = "wasm32")]
            let Some(mut source) = random.map(|make| make()) else {
                let _ = send.send(Err("This device can't make random numbers.".to_owned()));
                return;
            };
            let text = library
                .to_encrypted_text(
                    &passphrase,
                    hint.as_deref(),
                    now,
                    (
                        &prosecraft_core::storage::crypto::VaultOptions::default(),
                        source.as_mut(),
                    ),
                )
                .map_err(|e| e.to_string());
            let _ = send.send(text);
            ctx.request_repaint();
        };
        // Sealing takes a moment (Argon2id); the web has no threads to put it on.
        #[cfg(not(target_arch = "wasm32"))]
        let _ = std::thread::Builder::new()
            .name("encrypted-backup".into())
            .spawn(work);
        #[cfg(target_arch = "wasm32")]
        work();
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
