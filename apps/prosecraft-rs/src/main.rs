//! LoveCraft on the desktop.
// A window app on Windows: release builds open no console window beside it (the MCP server,
// lovecraft-mcp, stays a console program for its stdio). Debug builds keep the console for logs.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use craft_dialogs::{Answer, Dialogs, FileDialog};
use prosecraft_core::about;
use prosecraft_core::import::markdown_folder::{
    MAX_MARKDOWN_FILE_BYTES, MAX_MARKDOWN_FILES, MarkdownFile, is_importable_markdown_path,
};
use prosecraft_core::library::{Library, new_device_id};
use prosecraft_native::{LibraryFile, OsRandom};
use prosecraft_ui_egui::{HostRequest, LibraryStore, ProseApp, window_title};

/// The window's app id (Wayland app id, X11 class). It keeps the app's former name, ProseCraft-rs,
/// as the desktop entry and the docks know the app by it.
const APP_ID: &str = prosecraft_core::about::ids::APP_ID;

/// The window's icon (the title bar, taskbar and X11 docks; Wayland docks find the icon by the
/// app id), rendered from the app icon's master by packaging/icons/render.sh.
const WINDOW_ICON_PNG: &[u8] = include_bytes!("../assets/icon-256.png");

/// The window's icon, or none (the system's default) should the embedded PNG not decode.
fn window_icon() -> Option<eframe::egui::IconData> {
    match eframe::icon_data::from_png_bytes(WINDOW_ICON_PNG) {
        Ok(icon) => Some(icon),
        Err(e) => {
            eprintln!("the window icon couldn't be read: {e}");
            None
        }
    }
}

/// What to do with a dialog's answer.
enum Then {
    Open,
    /// Import this EPUB as a new project.
    ImportEpub,
    /// Read this zipped Scrivener project, or this `.scriv` folder.
    ScrivenerZip,
    ScrivenerFolder,
    /// Read this folder's Markdown files for the Markdown import.
    MarkdownFolder,
    /// Add this screenplay to the open project.
    Fountain,
    /// Import this citation file into the Sources dialog.
    Citations,
    /// Put this picture into the open scene.
    Picture,
    /// Make this image the book's cover.
    Cover,
    /// The wiki files folder; then add files (to this Wiki folder) when some were being added.
    WikiRoot(Option<Option<String>>),
    /// Add these files to the Wiki (in this folder).
    WikiFiles(Option<String>),
    /// Write these bytes where the person chose (a backup or an exported manuscript).
    Export(Vec<u8>),
    /// The folder to sync through.
    SyncFolder,
}

/// The library file, saved atomically.
struct DiskStore {
    file: LibraryFile,
}

impl LibraryStore for DiskStore {
    fn save(&mut self, text: &str) -> Result<(), String> {
        self.file.save(text).map_err(|e| e.to_string())
    }
    fn now_ms(&self) -> f64 {
        chrono::Utc::now().timestamp_millis() as f64
    }
    fn new_id(&mut self) -> String {
        // A random UUID; should the OS have no random numbers, the clock keeps ids distinct.
        new_device_id(&mut OsRandom).unwrap_or_else(|_| {
            format!(
                "id-{}",
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
            )
        })
    }
    fn today(&self) -> String {
        chrono::Local::now().format("%Y-%m-%d").to_string()
    }
    fn supports_sync(&self) -> bool {
        true
    }
    fn sync_fs(
        &mut self,
        folder: &str,
    ) -> Option<Box<dyn prosecraft_core::sync::SyncFileSystem + Send>> {
        Some(Box::new(prosecraft_native::sync_fs::FolderFs::new(folder)))
    }
    fn load_setting(&mut self, key: &str) -> Option<String> {
        prosecraft_native::load_setting(self.file.path().parent()?, key)
    }
    fn save_setting(&mut self, key: &str, value: Option<&str>) -> Result<(), String> {
        let dir = self.file.path().parent().ok_or("there is no data folder")?;
        prosecraft_native::save_setting(dir, key, value).map_err(|e| e.to_string())
    }
}

