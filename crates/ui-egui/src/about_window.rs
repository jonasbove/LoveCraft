//! The About window (ProseCraft's `aboutDialog`): the version, the licence and who made it, the
//! attributions, and a diagnostics report to copy, which describes the setup and never the
//! writing.

use egui::{RichText, Ui};
use prosecraft_core::about;

use crate::AppState;

/// The open window: its report, and "Copied" until it fades.
#[derive(Clone, Debug, Default)]
pub(crate) struct About {
    report: String,
    copied_at: Option<f64>,
}

/// Where the app runs (`os` and `storage backend` in the report).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Platform {
    pub os: String,
    pub storage: String,
}

impl Default for Platform {
    /// The desktop: its operating system and egui, and the library file.
    fn default() -> Self {
        let os = match std::env::consts::OS {
            "linux" => "Linux",
            "macos" => "macOS",
            "windows" => "Windows",
            "freebsd" => "FreeBSD",
            _ => "unknown",
        };
        Self {
            os: format!("{os}, egui"),
            storage: "library file".into(),
        }
    }
}

/// `openAbout`: the report as things are now.
pub(crate) fn open(state: &mut AppState, now_ms: f64) {
    let project = state
        .library
        .as_ref()
        .and_then(|l| l.projects().get(state.active))
        .map(|p| &p.view);
    let live = |f: &dyn Fn(&prosecraft_core::project::Document) -> bool| {
        project.map_or(0, |p| {
            p.documents.iter().filter(|d| !d.trashed && f(d)).count()
        })
    };
    let input = about::Diagnostics {
        version: about::VERSION.into(),
        os: state.platform.os.clone(),
        storage_backend: state.platform.storage.clone(),
        online: None,
        projects: state.library.as_ref().map_or(0, |l| l.projects().len()),
        chapters: project.map_or(0, |p| p.chapters.iter().filter(|c| !c.trashed).count()),
        scenes: live(&|d| d.chapter_id.is_some() && d.space.is_none()),
        wiki_pages: live(&|d| d.space.as_deref() == Some("Wiki")),
        words: project.map_or(0, prosecraft_core::project::Project::word_count),
        comments: project.map_or(0, |p| p.comments.len()),
        snapshots: project.map_or(0, |p| p.snapshots.len()),
        features: vec![
            ("zen mode".into(), state.writing.zen),
            ("typewriter mode".into(), state.writing.typewriter),
            ("fade other paragraphs".into(), state.writing.fade),
            ("markdown editor".into(), state.markdown_enabled),
            ("proofreading".into(), state.proofread.enabled),
        ],
    };
    // `toISOString`.
    let date = chrono::DateTime::from_timestamp_millis(now_ms as i64)
        .map(|t| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_default();
    state.about = Some(About {
        report: about::diagnostics_report(&input, &date),
        copied_at: None,
    });
}

/// The window, while it is open.
pub(crate) fn show(ctx: &egui::Context, state: &mut AppState) {
    let Some(about) = state.about.as_mut() else {
        return;
    };
    let palette = state.palette;
    let now = ctx.input(|i| i.time);
    let mut open = true;
    let mut done = false;
    let mut url = None;
    egui::Window::new(about::APP_NAME)
        .id(egui::Id::unique("about-window"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui: &mut Ui| {
            ui.set_max_width(460.0);
            let muted = |text: &str| RichText::new(text).size(12.0).color(palette.text_muted);
            ui.label(RichText::new(format!("Version {}", about::VERSION)).strong().color(palette.text_heading));
            ui.label(muted(&format!("Licensed under the {} license", about::LICENSE)));
            ui.add(egui::Label::new(muted(&about::credit_line())).wrap());
            if ui.link(about::ORIGINAL_URL).clicked() {
                url = Some(about::ORIGINAL_URL.to_owned());
            }
            ui.add_space(10.0);
            ui.label(RichText::new("Open source licences / Attribution").strong().color(palette.text_heading));
            ui.add(
                egui::Label::new(muted(
                    "Bundled citation style rules are derived from the Citation Style Language project styles (CC BY-SA 3.0).",
                ))
                .wrap(),
            );
            let styles = "https://github.com/citation-style-language/styles";
            if ui.link(styles).clicked() {
                url = Some(styles.to_owned());
            }
            ui.add_space(10.0);
            ui.label(RichText::new("Diagnostics").strong().color(palette.text_heading));
            ui.add(
                egui::Label::new(muted(
                    "The report below contains no titles or text from your writing. It only describes the app and your setup.",
                ))
                .wrap(),
            );
            let mut report = about.report.clone();
            let field = ui.add(
                egui::TextEdit::multiline(&mut report)
                    .interactive(true)
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(8)
                    .desired_width(f32::INFINITY),
            );
            crate::views::name_field(ui, &field, "Diagnostics report");
            ui.horizontal(|ui| {
                if ui.button("Copy diagnostics").clicked() {
                    ui.ctx().copy_text(about.report.clone());
                    about.copied_at = Some(now);
                }
                // "Copied", for two seconds (`aboutCopyTimer`).
                if let Some(at) = about.copied_at {
                    if now - at < 2.0 {
                        let status = ui.label(muted("Copied"));
                        ui.ctx().accesskit_node_builder(status.id, |node| {
                            node.set_role(egui::accesskit::Role::Status);
                            node.set_value("Copied");
                        });
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                    } else {
                        about.copied_at = None;
                    }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(muted(&about::name_and_version()));
                done = ui.button("Done").clicked();
            });
        });
    if let Some(url) = url {
        state.requests.push(crate::HostRequest::OpenUrl(url));
    }
    if !open || done || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.about = None;
    }
}
