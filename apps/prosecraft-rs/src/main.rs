//! prosecraft-rs on the desktop.
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
use prosecraft_core::library::{Library, new_device_id};
use prosecraft_native::{LibraryFile, OsRandom};
use prosecraft_ui_egui::{HostRequest, LibraryStore, ProseApp, window_title};

/// The window's app id (Wayland app id, X11 class). A placeholder until the bundle id is decided.
const APP_ID: &str = "io.github.jonasbove.prosecraft_rs";

/// What to do with a dialog's answer.
enum Then {
    Open,
    /// Write these bytes where the person chose (a backup or an exported manuscript).
    Export(Vec<u8>),
    /// The folder to sync through.
    SyncFolder,
}

/// The library file, saved atomically.
struct DiskStore {
    file: LibraryFile,
    /// CraftCloud's `craftcloud` program, found on `PATH` at start.
    craftcloud: Option<std::path::PathBuf>,
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
    fn has_craftcloud(&self) -> bool {
        self.craftcloud.is_some()
    }
    fn craftcloud_folder(&mut self) -> Result<String, String> {
        let program = self
            .craftcloud
            .as_ref()
            .ok_or("CraftCloud isn't installed.")?;
        prosecraft_native::craftcloud::prosecraft_folder(program)
            .map(|p| p.to_string_lossy().into_owned())
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
        ProseApp::with_library(
            library,
            Box::new(DiskStore {
                file,
                craftcloud: prosecraft_native::craftcloud::find_on_path(),
            }),
        ),
        None,
    )
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

/// Reads a backup for opening. The file is only read, never written.
fn read_backup(app: &mut ProseApp, path: &Path) {
    let name = file_name(path);
    if std::fs::metadata(path).is_ok_and(|m| m.len() > ProseApp::MAX_BACKUP_BYTES) {
        eprintln!("{name}: far too large to be a ProseCraft backup");
        return;
    }
    match std::fs::read(path) {
        Ok(bytes) => app.open_backup_bytes(&name, &bytes),
        Err(error) => eprintln!("{name}: {error}"),
    }
}

impl eframe::App for DesktopApp {
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
                (Then::Export(text), Answer::Paths(paths)) => {
                    if let Some(path) = paths.first()
                        && let Err(e) = craft_atomic::atomic_write(path, &text)
                    {
                        eprintln!("{}: {e}", path.display());
                    }
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

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "--version") {
        println!("{}", about::name_and_version());
        return ExitCode::SUCCESS;
    }
    let (mut app, problem) = start();
    if let Some(problem) = problem {
        eprintln!("{problem}");
    }
    for path in &args {
        read_backup(&mut app, &PathBuf::from(path));
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(about::APP_NAME)
            .with_app_id(APP_ID)
            .with_inner_size([1280.0, 850.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    let result = eframe::run_native(
        about::APP_NAME,
        options,
        Box::new(|cc| {
            prosecraft_ui_egui::setup(&cc.egui_ctx);
            let ctx = cc.egui_ctx.clone();
            let dialogs = Dialogs::new(Arc::new(move || ctx.request_repaint()));
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