/// The app with its library, or why it runs without one. A library that exists but can't be
/// read is never written over: the app then only shows backups, and says why.
fn start() -> (ProseApp, Option<String>) {
    let Some(file) = LibraryFile::default_location() else {
        return (
            ProseApp::new(),
            Some("There is no data folder for the library on this system.".into()),
        );
    };
    let library = match file.load() {
        Ok(Some(text)) => match Library::from_text(&text) {
            Ok(library) => library,
            Err(e) => {
                let why = format!(
                    "The library at {} couldn't be read ({e}); it is left untouched.",
                    file.path().display()
                );
                return (ProseApp::new(), Some(why));
            }
        },
        Ok(None) => match new_device_id(&mut OsRandom) {
            Ok(id) => Library::new(id),
            Err(e) => {
                return (
                    ProseApp::new(),
                    Some(format!("Couldn't start a library: {e}")),
                );
            }
        },
        Err(e) => {
            let why = format!(
                "The library at {} couldn't be read ({e}); it is left untouched.",
                file.path().display()
            );
            return (ProseApp::new(), Some(why));
        }
    };
    (
        ProseApp::with_library(library, Box::new(DiskStore { file })),
        None,
    )
}

/// Wiki files in a folder on this computer (`prosecraft_native::wiki_files`).
struct WikiFiles;

impl prosecraft_ui_egui::WikiFileStore for WikiFiles {
    fn import(&self, root: &str, folder: &str, key: &str, source: &str) -> Result<u64, String> {
        prosecraft_native::wiki_files::import(root, folder, key, Path::new(source))
    }
    fn read_markdown(&self, root: &str, folder: &str, key: &str) -> Result<String, String> {
        prosecraft_native::wiki_files::read_markdown(root, folder, key)
    }
    fn read(&self, root: &str, folder: &str, key: &str, max: u64) -> Result<Vec<u8>, String> {
        let path = prosecraft_native::wiki_files::file_path(root, folder, key)?;
        read_capped(&path, max).map_err(|e| e.to_string())
    }
    fn file_path(&self, root: &str, folder: &str, key: &str) -> Result<String, String> {
        prosecraft_native::wiki_files::file_path(root, folder, key)
            .map(|p| p.to_string_lossy().into_owned())
    }
    fn delete(&self, root: &str, folder: &str, key: &str) -> Result<(), String> {
        prosecraft_native::wiki_files::delete(root, folder, key)
    }
    fn prepare_folder(&self, root: &str, folder: &str) -> Result<String, String> {
        prosecraft_native::wiki_files::prepare_project_folder(root, folder)
            .map(|p| p.to_string_lossy().into_owned())
    }
    fn open(&self, target: &str) -> Result<(), String> {
        prosecraft_native::wiki_files::open(target.as_ref())
    }
    fn reveal(&self, path: &str) -> Result<(), String> {
        prosecraft_native::wiki_files::reveal(Path::new(path))
    }
}

struct DesktopApp {
    app: ProseApp,
    dialogs: Dialogs<Then>,
    title: String,
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Reads a backup for opening. The file is only read, never written; why it can't be read
/// shows in the app's status line.
fn read_backup(app: &mut ProseApp, path: &Path) {
    match prosecraft_native::read_backup_file(path, ProseApp::MAX_BACKUP_BYTES) {
        Ok(bytes) => app.open_backup_bytes(&file_name(path), &bytes),
        Err(why) => app.report(&why),
    }
}

impl eframe::App for DesktopApp {
    /// The native menu's key equivalents, before egui reads the input.
    fn raw_input_hook(&mut self, _ctx: &eframe::egui::Context, raw: &mut eframe::egui::RawInput) {
        self.app.raw_input_hook(raw);
    }

    /// The last keystrokes are saved before the window closes.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.app.flush();
    }

    fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) {
        for (then, answer) in self.dialogs.take_answers() {
            match (then, answer) {
                (Then::Open, Answer::Paths(paths)) => {
                    for path in paths {
                        read_backup(&mut self.app, &path);
                    }
                }
                (Then::ImportEpub, Answer::Paths(paths)) => {
                    for path in paths {
                        let name = path.file_name().map_or_else(
                            || path.display().to_string(),
                            |n| n.to_string_lossy().into_owned(),
                        );
                        // No more than the importer accepts (100 MB) and one byte to tell it the file is larger.
                        let mut bytes = Vec::new();
                        let read = std::fs::File::open(&path).and_then(|f| {
                            std::io::Read::read_to_end(
                                &mut std::io::Read::take(f, 100 * 1024 * 1024 + 1),
                                &mut bytes,
                            )
                        });
                        match read {
                            Ok(_) => self.app.import_epub(&name, &bytes),
                            Err(e) => eprintln!("{}: {e}", path.display()),
                        }
                    }
                }
                (Then::Cover, Answer::Paths(paths)) => {
                    if let Some(path) = paths.first() {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let limit = prosecraft_core::publish::cover::MAX_FILE_BYTES as u64;
                        match read_capped(path, limit) {
                            Ok(bytes) => self.app.set_cover_image(&name, &bytes),
                            Err(e) => eprintln!("{}: {e}", path.display()),
                        }
                    }
                }
                (Then::Picture, Answer::Paths(paths)) => {
                    if let Some(path) = paths.first() {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        // One byte more than a picture may have, so a larger file is refused.
                        let limit = prosecraft_core::picture::MAX_FILE_BYTES as u64;
                        match read_capped(path, limit) {
                            Ok(bytes) => self.app.insert_picture(&name, &bytes),
                            Err(e) => eprintln!("{}: {e}", path.display()),
                        }
                    }
                }
                (Then::Fountain, Answer::Paths(paths)) => {
                    if let Some(path) = paths.first() {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        match read_capped(path, 64 * 1024 * 1024) {
                            Ok(bytes) => {
                                // `file.text()`: UTF-8 (a byte-order mark dropped), anything else as U+FFFD.
                                let text = String::from_utf8_lossy(&bytes);
                                self.app.import_fountain(
                                    &name,
                                    text.strip_prefix('\u{feff}').unwrap_or(&text),
                                );
                            }
                            Err(e) => eprintln!("{}: {e}", path.display()),
                        }
                    }
                }
                (Then::ScrivenerZip, Answer::Paths(paths)) => {
                    if let Some(path) = paths.first() {
                        match read_capped(path, SCRIVENER_LIMIT) {
                            Ok(bytes) => self.app.import_scrivener_zip(&bytes),
                            Err(e) => eprintln!("{}: {e}", path.display()),
                        }
                    }
                }
                (Then::ScrivenerFolder, Answer::Paths(paths)) => {
                    if let Some(path) = paths.first() {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        self.app
                            .import_scrivener_folder(&name, read_scrivener_folder(path));
                    }
                }
                (Then::MarkdownFolder, Answer::Paths(paths)) => {
                    if let Some(path) = paths.first() {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        match read_markdown_folder(path) {
                            Ok((files, ignored)) => {
                                self.app.import_markdown_folder(&name, files, ignored);
                            }
                            Err(message) => self.app.import_markdown_folder_failed(&message),
                        }
                    }
                }
                (Then::Citations, Answer::Paths(paths)) => {
                    if let Some(path) = paths.first() {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        match read_capped(path, CITATIONS_LIMIT) {
                            Ok(bytes) => self
                                .app
                                .import_citation_file(&name, &String::from_utf8_lossy(&bytes)),
                            Err(e) => eprintln!("{}: {e}", path.display()),
                        }
                    }
                }
                (Then::Export(text), Answer::Paths(paths)) => {
                    if let Some(path) = paths.first()
                        && let Err(e) = craft_atomic::atomic_write(path, &text)
                    {
                        eprintln!("{}: {e}", path.display());
                    }
                }
                (Then::WikiRoot(add_to), Answer::Paths(paths)) => {
                    if let Some(path) = paths.first() {
                        self.app.set_wiki_root(&path.to_string_lossy(), add_to);
                    }
                }
                (Then::WikiFiles(folder), Answer::Paths(paths)) => {
                    let paths: Vec<String> = paths
                        .iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect();
                    self.app.add_wiki_files(folder, &paths);
                }
                (Then::SyncFolder, Answer::Paths(paths)) => {
                    if let Some(path) = paths.first() {
                        self.app
                            .set_sync_folder(path.to_string_lossy().into_owned());
                    }
                }
                (_, Answer::Unavailable(why)) => eprintln!("file dialog unavailable: {why}"),
                _ => {}
            }
        }
        self.app.show(ui);
        for request in self.app.take_requests() {
            match request {
                HostRequest::ChooseWikiRoot { add_to } => {
                    let dialog = FileDialog::folder().title("Choose a folder for wiki files");
                    self.dialogs.ask(dialog, Then::WikiRoot(add_to));
                }
                HostRequest::PickWikiFiles { folder } => {
                    let dialog = FileDialog::open_many().title("Add wiki files");
                    self.dialogs.ask(dialog, Then::WikiFiles(folder));
                }
                HostRequest::OpenUrl(url) => {
                    if let Err(e) = prosecraft_native::wiki_files::open(url.as_ref()) {
                        eprintln!("{url}: {e}");
                    }
                }
                HostRequest::ChooseSyncFolder => {
                    let dialog = FileDialog::folder().title("Choose a sync folder");
                    self.dialogs.ask(dialog, Then::SyncFolder);
                }
                HostRequest::OpenBackup => {
                    let dialog = FileDialog::open()
                        .title("Open a ProseCraft backup")
                        .filter("ProseCraft backup", &["prosecraft", "json"]);
                    self.dialogs.ask(dialog, Then::Open);
                }
                // A backup CraftCloud synced here, opened as Open backup opens one.
                HostRequest::OpenBackupPath(path) => read_backup(&mut self.app, Path::new(&path)),
                HostRequest::ImportCitations => {
                    let dialog = FileDialog::open().title("Import citation file").filter(
                        "Citation file",
                        &["bib", "bibtex", "ris", "json", "csljson"],
                    );
                    self.dialogs.ask(dialog, Then::Citations);
                }
                HostRequest::ChooseCover => {
                    let dialog = FileDialog::open()
                        .title("Choose a cover image")
                        .filter("Images", &prosecraft_core::publish::cover::EXTENSIONS);
                    self.dialogs.ask(dialog, Then::Cover);
                }
                HostRequest::InsertPicture => {
                    let dialog = FileDialog::open()
                        .title("Insert image")
                        .filter("Images", &prosecraft_core::picture::EXTENSIONS);
                    self.dialogs.ask(dialog, Then::Picture);
                }
                HostRequest::ImportFountain => {
                    let dialog = FileDialog::open()
                        .title("Import a Fountain screenplay")
                        .filter("Fountain screenplay", &["fountain", "txt"]);
                    self.dialogs.ask(dialog, Then::Fountain);
                }
                HostRequest::ImportScrivenerZip => {
                    let dialog = FileDialog::open()
                        .title("Import a Scrivener project")
                        .filter("Zipped Scrivener project", &["zip"]);
                    self.dialogs.ask(dialog, Then::ScrivenerZip);
                }
                HostRequest::ImportScrivenerFolder => {
                    let dialog =
                        FileDialog::folder().title("Import a Scrivener project folder (.scriv)");
                    self.dialogs.ask(dialog, Then::ScrivenerFolder);
                }
                HostRequest::ImportMarkdownFolder => {
                    let dialog = FileDialog::folder().title("Import a Markdown folder");
                    self.dialogs.ask(dialog, Then::MarkdownFolder);
                }
                HostRequest::ImportEpub => {
                    let dialog = FileDialog::open()
                        .title("Import EPUB")
                        .filter("EPUB", &["epub"]);
                    self.dialogs.ask(dialog, Then::ImportEpub);
                }
                HostRequest::ExportBackup { file_name, text } => {
                    let dialog = FileDialog::save()
                        .title("Export a backup")
                        .filter("ProseCraft backup", &["prosecraft"])
                        .file_name(file_name);
                    self.dialogs.ask(dialog, Then::Export(text.into_bytes()));
                }
                HostRequest::ExportFile {
                    file_name,
                    bytes,
                    kind,
                    extension,
                } => {
                    let dialog = FileDialog::save()
                        .title("Export the manuscript")
                        .filter(&kind, &[extension.as_str()])
                        .file_name(file_name);
                    self.dialogs.ask(dialog, Then::Export(bytes));
                }
            }
        }
        self.dialogs.show_pending(Some(&*frame));
        let title = window_title(&self.app);
        if title != self.title {
            ui.ctx()
                .send_viewport_cmd(eframe::egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

/// The most a Scrivener project may unpack to (the importer refuses anything larger).
const SCRIVENER_LIMIT: u64 = 200 * 1024 * 1024;
/// The largest citation file read for import (a large BibTeX library is a few megabytes).
const CITATIONS_LIMIT: u64 = 32 * 1024 * 1024;
/// ProseCraft skips any one file of a project folder larger than this.
const MAX_SCRIVENER_ENTRY_BYTES: u64 = 50_000_000;

/// The file's bytes, or no more than `limit` and one (the importer then says it is too large).
fn read_capped(path: &std::path::Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut std::io::Read::take(std::fs::File::open(path)?, limit + 1),
        &mut bytes,
    )?;
    Ok(bytes)
}

/// A `.scriv` folder's files by their paths inside it, as a folder picker gives them: files
/// larger than ProseCraft's 50 MB skipped, and no more than the importer accepts (it then says
/// the project is too large).
fn read_scrivener_folder(root: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut entries = Vec::new();
    let mut total: u64 = 0;
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut children: Vec<_> = read.filter_map(Result::ok).map(|e| e.path()).collect();
        children.sort();
        for path in children.into_iter().rev() {
            if path.is_symlink() {
                continue;
            }
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if size > MAX_SCRIVENER_ENTRY_BYTES || total > SCRIVENER_LIMIT || entries.len() > 20_000
            {
                continue;
            }
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            total += bytes.len() as u64;
            let name = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            entries.push((name, bytes));
        }
    }
    entries
}

/// A folder's Markdown files by their paths inside it, as ProseCraft reads a picked folder: any
/// other file, or a Markdown file over 2 MB, is counted as ignored; more Markdown files than
/// ProseCraft takes is an error. Text is decoded as the browser's `File.text()` does.
fn read_markdown_folder(root: &std::path::Path) -> Result<(Vec<MarkdownFile>, usize), String> {
    let mut files = Vec::new();
    let mut ignored = 0;
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut children: Vec<_> = read.filter_map(Result::ok).map(|e| e.path()).collect();
        children.sort();
        for path in children.into_iter().rev() {
            if path.is_symlink() {
                continue;
            }
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let relative = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if !is_importable_markdown_path(&relative) || size > MAX_MARKDOWN_FILE_BYTES as u64 {
                ignored += 1;
                continue;
            }
            if files.len() >= MAX_MARKDOWN_FILES {
                return Err(format!(
                    "That folder has more than {MAX_MARKDOWN_FILES} Markdown files."
                ));
            }
            let Ok(bytes) = std::fs::read(&path) else {
                ignored += 1;
                continue;
            };
            let text = String::from_utf8_lossy(&bytes);
            files.push(MarkdownFile {
                path: relative,
                text: text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned(),
            });
        }
    }
    Ok((files, ignored))
}

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "--version") {
        println!("{}", about::name_and_version());
        return ExitCode::SUCCESS;
    }
    let (mut app, problem) = start();
    app.set_wiki_files(Box::new(WikiFiles));
    // The installed fonts, listed in the background as the window opens.
    app.set_installed_fonts(Arc::new(InstalledFonts::start()));
    // Rich paste: the clipboard's HTML, as a browser hands a page's selection over.
    app.set_clipboard_html(Arc::new(|| {
        arboard::Clipboard::new().ok()?.get().html().ok()
    }));
    // A copy in the editor: its HTML, and its text for other apps.
    // One clipboard kept for the app's life: on Linux what is put on it is served by it.
    static CLIPBOARD: std::sync::OnceLock<std::sync::Mutex<Option<arboard::Clipboard>>> =
        std::sync::OnceLock::new();
    app.set_clipboard_writer(Arc::new(|html: &str, text: &str| {
        let lock = CLIPBOARD.get_or_init(|| std::sync::Mutex::new(arboard::Clipboard::new().ok()));
        if let Ok(mut held) = lock.lock()
            && let Some(clipboard) = held.as_mut()
        {
            // No text for a footnote marker alone, as a browser writes none.
            let _ = clipboard
                .set()
                .html(html, Some(text).filter(|t| !t.is_empty()));
        }
    }));
    // Edit > Paste in the menu bar: the clipboard's text.
    app.set_clipboard_text(Arc::new(|| arboard::Clipboard::new().ok()?.get_text().ok()));
    // The clipboard keys are the app's (`clipboard_shortcuts` below): a paste whose clipboard
    // has HTML but no text, which egui-winit sends nothing for, reads the HTML here.
    app.set_clipboard_keys(Arc::new(|| {
        let mut clipboard = arboard::Clipboard::new().ok()?;
        // HTML first: egui's own copies (from a title field) offer none, so its text, which
        // egui-winit serves, isn't asked for on every paste.
        let html = clipboard
            .get()
            .html()
            .ok()
            .filter(|html| !html.is_empty())?;
        let text = clipboard.get_text().unwrap_or_default();
        text.is_empty().then_some(html)
    }));
    // Zotero on this computer, through its local HTTP server.
    app.set_zotero(Arc::new(prosecraft_core::citations::zotero::request));
    // CraftCloud, when its program is installed ("without it, show nothing"). What an earlier
    // save to it left behind (a crash mid-save) goes first.
    if let Some(program) = prosecraft_native::craftcloud::find_on_path() {
        prosecraft_native::craftcloud::clean_stale_stages(
            &std::env::temp_dir(),
            std::time::Duration::from_secs(2 * 60 * 60),
        );
        app.set_craftcloud(Arc::new(prosecraft_native::craftcloud::ProcessRunner::new(
            program,
        )));
    }
    if let Some(problem) = problem {
        eprintln!("{problem}");
    }
    for path in &args {
        read_backup(&mut app, &PathBuf::from(path));
    }
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title(about::APP_NAME)
        .with_app_id(APP_ID)
        .with_inner_size([1280.0, 850.0])
        .with_drag_and_drop(true);
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        // The app turns Cmd/Ctrl+X, C and V into cut, copy and paste itself
        // (`ProseApp::set_clipboard_keys`), so that it sees a paste with no text.
        clipboard_shortcuts: false,
        ..Default::default()
    };
    // eframe keeps the window's state in a folder named after this; it stays the name the
    // first builds used, so a renamed app keeps it.
    let result = eframe::run_native(
        prosecraft_core::about::ids::WINDOW_STATE_KEY,
        options,
        Box::new(|cc| {
            prosecraft_ui_egui::setup(&cc.egui_ctx);
            let ctx = cc.egui_ctx.clone();
            let dialogs = Dialogs::new(Arc::new(move || ctx.request_repaint()));
            // ProseCraft's native menu bar on macOS (its Tauri `run` menu).
            #[cfg(target_os = "macos")]
            let app = {
                let mut app = app;
                app.install_native_menu(&cc.egui_ctx);
                app
            };
            Ok(Box::new(DesktopApp {
                app,
                dialogs,
                title: about::APP_NAME.to_owned(),
            }))
        }),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}: {error}", about::APP_NAME);
            ExitCode::FAILURE
        }
    }
}

