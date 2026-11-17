//! ProseCraft's Markdown folder import: the folder's Markdown files read, a dialog asking what
//! each folder holds (manuscript, wiki or nothing) and how chapters are made, then a new project.

use std::collections::HashMap;

use egui::{RichText, Ui};
use prosecraft_core::import::markdown_folder::{
    ChapterMode, FolderRole, ImportFolder, MarkdownFile, MarkdownImportPlan,
    build_markdown_project, list_import_folders, role_of,
};

use crate::{AppState, ProseApp};

/// The folder read, and the choices made in the dialog so far.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MarkdownImportDraft {
    files: Vec<MarkdownFile>,
    ignored: usize,
    folders: Vec<ImportFolder>,
    roles: HashMap<String, FolderRole>,
    mode: ChapterMode,
    name: String,
}

impl ProseApp {
    /// A folder's Markdown files (`handleMarkdownFolder`), by their paths inside it, and how
    /// many other files it holds: the dialog opens with ProseCraft's suggestions.
    pub fn import_markdown_folder(
        &mut self,
        folder_name: &str,
        files: Vec<MarkdownFile>,
        ignored: usize,
    ) {
        if files.is_empty() {
            self.state.status = Some("No Markdown files found in that folder.".into());
            return;
        }
        let folders = list_import_folders(&files);
        let roles = folders
            .iter()
            .map(|f| (f.path.clone(), f.suggested))
            .collect();
        self.state.markdown_import = Some(MarkdownImportDraft {
            files,
            ignored,
            folders,
            roles,
            mode: ChapterMode::Folders,
            name: folder_name.to_owned(),
        });
        self.state.status = None;
    }

    /// A folder that could not be read (more Markdown files than ProseCraft takes, say).
    pub fn import_markdown_folder_failed(&mut self, message: &str) {
        self.state.status = Some(message.to_owned());
    }

    /// `importMarkdownFolder`: the project built from the plan, added and opened.
    pub(crate) fn finish_markdown_import(&mut self, draft: MarkdownImportDraft) {
        let plan = MarkdownImportPlan {
            roles: draft.roles,
            chapter_mode: draft.mode,
        };
        let mut report = None;
        let mut name = String::new();
        let saved = self.add_and_open(|store, _| {
            let (project, made) = build_markdown_project(
                &draft.files,
                &plan,
                &mut || store.new_id(),
                Some(&draft.name),
            );
            name = project
                .get("name")
                .and_then(|n| n.as_js_string())
                .map(|n| n.to_string_lossy())
                .unwrap_or_default();
            report = Some(made);
            project
        });
        let (Some(saved), Some(r)) = (saved, report) else {
            return;
        };
        self.state.status = Some(match saved {
            Ok(()) => {
                let links = if r.links_unresolved > 0 {
                    format!(", {} link(s) without a target page", r.links_unresolved)
                } else {
                    String::new()
                };
                format!(
                    "Imported “{name}”: {} chapter(s), {} scene(s), {} wiki page(s){links}.",
                    r.chapters, r.scenes, r.wiki_pages
                )
            }
            Err(_) => "Could not save the imported project.".into(),
        });
    }
}

fn role_label(role: FolderRole) -> &'static str {
    match role {
        FolderRole::Manuscript => "Manuscript",
        FolderRole::Wiki => "Wiki",
        FolderRole::Skip => "Skip",
    }
}

fn mode_label(mode: ChapterMode) -> &'static str {
    match mode {
        ChapterMode::Folders => "Every folder is a chapter, every file a scene",
        ChapterMode::Files => "Every file is a chapter",
    }
}

