//! prosecraft-rs on the desktop.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::process::ExitCode;

use prosecraft_core::about;
use prosecraft_ui_egui::ProseApp;

/// The window's app id (Wayland app id, X11 class). A placeholder until the bundle id is decided.
const APP_ID: &str = "io.github.jonasbove.prosecraft_rs";

struct DesktopApp(ProseApp);

impl eframe::App for DesktopApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        eframe::egui::CentralPanel::default().show(ui, |ui| self.0.show(ui));
    }
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("{}", about::name_and_version());
        return ExitCode::SUCCESS;
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(about::APP_NAME)
            .with_app_id(APP_ID)
            .with_inner_size([1280.0, 850.0]),
        ..Default::default()
    };
    let result = eframe::run_native(
        about::APP_NAME,
        options,
        Box::new(|_cc| Ok(Box::new(DesktopApp(ProseApp::new())))),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}: {error}", about::APP_NAME);
            ExitCode::FAILURE
        }
    }
}
