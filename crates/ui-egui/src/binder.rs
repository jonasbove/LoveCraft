//! The binder: project switcher, chapters and scenes, the Wiki, and the Trash
//! (ProseCraft's left sidebar).

use egui::accesskit::Role;
use egui::{RichText, Ui};
use prosecraft_core::project::{Project, WikiItemKind};

use crate::editing::Intent;
use crate::theme::Palette;
use crate::widgets::eyebrow;
use crate::{AppState, Selection, fonts};

/// A row of the binder tree: a selectable label exposed as a tree item with its level and
/// selection, so a screen reader hears "Opening, level 2, selected".
fn tree_row(ui: &mut Ui, palette: &Palette, label: &str, level: usize, selected: bool) -> bool {
    let color = if selected {
        palette.selected_text
    } else {
        palette.text_primary
    };
    let response = ui.selectable_label(selected, RichText::new(label).size(13.5).color(color));
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::TreeItem);
        node.set_level(level);
        node.set_selected(selected);
    });
    response.clicked()
}

fn indent(ui: &mut Ui, depth: usize, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add_space(16.0 * depth as f32);
        add(ui);
    });
}

/// The binder's footer: open a backup, and export the library as one.
fn footer(ui: &mut Ui, state: &mut AppState) {
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        if ui.button("Open backup…").clicked() {
            state.requests.push(crate::HostRequest::OpenBackup);
        }
        if state.has_home && state.library.is_some() && ui.button("Export backup…").clicked() {
            state.export_wanted = true;
        }
    });
}

pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    egui::Panel::bottom("binder-footer")
        .show_separator_line(false)
        .resizable(false)
        .frame(egui::Frame::NONE)
        .show(ui, |ui| footer(ui, state));
    let palette = state.palette;
    let Some(library) = state.library.as_ref() else {
        eyebrow(ui, &palette, "No project open");
        ui.label(
            RichText::new("Drop a ProseCraft backup (.prosecraft) on the window to open it.")
                .color(palette.text_muted),
        );
        return;
    };
    let projects = library.projects();
    let mut active = state.active.min(projects.len().saturating_sub(1));
    let Some(current) = projects.get(active) else {
        ui.label("This backup has no projects.");
        return;
    };

    ui.add_space(6.0);
    // ProseCraft shows the project's name as the switcher, with no label beside it. The name is
    // what's visible, so it leads the accessible name; "Project" says what the control is.
    let switcher = craft_ui::ComboBox::from_id_salt("project-switcher")
        .selected_text(
            RichText::new(current.view.name.as_str())
                .font(fonts::serif(17.0))
                .color(palette.text_heading),
        )
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for (index, project) in projects.iter().enumerate().filter(|(_, p)| !p.view.trashed) {
                ui.selectable_value(&mut active, index, project.view.name.as_str());
            }
        });
    ui.ctx()
        .accesskit_node_builder(switcher.response.id, |node| {
            node.set_label(current.view.name.as_str());
            node.set_description("Project");
        });
    if active != state.active {
        state.active = active;
        state.selection = None;
    }
    let Some(project) = library.projects().get(state.active).map(|p| &p.view) else {
        return;
    };
    if !project.author.is_empty() {
        ui.label(
            RichText::new(project.author.as_str())
                .size(12.0)
                .color(palette.text_muted),
        );
    }
    ui.add_space(14.0);

    let mut chosen = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            chosen = chapters(
                ui,
                &palette,
                project,
                state.selection.as_ref(),
                state.editable,
                &mut state.intents,
            );
            ui.add_space(14.0);
            ui.separator();
            ui.add_space(8.0);
            chosen = wiki(ui, &palette, project, state.selection.as_ref()).or(chosen.take());
            ui.add_space(14.0);
            ui.separator();
            ui.add_space(8.0);
            trash(ui, &palette, project, state.editable, &mut state.intents);
        });
    if let Some(selection) = chosen {
        state.select(selection);
    }
}

