//! prosecraft-rs on the desktop.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::path::Path;
use std::process::ExitCode;

use prosecraft_core::about;
use prosecraft_ui_egui::{ProseApp, window_title};

/// The window's app id (Wayland app id, X11 class). A placeholder until the bundle id is decided.
const APP_ID: &str = "io.github.jonasbove.prosecraft_rs";

struct DesktopApp {
    app: ProseApp,
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
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.app.show(ui);
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
    let mut app = ProseApp::new();
    if let Some(path) = args.first() {
        read_backup(&mut app, Path::new(path));
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
            Ok(Box::new(DesktopApp {
                app,
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