/// The import dialog.
pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    let Some(draft) = state.markdown_import.as_mut() else {
        return;
    };
    let mut open = true;
    let mut import = false;
    let heading = |ui: &mut Ui, title: &str, detail: &str| {
        ui.label(RichText::new(title).strong().color(palette.text_heading));
        ui.label(RichText::new(detail).size(12.0).color(palette.text_muted));
    };
    egui::Window::new("Import")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size(egui::vec2(520.0, 470.0))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            crate::widgets::eyebrow(ui, &palette, "Import");
            ui.label(
                RichText::new("Markdown folder")
                    .size(18.0)
                    .strong()
                    .color(palette.text_heading),
            );
            ui.add_space(8.0);
            let ignored = if draft.ignored > 0 {
                format!(", {} other file(s) ignored", draft.ignored)
            } else {
                String::new()
            };
            let label = ui.label(
                RichText::new("Project name")
                    .strong()
                    .color(palette.text_heading),
            );
            ui.label(
                RichText::new(format!(
                    "{} Markdown file(s) found{ignored}.",
                    draft.files.len()
                ))
                .size(12.0)
                .color(palette.text_muted),
            );
            let name = ui
                .add(
                    egui::TextEdit::singleline(&mut draft.name)
                        .char_limit(100)
                        .desired_width(f32::INFINITY),
                )
                .labelled_by(label.id);
            ui.add_space(10.0);
            heading(
                ui,
                "What is in each folder?",
                "Manuscript folders become chapters and scenes, wiki folders become wiki pages. A folder with several subfolders (say, several drafts) can be set per subfolder. Hidden folders and folders starting with “_” are skipped.",
            );
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .max_height(170.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for folder in &draft.folders {
                        ui.horizontal(|ui| {
                            ui.add_space(folder.depth as f32 * 16.0);
                            ui.label(format!("{} ", folder.name));
                            ui.label(
                                RichText::new(format!("({})", folder.files))
                                    .italics()
                                    .color(palette.text_muted),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let role = draft
                                        .roles
                                        .entry(folder.path.clone())
                                        .or_insert(FolderRole::Skip);
                                    let combo = craft_ui::ComboBox::from_id_salt((
                                        "markdown-role",
                                        &folder.path,
                                    ))
                                    .selected_text(role_label(*role))
                                    .width(130.0)
                                    .show_ui(ui, |ui| {
                                        for choice in [
                                            FolderRole::Manuscript,
                                            FolderRole::Wiki,
                                            FolderRole::Skip,
                                        ] {
                                            ui.selectable_value(role, choice, role_label(choice));
                                        }
                                    });
                                    let label = format!("Use {} as", folder.path);
                                    ui.ctx().accesskit_node_builder(combo.response.id, |node| {
                                        node.set_label(label);
                                    });
                                },
                            );
                        });
                    }
                });
            let selected = draft
                .files
                .iter()
                .filter(|f| role_of(&f.path, &draft.roles) != FolderRole::Skip)
                .count();
            ui.label(
                RichText::new(format!("{selected} Markdown file(s) selected."))
                    .size(12.0)
                    .color(palette.text_muted),
            );
            if draft.roles.values().any(|r| *r == FolderRole::Manuscript) {
                ui.add_space(10.0);
                heading(
                    ui,
                    "Chapters",
                    "Files are ordered by their position header when they all have one, otherwise by name.",
                );
                let combo = craft_ui::ComboBox::from_id_salt("markdown-chapter-mode")
                    .selected_text(mode_label(draft.mode))
                    .width(320.0)
                    .show_ui(ui, |ui| {
                        for mode in [ChapterMode::Folders, ChapterMode::Files] {
                            ui.selectable_value(&mut draft.mode, mode, mode_label(mode));
                        }
                    });
                ui.ctx().accesskit_node_builder(combo.response.id, |node| {
                    node.set_label("How to make chapters");
                });
            }
            ui.add_space(10.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Nothing in your folder is changed.")
                        .size(12.0)
                        .color(palette.text_muted),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ready = !draft.name.trim().is_empty();
                    import = ui
                        .add_enabled(ready, egui::Button::new(RichText::new("Import").strong()))
                        .clicked()
                        || (ready
                            && name.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                });
            });
        });
    if import {
        state.markdown_import_done = state.markdown_import.take();
    } else if !open || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.markdown_import = None;
    }
}
