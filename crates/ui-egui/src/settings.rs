//! The Settings dialog. Its "Device sync" group is ProseCraft's (in `+page.svelte`): the
//! sync method, the folder, the passphrase, Sync now and Forget folder, with ProseCraft's
//! messages. Only folder sync is ported (not Google Drive), and of its methods the single
//! encrypted file and the encrypted folder (not the open folder of Markdown files yet).

use egui::{RichText, Ui};
use prosecraft_core::sync::SyncMode;

use crate::AppState;
use crate::widgets::eyebrow;

/// The Device sync group's state.
#[derive(Clone, Debug, Default)]
pub(crate) struct SyncPanel {
    /// This host can sync through a folder (the desktop app).
    pub available: bool,
    /// CraftCloud is installed: its folder is offered in one click.
    pub craftcloud: bool,
    pub folder: String,
    pub mode: SyncMode,
    pub passphrase: String,
    pub status: String,
    pub busy: bool,
}

/// What the dialog asked for, handled by the app after the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SyncAction {
    ChooseFolder,
    UseCraftCloud,
    SetMode(SyncMode),
    SyncNow,
    Forget,
}

/// `syncFolderReadyMessage` and the messages after choosing a folder.
pub(crate) fn folder_saved_message(mode: SyncMode, same: bool) -> &'static str {
    match (same, mode) {
        (true, SyncMode::OpenFolder) => "Folder ready. Choose Sync now.",
        (true, _) => "Folder ready. Enter your passphrase and choose Sync now.",
        (false, SyncMode::OpenFolder) => {
            "Folder saved. The first sync writes plain Markdown and JSON files."
        }
        (false, SyncMode::EncryptedFile) => {
            "Folder saved. The first sync creates one encrypted sync file."
        }
        (false, SyncMode::EncryptedFolder) => {
            "Folder saved. The first sync creates an encrypted vault inside it."
        }
    }
}

/// `syncCreatedMessage` for a local folder.
pub(crate) fn created_message(mode: SyncMode, count: usize) -> String {
    match mode {
        SyncMode::OpenFolder => format!("Set up the open folder. {count} project(s) synced."),
        SyncMode::EncryptedFile => {
            format!("Created an encrypted sync file. {count} project(s) synced.")
        }
        SyncMode::EncryptedFolder => {
            format!("Created an encrypted vault in this folder. {count} project(s) synced.")
        }
    }
}

/// `Synced N project(s) (2 imported, 1 conflicted copy).`
pub(crate) fn synced_message(result: &prosecraft_core::sync::SyncResult) -> String {
    let mut details = Vec::new();
    if result.imported > 0 {
        details.push(format!("{} imported", result.imported));
    }
    if result.exported > 0 {
        details.push(format!("{} exported", result.exported));
    }
    if result.removed > 0 {
        details.push(format!("{} removed", result.removed));
    }
    if result.conflicts > 0 {
        details.push(format!(
            "{} conflicted {}",
            result.conflicts,
            if result.conflicts == 1 {
                "copy"
            } else {
                "copies"
            }
        ));
    }
    let details = if details.is_empty() {
        String::new()
    } else {
        format!(" ({})", details.join(", "))
    };
    format!("Synced {} project(s){details}.", result.projects.len())
}

fn help(ui: &mut Ui, palette: &crate::theme::Palette, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(12.0).color(palette.text_muted)).wrap());
}

fn mode_option(
    ui: &mut Ui,
    panel: &mut SyncPanel,
    actions: &mut Vec<SyncAction>,
    mode: SyncMode,
    title: &str,
    text: &str,
    enabled: bool,
) {
    let selected = panel.mode == mode;
    ui.add_enabled_ui(enabled && !panel.busy, |ui| {
        if ui.radio(selected, RichText::new(title).strong()).clicked() && !selected {
            actions.push(SyncAction::SetMode(mode));
        }
    });
    ui.horizontal(|ui| {
        ui.add_space(24.0);
        ui.add(egui::Label::new(RichText::new(text).size(12.0)).wrap());
    });
    ui.add_space(4.0);
}

