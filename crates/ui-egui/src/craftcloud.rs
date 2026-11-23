//! Open from CraftCloud… and Save to CraftCloud… (CraftCloud's `docs/CRAFT-INTEGRATION.md`,
//! section 5), in the app's own dialogs. The host hands the app a [`Runner`] for the
//! `craftcloud` program when it is installed; without one, none of this shows. Every command
//! runs on a background thread, so the window never waits on CraftCloud.
//!
//! (craft-libs' `craft-ui` `cloud` pages are built for a cloud the app signs in to and reads
//! bytes from: accounts, folders, conditional writes. CraftCloud's contract is a program that
//! lists files already synced here and copies a file in, so the dialogs are this app's.)

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use egui::{Key, RichText, Ui};
use prosecraft_core::craftcloud::{self as cc, CloudFile, Failure, Runner, Saved, SyncState};

use crate::{AppState, HostRequest, views};

/// What the app knows of CraftCloud, and its two dialogs.
#[derive(Default)]
pub(crate) struct Cloud {
    /// The `craftcloud` program, when it is installed.
    pub runner: Option<Arc<dyn Runner>>,
    pub open: Option<OpenDialog>,
    pub save: Option<SaveDialog>,
}

impl Cloud {
    /// Whether a dialog of this module is open (the menus' items wait).
    pub fn busy(&self) -> bool {
        self.open.is_some() || self.save.is_some()
    }
}

type Listing = Result<Vec<CloudFile>, Failure>;

/// Open from CraftCloud…: the backups CraftCloud has, newest first.
pub(crate) struct OpenDialog {
    loading: Option<Receiver<Listing>>,
    files: Vec<CloudFile>,
    error: Option<String>,
}

/// Save to CraftCloud…: the open project's backup, on its way.
pub(crate) struct SaveDialog {
    file_name: String,
    bytes: Arc<Vec<u8>>,
    step: SaveStep,
}

enum SaveStep {
    Saving(Receiver<Result<Saved, Failure>>),
    /// A file of that name is there: replace it? CraftCloud's own words with it.
    Replace(String),
    Failed(String),
}

/// Runs `work` on a background thread and hands back where its answer arrives. The window is
/// asked to draw again once it has.
fn spawn<T: Send + 'static>(
    ctx: &egui::Context,
    name: &str,
    work: impl FnOnce() -> T + Send + 'static,
) -> Receiver<T> {
    let (send, receive) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    let job = move || {
        let _ = send.send(work());
        ctx.request_repaint();
    };
    // The web has no threads, and no CraftCloud program either: it never gets here.
    #[cfg(not(target_arch = "wasm32"))]
    let _ = std::thread::Builder::new().name(name.to_owned()).spawn(job);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = name;
        job();
    }
    receive
}

/// Opens Open from CraftCloud… and starts listing.
pub(crate) fn start_open(ctx: &egui::Context, state: &mut AppState) {
    let Some(runner) = state.cloud.runner.clone() else {
        return;
    };
    let loading = spawn(ctx, "craftcloud-files", move || {
        cc::list_backups(runner.as_ref())
    });
    state.cloud.open = Some(OpenDialog {
        loading: Some(loading),
        files: Vec::new(),
        error: None,
    });
}

/// Starts saving `bytes` to CraftCloud as `file_name` (`force`: replacing what is there).
pub(crate) fn start_save(
    ctx: &egui::Context,
    state: &mut AppState,
    file_name: String,
    bytes: Arc<Vec<u8>>,
    force: bool,
) {
    let Some(runner) = state.cloud.runner.clone() else {
        return;
    };
    let (name, data) = (file_name.clone(), bytes.clone());
    let saving = spawn(ctx, "craftcloud-save", move || {
        cc::save_backup(runner.as_ref(), &name, &data, cc::FOLDER, force)
    });
    state.cloud.save = Some(SaveDialog {
        file_name,
        bytes,
        step: SaveStep::Saving(saving),
    });
}

/// An error line that screen readers announce.
fn alert(ui: &mut Ui, color: egui::Color32, text: &str) {
    let label = ui.colored_label(color, text);
    let said = text.to_owned();
    ui.ctx().accesskit_node_builder(label.id, move |node| {
        node.set_role(egui::accesskit::Role::Alert);
        node.set_label(said);
    });
}

/// "13 Nov 2026, 21:54 UTC" (CraftCloud's times are in UTC).
fn when(file: &CloudFile) -> String {
    file.modified_utc()
        .map(|t| t.format("%-d %b %Y, %H:%M UTC").to_string())
        .unwrap_or_default()
}

/// The dialogs, each frame.
pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    open_dialog(ui, state);
    save_dialog(ui, state);
}

fn open_dialog(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    let Some(dialog) = state.cloud.open.as_mut() else {
        return;
    };
    if let Some(receiver) = dialog.loading.as_ref() {
        match receiver.try_recv() {
            Ok(Ok(files)) => {
                dialog.files = files;
                dialog.loading = None;
            }
            Ok(Err(failure)) => {
                dialog.error = Some(failure.message());
                dialog.loading = None;
            }
            Err(TryRecvError::Disconnected) => {
                dialog.error = Some(Failure::Other(String::new()).message());
                dialog.loading = None;
            }
            Err(TryRecvError::Empty) => {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
    }
    let mut close = false;
    let mut retry = false;
    let mut chosen: Option<String> = None;
    egui::Window::new("Open from CraftCloud")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            ui.set_width(460.0);
            ui.label(
                RichText::new("ProseCraft backups in CraftCloud, newest first.")
                    .size(12.0)
                    .color(palette.text_muted),
            );
            ui.add_space(6.0);
            if dialog.loading.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Looking in CraftCloud…");
                });
            } else if let Some(error) = &dialog.error {
                alert(ui, palette.danger, error);
                retry = ui.button("Try again").clicked();
            } else if dialog.files.is_empty() {
                ui.label("There are no ProseCraft backups in CraftCloud yet. Save one with Save to CraftCloud….");
            } else {
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for file in &dialog.files {
                            if file_row(ui, palette, file) {
                                chosen = file.local_path.clone();
                            }
                        }
                    });
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let button = ui.button("Cancel");
                views::name_field(ui, &button, "Close Open from CraftCloud");
                close |= button.clicked();
            });
        });
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        close = true;
    }
    if let Some(path) = chosen {
        state.requests.push(HostRequest::OpenBackupPath(path));
        close = true;
    }
    if close {
        state.cloud.open = None;
    } else if retry {
        let ctx = ui.ctx().clone();
        start_open(&ctx, state);
    }
}