/// The installed fonts (ProseCraft's `list_system_font_families`, fontdb as it uses it), read on
/// a background thread.
struct InstalledFonts {
    database: Arc<std::sync::OnceLock<fontdb::Database>>,
}

impl InstalledFonts {
    fn start() -> Self {
        let database = Arc::new(std::sync::OnceLock::new());
        let filling = Arc::clone(&database);
        let _ = std::thread::Builder::new()
            .name("installed-fonts".into())
            .spawn(move || {
                let mut db = fontdb::Database::new();
                db.load_system_fonts();
                let _ = filling.set(db);
            });
        Self { database }
    }
}

impl prosecraft_ui_egui::editor_fonts::InstalledFonts for InstalledFonts {
    fn families(&self) -> Option<Vec<prosecraft_ui_egui::editor_fonts::InstalledFamily>> {
        let db = self.database.get()?;
        let mut families: std::collections::HashMap<String, (String, bool)> =
            std::collections::HashMap::new();
        for face in db.faces() {
            for (family, _) in &face.families {
                let family = family.trim();
                if family.is_empty() {
                    continue;
                }
                families
                    .entry(family.to_lowercase())
                    .and_modify(|(_, monospaced)| *monospaced |= face.monospaced)
                    .or_insert_with(|| (family.to_owned(), face.monospaced));
            }
        }
        let mut list: Vec<_> = families
            .into_values()
            .map(
                |(family, monospaced)| prosecraft_ui_egui::editor_fonts::InstalledFamily {
                    family,
                    monospaced,
                },
            )
            .collect();
        list.sort_by_key(|f| f.family.to_lowercase());
        Some(list)
    }