fn chapters(
    ui: &mut Ui,
    palette: &Palette,
    project: &Project,
    selection: Option<&Selection>,
    editable: bool,
    intents: &mut Vec<Intent>,
) -> Option<Selection> {
    let mut chosen = None;
    ui.horizontal(|ui| {
        eyebrow(ui, palette, "Chapters");
        if editable {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // An icon button, as ProseCraft's: "+" on screen, "Add chapter" for screen readers.
                let add = ui.small_button("+").on_hover_text("Add chapter");
                ui.ctx()
                    .accesskit_node_builder(add.id, |node| node.set_label("Add chapter"));
                if add.clicked() {
                    intents.push(Intent::AddChapter);
                }
            });
        }
    });
    ui.add_space(4.0);
    let mut any = false;
    for chapter in project.live_chapters() {
        any = true;
        let selected = selection == Some(&Selection::Chapter(chapter.id.clone()));
        let title = crate::views::label_of(project, &chapter.id);
        if tree_row(ui, palette, &title, 1, selected) {
            chosen = Some(Selection::Chapter(chapter.id.clone()));
        }
        for scene in project.scenes_in_chapter(&chapter.id) {
            let selected = selection == Some(&Selection::Document(scene.id.clone()));
            let title = if scene.title.is_empty() {
                "Untitled scene"
            } else {
                scene.title.as_str()
            };
            indent(ui, 1, |ui| {
                if tree_row(ui, palette, title, 2, selected) {
                    chosen = Some(Selection::Document(scene.id.clone()));
                }
            });
        }
    }
    if !any {
        ui.label(RichText::new("No chapters yet.").color(palette.text_muted));
    }
    chosen
}

fn wiki(
    ui: &mut Ui,
    palette: &Palette,
    project: &Project,
    selection: Option<&Selection>,
) -> Option<Selection> {
    let mut chosen = None;
    eyebrow(ui, palette, "Wiki");
    ui.add_space(4.0);
    let items = project.wiki_tree();
    if items.is_empty() {
        ui.label(RichText::new("Nothing here yet").color(palette.text_muted));
    }
    for item in items {
        indent(ui, item.depth, |ui| match item.kind {
            WikiItemKind::Page => {
                let selected = selection == Some(&Selection::Document(item.id.clone()));
                let title = if item.title.is_empty() {
                    "Untitled page"
                } else {
                    item.title.as_str()
                };
                if tree_row(ui, palette, title, item.depth + 1, selected) {
                    chosen = Some(Selection::Document(item.id.clone()));
                }
            }
            WikiItemKind::Folder => {
                ui.label(
                    RichText::new(item.title.as_str())
                        .size(13.5)
                        .color(palette.text_secondary)
                        .strong(),
                );
            }
            WikiItemKind::File => {
                ui.label(
                    RichText::new(format!("File: {}", item.title))
                        .size(13.0)
                        .color(palette.text_secondary),
                );
            }
            WikiItemKind::Link => {
                ui.label(
                    RichText::new(format!("Link: {}", item.title))
                        .size(13.0)
                        .color(palette.text_secondary),
                );
            }
        });
    }
    chosen
}

/// "Restore", named for its item so each button in the Trash can be told apart.
fn restore_button(ui: &mut Ui, item: &str) -> egui::Response {
    let response = ui.small_button("Restore");
    let name = format!("Restore {item}");
    ui.ctx()
        .accesskit_node_builder(response.id, |node| node.set_label(name));
    response
}

fn trash(
    ui: &mut Ui,
    palette: &Palette,
    project: &Project,
    editable: bool,
    intents: &mut Vec<Intent>,
) {
    let (chapters, documents) = project.trash();
    let count = chapters.len() + documents.len();
    egui::CollapsingHeader::new(
        RichText::new(format!("Trash ({count})"))
            .size(13.5)
            .color(palette.text_primary),
    )
    .id_salt("trash")
    .default_open(false)
    .show(ui, |ui| {
        if count == 0 {
            ui.label(RichText::new("Trash is empty").color(palette.text_muted));
        }
        for chapter in chapters {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("Chapter: {}", chapter.title))
                        .color(palette.text_secondary),
                );
                if editable && restore_button(ui, &chapter.title).clicked() {
                    intents.push(Intent::RestoreChapter(chapter.id.clone()));
                }
            });
        }
        for document in documents {
            ui.horizontal(|ui| {
                ui.label(RichText::new(document.title.as_str()).color(palette.text_secondary));
                if editable && restore_button(ui, &document.title).clicked() {
                    intents.push(Intent::RestoreDocument(document.id.clone()));
                }
            });
        }
    });
}