/// One backup: its name (greyed out when it isn't on this computer), where it is and when it
/// changed, and how it stands with the sync. Whether it was chosen.
fn file_row(ui: &mut Ui, palette: crate::theme::Palette, file: &CloudFile) -> bool {
    let mut chosen = false;
    ui.horizontal(|ui| {
        let here = file.is_here();
        let button = ui.add_enabled(here, egui::Button::new(file.name.as_str()).frame(false));
        let button = if here {
            button.on_hover_text(file.path.as_str())
        } else {
            button.on_disabled_hover_text(
                "Not on this computer: CraftCloud doesn't sync this folder here, or hasn't downloaded it yet.",
            )
        };
        chosen = button.clicked();
        let badge = if here {
            file.sync.badge()
        } else {
            "Not on this computer"
        };
        if !badge.is_empty() {
            let color = match file.sync {
                SyncState::Error | SyncState::Conflict => palette.danger,
                _ => palette.text_muted,
            };
            egui::Frame::new()
                .stroke(egui::Stroke::new(1.0, color))
                .corner_radius(8.0)
                .inner_margin(egui::Margin::symmetric(6, 1))
                .show(ui, |ui| {
                    ui.label(RichText::new(badge).size(10.0).color(color));
                });
        }
    });
    let folder = match file.folder() {
        "" => "CraftCloud".to_owned(),
        folder => folder.to_owned(),
    };
    let detail = [folder, when(file)]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    ui.label(RichText::new(detail).size(11.0).color(palette.text_muted));
    ui.add_space(4.0);
    chosen
}

fn save_dialog(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    let Some(dialog) = state.cloud.save.as_mut() else {
        return;
    };
    let mut done: Option<String> = None;
    if let SaveStep::Saving(receiver) = &dialog.step {
        match receiver.try_recv() {
            Ok(Ok(saved)) => done = Some(saved_message(&saved)),
            Ok(Err(Failure::Exists(said))) => dialog.step = SaveStep::Replace(said),
            // `--wait` was cut short: the backup is in the sync folder and uploads later.
            Ok(Err(Failure::NoClient(_))) => {
                done = Some(format!(
                    "{} is in your CraftCloud folder. The CraftCloud desktop client stopped before uploading it; it uploads when CraftCloud runs again.",
                    dialog.file_name
                ));
            }
            Ok(Err(failure)) => dialog.step = SaveStep::Failed(failure.message()),
            Err(TryRecvError::Disconnected) => {
                dialog.step = SaveStep::Failed(Failure::Other(String::new()).message());
            }
            Err(TryRecvError::Empty) => {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
    }
    if let Some(message) = done {
        state.cloud.save = None;
        state.info(message);
        return;
    }
    let mut close = false;
    let mut replace = false;
    let saving = matches!(dialog.step, SaveStep::Saving(_));
    egui::Window::new("Save to CraftCloud")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            ui.set_max_width(400.0);
            match &dialog.step {
                SaveStep::Saving(_) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(format!("Saving {} to CraftCloud…", dialog.file_name));
                    });
                    ui.label(
                        RichText::new("You can keep writing; this finishes once CraftCloud has uploaded it.")
                            .size(12.0)
                            .color(palette.text_muted),
                    );
                }
                SaveStep::Replace(said) => {
                    ui.label(RichText::new("Replace?").strong().color(palette.text_heading));
                    ui.label(format!(
                        "There is already a {} in CraftCloud's {} folder. Replace it with this backup? CraftCloud keeps the one it replaces among its earlier versions.",
                        dialog.file_name,
                        cc::FOLDER
                    ));
                    if !said.is_empty() {
                        ui.label(RichText::new(said).size(11.0).color(palette.text_muted));
                    }
                    ui.horizontal(|ui| {
                        replace = ui.button("Replace").clicked();
                        close |= ui.button("Cancel").clicked();
                    });
                }
                SaveStep::Failed(message) => {
                    alert(ui, palette.danger, message);
                    let button = ui.button("Close");
                    views::name_field(ui, &button, "Close Save to CraftCloud");
                    close |= button.clicked();
                }
            }
        });
    if !saving && ui.input(|i| i.key_pressed(Key::Escape)) {
        close = true;
    }
    if close {
        state.cloud.save = None;
    } else if replace {
        let (name, bytes) = (dialog.file_name.clone(), dialog.bytes.clone());
        let ctx = ui.ctx().clone();
        start_save(&ctx, state, name, bytes, true);
    }
}

/// What a finished save says.
fn saved_message(saved: &Saved) -> String {
    let how = match saved.state.as_str() {
        "queued" => " CraftCloud uploads it in a moment.",
        _ => "",
    };
    let replaced = if saved.replaced {
        " The one it replaced is kept among CraftCloud's earlier versions."
    } else {
        ""
    };
    format!("Saved to CraftCloud as {}.{how}{replaced}", saved.path)
}
