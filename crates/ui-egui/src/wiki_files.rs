//! Wiki files and web links in the app: the device's file store the host gives
//! ([`WikiFileStore`]), the "Add web link" dialog and the Markdown preview
//! (`researchLinkDialog`, `researchMarkdownDialog` in ProseCraft's `+page.svelte`).

use egui::{RichText, Ui};

use crate::theme::Palette;

/// The device setting holding the wiki files folder (`prosecraft-research-root`).
pub const ROOT_KEY: &str = "prosecraft-research-root";

/// Where wiki files live on this device; the desktop host provides it (ProseCraft's Tauri
/// `research_*` commands). Paths are the host's own, as text.
pub trait WikiFileStore {
    /// Copies a chosen file in under `storage_key`; its size.
    fn import(
        &self,
        root: &str,
        folder: &str,
        storage_key: &str,
        source: &str,
    ) -> Result<u64, String>;
    /// A Markdown file's text for the preview.
    fn read_markdown(&self, root: &str, folder: &str, storage_key: &str) -> Result<String, String>;
    /// Where a stored file is.
    fn file_path(&self, root: &str, folder: &str, storage_key: &str) -> Result<String, String>;
    fn delete(&self, root: &str, folder: &str, storage_key: &str) -> Result<(), String>;
    /// The project's folder, made if missing.
    fn prepare_folder(&self, root: &str, folder: &str) -> Result<String, String>;
    /// Opens a file or address with the system's app.
    fn open(&self, target: &str) -> Result<(), String>;
    /// Shows a file or folder in the file manager.
    fn reveal(&self, path: &str) -> Result<(), String>;
}

/// The "Add web link" form.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinkDraft {
    pub folder: Option<String>,
    pub url: String,
    pub title: String,
    pub description: String,
    pub error: String,
}

/// What the link dialog asks for.
pub enum LinkAction {
    None,
    Save,
    Close,
}

/// `researchLinkDialog`.
pub fn link_dialog(ctx: &egui::Context, palette: &Palette, draft: &mut LinkDraft) -> LinkAction {
    let mut action = LinkAction::None;
    let mut open = true;
    egui::Window::new("Add web link")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(RichText::new("WIKI").size(10.0).color(palette.text_muted));
            ui.label(RichText::new("Link details").strong());
            ui.label(
                RichText::new("Save a website or online resource in the Wiki")
                    .size(11.0)
                    .color(palette.text_muted),
            );
            ui.add_space(6.0);
            let field = |ui: &mut Ui, label: &str, value: &mut String, hint: &str, limit: usize| {
                ui.label(label);
                let response = ui.add(
                    egui::TextEdit::singleline(value)
                        .hint_text(hint)
                        .char_limit(limit)
                        .desired_width(360.0),
                );
                crate::views::name_field(ui, &response, label);
                response
            };
            let url = field(
                ui,
                "Web address",
                &mut draft.url,
                "https://example.com",
                usize::MAX,
            );
            if !url.has_focus() && draft.url.is_empty() && draft.title.is_empty() {
                url.request_focus();
            }
            field(
                ui,
                "Title",
                &mut draft.title,
                "Optional — defaults to the site name",
                200,
            );
            ui.label("Description");
            let description = ui.add(
                egui::TextEdit::multiline(&mut draft.description)
                    .hint_text("Optional note about this resource")
                    .char_limit(2000)
                    .desired_rows(3)
                    .desired_width(360.0),
            );
            crate::views::name_field(ui, &description, "Description");
            if !draft.error.is_empty() {
                ui.label(RichText::new(&draft.error).color(palette.danger));
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Only http and https links are supported.")
                        .size(11.0)
                        .color(palette.text_muted),
                );
                let enter = url.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button(RichText::new("Add link").strong()).clicked() || enter {
                    action = LinkAction::Save;
                }
            });
        });
    if !open {
        action = LinkAction::Close;
    }
    action
}

/// `researchMarkdownDialog`: the file drawn as the editor draws text; closed by its × or
/// Escape. Returns whether it stays open.
pub fn markdown_window(ctx: &egui::Context, palette: &Palette, title: &str, html: &str) -> bool {
    let mut open = !ctx.input(|i| i.key_pressed(egui::Key::Escape));
    egui::Window::new(title)
        .id(egui::Id::unique("wiki-markdown"))
        .open(&mut open)
        .collapsible(false)
        .default_size([620.0, 520.0])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(
                RichText::new("WIKI FILE")
                    .size(10.0)
                    .color(palette.text_muted),
            );
            egui::ScrollArea::vertical()
                .max_height(480.0)
                .show(ui, |ui| {
                    crate::views::read_only_html(ui, palette, html, "Markdown preview");
                });
        });
    open
}
