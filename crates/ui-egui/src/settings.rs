//! The Settings dialog. Its "Device sync" group is ProseCraft's (in `+page.svelte`): the
//! sync method, the folder, the passphrase, Sync now and Forget folder, with ProseCraft's
//! messages. Only folder sync is ported (not Google Drive), with all three of its methods.

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

/// ProseCraft's "Import a manuscript" group: EPUB, Scrivener, Markdown folders and Fountain.
fn import_manuscript(ui: &mut Ui, state: &mut AppState) {
    use crate::HostRequest;
    let palette = state.palette;
    ui.label(
        RichText::new("Import a manuscript")
            .strong()
            .color(palette.text_heading),
    );
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        for (label, request) in [
            ("Import EPUB…", HostRequest::ImportEpub),
            ("Import Scrivener zip…", HostRequest::ImportScrivenerZip),
            (
                "Import Scrivener folder…",
                HostRequest::ImportScrivenerFolder,
            ),
            ("Import Markdown folder…", HostRequest::ImportMarkdownFolder),
            ("Import Fountain screenplay…", HostRequest::ImportFountain),
        ] {
            // A screenplay joins the open project, so it needs one.
            let enabled = state.has_home
                && (request != HostRequest::ImportFountain || state.library.is_some());
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                state.requests.push(request);
            }
        }
    });
    help(
        ui,
        &palette,
        "Creates a new project from an EPUB file. Chapter structure and supported illustrations are preserved, and the cover becomes the project cover when it can be embedded.",
    );
    help(
        ui,
        &palette,
        "Creates a new project from a Scrivener project (.scriv), as a zip or as a folder. Manuscript chapters and scenes become chapters and scenes, the research folder becomes wiki pages. Labels, status, keywords, notes and snapshots are not imported.",
    );
    help(
        ui,
        &palette,
        "Creates a new project from a folder of Markdown files (for example an Obsidian vault). You choose which folders are the manuscript and which become wiki pages.",
    );
    help(
        ui,
        &palette,
        "Adds a Fountain or plain-text screenplay to the open project as a screenplay scene.",
    );
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
        mode_option(ui, panel, actions, SyncMode::OpenFolder, "Open folder", "Plain Markdown and JSON, one folder per project. Not encrypted.", true);
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
            "This sync location is not encrypted. Anyone with access to it can read and edit your writing. Delete scenes in ProseCraft rather than removing their files.",
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
        // The open folder has no passphrase: nothing in it is encrypted.
        let open = panel.mode == SyncMode::OpenFolder;
        let long_enough = open || panel.passphrase.encode_utf16().count() >= 8;
        if !open {
            ui.add_space(6.0);
            let field = ui.add_enabled(
                !panel.busy,
                egui::TextEdit::singleline(&mut panel.passphrase)
                    .password(true)
                    .hint_text("Sync passphrase")
                    .desired_width(260.0),
            );
            crate::views::name_field(ui, &field, "Sync passphrase");
            if field.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && long_enough
                && !panel.busy
            {
                actions.push(SyncAction::SyncNow);
            }
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

/// ProseCraft's `localStorage` key for the Theme preference.
pub const THEME_KEY: &str = "prosecraft-theme";

/// ProseCraft's Theme preference (`prosecraft-theme`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemePreference {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub fn id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "Follow system",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id() == id)
    }
}

/// The dialog's categories, as ProseCraft's (the ones with ported settings).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Category {
    #[default]
    General,
    Writing,
    Data,
}

impl Category {
    const ALL: [Self; 3] = [Self::General, Self::Writing, Self::Data];

    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Writing => "Writing",
            Self::Data => "Data and sync",
        }
    }
}

/// ProseCraft's Markdown editor preference (`prosecraft-markdown-editor-enabled`).
pub const MARKDOWN_EDITOR_KEY: &str = "prosecraft-markdown-editor-enabled";

/// The Writing category: text size, the writing experience (typewriter, fade, Zen), focus mode,
/// the Markdown editor, and the Timeline view.
fn writing(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    let heading = |ui: &mut Ui, title: &str, text: &str| {
        ui.label(RichText::new(title).strong().color(palette.text_heading));
        help(ui, &palette, text);
        ui.add_space(6.0);
    };
    let mut prefs = state.writing;
    heading(ui, "Text size", "Adjust the editor scale");
    let scale = craft_ui::ComboBox::from_id_salt("editor-scale")
        .selected_text(format!("{}%", prefs.scale))
        .show_ui(ui, |ui| {
            for scale in crate::WritingPrefs::SCALES {
                ui.selectable_value(&mut prefs.scale, scale, format!("{scale}%"));
            }
        });
    ui.ctx().accesskit_node_builder(scale.response.id, |node| {
        node.set_label("Scale");
    });
    ui.add_space(14.0);
    heading(
        ui,
        "Writing experience",
        "Control how the editor follows your focus",
    );
    ui.checkbox(&mut prefs.typewriter, "Typewriter mode");
    help(
        ui,
        &palette,
        "Keeps the caret near the vertical center of the page in both the rich-text and Markdown editors.",
    );
    ui.checkbox(&mut prefs.fade, "Fade other paragraphs");
    help(
        ui,
        &palette,
        "Keeps the paragraph you’re editing at full contrast and fades the rest while an editor is focused.",
    );
    ui.checkbox(&mut prefs.zen, "Hide the interface while typing (Zen mode)");
    help(
        ui,
        &palette,
        "Only hides the interface once you start typing in the editor. Move the mouse outside the editor or press Escape to bring it back.",
    );
    ui.add_space(8.0);
    let modifier = if cfg!(target_os = "macos") {
        "Cmd"
    } else {
        "Ctrl"
    };
    heading(
        ui,
        "Focus mode",
        &format!(
            "Hides everything except your text right now. Press {modifier}+Shift+Enter (or Escape) to bring it back."
        ),
    );
    if ui.button("Enter focus mode").clicked() {
        state.settings_open = false;
        state.view = crate::WorkspaceView::Write;
        state.focus_wanted = true;
    }
    ui.add_space(14.0);
    let mut timeline = prefs.timeline;
    if prefs != state.writing {
        state.writing_choice = Some(prefs);
    }
    ui.label(
        RichText::new("Markdown editor")
            .strong()
            .color(palette.text_heading),
    );
    help(
        ui,
        &palette,
        "Enable an optional plain-text Markdown editor alongside rich text",
    );
    ui.add_space(6.0);
    let mut enabled = state.markdown_enabled;
    if ui
        .checkbox(&mut enabled, "Enable Markdown editor")
        .changed()
    {
        state.markdown_enabled_choice = Some(enabled);
    }
    help(
        ui,
        &palette,
        "When enabled, use the Markdown editor button above a document to switch between Markdown source and rich text.",
    );
    ui.add_space(14.0);
    heading(
        ui,
        "Timeline view",
        "An experimental chronological view of your story",
    );
    if ui.checkbox(&mut timeline, "Show Timeline view").changed() {
        let mut prefs = state.writing_choice.unwrap_or(state.writing);
        prefs.timeline = timeline;
        state.writing_choice = Some(prefs);
    }
    help(
        ui,
        &palette,
        "The Timeline is still taking shape. Turn it on to try it; your timeline data is kept either way.",
    );
}