fn device_sync(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    let panel = &mut state.sync;
    let actions = &mut state.sync_actions;
    eyebrow(ui, &palette, "Device sync");
    if !panel.available {
        help(
            ui,
            &palette,
            "Device sync works through a folder on this computer, so it is available in the desktop app.",
        );
        return;
    }
    help(
        ui,
        &palette,
        "Keep this device in sync through a folder your devices already share, such as a Nextcloud folder.",
    );
    ui.add_space(6.0);
    let group = ui.scope(|ui| {
        mode_option(ui, panel, actions, SyncMode::EncryptedFile, "Single encrypted file", "All projects in one passphrase-protected file. Best for cloud folders and Google Drive.", true);
        mode_option(ui, panel, actions, SyncMode::OpenFolder, "Open folder", "Plain Markdown and JSON, one folder per project. Not encrypted. (Not available in prosecraft-rs yet.)", false);
        if panel.mode == SyncMode::EncryptedFolder {
            mode_option(ui, panel, actions, SyncMode::EncryptedFolder, "Encrypted folder (existing layout)", "One encrypted file per project record. Keeps existing sync folders working.", true);
        }
    });
    ui.ctx().accesskit_node_builder(group.response.id, |node| {
        node.set_role(egui::accesskit::Role::RadioGroup);
        node.set_label("Sync method");
    });
    match panel.mode {
        SyncMode::EncryptedFile => help(
            ui,
            &palette,
            "Writes one prosecraft-sync.prosecraft file. Projects are encrypted with your passphrase before they are written.",
        ),
        SyncMode::EncryptedFolder => help(
            ui,
            &palette,
            "Writes vault.json and one encrypted file per project record.",
        ),
        SyncMode::OpenFolder => help(
            ui,
            &palette,
            "This sync location is not encrypted. Anyone with access to it can read and edit your writing.",
        ),
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let path = if panel.folder.is_empty() {
            "No folder chosen"
        } else {
            panel.folder.as_str()
        };
        ui.add(egui::Label::new(RichText::new(path).monospace().size(12.0)).truncate())
            .on_hover_text(path);
        if ui
            .add_enabled(!panel.busy, egui::Button::new("Choose folder…"))
            .clicked()
        {
            actions.push(SyncAction::ChooseFolder);
        }
    });
    if panel.craftcloud {
        if ui
            .add_enabled(!panel.busy, egui::Button::new("Use the CraftCloud folder"))
            .clicked()
        {
            actions.push(SyncAction::UseCraftCloud);
        }
        help(
            ui,
            &palette,
            "Syncs through a ProseCraft folder inside the folder CraftCloud keeps in sync, as one encrypted file.",
        );
    }
    if !panel.folder.is_empty() {
        ui.add_space(6.0);
        let field = ui.add_enabled(
            !panel.busy,
            egui::TextEdit::singleline(&mut panel.passphrase)
                .password(true)
                .hint_text("Sync passphrase")
                .desired_width(260.0),
        );
        crate::views::name_field(ui, &field, "Sync passphrase");
        let long_enough = panel.passphrase.encode_utf16().count() >= 8;
        if field.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && long_enough
            && !panel.busy
        {
            actions.push(SyncAction::SyncNow);
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let label = if panel.busy { "Syncing…" } else { "Sync now" };
            if ui
                .add_enabled(!panel.busy && long_enough, egui::Button::new(label))
                .clicked()
            {
                actions.push(SyncAction::SyncNow);
            }
            if ui
                .add_enabled(!panel.busy, egui::Button::new("Forget folder"))
                .clicked()
            {
                actions.push(SyncAction::Forget);
            }
        });
    }
    if !panel.status.is_empty() {
        ui.add_space(6.0);
        let status = ui.add(
            egui::Label::new(
                RichText::new(&panel.status)
                    .size(12.0)
                    .color(palette.text_secondary),
            )
            .wrap(),
        );
        let text = panel.status.clone();
        ui.ctx().accesskit_node_builder(status.id, move |node| {
            node.set_role(egui::accesskit::Role::Status);
            node.set_label(text);
        });
    }
}

/// The Settings dialog, while open.
pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    if !state.settings_open {
        return;
    }
    let mut open = true;
    egui::Window::new("Settings")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(460.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            ui.set_width(440.0);
            device_sync(ui, state);
        });
    if !open || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.settings_open = false;
    }
}
