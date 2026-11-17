//! prosecraft-rs user interface, drawn with egui. The desktop and web apps both host [`ProseApp`].
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use prosecraft_core::about;

/// The whole app's state. For now it only shows the start page.
#[derive(Debug, Default)]
pub struct ProseApp {
    about_open: bool,
}

impl ProseApp {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the About section is expanded.
    pub fn about_open(&self) -> bool {
        self.about_open
    }

    /// Draws the app into `ui`.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.heading(about::APP_NAME);
        ui.label("Outline. Write. Publish.");
        ui.add_space(8.0);
        let label = if self.about_open {
            "Hide about"
        } else {
            "About"
        };
        if ui.button(label).clicked() {
            self.about_open = !self.about_open;
        }
        if self.about_open {
            ui.label(about::name_and_version());
            ui.label(about::credit_line());
        }
    }
}