fn general(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    ui.label(
        RichText::new("Appearance")
            .strong()
            .color(palette.text_heading),
    );
    help(ui, &palette, "Choose how ProseCraft looks");
    ui.add_space(8.0);
    ui.label(
        RichText::new("Theme")
            .size(12.0)
            .color(palette.text_secondary),
    );
    let current = state.theme;
    let mut chosen = current;
    let combo = craft_ui::ComboBox::from_id_salt("theme-preference")
        .selected_text(current.label())
        .show_ui(ui, |ui| {
            for option in ThemePreference::ALL {
                ui.selectable_value(&mut chosen, option, option.label());
            }
        });
    ui.ctx().accesskit_node_builder(combo.response.id, |node| {
        node.set_label("Theme");
        node.set_value(current.label());
    });
    if chosen != current {
        state.theme_choice = Some(chosen);
    }
}

/// The Settings dialog, while open: ProseCraft's categories on the left, the chosen one's
/// settings beside them.
/// "Wiki files": where imported files live on this device.
fn wiki_files(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    eyebrow(ui, &palette, "Wiki files");
    if !state.wiki_available {
        help(
            ui,
            &palette,
            "Wiki files are stored in a folder on this computer, so they are available in the desktop app.",
        );
        return;
    }
    help(
        ui,
        &palette,
        "Imported wiki files live in a project folder on this device. They are stored as plain files, not encrypted, and are not included in project backups or device sync. Their metadata travels with the project. Files can be up to 200 MB each.",
    );
    ui.add_space(6.0);
    let root = state.wiki_root.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        let path = if root.is_empty() {
            "No folder chosen"
        } else {
            root.as_str()
        };
        ui.add(egui::Label::new(RichText::new(path).monospace().size(12.0)).truncate())
            .on_hover_text(path);
        let choose = ui.button("Choose folder…");
        crate::views::name_field(ui, &choose, "Choose wiki files folder");
        if choose.clicked() {
            state.intents.push(crate::editing::Intent::ChooseWikiRoot);
        }
    });
    if ui.button("Reveal project folder").clicked() {
        state
            .intents
            .push(crate::editing::Intent::RevealWikiProjectFolder);
    }
    if let Some(status) = &state.wiki_settings_status {
        ui.label(RichText::new(status).size(11.0).color(palette.text_muted));
    }
}

pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    if !state.settings_open {
        return;
    }
    let mut open = true;
    egui::Window::new("Settings")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        // A fixed size, so the window doesn\'t jump between categories or drift while it grows.
        .fixed_size(egui::vec2(620.0, 520.0))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(140.0);
                    let palette = state.palette;
                    for category in Category::ALL {
                        let selected = state.settings_tab == category;
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(140.0, 30.0), egui::Sense::hover());
                        let tab = craft_ui::tabs::Tab::new(
                            egui::Id::unique(("settings-category", category.label())),
                            category.label(),
                            selected,
                        )
                        .show_at(ui, rect, |ui, response| {
                            let fill = if selected {
                                Some(palette.selected_bg)
                            } else if response.hovered() {
                                Some(palette.hover_bg)
                            } else {
                                None
                            };
                            if let Some(fill) = fill {
                                ui.painter()
                                    .rect_filled(rect, egui::CornerRadius::same(6), fill);
                            }
                            let color = if selected {
                                palette.selected_text
                            } else {
                                palette.text_secondary
                            };
                            ui.painter().text(
                                rect.left_center() + egui::vec2(10.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                category.label(),
                                egui::FontId::proportional(13.0),
                                color,
                            );
                        });
                        if tab.clicked() {
                            state.settings_tab = category;
                        }
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_width(440.0);
                    match state.settings_tab {
                        Category::General => general(ui, state),
                        Category::Writing => writing(ui, state),
                        Category::Data => {
                            egui::ScrollArea::vertical()
                                .id_salt("settings-data")
                                .show(ui, |ui| {
                                    import_manuscript(ui, state);
                                    ui.add_space(14.0);
                                    device_sync(ui, state);
                                    ui.add_space(14.0);
                                    wiki_files(ui, state);
                                });
                        }
                    }
                });
            });
        });
    if !open || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.settings_open = false;
    }
}