    fn faces(&self, family: &str) -> Option<prosecraft_ui_egui::editor_fonts::FamilyFaces> {
        let db = self.database.get()?;
        let load = |weight: u16, style: fontdb::Style| {
            let id = db.query(&fontdb::Query {
                families: &[fontdb::Family::Name(family)],
                weight: fontdb::Weight(weight),
                style,
                ..fontdb::Query::default()
            })?;
            let face = db.face(id)?;
            // Only the face asked for: not a regular standing in for a missing italic.
            if style != fontdb::Style::Normal && face.style == fontdb::Style::Normal {
                return None;
            }
            if weight >= 600 && face.weight.0 < 600 {
                return None;
            }
            db.with_face_data(id, |data, index| prosecraft_ui_egui::fonts::Face {
                bytes: data.to_vec(),
                index,
            })
        };
        let regular = load(400, fontdb::Style::Normal)?;
        Some(prosecraft_ui_egui::editor_fonts::FamilyFaces {
            regular: Some(regular),
            italic: load(400, fontdb::Style::Italic),
            bold: load(700, fontdb::Style::Normal),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The embedded window icon decodes, at the size packaging/icons/render.sh writes.
    #[test]
    fn window_icon_decodes() {
        let icon = window_icon().expect("the embedded icon decodes");
        assert_eq!((icon.width, icon.height), (256, 256));
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
    }
}
